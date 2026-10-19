//! 巻き戻しを LLM の道具から独立した口として公開する（設計書 01-05「巻き戻し」）。

use super::{Agent, AgentError, AgentEvent};
use crate::rewind::{
    CheckpointInfo, CheckpointLimits, CheckpointReason, CheckpointStats, IrreversibleOperation,
    RewindError, RewindReport, RewindScope,
};
use crate::session::Record;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::mpsc::SyncSender;

struct BranchMove {
    to: Option<u64>,
    checkpoint: Option<String>,
    request: Option<String>,
    scope: RewindScope,
}

impl Agent {
    /// セッションの時点を、会話を戻せるかとともに返す。
    pub fn checkpoints(&self) -> Result<Vec<CheckpointInfo>, AgentError> {
        let records = crate::session::read_records(self.session.directory())?;
        let tree = crate::session::SessionTree::from_records(&records)?;
        self.checkpoints_from_records(&records, &tree)
    }

    fn checkpoints_from_records(
        &self,
        records: &[Record],
        tree: &crate::session::SessionTree,
    ) -> Result<Vec<CheckpointInfo>, AgentError> {
        let mut points = self.recorder()?.checkpoints_from_tree(tree)?;
        let mut calls = BTreeMap::new();
        for record in records {
            if let Record::ToolCall { call, .. } | Record::ToolStart { call } = record {
                calls.entry(call.id.as_str()).or_insert(call);
            }
        }
        for point in &mut points {
            point.tool = point
                .checkpoint
                .call_id
                .as_deref()
                .and_then(|id| calls.get(id))
                .map(|call| (*call).clone());
            point.request = tree
                .checkpoint_request(&point.checkpoint.id)
                .and_then(|node| tree.entry(node))
                .and_then(|(_, index)| records.get(index))
                .and_then(crate::session::request_text)
                .map(|text| text.lines().next().unwrap_or_default().to_owned());
        }
        Ok(points)
    }

