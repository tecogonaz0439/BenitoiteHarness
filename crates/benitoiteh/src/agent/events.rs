//! 画面と exec に渡すイベント（設計書 01-01「イベント」、実装プラン P1-09）。

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::approval::{ApprovalFacts, ApprovalKind, ApprovalOutcome};
use crate::provider::{FinishReason, ProviderError, Usage};
use crate::session::{ApprovalPhase, CompactionReason, Record, RecordedMode, SkippedInstruction};

/// 一連の処理の終了状態。
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    /// 道具を呼ばない応答で終わった。
    Completed,
    /// 提供者の誤りや上限で終わった。
    Failed,
    /// 利用者が取り消した。
    Cancelled,
}

impl Outcome {
    /// 記録と終了状態の判定で使う綴り。
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }
}

/// JSON Lines と画面に共通のイベント。記録では差分を除き、応答全体を別途残す。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AgentEvent {
    /// 送り先と送る内容の通知。
    SessionStart {
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
    },
    /// 番の開始。
    TurnStart { turn: u64 },
    /// 応答の文の差分。
    TextDelta { turn: u64, text: String },
    /// 推論の文の差分。
    ReasoningDelta { turn: u64, text: String },
    /// 受け取りを終えた呼び出し。
    ToolCall {
        turn: u64,
        call_id: String,
        name: String,
        arguments: Value,
    },
    /// 実行前の確認。
    ApprovalRequest {
        call_id: String,
        kind: ApprovalKind,
        facts: ApprovalFacts,
        justification: Option<String>,
        summary: String,
        user_message: Option<String>,
    },
    /// 確認の答え。
    ApprovalResult {
        call_id: String,
        kind: ApprovalKind,
        facts: ApprovalFacts,
        justification: Option<String>,
        summary: String,
        decision: ApprovalOutcome,
        user_message: Option<String>,
    },
    /// 画面用の一行の結果。記録には LLM に返す結果全体を残す。
    ToolResult {
        turn: u64,
        call_id: String,
        name: String,
        is_error: bool,
        summary: String,
    },
    /// 番の終了。
    TurnEnd {
        turn: u64,
        finish_reason: FinishReason,
        usage: Usage,
    },
    /// 圧縮による会話の置き換え。
    Compaction {
        reason: CompactionReason,
        summarized_items: usize,
        kept_items: usize,
        estimate_before: u64,
        estimate_after: u64,
    },
    /// 同じ要求のやり直し。
    Retry {
        attempt: u64,
        delay_ms: u64,
        reason: ProviderError,
    },
    /// 秘密を含まない診断。
    Error { kind: String, message: String },
    /// 一連の処理の終了。
    SessionEnd { outcome: Outcome, reason: String },
}

impl AgentEvent {
    pub(super) fn record(&self) -> Option<Record> {
        Some(match self {
            Self::SessionStart {
                session_id,
                resumed,
                base_url,
                system_prompt_sha256,
                tools_sha256,
                mode,
                provider,
                model,
                host,
                benitoiteh_version,
                notice,
                instruction_files,
                skipped_instructions,
            } => Record::SessionStart {
                session_id: session_id.clone(),
                resumed: *resumed,
                base_url: base_url.clone(),
                system_prompt_sha256: system_prompt_sha256.clone(),
                tools_sha256: tools_sha256.clone(),
                mode: *mode,
                provider: provider.clone(),
                model: model.clone(),
                host: host.clone(),
                benitoiteh_version: benitoiteh_version.clone(),
                notice: notice.clone(),
                instruction_files: instruction_files.clone(),
                skipped_instructions: skipped_instructions.clone(),
            },
            Self::TurnStart { turn } => Record::TurnStart { turn: *turn },
            Self::TurnEnd {
                turn,
                finish_reason,
                usage,
            } => Record::TurnEnd {
                turn: *turn,
                finish_reason: finish_reason.clone(),
                usage: usage.clone(),
            },
            Self::ApprovalRequest {
                call_id,
                kind,
                facts,
                justification,
                summary,
                user_message,
            } => Record::Approval {
                phase: ApprovalPhase::Request,
                id: call_id.clone(),
                tool_id: call_id.clone(),
                request: Some(summary.clone()),
                outcome: None,
                kind: Some(*kind),
                facts: facts.clone(),
                justification: justification.clone(),
                user_message: user_message.clone(),
            },
            Self::ApprovalResult {
                call_id,
                summary,
                decision,
                kind,
                facts,
                justification,
                user_message,
            } => Record::Approval {
                phase: ApprovalPhase::Result,
                id: call_id.clone(),
                tool_id: call_id.clone(),
                request: Some(summary.clone()),
                outcome: Some(*decision),
                kind: Some(*kind),
                facts: facts.clone(),
                justification: justification.clone(),
                user_message: user_message.clone(),
            },
            Self::Retry {
                attempt,
                delay_ms,
                reason,
            } => Record::Retry {
                attempt: *attempt,
                delay_ms: *delay_ms,
                error: reason.clone(),
            },
            Self::Error { kind, message } => Record::Error {
                kind: kind.clone(),
                message: message.clone(),
            },
            Self::SessionEnd { outcome, reason } => Record::SessionEnd {
                outcome: outcome.as_str().into(),
                reason: reason.clone(),
            },
            Self::TextDelta { .. }
            | Self::ReasoningDelta { .. }
            | Self::ToolCall { .. }
            | Self::ToolResult { .. }
            | Self::Compaction { .. } => return None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // bash の実装前でも、イベントと記録の公開の形が事実と説明を混ぜないことを固定する。
    #[test]
    fn approval_events_record_facts_justification_and_user_message_separately()
    -> Result<(), serde_json::Error> {
        let facts = ApprovalFacts {
            command: Some("cargo test".into()),
            cwd: Some("/project".into()),
            path: None,
            sandbox_policy: Some("workspace-write".into()),
        };
        for event in [
            AgentEvent::ApprovalRequest {
                call_id: "call".into(),
                kind: ApprovalKind::Escalate,
                facts: facts.clone(),
                justification: Some("Needs network".into()),
                summary: "Run cargo test".into(),
                user_message: None,
            },
            AgentEvent::ApprovalResult {
                call_id: "call".into(),
                kind: ApprovalKind::Escalate,
                facts,
                justification: Some("Needs network".into()),
                summary: "Run cargo test".into(),
                decision: ApprovalOutcome::DeniedWithMessage,
                user_message: Some("Stay offline".into()),
            },
        ] {
            let value = serde_json::to_value(&event)?;
            let record = serde_json::to_value(event.record())?;
            for output in [value, record] {
                assert_eq!(output.get("kind"), Some(&json!("escalate")));
                assert_eq!(
                    output.get("facts"),
                    Some(
                        &json!({"command":"cargo test","cwd":"/project","path":null,"sandbox_policy":"workspace-write"})
                    )
                );
                assert_eq!(output.get("justification"), Some(&json!("Needs network")));
                assert_eq!(
                    output.get("user_message"),
                    Some(&if matches!(event, AgentEvent::ApprovalResult { .. }) {
                        json!("Stay offline")
                    } else {
                        json!(null)
                    })
                );
            }
        }
        Ok(())
    }
}
