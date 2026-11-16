//! 形式の版 2 の記録と集計（設計書 01-05「セッションの記録」「LLM を使ったテストのための記録」）。

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::Value;

use crate::approval::{ApprovalFacts, ApprovalKind};
use crate::config::Mode;
use crate::provider::{
    AdapterKind, AssistantPart, AssistantTurn, Conversation, FinishReason, Item, ProviderError,
    ToolCall, ToolResult, Usage, UserMessage,
};

/// 記録ファイルへ書く `Value` 木の最大深さ。Record の外枠を加えても
/// `serde_json` の既定の再帰上限（128）に十分収まる値を保つ（設計書 01-05、BH-101）。
pub(crate) const MAX_RECORD_VALUE_DEPTH: usize = 64;
// 本変更より前に書かれた、SSE 上限 128 由来の深い既成行を読み直す安全上限。
const MAX_LEGACY_JSON_DEPTH: usize = 192;

/// 再開用の記録の形式。利用者向けイベントの形式とは別に管理する。
pub const FORMAT_VERSION: u32 = 2;

/// 認証情報を持たない提供者の識別。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RecordedProvider {
    pub name: String,
    pub base_url: String,
    pub model: String,
    pub adapter: AdapterKind,
}

/// 起動時に検査した処理系の実のパス、ハッシュ、版とビルドの言明。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RecordedBenitoite {
    #[serde(default)]
    pub sha256: String,
    #[serde(default)]
    pub build: serde_json::Value,
    #[serde(default)]
    pub skill_content_hash: Option<String>,
    pub path: PathBuf,
    pub version: String,
}

/// 設定の型を変更せず記録へ写すモード。
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecordedMode {
    Restricted,
    Normal,
}

impl From<Mode> for RecordedMode {
    fn from(value: Mode) -> Self {
        match value {
            Mode::Restricted => Self::Restricted,
            Mode::Normal => Self::Normal,
        }
    }
}

/// Skill を見つけた場所の分類。
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SkillSource {
    /// 利用者のホームの中。
    User,
    /// プロジェクトの探索範囲の中。
    Project,
}

/// 起動時に読んだ Skill の識別情報。全文は記録しない。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct RecordedSkill {
    pub name: String,
    pub path: PathBuf,
    pub sha256: String,
    pub source: SkillSource,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plugin: Option<String>,
}

/// 起動時に検査した Agent Plugins の識別情報。部品の内容は記録しない。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct RecordedPlugin {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    pub path: PathBuf,
    pub sha256: String,
    pub source: SkillSource,
}

/// 圧縮の実効値の記録。後の版の欄を読めるよう、設定の未知の欄の検査とは分ける。
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct RecordedCompaction {
    /// 出力の上限へ足す予備。
    pub reserve_tokens: u64,
    /// 残す会話の目安。
    pub keep_tokens: u64,
    /// 道具の結果の刈り込み。既定の偽は書かない。
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub prune_tool_results: bool,
}

impl From<crate::config::CompactionConfig> for RecordedCompaction {
    fn from(config: crate::config::CompactionConfig) -> Self {
        Self {
            reserve_tokens: config.reserve_tokens,
            keep_tokens: config.keep_tokens,
            prune_tool_results: config.prune_tool_results,
        }
    }
}

/// 刈り込んだ道具の結果。再生で同じ文に替えるため、刈り込んだ後の文を持つ。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PrunedResult {
    /// 道具の結果の内部の ID。
    pub id: String,
    /// 刈り込んだ後の文。
    pub text: String,
}

/// 最初の行。キーや HTTP のヘッダを受け取る欄を持たない。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Header {
    pub format: u32,
    pub session_id: String,
    pub started_at: String,
    pub benitoiteh_version: String,
    pub mode: RecordedMode,
    pub project_dir: PathBuf,
    pub provider: RecordedProvider,
    pub benitoite: Option<RecordedBenitoite>,
    pub system_prompt_sha256: String,
    pub tools_sha256: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub after_write_check: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completion_check: Option<bool>,
    /// 起動時の実効値。省略は既定値で、再開では現在の利用者の設定を使う。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compaction: Option<RecordedCompaction>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub read_line_numbers: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub multiple_edits: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nested_instructions: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plan_tool: Option<bool>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub skills: Vec<RecordedSkill>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub plugins: Vec<RecordedPlugin>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub forked_from: Option<ForkedFrom>,
}

