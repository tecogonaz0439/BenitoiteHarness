//! 記録の起動値と推論の区間を復元する（設計書 01-05「再開」）。

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use super::{FileHistory, Record, RecordedMode, SessionError, read_records, replay, text};
use crate::provider::{Conversation, Destination, Item};

/// 各応答を作ったプロンプトと送り先。圧縮の応答にも行の時点の区間を割り当てる。
#[derive(Clone, Debug)]
pub struct ReasoningContext {
    pub system_prompt_sha256: String,
    pub tools_sha256: String,
    pub destination: Destination,
}

/// 再開に戻す状態。承認とファイルの版は含めない。
#[derive(Clone, Debug)]
pub struct ResumeState {
    pub conversation: Conversation,
    pub missing_results: Vec<Record>,
    pub max_turn: u64,
    pub mode: RecordedMode,
    pub destination: Destination,
    pub project_dir: PathBuf,
    pub reasoning: Vec<ReasoningContext>,
    pub files: FileHistory,
}

impl ResumeState {
    /// ロックを保持したセッションの記録を読み、実行し直さない結果を取り出す。
    pub fn read(directory: &Path) -> Result<Self, SessionError> {
        let records = read_records(directory)?;
        Self::from_records(&records)
    }

    pub(super) fn from_records(records: &[Record]) -> Result<Self, SessionError> {
        let Some(Record::Header(header)) = records.first() else {
            return Err(SessionError::Invalid(text::INVALID_FORMAT));
        };
        let mut segment = ReasoningContext {
            system_prompt_sha256: header.system_prompt_sha256.clone(),
            tools_sha256: header.tools_sha256.clone(),
            destination: Destination {
                provider: header.provider.name.clone(),
                base_url: header.provider.base_url.clone(),
                model: header.provider.model.clone(),
            },
        };
        let mut state = Self {
            conversation: replay::replay_records(records),
            missing_results: Vec::new(),
            max_turn: 0,
            mode: header.mode,
            destination: segment.destination.clone(),
            project_dir: header.project_dir.clone(),
            reasoning: Vec::new(),
            files: FileHistory::from_records(records),
        };
        let mut item_reasoning = Vec::new();
        let mut results = BTreeSet::new();
        let mut calls = BTreeMap::new();
        for record in records {
            match record {
                Record::SessionStart {
                    mode,
                    provider,
                    model,
                    base_url,
                    system_prompt_sha256,
                    tools_sha256,
                    ..
                } => {
                    state.mode = *mode;
                    segment = ReasoningContext {
                        system_prompt_sha256: system_prompt_sha256
                            .as_ref()
                            .unwrap_or(&header.system_prompt_sha256)
                            .clone(),
                        tools_sha256: tools_sha256
                            .as_ref()
                            .unwrap_or(&header.tools_sha256)
                            .clone(),
                        destination: Destination {
                            provider: provider.clone(),
                            model: model.clone(),
                            base_url: base_url
                                .as_ref()
                                .unwrap_or(&header.provider.base_url)
                                .clone(),
                        },
                    };
                }
                Record::ProjectDirChanged { project_dir } => {
                    state.project_dir = project_dir.clone()
                }
                Record::Assistant { turn, response } => {
                    item_reasoning.push(Some(segment.clone()));
                    for part in &response.parts {
                        if let crate::provider::AssistantPart::ToolCall(call) = part {
                            calls.insert(call.id.clone(), (*turn, call.name.clone()));
                        }
                    }
                }
                Record::User { .. } => item_reasoning.push(None),
                Record::Rewind { items, .. } => item_reasoning.truncate(*items),
                Record::ToolResult { result, .. } => {
                    item_reasoning.push(None);
                    results.insert(result.id.clone());
                }
                Record::ToolCall { turn, call, .. } => {
                    calls.insert(call.id.clone(), (*turn, call.name.clone()));
                }
                Record::Compaction { conversation, .. } => {
                    for item in &conversation.items {
                        match item {
                            Item::ToolResult(result) => {
                                results.insert(result.id.clone());
                            }
                            Item::AssistantTurn(response) => {
                                for part in &response.parts {
                                    if let crate::provider::AssistantPart::ToolCall(call) = part {
                                        let turn = call
                                            .id
                                            .strip_prefix("turn-")
                                            .and_then(|rest| rest.split_once("-call-"))
                                            .and_then(|(turn, _)| turn.parse::<u64>().ok())
                                            .unwrap_or(state.max_turn);
                                        calls
                                            .entry(call.id.clone())
                                            .or_insert((turn, call.name.clone()));
                                    }
                                }
                            }
                            Item::UserMessage(_) => {}
                        }
                    }
                    item_reasoning = conversation
                        .items
                        .iter()
                        .map(|item| {
                            matches!(item, Item::AssistantTurn(_)).then_some(segment.clone())
                        })
                        .collect();
                }
                Record::Header(_)
                | Record::TurnStart { .. }
                | Record::ToolStart { .. }
                | Record::Approval { .. }
                | Record::Retry { .. }
                | Record::Error { .. }
                | Record::TurnEnd { .. }
                | Record::SessionEnd { .. }
                | Record::Summary { .. }
                | Record::Checkpoint(_)
                | Record::WriteTarget { .. }
                | Record::Spilled { .. } => {}
            }
            if let Record::TurnStart { turn }
            | Record::Assistant { turn, .. }
            | Record::ToolCall { turn, .. }
            | Record::ToolResult { turn, .. }
            | Record::TurnEnd { turn, .. } = record
            {
                state.max_turn = state.max_turn.max(*turn);
            }
        }
        state.reasoning = item_reasoning.into_iter().flatten().collect();
        state.max_turn = calls
            .values()
            .fold(state.max_turn, |maximum, (turn, _)| maximum.max(*turn));
        state.destination = segment.destination;
        for item in &state.conversation.items {
            if let Item::ToolResult(result) = item
                && !results.contains(&result.id)
            {
                let (turn, name) = calls
                    .get(&result.id)
                    .cloned()
                    .unwrap_or((state.max_turn, String::new()));
                state.missing_results.push(Record::ToolResult {
                    turn,
                    name,
                    result: result.clone(),
                    spill: None,
                });
            }
        }
        Ok(state)
    }
}
