//! 巻き戻しを LLM の道具から独立した口として公開する（設計書 01-05「巻き戻し」）。

use super::{Agent, AgentError, AgentEvent};
use crate::rewind::{
    CheckpointInfo, CheckpointLimits, CheckpointReason, CheckpointStats, IrreversibleOperation,
    RewindError, RewindReport, RewindScope,
};
use crate::session::Record;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::mpsc::SyncSender;

impl Agent {
    /// セッションの時点を、会話を戻せるかとともに返す。
    pub fn checkpoints(&self) -> Result<Vec<CheckpointInfo>, AgentError> {
        let mut points = self.recorder()?.checkpoints()?;
        let records = crate::session::read_records(self.session.directory())?;
        for point in &mut points {
            point.tool = point.checkpoint.call_id.as_ref().and_then(|id| {
                records.iter().find_map(|record| {
                    if let Record::ToolCall { call, .. } | Record::ToolStart { call } = record {
                        (call.id == *id).then(|| call.clone())
                    } else {
                        None
                    }
                })
            });
            if point.checkpoint.reason == CheckpointReason::Request {
                point.request = point
                    .checkpoint
                    .items
                    .filter(|_| point.conversation_available)
                    .and_then(|index| self.conversation.items.get(index))
                    .and_then(request_text);
                if point.request.is_none() {
                    let mut after = false;
                    for record in &records {
                        if let Record::Checkpoint(checkpoint) = record {
                            if after {
                                break;
                            }
                            after = checkpoint.id == point.checkpoint.id;
                        }
                        if after
                            && let Record::User {
                                kind: crate::session::UserKind::Request,
                                message,
                            } = record
                        {
                            point.request = message
                                .parts
                                .iter()
                                .find(|part| part.kind == crate::provider::UserPartKind::Request)
                                .map(|part| {
                                    part.text.lines().next().unwrap_or_default().to_owned()
                                });
                            break;
                        }
                    }
                }
            }
        }
        Ok(points)
    }
    /// ファイルも会話も変えず、実行前に戻せない操作を返す。
    pub fn rewind_preview(
        &self,
        checkpoint: &str,
        scope: RewindScope,
    ) -> Result<RewindPreview, AgentError> {
        let selected = self
            .checkpoints()?
            .into_iter()
            .find(|point| point.checkpoint.id == checkpoint)
            .ok_or(RewindError::Invalid(super::text::UNKNOWN_CHECKPOINT))?;
        if scope != RewindScope::Files && !selected.conversation_available {
            return Err(RewindError::Invalid(super::text::CONVERSATION_REWIND_UNAVAILABLE).into());
        }
        Ok(RewindPreview {
            selected,
            scope,
            irreversible: self.irreversible(checkpoint)?,
        })
    }
    /// 記録する作業量の上限を変更する。
    pub fn set_checkpoint_limits(&mut self, limits: CheckpointLimits) -> Result<(), AgentError> {
        Ok(self.recorder()?.set_limits(limits)?)
    }
    /// 最後の記録の読み取り量を返す。
    pub fn checkpoint_stats(&self) -> Result<CheckpointStats, AgentError> {
        Ok(self.recorder()?.stats()?)
    }
    /// 一連の処理が終わった後に、選んだファイルと会話を戻す。&mut self の排他性で run と同時に呼べない。
    pub fn rewind(
        &mut self,
        checkpoint: &str,
        scope: RewindScope,
        allow_incomplete: bool,
        events: SyncSender<AgentEvent>,
    ) -> Result<RewindReport, AgentError> {
        let files = scope != RewindScope::Conversation;
        let conversation = scope != RewindScope::Files;
        let selected = self
            .checkpoints()?
            .into_iter()
            .find(|info| info.checkpoint.id == checkpoint)
            .ok_or(RewindError::Invalid(super::text::UNKNOWN_CHECKPOINT))?;
        if conversation
            && (!selected.conversation_available
                || selected
                    .checkpoint
                    .items
                    .is_none_or(|count| count > self.conversation.items.len()))
        {
            return Err(RewindError::Invalid(super::text::CONVERSATION_REWIND_UNAVAILABLE).into());
        }
        let irreversible = self.irreversible(checkpoint)?;
        // 再開の直後は run がまだ時点の番を設定していない（設計書 01-05「再開」）。
        self.recorder()?.set_turn(self.turn)?;
        let restored = if files {
            let prepared = self.recorder()?.prepare_restore_files(checkpoint)?;
            self.checkpoint_notices(&events)?;
            prepared.restore(allow_incomplete)
        } else {
            Ok(RewindReport::default())
        };
        self.checkpoint_notices(&events)?;
        let mut report = restored?;
        report.irreversible = irreversible;
        for path in &report.changed_paths {
            self.tool_context
                .clear_read_version(&self.tool_context.project.real_path().join(path))
                .map_err(|_| AgentError::Worker)?;
        }
        if conversation {
            let items = selected.checkpoint.items.ok_or(RewindError::Invalid(
                super::text::CONVERSATION_REWIND_UNAVAILABLE,
            ))?;
            // 再開が補った結果は巻き戻しより前に確定する。次の run で捨てた枝へ
            // 結果だけを追記すると replay とメモリの会話がずれる（設計書 01-05「再開」「会話を戻すとき」）。
            for record in std::mem::take(&mut self.resume_results) {
                self.writer.append(&record)?;
            }
            self.writer.append(&Record::Rewind {
                items,
                checkpoint: checkpoint.into(),
            })?;
            self.conversation.items.truncate(items);
            self.recorder()?.advance_generation()?;
            self.previous_last_item = None;
            self.last_usage = None;
            self.added_bytes = 0;
            self.largest_input = None;
            self.repeated = None;
            // 再開の最初の依頼の前でも、残った別のモデルの推論を要約する必要は
            // 消さない。会話を空に戻したときだけ解除する（設計書 01-05「再開」）。
            self.resume_has_reasoning = self.resume_has_reasoning
                && self.conversation.items.iter().any(|item| match item {
                    crate::provider::Item::AssistantTurn(turn) => {
                        super::reasoning::has_reasoning(turn)
                    }
                    crate::provider::Item::UserMessage(_)
                    | crate::provider::Item::ToolResult(_) => false,
                });
            self.started = !self.conversation.items.is_empty();
            if !self.started {
                self.resume_compaction = false;
                self.environment_changes = None;
            }
            report.conversation_rewound = true;
        }
        Ok(report)
    }
    fn recorder(&self) -> Result<&crate::rewind::CheckpointRecorder, AgentError> {
        self.tool_context
            .checkpoints
            .as_ref()
            .ok_or(AgentError::Worker)
    }
    pub(super) fn checkpoint_notices(
        &mut self,
        events: &SyncSender<AgentEvent>,
    ) -> Result<(), AgentError> {
        for notice in self.recorder()?.take_notices()? {
            self.emit(
                events,
                AgentEvent::Checkpoint {
                    checkpoint: notice.checkpoint,
                },
            )?;
            if !notice.hard_links.is_empty() {
                self.emit(
                    events,
                    AgentEvent::HardLinks {
                        paths: notice.hard_links,
                    },
                )?;
            }
        }
        Ok(())
    }
    fn irreversible(&self, checkpoint: &str) -> Result<Vec<IrreversibleOperation>, AgentError> {
        let records = crate::session::read_records(self.session.directory())?;
        let mut calls = BTreeMap::new();
        for record in &records {
            if let Record::ToolStart { call } = record {
                calls.insert(call.id.clone(), call);
            }
        }
        let write_paths = records
            .iter()
            .filter_map(|record| {
                if let Record::WriteTarget { call_id, real_path } = record {
                    Some((call_id.as_str(), real_path))
                } else {
                    None
                }
            })
            .collect::<BTreeMap<_, _>>();
        let mut manifests = BTreeMap::new();
        let mut selected = false;
        let mut result = BTreeMap::new();
        let mut candidates = BTreeSet::new();
        let mut latest_checkpoint = checkpoint.to_owned();
        for record in &records {
            if let Record::Checkpoint(point) = record {
                latest_checkpoint = point.id.clone();
                if point.id == checkpoint {
                    selected = true;
                }
                if selected
                    && point.reason == CheckpointReason::Bash
                    && let Some(id) = &point.call_id
                {
                    candidates.insert(id.clone());
                }
            }
            if selected
                && let Record::ToolStart { call } = record
                && matches!(
                    call.name.as_str(),
                    "web_fetch" | "benitoite_run" | "benitoite_test"
                )
            {
                // 取得の成否では外への通信の有無を証明できないので保守的に示す。
                candidates.insert(call.id.clone());
            }
            if selected
                && let Record::ToolResult { result: output, .. } = record
                && !output.is_error
                && let Some(call) = calls.get(&output.id)
                && matches!(call.name.as_str(), "write" | "edit")
            {
                let manifest = manifests
                    .entry(latest_checkpoint.clone())
                    .or_insert_with(|| {
                        self.recorder()
                            .ok()
                            .and_then(|recorder| recorder.manifest(&latest_checkpoint).ok())
                    });
                let excluded = write_paths.get(call.id.as_str()).is_none_or(|resolved| {
                    self.recorder().is_ok_and(|recorder| {
                        recorder.excluded_in_manifest(manifest.as_ref(), resolved)
                    })
                });
                if excluded {
                    candidates.insert(call.id.clone());
                }
            }
        }
        for id in candidates {
            if let Some(call) = calls.get(&id) {
                result.insert(
                    id.clone(),
                    IrreversibleOperation {
                        call_id: id,
                        summary: super::text::rewind_operation(&call.name, &call.arguments),
                    },
                );
            }
        }
        Ok(result.into_values().collect())
    }
}

/// 実行前の確認に使う、時点と範囲と戻らない操作。
#[derive(Clone, Debug)]
pub struct RewindPreview {
    pub selected: CheckpointInfo,
    pub scope: RewindScope,
    pub irreversible: Vec<IrreversibleOperation>,
}

fn request_text(item: &crate::provider::Item) -> Option<String> {
    if let crate::provider::Item::UserMessage(message) = item {
        message
            .parts
            .iter()
            .find(|part| part.kind == crate::provider::UserPartKind::Request)
            .map(|part| part.text.lines().next().unwrap_or_default().to_owned())
    } else {
        None
    }
}