/// 新しいセッションへ写した道の元の ID と先端。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ForkedFrom {
    #[serde(deserialize_with = "fork_session_id")]
    pub session: String,
    pub node: u64,
}

fn fork_session_id<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<String, D::Error> {
    let value = String::deserialize(deserializer)?;
    value
        .parse::<super::SessionId>()
        .map_err(|_| serde::de::Error::custom(super::text::INVALID_FORMAT))?;
    Ok(value)
}

/// 利用者の入力を記録するときの用途（操縦の綴りは記録では steer）。
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UserKind {
    Request,
    Steer,
    Environment,
}

/// 確認の要求と結果を区別する。
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalPhase {
    Request,
    Result,
}

/// 確認の結果。セッションを再開しても承認の状態には戻さない。
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalOutcome {
    Approved,
    ApprovedForSession,
    Denied,
    DeniedWithMessage,
    Cancelled,
}

/// 非対話の実行の集計。数を足す責任はループ、最後に記録する責任は exec が持つ。
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Summary {
    pub turns: u64,
    #[serde(default)]
    pub compactions: u64,
    pub tool_calls: BTreeMap<String, u64>,
    pub checks: u64,
    #[serde(default)]
    pub after_write_checks: u64,
    pub fixes: u64,
    pub approvals: ApprovalCounts,
    pub tokens: TokenCounts,
    pub elapsed_ms: u64,
    pub reason: String,
}

/// 確認の結果の内訳。
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ApprovalCounts {
    pub approved: u64,
    pub denied: u64,
    pub cancelled: u64,
    #[serde(default)]
    pub policy_denied: u64,
}

/// ループが応答ごとに足す、報告済みトークンの合計。
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct TokenCounts {
    pub input: u64,
    pub cache_read: u64,
    pub cache_write: u64,
    pub output: u64,
}

impl Summary {
    /// 応答を数え、報告された量だけ合計する。
    pub fn add_turn(&mut self, usage: &Usage) {
        self.turns = self.turns.saturating_add(1);
        self.add_usage(usage);
    }

    /// 要約の要求など、番を増やさずに使用量だけを足す。
    pub fn add_usage(&mut self, usage: &Usage) {
        self.tokens.input = self
            .tokens
            .input
            .saturating_add(usage.input_tokens.unwrap_or(0));
        self.tokens.cache_read = self
            .tokens
            .cache_read
            .saturating_add(usage.cache_read_tokens.unwrap_or(0));
        self.tokens.cache_write = self
            .tokens
            .cache_write
            .saturating_add(usage.cache_write_tokens.unwrap_or(0));
        self.tokens.output = self
            .tokens
            .output
            .saturating_add(usage.output_tokens.unwrap_or(0));
    }

    /// 検証後の呼び出しを道具ごとに数える。検査・修正の区別はループが数える。
    pub fn add_tool_call(&mut self, name: &str) {
        let count = self.tool_calls.entry(name.to_owned()).or_default();
        *count = count.saturating_add(1);
    }

    /// 確認の結果を数える。
    pub fn add_approval(&mut self, outcome: ApprovalOutcome) {
        let count = match outcome {
            ApprovalOutcome::Approved | ApprovalOutcome::ApprovedForSession => {
                &mut self.approvals.approved
            }
            ApprovalOutcome::Denied | ApprovalOutcome::DeniedWithMessage => {
                &mut self.approvals.denied
            }
            ApprovalOutcome::Cancelled => &mut self.approvals.cancelled,
        };
        *count = count.saturating_add(1);
    }
}