    /// ファイルも会話も変えず、実行前に戻せない操作を返す。
    pub fn rewind_preview(
        &self,
        checkpoint: &str,
        scope: RewindScope,
    ) -> Result<RewindPreview, AgentError> {
        let records = crate::session::read_records(self.session.directory())?;
        let tree = crate::session::SessionTree::from_records(&records)?;
        let selected = self
            .checkpoints_from_records(&records, &tree)?
            .into_iter()
            .find(|point| point.checkpoint.id == checkpoint)
            .ok_or(RewindError::Invalid(super::text::UNKNOWN_CHECKPOINT))?;
        if scope != RewindScope::Files && !selected.conversation_available {
            return Err(RewindError::Invalid(super::text::CONVERSATION_REWIND_UNAVAILABLE).into());
        }
        Ok(RewindPreview {
            selected,
            scope,
            irreversible: self.irreversible(checkpoint, &records)?,
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

    /// 一連の処理が終わった後に、選んだファイルと会話を戻す。
    pub fn rewind(
        &mut self,
        checkpoint: &str,
        scope: RewindScope,
        allow_incomplete: bool,
        events: SyncSender<AgentEvent>,
    ) -> Result<RewindReport, AgentError> {
        let (records, tree) = self.records_for_move(scope != RewindScope::Files)?;
        let selected = self
            .checkpoints_from_records(&records, &tree)?
            .into_iter()
            .find(|info| info.checkpoint.id == checkpoint)
            .ok_or(RewindError::Invalid(super::text::UNKNOWN_CHECKPOINT))?;
        if scope != RewindScope::Files && !selected.conversation_available {
            return Err(RewindError::Invalid(super::text::CONVERSATION_REWIND_UNAVAILABLE).into());
        }
        let request = tree
            .checkpoint_request(checkpoint)
            .and_then(|node| tree.entry(node))
            .and_then(|(_, index)| records.get(index))
            .and_then(crate::session::request_text);
        self.move_branch(
            records,
            tree,
            BranchMove {
                to: selected.checkpoint.node,
                checkpoint: Some(checkpoint.into()),
                request,
                scope,
            },
            allow_incomplete,
            events,
        )
    }

    /// どの枝でも、依頼の直前と枝の先端を移動の候補として返す。
    pub fn branch_targets(&self) -> Result<Vec<BranchInfo>, AgentError> {
        let records = crate::session::read_records(self.session.directory())?;
        let tree = crate::session::SessionTree::from_records(&records)?;
        self.branch_targets_from_records(&records, &tree)
    }

    /// 一度読んだ記録から表示行と移動先を組み立てる。今の先端へは仕事を送らない。
    pub fn branch_tree(&self) -> Result<Vec<BranchTreeRow>, AgentError> {
        let records = crate::session::read_records(self.session.directory())?;
        let tree = crate::session::SessionTree::from_records(&records)?;
        let mut targets = self
            .branch_targets_from_records(&records, &tree)?
            .into_iter()
            .map(|info| (info.target, info))
            .collect::<BTreeMap<_, _>>();
        Ok(crate::session::tree_rows(&records, &tree)
            .into_iter()
            .map(|row| {
                let target = row.node.and_then(|node| {
                    if row.is_current && row.is_tip {
                        None
                    } else if row.request.is_some() {
                        Some(BranchTarget::BeforeRequest(node))
                    } else if row.is_tip {
                        Some(BranchTarget::Tip(node))
                    } else {
                        None
                    }
                });
                let branch = target.and_then(|target| targets.remove(&target));
                BranchTreeRow { row, branch }
            })
            .collect())
    }

    /// 枝を移る前に、移る先の時点と戻せない操作を確かめる。
    pub fn branch_preview(
        &self,
        target: BranchTarget,
        scope: RewindScope,
    ) -> Result<BranchPreview, AgentError> {
        let records = crate::session::read_records(self.session.directory())?;
        let tree = crate::session::SessionTree::from_records(&records)?;
        let selected = self
            .branch_targets_from_records(&records, &tree)?
            .into_iter()
            .find(|info| info.target == target)
            .ok_or(RewindError::Invalid(
                super::text::CONVERSATION_REWIND_UNAVAILABLE,
            ))?;
        if scope == RewindScope::Files
            || (scope == RewindScope::FilesAndConversation && selected.checkpoint.is_none())
        {
            return Err(RewindError::Invalid(super::text::CONVERSATION_REWIND_UNAVAILABLE).into());
        }
        let checkpoint = self
            .recorder()?
            .checkpoints_from_tree(&tree)?
            .into_iter()
            .find(|point| Some(&point.checkpoint.id) == selected.checkpoint.as_ref());
        let irreversible = selected
            .checkpoint
            .as_deref()
            .map(|id| self.irreversible(id, &records))
            .transpose()?
            .unwrap_or_default();
        Ok(BranchPreview {
            selected,
            checkpoint,
            scope,
            irreversible,
        })
    }

    fn branch_targets_from_records(
        &self,
        records: &[Record],
        tree: &crate::session::SessionTree,
    ) -> Result<Vec<BranchInfo>, AgentError> {
        // 最後のプロジェクトの切り替えより前のファイルの時点は使わない。
        let points = self
            .recorder()?
            .checkpoints_from_tree(tree)?
            .into_iter()
            .map(|point| point.checkpoint.id)
            .collect::<BTreeSet<_>>();
        let mut targets = Vec::new();
        for record in records {
            if let Some(request) = crate::session::request_text(record)
                && let Some(node) = record.node()
            {
                let checkpoint = tree
                    .request_checkpoint(node)
                    .filter(|id| points.contains(*id))
                    .map(str::to_owned);
                targets.push(BranchInfo {
                    target: BranchTarget::BeforeRequest(node),
                    request: Some(request),
                    checkpoint,
                });
            }
        }
        for node in tree.leaves() {
            let checkpoint = tree
                .branch_checkpoint(node)
                .filter(|id| points.contains(*id))
                .map(str::to_owned);
            targets.push(BranchInfo {
                target: BranchTarget::Tip(node),
                request: None,
                checkpoint,
            });
        }
        Ok(targets)
    }

    /// 一連の処理をしていないときに、指定した枝へ移る。今の先端なら何もしない。
    pub fn switch_branch(
        &mut self,
        target: BranchTarget,
        expected_checkpoint: Option<&str>,
        scope: RewindScope,
        allow_incomplete: bool,
        events: SyncSender<AgentEvent>,
    ) -> Result<RewindReport, AgentError> {
        if scope == RewindScope::Files {
            return Err(RewindError::Invalid(super::text::CONVERSATION_REWIND_UNAVAILABLE).into());
        }
        let mut records = crate::session::read_records(self.session.directory())?;
        let mut tree = crate::session::SessionTree::from_records(&records)?;
        let selected = self
            .branch_targets_from_records(&records, &tree)?
            .into_iter()
            .find(|info| info.target == target)
            .ok_or(RewindError::Invalid(
                super::text::CONVERSATION_REWIND_UNAVAILABLE,
            ))?;
        // 確認した時点との照合を、補う結果やバックアップを追記する前に済ませる。
        // 記録は信頼せず、読み直しで選ばれた時点が変わっていたら移らない
        // （設計書 01-05「セッションの木」、03-03「確認の共通の規則」）。
        if selected.checkpoint.as_deref() != expected_checkpoint {
            return Err(RewindError::Invalid(super::text::BRANCH_CHANGED).into());
        }
        // 補う結果の追記は先端を進めるため、同じ先端への移動は追記の前に止める。
        if matches!(target, BranchTarget::Tip(node) if Some(node) == self.current_node) {
            return Ok(RewindReport::default());
        }
        if scope == RewindScope::FilesAndConversation && selected.checkpoint.is_none() {
            return Err(RewindError::Invalid(super::text::CONVERSATION_REWIND_UNAVAILABLE).into());
        }
        for record in self.flush_resume_results()? {
            tree.push_record(&record, records.len())?;
            records.push(record);
        }
        let to = match target {
            BranchTarget::BeforeRequest(node) => {
                tree.entry(node)
                    .ok_or(RewindError::Invalid(
                        super::text::CONVERSATION_REWIND_UNAVAILABLE,
                    ))?
                    .0
            }
            BranchTarget::Tip(node) => Some(node),
        };
        self.move_branch(
            records,
            tree,
            BranchMove {
                to,
                checkpoint: selected.checkpoint,
                request: selected.request,
                scope,
            },
            allow_incomplete,
            events,
        )
    }

    fn records_for_move(
        &mut self,
        flush: bool,
    ) -> Result<(Vec<Record>, crate::session::SessionTree), AgentError> {
        let mut records = crate::session::read_records(self.session.directory())?;
        let mut tree = crate::session::SessionTree::from_records(&records)?;
        if flush {
            for record in self.flush_resume_results()? {
                tree.push_record(&record, records.len())?;
                records.push(record);
            }
        }
        Ok((records, tree))
    }

    fn move_branch(
        &mut self,
        mut records: Vec<Record>,
        mut tree: crate::session::SessionTree,
        movement: BranchMove,
        allow_incomplete: bool,
        events: SyncSender<AgentEvent>,
    ) -> Result<RewindReport, AgentError> {
        let conversation = movement.scope != RewindScope::Files;
        let irreversible = movement
            .checkpoint
            .as_deref()
            .map(|id| self.irreversible(id, &records))
            .transpose()?
            .unwrap_or_default();
        let mut branch = Record::Branch {
            to: movement.to,
            from: self.current_node,
            checkpoint: None,
        };
        // 組み立ての失敗では記録もファイルも変えない。復元の準備は読み込んだ記録で済ませる。
        let prepared_path = if conversation {
            tree.push_record(&branch, records.len())?;
            records.push(branch.clone());
            let state = crate::session::ResumeState::from_records_with_tree(&records, tree)?;
            Some(self.prepare_path_state(state)?)
        } else {
            None
        };
        self.recorder()?.set_turn(self.turn)?;
        let mut report = if movement.scope != RewindScope::Conversation {
            let id = movement
                .checkpoint
                .as_deref()
                .ok_or(RewindError::Invalid(super::text::UNKNOWN_CHECKPOINT))?;
            let prepared = self.recorder()?.prepare_restore_files(id)?;
            self.checkpoint_notices(&events)?;
            let report = prepared.restore(allow_incomplete)?;
            // 以降の通知・追記が失敗しても、書き戻したファイルの読んだ版を残さない。
            for path in &report.changed_paths {
                self.tool_context
                    .clear_read_version(&self.tool_context.project.real_path().join(path))
                    .map_err(|_| AgentError::Worker)?;
            }
            report
        } else {
            let before = self
                .recorder()?
                .capture(
                    CheckpointReason::BeforeRewind,
                    None,
                    &std::sync::atomic::AtomicBool::new(false),
                )
                .ok();
            RewindReport {
                before_rewind: before.map(|point| point.id),
                ..RewindReport::default()
            }
        };
        self.checkpoint_notices(&events)?;
        report.irreversible = irreversible;
        if let Some(prepared) = prepared_path {
            if let Record::Branch { checkpoint, .. } = &mut branch {
                *checkpoint = report.before_rewind.clone();
            }
            self.commit_branch(&branch, prepared)?;
            report.conversation_rewound = true;
            report.request = movement.request;
        }
        Ok(report)
    }

    pub(super) fn recorder(&self) -> Result<&crate::rewind::CheckpointRecorder, AgentError> {
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
    fn irreversible(
        &self,
        checkpoint: &str,
        records: &[Record],
    ) -> Result<Vec<IrreversibleOperation>, AgentError> {
        let mut calls = BTreeMap::new();
        for record in records {
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
        for record in records {
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
                    "web_fetch" | "benitoite_run" | "benitoite_test" | "benitoite_scratch"
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

/// 木の中の移動先。依頼そのものの番号と先端の番号を区別する。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum BranchTarget {
    BeforeRequest(u64),
    Tip(u64),
}

/// 画面が移動先と、ファイルを戻せるかを示す情報。
#[derive(Clone, Debug)]
pub struct BranchInfo {
    pub target: BranchTarget,
    pub request: Option<String>,
    pub checkpoint: Option<String>,
}

/// セッションの木の表示行と、選択したときの移動先。
#[derive(Clone, Debug)]
pub struct BranchTreeRow {
    pub row: crate::session::TreeRow,
    pub branch: Option<BranchInfo>,
}

/// 枝の移動の確認。時点のない移動先も会話だけなら選べる。
#[derive(Clone, Debug)]
pub struct BranchPreview {
    pub selected: BranchInfo,
    pub checkpoint: Option<CheckpointInfo>,
    pub scope: RewindScope,
    pub irreversible: Vec<IrreversibleOperation>,
}
