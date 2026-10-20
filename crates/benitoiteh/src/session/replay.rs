//! 途中で切れた最終行の破棄と、未完の呼び出しの結果の補完（設計書 01-05「再開」）。

use std::collections::{BTreeMap, BTreeSet};
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
    Ok(super::ResumeState::from_records(records)?.conversation)
}

/// 記録の順から親を確定した、セッションの木。
#[derive(Clone, Debug)]
pub struct SessionTree {
    pub tip: Option<u64>,
    pub next_node: u64,
    nodes: BTreeMap<u64, (Option<u64>, usize)>,
    checkpoint_requests: BTreeMap<String, u64>,
    request_checkpoints: BTreeMap<u64, String>,
    branch_checkpoints: BTreeMap<u64, Option<String>>,
    path_nodes: BTreeSet<u64>,
    pending_request: Option<(String, Option<u64>)>,
}

impl SessionTree {
    /// 前方参照と連番の逆行を拒み、空の会話も許す。
    pub fn from_records(records: &[Record]) -> Result<Self, SessionError> {
        let mut tree = Self {
            tip: None,
            next_node: 1,
            nodes: BTreeMap::new(),
            checkpoint_requests: BTreeMap::new(),
            request_checkpoints: BTreeMap::new(),
            branch_checkpoints: BTreeMap::new(),
            path_nodes: BTreeSet::new(),
            pending_request: None,
        };
        for (index, record) in records.iter().enumerate() {
            tree.index_record(record, index)?;
        }
        tree.path_nodes = tree.path(tree.tip).into_iter().collect();
        Ok(tree)
    }

    fn index_record(&mut self, record: &Record, index: usize) -> Result<(), SessionError> {
        if let Record::Checkpoint(point) = record
            && point.reason == crate::rewind::CheckpointReason::Request
        {
            // 依頼を書く前に止まった時点を、次の依頼へ結び付けない
            // （設計書 01-05「セッションの木」）。
            self.pending_request = Some((point.id.clone(), point.node));
        }
        if let Some(node) = record.node() {
            if node < self.next_node {
                return Err(SessionError::Invalid(text::INVALID_FORMAT));
            }
            self.next_node = node
                .checked_add(1)
                .ok_or(SessionError::Invalid(text::INVALID_FORMAT))?;
            if let Some((id, parent)) = self.pending_request.take()
                && parent == self.tip
                && request_text(record).is_some()
            {
                self.checkpoint_requests.insert(id.clone(), node);
                self.request_checkpoints.insert(node, id);
            }
            self.nodes.insert(node, (self.tip, index));
            self.tip = Some(node);
        }
        if let Record::Branch {
            to,
            from,
            checkpoint,
        } = record
        {
            if to
                .iter()
                .chain(from.iter())
                .any(|node| !self.nodes.contains_key(node))
            {
                return Err(SessionError::Invalid(text::INVALID_FORMAT));
            }
            if let Some(from) = from {
                self.branch_checkpoints.insert(*from, checkpoint.clone());
            }
            self.tip = *to;
        }
        Ok(())
    }

    /// 同じ操作が追記した記録を索引にも反映し、全体の読み直しを避ける。
    pub(crate) fn push_record(
        &mut self,
        record: &Record,
        index: usize,
    ) -> Result<(), SessionError> {
        self.index_record(record, index)?;
        if let Some(node) = record.node() {
            self.path_nodes.insert(node);
        }
        if matches!(record, Record::Branch { .. }) {
            self.path_nodes = self.path(self.tip).into_iter().collect();
        }
        Ok(())
    }

    /// 指定した先端までの節点を、会話の順で返す。親は必ず既出なので循環しない。
    pub fn path(&self, tip: Option<u64>) -> Vec<u64> {
        let mut result = Vec::new();
        let mut current = tip;
        while let Some(node) = current {
            let Some((parent, _)) = self.nodes.get(&node) else {
                break;
            };
            result.push(node);
            current = *parent;
        }
        result.reverse();
        result
    }

    /// 節点の親と記録上の位置。
    pub fn entry(&self, node: u64) -> Option<(Option<u64>, usize)> {
        self.nodes.get(&node).copied()
    }

    /// 節点の番号、親、記録の位置を番号順に列挙する。
    pub(crate) fn nodes(&self) -> impl Iterator<Item = (u64, Option<u64>, usize)> + '_ {
        self.nodes
            .iter()
            .map(|(node, (parent, index))| (*node, *parent, *index))
    }

    /// 子のない節点。枝の移動の先端の候補となる。
    pub fn leaves(&self) -> Vec<u64> {
        let parents = self
            .nodes
            .values()
            .filter_map(|(parent, _)| *parent)
            .collect::<BTreeSet<_>>();
        self.nodes
            .keys()
            .filter(|node| !parents.contains(node))
            .copied()
            .collect()
    }

    /// 依頼の直前の最後の Request の時点に対応する節点。
    pub fn checkpoint_request(&self, id: &str) -> Option<u64> {
        self.checkpoint_requests.get(id).copied()
    }

    /// 依頼の節点に対応する時点の ID。
    pub fn request_checkpoint(&self, node: u64) -> Option<&str> {
        self.request_checkpoints.get(&node).map(String::as_str)
    }

    /// その先端を最後に離れたときの時点。保存できなかった場合は None。
    pub fn branch_checkpoint(&self, node: u64) -> Option<&str> {
        self.branch_checkpoints
            .get(&node)
            .and_then(Option::as_deref)
    }

    /// 今の道の上かを、一覧の各時点で道を辿り直さずに判定する。
    pub fn path_nodes(&self) -> &BTreeSet<u64> {
        &self.path_nodes
    }
}

