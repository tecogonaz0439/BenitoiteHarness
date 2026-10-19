//! 途中で切れた最終行の破棄と、未完の呼び出しの結果の補完（設計書 01-05「再開」）。

use std::collections::BTreeSet;
use std::fs::File;
use std::io::{BufRead, BufReader, Read};
use std::path::Path;

use crate::provider::{AssistantPart, Conversation, Item, ToolCall, ToolResult};
use crate::safe_fs::{ScopeRelPath, ScopeRoot};

use super::{FORMAT_VERSION, MAX_BYTES, Record, SessionError, read_spill, text};

/// session.jsonl を読み、spill へ移した記録を元の型に戻す。
///
/// 改行で終わらない最後の行だけを破棄する。途中の不正な JSON は誤りとして返す。
pub fn read_records(directory: &Path) -> Result<Vec<Record>, SessionError> {
    let root = ScopeRoot::open(directory)?;
    let file = root.open_read(&ScopeRelPath::new("session.jsonl")?)?;
    Ok(read_records_from(&root, file)?.0)
}

pub(super) fn read_records_from(
    root: &ScopeRoot,
    file: File,
) -> Result<(Vec<Record>, u64), SessionError> {
    let mut reader = BufReader::new(file);
    let mut records = Vec::new();
    let mut complete = 0_u64;
    loop {
        let mut line = Vec::new();
        let read = reader
            .by_ref()
            .take(
                u64::try_from(MAX_BYTES)
                    .unwrap_or(u64::MAX)
                    .saturating_add(1),
            )
            .read_until(b'\n', &mut line)?;
        if read == 0 {
            break;
        }
        if line.len() > MAX_BYTES {
            return Err(SessionError::Invalid(text::RECORD_TOO_LARGE));
        }
        if line.last() != Some(&b'\n') {
            break;
        }
        let value: serde_json::Value = serde_json::from_slice(&line)?;
        if records.is_empty() {
            check_format(&value)?;
        }
        let mut record: Record = serde_json::from_value(value)?;
        if let Record::Spilled { parts } = record {
            if parts.is_empty() {
                return Err(SessionError::Invalid(text::INVALID_SPILL));
            }
            let mut bytes = Vec::new();
            for part in parts {
                let mut file = read_spill(root, &part)?;
                if file.metadata()?.len() > u64::try_from(MAX_BYTES).unwrap_or(u64::MAX) {
                    return Err(SessionError::Invalid(text::INVALID_SPILL));
                }
                file.read_to_end(&mut bytes)?;
            }
            record = serde_json::from_value(serde_json::from_slice::<serde_json::Value>(&bytes)?)?;
            if matches!(record, Record::Spilled { .. }) {
                return Err(SessionError::Invalid(text::INVALID_SPILL));
            }
        }
        if records.is_empty()
            && !matches!(&record, Record::Header(header) if header.format == FORMAT_VERSION)
        {
            return Err(SessionError::Invalid(text::INVALID_FORMAT));
        }
        if !records.is_empty() && matches!(record, Record::Header(_)) {
            return Err(SessionError::Invalid(text::INVALID_FORMAT));
        }
        records.push(record);
        complete = complete.saturating_add(u64::try_from(read).unwrap_or(u64::MAX));
    }
    if records.is_empty() {
        return Err(SessionError::Invalid(text::INVALID_FORMAT));
    }
    Ok((records, complete))
}

pub(super) fn check_format(value: &serde_json::Value) -> Result<(), SessionError> {
    if value.get("type").and_then(serde_json::Value::as_str) != Some("header") {
        return Err(SessionError::Invalid(text::INVALID_FORMAT));
    }
    let version = value
        .get("format")
        .and_then(serde_json::Value::as_u64)
        .ok_or(SessionError::Invalid(text::INVALID_FORMAT))?;
    if version != u64::from(FORMAT_VERSION) {
        return Err(SessionError::Invalid(text::UNSUPPORTED_FORMAT));
    }
    Ok(())
}