/// 道具の出力の保存先と、上限で捨てた量。パスはセッションからの相対。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SpillInfo {
    pub path: PathBuf,
    pub written_bytes: u64,
    pub discarded_bytes: u64,
}

/// 読まなかった指示の探索時のパスと理由。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SkippedInstruction {
    /// 探索時の絶対パス。
    pub path: PathBuf,
    /// 利用者へ示す除外理由。
    pub reason: String,
}

/// 一行ずつ追記する記録。ストリームの差分や認証の型には依存しない。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Record {
    /// 必ず最初に書くヘッダ。
    Header(Header),
    /// 画面にも示す開始の通知。ヘッダとは別の行。
    SessionStart {
        /// この段の実効値。旧記録にはない。新しい記録では既定値でも残す。
        #[serde(default, skip_serializing_if = "Option::is_none")]
        compaction: Option<RecordedCompaction>,
        /// この段の完了の促し。旧記録の欠落と無効を区別する。
        #[serde(default, skip_serializing_if = "Option::is_none")]
        completion_check: Option<bool>,
        session_id: String,
        #[serde(default)]
        resumed: bool,
        #[serde(default)]
        base_url: Option<String>,
        #[serde(default)]
        system_prompt_sha256: Option<String>,
        #[serde(default)]
        tools_sha256: Option<String>,
        mode: RecordedMode,
        provider: String,
        model: String,
        host: String,
        benitoiteh_version: String,
        notice: Option<String>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        instruction_files: Vec<PathBuf>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        skipped_instructions: Vec<SkippedInstruction>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        plugins: Vec<RecordedPlugin>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        skipped_plugins: Vec<SkippedInstruction>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        plugin_notices: Vec<SkippedInstruction>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        skills: Vec<RecordedSkill>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        skipped_skills: Vec<SkippedInstruction>,
    },
    /// 番の開始。
    TurnStart { turn: u64 },
    /// 一つの UserMessage を分割せず保存する。
    User {
        node: u64,
        kind: UserKind,
        message: UserMessage,
    },
    /// RawItems を含む、受け取りを終えた応答。
    Assistant {
        node: u64,
        turn: u64,
        response: AssistantTurn,
    },
    /// 応答の後で記録する検証の結果。None は検証を通ったことを表す。
    ToolCall {
        turn: u64,
        call: ToolCall,
        validation_error: Option<String>,
    },
    /// 子プロセスまたは書く道具を実行する直前の記録。
    ToolStart {
        #[serde(flatten)]
        call: ToolCall,
    },
    /// 書く直前に確定した実のパス。後のリンクの差し替えに左右されず効果を分類する。
    WriteTarget { call_id: String, real_path: PathBuf },
    /// LLM に渡した結果と、全体または先頭を保存した場所。
    ToolResult {
        node: u64,
        turn: u64,
        name: String,
        result: ToolResult,
        spill: Option<SpillInfo>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        stderr_spill: Option<SpillInfo>,
    },
    /// 確認の要求または結果。追加の欄がない形式の版 2 の記録も読む。
    Approval {
        phase: ApprovalPhase,
        id: String,
        tool_id: String,
        request: Option<String>,
        outcome: Option<ApprovalOutcome>,
        #[serde(default)]
        kind: Option<ApprovalKind>,
        #[serde(default)]
        facts: ApprovalFacts,
        #[serde(default)]
        justification: Option<String>,
        #[serde(default)]
        user_message: Option<String>,
    },
    /// 画像を外した送り直しの成功。結果の内部 ID で、以後の再生でも画像を除く。
    ImagesRemoved { node: u64, results: Vec<String> },
    /// 圧縮の前の道具の結果の刈り込み（設計書 01-04「道具の結果の刈り込み」）。
    /// 以後の再生でも、同じ ID の結果の文を刈り込んだ文に替える。
    ToolResultsPruned {
        node: u64,
        results: Vec<PrunedResult>,
    },
    /// やり直しの分類と待ち時間。
    Retry {
        attempt: u64,
        delay_ms: u64,
        error: ProviderError,
    },
    /// 認証情報を含めない利用者向けの診断。
    Error { kind: String, message: String },
    /// 番で報告されたトークンの量。
    TurnEnd {
        turn: u64,
        finish_reason: FinishReason,
        usage: Usage,
    },
    /// 実行の終わり方。
    SessionEnd { outcome: String, reason: String },
    /// 非対話の実行では session_end の後の最後の行。
    Summary {
        #[serde(flatten)]
        summary: Summary,
    },
    /// 後の段階で使う、プロジェクトの変更。
    ProjectDirChanged { project_dir: PathBuf },
    /// 今の先端を既存の節点へ移し、枝を残す。
    Branch {
        to: Option<u64>,
        from: Option<u64>,
        checkpoint: Option<String>,
    },
    /// 道具または依頼の前のファイルの時点。
    Checkpoint(crate::rewind::Checkpoint),
    /// 圧縮後の会話を起点として保持する。後続の行だけをこの後へ加える。
    Compaction {
        node: u64,
        conversation: Conversation,
        #[serde(default)]
        reason: CompactionReason,
        #[serde(default)]
        summarized_items: usize,
        #[serde(default)]
        kept_items: usize,
        /// この圧縮の要約の要求とやり直しだけの使用量。
        #[serde(default, skip_serializing_if = "Option::is_none")]
        usage: Option<Usage>,
        /// 再構築前に選んだ範囲の見積もり。推論を除いた送信量より多くなりうる。
        #[serde(default, skip_serializing_if = "Option::is_none")]
        kept_estimate: Option<u64>,
    },
    /// 行の上限を超えた記録の JSON の分割保存先。読み手が元の Record に戻す。
    Spilled { parts: Vec<PathBuf> },
}

