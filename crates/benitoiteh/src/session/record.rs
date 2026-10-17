//! 形式の版 1 の記録と集計（設計書 01-05「セッションの記録」「LLM を使ったテストのための記録」）。

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::config::Mode;
use crate::provider::{
    AdapterKind, AssistantTurn, Conversation, FinishReason, ProviderError, ToolCall, ToolResult,
    Usage, UserMessage,
};

/// 再開用の記録の形式。利用者向けイベントの形式とは別に管理する。
pub const FORMAT_VERSION: u32 = 1;

/// 認証情報を持たない提供者の識別。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RecordedProvider {
    pub name: String,
    pub base_url: String,
    pub model: String,
    pub adapter: AdapterKind,
}

/// P2 以降に記録する処理系の識別。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RecordedBenitoite {
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
    pub skill_version: String,
    pub system_prompt_sha256: String,
    pub tools_sha256: String,
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
    Denied,
    Cancelled,
}

/// 非対話の実行の集計。数を足す責任はループ、最後に記録する責任は exec が持つ。
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Summary {
    pub turns: u64,
    pub tool_calls: BTreeMap<String, u64>,
    pub checks: u64,
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
            ApprovalOutcome::Approved => &mut self.approvals.approved,
            ApprovalOutcome::Denied => &mut self.approvals.denied,
            ApprovalOutcome::Cancelled => &mut self.approvals.cancelled,
        };
        *count = count.saturating_add(1);
    }
}

/// 道具の出力の保存先と、上限で捨てた量。パスはセッションからの相対。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
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
        session_id: String,
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
    },
    /// 番の開始。
    TurnStart { turn: u64 },
    /// 一つの UserMessage を分割せず保存する。
    User {
        kind: UserKind,
        message: UserMessage,
    },
    /// RawItems を含む、受け取りを終えた応答。
    Assistant { turn: u64, response: AssistantTurn },
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
    /// LLM に渡した結果と、全体または先頭を保存した場所。
    ToolResult {
        turn: u64,
        name: String,
        result: ToolResult,
        spill: Option<SpillInfo>,
    },
    /// 確認の要求または結果。P1 では書かない。
    Approval {
        phase: ApprovalPhase,
        id: String,
        tool_id: String,
        request: Option<String>,
        outcome: Option<ApprovalOutcome>,
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
    /// 後の段階で使う、残す会話の項目数と記録時点。
    Rewind { items: usize, checkpoint: String },
    /// 圧縮後の会話を起点として保持する。後続の行だけをこの後へ加える。
    Compaction { conversation: Conversation },
    /// 行の上限を超えた記録の JSON の分割保存先。読み手が元の Record に戻す。
    Spilled { parts: Vec<PathBuf> },
}