/// 最後の圧縮を起点に会話を復元し、結果のない呼び出しを自動で再実行せず補う。
pub fn replay(directory: &Path) -> Result<Conversation, SessionError> {
    let records = read_records(directory)?;
    Ok(replay_records(&records))
}

pub(super) fn replay_records(records: &[Record]) -> Conversation {
    let mut conversation = Conversation::default();
    let mut starts = BTreeSet::new();
    for record in records {
        match record {
            Record::User { message, .. } => {
                conversation.items.push(Item::UserMessage(message.clone()))
            }
            Record::Assistant { response, .. } => conversation
                .items
                .push(Item::AssistantTurn(response.clone())),
            Record::ToolStart { call } => {
                starts.insert(call.id.clone());
            }
            Record::ToolResult { result, .. } => {
                conversation.items.push(Item::ToolResult(result.clone()))
            }
            Record::ImagesRemoved { results } => {
                crate::provider::remove_images(&mut conversation, Some(results));
            }
            Record::Compaction {
                conversation: compacted,
                ..
            } => {
                conversation = compacted.clone();
                starts.clear();
            }
            Record::Rewind { items, .. } => {
                conversation.items.truncate(*items);
                starts.clear();
            }
            Record::Checkpoint(_)
            | Record::Header(_)
            | Record::SessionStart { .. }
            | Record::TurnStart { .. }
            | Record::ToolCall { .. }
            | Record::Approval { .. }
            | Record::Retry { .. }
            | Record::Error { .. }
            | Record::TurnEnd { .. }
            | Record::SessionEnd { .. }
            | Record::Summary { .. }
            | Record::ProjectDirChanged { .. }
            | Record::WriteTarget { .. }
            | Record::Spilled { .. } => {}
        }
    }
    fill_missing(conversation, &starts)
}

fn fill_missing(conversation: Conversation, starts: &BTreeSet<String>) -> Conversation {
    let known = conversation
        .items
        .iter()
        .filter_map(|item| match item {
            Item::ToolResult(result) => Some((result.id.clone(), result.clone())),
            Item::UserMessage(_) | Item::AssistantTurn(_) => None,
        })
        .collect::<std::collections::BTreeMap<_, _>>();
    let mut rebuilt = Conversation::default();
    let mut pending = Vec::<ToolCall>::new();
    let mut emitted = BTreeSet::new();
    for item in conversation.items {
        match item {
            Item::ToolResult(result) => {
                pending.retain(|call| call.id != result.id);
                if emitted.insert(result.id.clone()) {
                    rebuilt.items.push(Item::ToolResult(result));
                }
            }
            Item::UserMessage(message) => {
                finish_calls(&mut rebuilt, &mut pending, starts, &known, &mut emitted);
                rebuilt.items.push(Item::UserMessage(message));
            }
            Item::AssistantTurn(response) => {
                finish_calls(&mut rebuilt, &mut pending, starts, &known, &mut emitted);
                pending.extend(response.parts.iter().filter_map(|part| match part {
                    AssistantPart::ToolCall(call) => Some(call.clone()),
                    AssistantPart::Text(_) | AssistantPart::Reasoning(_) => None,
                }));
                rebuilt.items.push(Item::AssistantTurn(response));
            }
        }
    }
    finish_calls(&mut rebuilt, &mut pending, starts, &known, &mut emitted);
    rebuilt
}

fn finish_calls(
    conversation: &mut Conversation,
    calls: &mut Vec<ToolCall>,
    starts: &BTreeSet<String>,
    known: &std::collections::BTreeMap<String, ToolResult>,
    emitted: &mut BTreeSet<String>,
) {
    for call in calls.drain(..) {
        if !emitted.insert(call.id.clone()) {
            continue;
        }
        if let Some(result) = known.get(&call.id) {
            conversation.items.push(Item::ToolResult(result.clone()));
            continue;
        }
        let result = ToolResult {
            text: if starts.contains(&call.id) {
                text::INTERRUPTED
            } else {
                text::CANCELLED
            }
            .to_owned(),
            id: call.id,
            provider_id: call.provider_id,
            is_error: true,
            images: Vec::new(),
        };
        conversation.items.push(Item::ToolResult(result));
    }
}
