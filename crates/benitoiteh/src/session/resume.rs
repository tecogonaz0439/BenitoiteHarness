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
    pub tree: super::SessionTree,
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
        Self::from_records(records)
    }

    pub(super) fn from_records(records: Vec<Record>) -> Result<Self, SessionError> {
        let tree = super::SessionTree::from_records(&records)?;
        Self::from_records_with_tree(records, tree)
    }

    /// 呼び出した操作が検査済みの木を使い、記録の再読込と索引の再構築を避ける。
    pub(crate) fn from_records_with_tree(
        records: Vec<Record>,
        tree: super::SessionTree,
    ) -> Result<Self, SessionError> {
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
            conversation: Conversation::default(),
            tree,
            missing_results: Vec::new(),
            max_turn: 0,
            mode: header.mode,
            destination: segment.destination.clone(),
            project_dir: header.project_dir.clone(),
            reasoning: Vec::new(),
            files: FileHistory::from_records(&records),
        };
        let mut node_reasoning = BTreeMap::new();
        let mut results = BTreeSet::new();
        let mut calls = BTreeMap::new();
        for record in &records {
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
                Record::Assistant {
                    node,
                    turn,
                    response,
                } => {
                    node_reasoning.insert(*node, vec![segment.clone()]);
                    for part in &response.parts {
                        if let crate::provider::AssistantPart::ToolCall(call) = part {
                            calls.insert(call.id.clone(), (*turn, call.name.clone()));
                        }
                    }
                }

                Record::ToolResult { .. } => {}
                Record::ToolCall { turn, call, .. } => {
                    calls.insert(call.id.clone(), (*turn, call.name.clone()));
                }
                Record::Compaction {
                    node, conversation, ..
                } => {
                    for item in &conversation.items {
                        match item {
                            Item::ToolResult(_) => {}
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
                    node_reasoning.insert(
                        *node,
                        conversation
                            .items
                            .iter()
                            .filter(|item| matches!(item, Item::AssistantTurn(_)))
                            .map(|_| segment.clone())
                            .collect(),
                    );
                }
                Record::User { .. }
                | Record::Branch { .. }
                | Record::Header(_)
                | Record::TurnStart { .. }
                | Record::ToolStart { .. }
                | Record::Approval { .. }
                | Record::ImagesRemoved { .. }
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
        for node in state.tree.path(state.tree.tip) {
            if let Some((_, index)) = state.tree.entry(node) {
                match records.get(index) {
                    Some(Record::Compaction { conversation, .. }) => {
                        state.reasoning.clear();
                        results.clear();
                        results.extend(conversation.items.iter().filter_map(|item| match item {
                            Item::ToolResult(result) => Some(result.id.clone()),
                            Item::UserMessage(_) | Item::AssistantTurn(_) => None,
                        }));
                    }
                    Some(Record::ToolResult { result, .. }) => {
                        results.insert(result.id.clone());
                    }
                    _ => {}
                }
            }
            if let Some(contexts) = node_reasoning.remove(&node) {
                state.reasoning.extend(contexts);
            }
        }
        state.max_turn = calls
            .values()
            .fold(state.max_turn, |maximum, (turn, _)| maximum.max(*turn));
        state.destination = segment.destination;
        // 区間・番の番号・ファイルの履歴を先に取り、記録の本文は会話へ移す。
        // 大きな道具の結果を二重に持たない（設計書 01-05「再開」「セッションの木」）。
        state.conversation = replay::replay_records(records, &state.tree);
        for item in &state.conversation.items {
            if let Item::ToolResult(result) = item
                && !results.contains(&result.id)
            {
                let (turn, name) = calls
                    .get(&result.id)
                    .cloned()
                    .unwrap_or((state.max_turn, String::new()));
                state.missing_results.push(Record::ToolResult {
                    node: 0,
                    turn,
                    name,
                    result: result.clone(),
                    spill: None,
                    stderr_spill: None,
                });
            }
        }
        Ok(state)
    }
}