/// 記録とイベントで共有する圧縮の理由。
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompactionReason {
    /// 番の前の見積もりが閾値を超えた。
    #[default]
    Threshold,
    /// 利用者が圧縮を求めた。
    Manual,
    /// 提供者が文脈の超過を返した。
    Overflow,
    /// 再開で推論を引き継げない会話を要約する。
    Resume,
}

impl CompactionReason {
    /// 画面にも記録と同じ綴りを示す。
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Threshold => "threshold",
            Self::Manual => "manual",
            Self::Overflow => "overflow",
            Self::Resume => "resume",
        }
    }
}

impl Record {
    /// 会話を作る節点の番号。制御とファイルの記録は節点を持たない。
    pub fn node(&self) -> Option<u64> {
        match self {
            Self::User { node, .. }
            | Self::Assistant { node, .. }
            | Self::ToolResult { node, .. }
            | Self::Compaction { node, .. }
            | Self::ImagesRemoved { node, .. }
            | Self::ToolResultsPruned { node, .. } => Some(*node),
            Self::Header(_)
            | Self::SessionStart { .. }
            | Self::TurnStart { .. }
            | Self::ToolCall { .. }
            | Self::ToolStart { .. }
            | Self::WriteTarget { .. }
            | Self::Approval { .. }
            | Self::Retry { .. }
            | Self::Error { .. }
            | Self::TurnEnd { .. }
            | Self::SessionEnd { .. }
            | Self::Summary { .. }
            | Self::ProjectDirChanged { .. }
            | Self::Branch { .. }
            | Self::Checkpoint(_)
            | Self::Spilled { .. } => None,
        }
    }

    pub(crate) fn set_node(&mut self, value: u64) {
        match self {
            Self::User { node, .. }
            | Self::Assistant { node, .. }
            | Self::ToolResult { node, .. }
            | Self::Compaction { node, .. }
            | Self::ImagesRemoved { node, .. }
            | Self::ToolResultsPruned { node, .. } => *node = value,
            Self::Header(_)
            | Self::SessionStart { .. }
            | Self::TurnStart { .. }
            | Self::ToolCall { .. }
            | Self::ToolStart { .. }
            | Self::WriteTarget { .. }
            | Self::Approval { .. }
            | Self::Retry { .. }
            | Self::Error { .. }
            | Self::TurnEnd { .. }
            | Self::SessionEnd { .. }
            | Self::Summary { .. }
            | Self::ProjectDirChanged { .. }
            | Self::Branch { .. }
            | Self::Checkpoint(_)
            | Self::Spilled { .. } => {}
        }
    }
}