/// 環境の変化と操縦を除いた、依頼の全文。
pub fn request_text(record: &Record) -> Option<String> {
    request_text_ref(record).map(str::to_owned)
}

/// 表示の見出しを作るときに、依頼の全文を複製しない。
pub(crate) fn request_text_ref(record: &Record) -> Option<&str> {
    if let Record::User {
        kind: super::UserKind::Request,
        message,
        ..
    } = record
    {
        return message
            .parts
            .iter()
            .find(|part| part.kind == crate::provider::UserPartKind::Request)
            .map(|part| part.text.as_str());
    }
    None
}

pub(super) fn replay_records(mut records: Vec<Record>, tree: &SessionTree) -> Conversation {
    let mut conversation = Conversation::default();
    let starts = records
        .iter()
        .filter_map(|record| {
            if let Record::ToolStart { call } = record {
                Some(call.id.clone())
            } else {
                None
            }
        })
        .collect();
    for node in tree.path(tree.tip) {
        let Some((_, index)) = tree.entry(node) else {
            continue;
        };
        let Some(record) = records.get_mut(index) else {
            continue;
        };
        // 索引をずらさず、道の上の本文の所有権を移す。残った記録は復元後に使わない
        // （設計書 01-05「セッションの木」）。
        let record = std::mem::replace(record, Record::TurnStart { turn: 0 });
        match record {
            Record::User { message, .. } => conversation.items.push(Item::UserMessage(message)),
            Record::Assistant { response, .. } => {
                conversation.items.push(Item::AssistantTurn(response))
            }
            Record::ToolResult { result, .. } => conversation.items.push(Item::ToolResult(result)),
            Record::ImagesRemoved { results, .. } if !results.is_empty() => {
                crate::provider::remove_images(&mut conversation, Some(&results));
            }
            Record::Compaction {
                conversation: compacted,
                ..
            } => conversation = compacted,
            Record::Header(_)
            | Record::SessionStart { .. }
            | Record::TurnStart { .. }
            | Record::ToolCall { .. }
            | Record::ToolStart { .. }
            | Record::WriteTarget { .. }
            | Record::Approval { .. }
            | Record::ImagesRemoved { .. }
            | Record::Retry { .. }
            | Record::Error { .. }
            | Record::TurnEnd { .. }
            | Record::SessionEnd { .. }
            | Record::Summary { .. }
            | Record::ProjectDirChanged { .. }
            | Record::Branch { .. }
            | Record::Checkpoint(_)
            | Record::Spilled { .. } => {}
        }
    }
    fill_missing(conversation, &starts)
}

fn fill_missing(conversation: Conversation, starts: &BTreeSet<String>) -> Conversation {
    let known = conversation
        .items
        .iter()
        .enumerate()
        .filter_map(|(index, item)| match item {
            Item::ToolResult(result) => Some((result.id.clone(), index)),
            Item::UserMessage(_) | Item::AssistantTurn(_) => None,
        })
        .collect::<std::collections::BTreeMap<_, _>>();
    let mut rebuilt = Conversation::default();
    let mut pending = Vec::<ToolCall>::new();
    let mut emitted = BTreeSet::new();
    // 結果を先の位置から補う場合も、本文を複製せず一度だけ取り出す
    // （設計書 01-05「再開」）。
    let mut items = conversation.items.into_iter().map(Some).collect::<Vec<_>>();
    for index in 0..items.len() {
        let Some(item) = items.get_mut(index).and_then(Option::take) else {
            continue;
        };
        match item {
            Item::ToolResult(result) => {
                pending.retain(|call| call.id != result.id);
                if emitted.insert(result.id.clone()) {
                    rebuilt.items.push(Item::ToolResult(result));
                }
            }
            Item::UserMessage(message) => {
                finish_calls(
                    &mut rebuilt,
                    &mut pending,
                    starts,
                    &known,
                    &mut items,
                    &mut emitted,
                );
                rebuilt.items.push(Item::UserMessage(message));
            }
            Item::AssistantTurn(response) => {
                finish_calls(
                    &mut rebuilt,
                    &mut pending,
                    starts,
                    &known,
                    &mut items,
                    &mut emitted,
                );
                pending.extend(response.parts.iter().filter_map(|part| match part {
                    AssistantPart::ToolCall(call) => Some(call.clone()),
                    AssistantPart::Text(_) | AssistantPart::Reasoning(_) => None,
                }));
                rebuilt.items.push(Item::AssistantTurn(response));
            }
        }
    }
    finish_calls(
        &mut rebuilt,
        &mut pending,
        starts,
        &known,
        &mut items,
        &mut emitted,
    );
    rebuilt
}

fn finish_calls(
    conversation: &mut Conversation,
    calls: &mut Vec<ToolCall>,
    starts: &BTreeSet<String>,
    known: &std::collections::BTreeMap<String, usize>,
    items: &mut [Option<Item>],
    emitted: &mut BTreeSet<String>,
) {
    for call in calls.drain(..) {
        if !emitted.insert(call.id.clone()) {
            continue;
        }
        if let Some(result) = known
            .get(&call.id)
            .and_then(|index| items.get_mut(*index))
            .and_then(Option::take)
        {
            conversation.items.push(result);
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