/// 値の入れ子の深さ（Object / Array の最大階層）を反復で数える。
pub(crate) fn value_depth(value: &Value) -> usize {
    let mut max = 0_usize;
    let mut stack = vec![(value, 0_usize)];
    while let Some((current, depth)) = stack.pop() {
        match current {
            Value::Object(fields) => {
                let next = depth.saturating_add(1);
                max = max.max(next);
                for child in fields.values() {
                    stack.push((child, next));
                }
            }
            Value::Array(items) => {
                let next = depth.saturating_add(1);
                max = max.max(next);
                for child in items {
                    stack.push((child, next));
                }
            }
            Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {}
        }
    }
    max
}

fn prune_deep_value(value: &mut Value, max_depth: usize) {
    fn prune_at(value: &mut Value, depth: usize, max_depth: usize) {
        match value {
            Value::Object(_) | Value::Array(_) if depth >= max_depth => {
                *value = Value::Null;
            }
            Value::Object(fields) => {
                let next = depth.saturating_add(1);
                for child in fields.values_mut() {
                    prune_at(child, next, max_depth);
                }
            }
            Value::Array(items) => {
                let next = depth.saturating_add(1);
                for child in items.iter_mut() {
                    prune_at(child, next, max_depth);
                }
            }
            Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {}
        }
    }
    prune_at(value, 0, max_depth);
}

/// 深い `usage.raw` を落として既知の使用量欄だけを保つ（設計書 01-05、BH-101）。
pub(crate) fn normalize_usage(usage: &mut Usage) {
    if value_depth(&usage.raw) > MAX_RECORD_VALUE_DEPTH {
        usage.raw = Value::Null;
    }
}

fn sanitize_call(call: &mut ToolCall) {
    if !call.raw_arguments.is_empty() {
        // 元の引数文字列から再開時に復元できるため、深い木になり得る構造化値は書かない。
        call.arguments = Value::Null;
    } else if value_depth(&call.arguments) > MAX_RECORD_VALUE_DEPTH {
        call.raw_arguments = call.arguments.to_string();
        call.arguments = Value::Null;
    }
}

fn sanitize_turn(turn: &mut AssistantTurn) {
    if turn.raw_items.adapter == AdapterKind::AnthropicMessages {
        for item in &mut turn.raw_items.items {
            if let Value::Object(fields) = item
                && fields.get("type").and_then(Value::as_str) == Some("tool_use")
                && fields
                    .get("input")
                    .is_some_and(|input| value_depth(input) > MAX_RECORD_VALUE_DEPTH)
                && let Some(id) = fields.get("id").and_then(Value::as_str)
                && turn.parts.iter().any(|part| {
                    matches!(
                        part,
                        AssistantPart::ToolCall(call)
                            if call.provider_id.as_deref() == Some(id) && !call.raw_arguments.is_empty()
                    )
                })
            {
                fields.remove("input");
            }
        }
    }
    for part in &mut turn.parts {
        match part {
            AssistantPart::Text(_) => {}
            AssistantPart::Reasoning(reasoning) => {
                prune_deep_value(&mut reasoning.opaque, MAX_RECORD_VALUE_DEPTH);
            }
            AssistantPart::ToolCall(call) => sanitize_call(call),
        }
    }
    for item in &mut turn.raw_items.items {
        prune_deep_value(item, MAX_RECORD_VALUE_DEPTH);
    }
    normalize_usage(&mut turn.usage);
}

pub(super) fn sanitize_record(record: &Record) -> Record {
    let mut sanitized = record.clone();
    match &mut sanitized {
        Record::Header(header) => {
            if let Some(benitoite) = &mut header.benitoite {
                prune_deep_value(&mut benitoite.build, MAX_RECORD_VALUE_DEPTH);
            }
        }
        Record::Assistant { response, .. } => sanitize_turn(response),
        Record::ToolCall { call, .. } | Record::ToolStart { call } => sanitize_call(call),
        Record::TurnEnd { usage, .. } => normalize_usage(usage),
        Record::Compaction {
            conversation,
            usage,
            ..
        } => {
            for item in &mut conversation.items {
                match item {
                    Item::AssistantTurn(turn) => sanitize_turn(turn),
                    Item::UserMessage(_) | Item::ToolResult(_) => {}
                }
            }
            if let Some(usage) = usage {
                normalize_usage(usage);
            }
        }
        Record::SessionStart { .. }
        | Record::TurnStart { .. }
        | Record::User { .. }
        | Record::WriteTarget { .. }
        | Record::Approval { .. }
        | Record::ToolResult { .. }
        | Record::ImagesRemoved { .. }
        | Record::ToolResultsPruned { .. }
        | Record::Retry { .. }
        | Record::Error { .. }
        | Record::SessionEnd { .. }
        | Record::Summary { .. }
        | Record::ProjectDirChanged { .. }
        | Record::Branch { .. }
        | Record::Checkpoint(_)
        | Record::Spilled { .. } => {}
    }
    sanitized
}

fn restore_call(call: &mut ToolCall) {
    if call.arguments.is_null() && !call.raw_arguments.is_empty() {
        call.arguments = serde_json::from_str(&call.raw_arguments).unwrap_or(Value::Null);
    }
}

fn restore_turn(turn: &mut AssistantTurn) {
    for part in &mut turn.parts {
        match part {
            AssistantPart::ToolCall(call) => restore_call(call),
            AssistantPart::Text(_) | AssistantPart::Reasoning(_) => {}
        }
    }
    if turn.raw_items.adapter == AdapterKind::AnthropicMessages {
        for item in &mut turn.raw_items.items {
            if let Value::Object(fields) = item
                && fields.get("type").and_then(Value::as_str) == Some("tool_use")
                && !fields.contains_key("input")
                && let Some(id) = fields.get("id").and_then(Value::as_str)
                && let Some(raw_arguments) = turn.parts.iter().find_map(|part| match part {
                    AssistantPart::ToolCall(call)
                        if call.provider_id.as_deref() == Some(id)
                            && !call.raw_arguments.is_empty() =>
                    {
                        Some(call.raw_arguments.as_str())
                    }
                    AssistantPart::Text(_)
                    | AssistantPart::Reasoning(_)
                    | AssistantPart::ToolCall(_) => None,
                })
            {
                let input = serde_json::from_str::<Value>(raw_arguments)
                    .unwrap_or_else(|_| serde_json::json!({}));
                fields.insert("input".to_owned(), input);
            }
        }
    }
}

pub(super) fn restore_record(record: &mut Record) {
    match record {
        Record::Assistant { response, .. } => restore_turn(response),
        Record::ToolCall { call, .. } | Record::ToolStart { call } => restore_call(call),
        Record::Compaction { conversation, .. } => {
            for item in &mut conversation.items {
                match item {
                    Item::AssistantTurn(turn) => restore_turn(turn),
                    Item::UserMessage(_) | Item::ToolResult(_) => {}
                }
            }
        }
        Record::Header(_)
        | Record::SessionStart { .. }
        | Record::TurnStart { .. }
        | Record::User { .. }
        | Record::WriteTarget { .. }
        | Record::Approval { .. }
        | Record::ToolResult { .. }
        | Record::ImagesRemoved { .. }
        | Record::ToolResultsPruned { .. }
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

fn json_nesting_depth(bytes: &[u8]) -> usize {
    let mut depth = 0_usize;
    let mut max = 0_usize;
    let mut in_string = false;
    let mut escaped = false;
    for &byte in bytes {
        if in_string {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                in_string = false;
            }
            continue;
        }
        match byte {
            b'"' => in_string = true,
            b'{' | b'[' => {
                depth = depth.saturating_add(1);
                max = max.max(depth);
            }
            b'}' | b']' => {
                depth = depth.saturating_sub(1);
            }
            _ => {}
        }
    }
    max
}

fn skip_ws(bytes: &[u8], pos: &mut usize) {
    while matches!(bytes.get(*pos), Some(b' ' | b'\t' | b'\n' | b'\r')) {
        *pos = pos.saturating_add(1);
    }
}

fn scan_string(bytes: &[u8], pos: &mut usize) -> serde_json::Result<String> {
    let start = *pos;
    if bytes.get(start) != Some(&b'"') {
        return Err(serde::de::Error::custom("expected JSON string"));
    }
    *pos = start.saturating_add(1);
    let mut escaped = false;
    while let Some(&byte) = bytes.get(*pos) {
        *pos = pos.saturating_add(1);
        if escaped {
            escaped = false;
        } else if byte == b'\\' {
            escaped = true;
        } else if byte == b'"' {
            let slice = bytes
                .get(start..*pos)
                .ok_or_else(|| serde::de::Error::custom("invalid JSON string slice"))?;
            return serde_json::from_slice(slice);
        }
    }
    Err(serde::de::Error::custom("unterminated JSON string"))
}

fn scan_scalar(bytes: &[u8], pos: &mut usize) -> serde_json::Result<Value> {
    let start = *pos;
    while let Some(&byte) = bytes.get(*pos) {
        if matches!(byte, b' ' | b'\t' | b'\n' | b'\r' | b',' | b'}' | b']') {
            break;
        }
        *pos = pos.saturating_add(1);
    }
    if *pos == start {
        return Err(serde::de::Error::custom("expected JSON value"));
    }
    let slice = bytes
        .get(start..*pos)
        .ok_or_else(|| serde::de::Error::custom("invalid JSON scalar slice"))?;
    serde_json::from_slice(slice)
}

enum ObjectState {
    KeyOrEnd { first: bool },
    Value(String),
    CommaOrEnd,
}

enum DeepFrame {
    Array {
        items: Vec<Value>,
        comma_or_end: bool,
    },
    Object {
        map: serde_json::Map<String, Value>,
        state: ObjectState,
    },
}

fn deliver_value(
    stack: &mut [DeepFrame],
    root: &mut Option<Value>,
    value: Value,
) -> serde_json::Result<()> {
    match stack.last_mut() {
        Some(DeepFrame::Array {
            items,
            comma_or_end,
        }) => {
            items.push(value);
            *comma_or_end = true;
            Ok(())
        }
        Some(DeepFrame::Object { map, state }) => {
            let ObjectState::Value(key) = std::mem::replace(state, ObjectState::CommaOrEnd) else {
                return Err(serde::de::Error::custom("unexpected JSON value in object"));
            };
            map.insert(key, value);
            Ok(())
        }
        None => {
            *root = Some(value);
            Ok(())
        }
    }
}

fn parse_deep_value(bytes: &[u8]) -> serde_json::Result<Value> {
    let mut stack = Vec::new();
    let mut root: Option<Value> = None;
    let mut pos = 0_usize;

    loop {
        if stack.is_empty()
            && let Some(value) = root.take()
        {
            skip_ws(bytes, &mut pos);
            if pos == bytes.len() {
                return Ok(value);
            }
            return Err(serde::de::Error::custom("trailing JSON characters"));
        }

        match stack.last_mut() {
            Some(DeepFrame::Array {
                comma_or_end: true, ..
            }) => {
                skip_ws(bytes, &mut pos);
                match bytes.get(pos) {
                    Some(b',') => {
                        pos = pos.saturating_add(1);
                        if let Some(DeepFrame::Array { comma_or_end, .. }) = stack.last_mut() {
                            *comma_or_end = false;
                        }
                    }
                    Some(b']') => {
                        pos = pos.saturating_add(1);
                        if let Some(DeepFrame::Array { items, .. }) = stack.pop() {
                            deliver_value(&mut stack, &mut root, Value::Array(items))?;
                        }
                        continue;
                    }
                    _ => return Err(serde::de::Error::custom("expected ',' or ']' in array")),
                }
            }
            Some(DeepFrame::Array {
                items,
                comma_or_end: false,
            }) => {
                skip_ws(bytes, &mut pos);
                if items.is_empty() && bytes.get(pos) == Some(&b']') {
                    pos = pos.saturating_add(1);
                    if let Some(DeepFrame::Array { items, .. }) = stack.pop() {
                        deliver_value(&mut stack, &mut root, Value::Array(items))?;
                    }
                    continue;
                }
            }
            Some(DeepFrame::Object {
                state: ObjectState::KeyOrEnd { first },
                ..
            }) => {
                let first = *first;
                skip_ws(bytes, &mut pos);
                match bytes.get(pos) {
                    Some(b'}') if first => {
                        pos = pos.saturating_add(1);
                        if let Some(DeepFrame::Object { map, .. }) = stack.pop() {
                            deliver_value(&mut stack, &mut root, Value::Object(map))?;
                        }
                        continue;
                    }
                    Some(b'"') => {
                        let key = scan_string(bytes, &mut pos)?;
                        skip_ws(bytes, &mut pos);
                        if bytes.get(pos) != Some(&b':') {
                            return Err(serde::de::Error::custom("expected ':' after object key"));
                        }
                        pos = pos.saturating_add(1);
                        if let Some(DeepFrame::Object { state, .. }) = stack.last_mut() {
                            *state = ObjectState::Value(key);
                        }
                    }
                    _ => return Err(serde::de::Error::custom("expected object key or '}'")),
                }
            }
            Some(DeepFrame::Object {
                state: ObjectState::CommaOrEnd,
                ..
            }) => {
                skip_ws(bytes, &mut pos);
                match bytes.get(pos) {
                    Some(b',') => {
                        pos = pos.saturating_add(1);
                        if let Some(DeepFrame::Object { state, .. }) = stack.last_mut() {
                            *state = ObjectState::KeyOrEnd { first: false };
                        }
                        continue;
                    }
                    Some(b'}') => {
                        pos = pos.saturating_add(1);
                        if let Some(DeepFrame::Object { map, .. }) = stack.pop() {
                            deliver_value(&mut stack, &mut root, Value::Object(map))?;
                        }
                        continue;
                    }
                    _ => return Err(serde::de::Error::custom("expected ',' or '}' in object")),
                }
            }
            Some(DeepFrame::Object {
                state: ObjectState::Value(_),
                ..
            })
            | None => {}
        }

        skip_ws(bytes, &mut pos);
        match bytes.get(pos) {
            Some(b'{') => {
                if stack.len() >= MAX_LEGACY_JSON_DEPTH {
                    return Err(serde::de::Error::custom("JSON recursion limit exceeded"));
                }
                pos = pos.saturating_add(1);
                stack.push(DeepFrame::Object {
                    map: serde_json::Map::new(),
                    state: ObjectState::KeyOrEnd { first: true },
                });
            }
            Some(b'[') => {
                if stack.len() >= MAX_LEGACY_JSON_DEPTH {
                    return Err(serde::de::Error::custom("JSON recursion limit exceeded"));
                }
                pos = pos.saturating_add(1);
                stack.push(DeepFrame::Array {
                    items: Vec::new(),
                    comma_or_end: false,
                });
            }
            Some(b'"') => {
                let text = scan_string(bytes, &mut pos)?;
                deliver_value(&mut stack, &mut root, Value::String(text))?;
            }
            _ => {
                let value = scan_scalar(bytes, &mut pos)?;
                deliver_value(&mut stack, &mut root, value)?;
            }
        }
    }
}

pub(super) fn deserialize_json<T: DeserializeOwned>(bytes: &[u8]) -> serde_json::Result<T> {
    let depth = json_nesting_depth(bytes);
    if depth <= 120 || depth > MAX_LEGACY_JSON_DEPTH {
        serde_json::from_slice(bytes)
    } else {
        serde_json::from_value(parse_deep_value(bytes)?)
    }
}
