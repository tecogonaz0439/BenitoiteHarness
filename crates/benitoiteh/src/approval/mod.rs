//! 道具から独立した確認の受け手と記録（設計書 03-03「非対話での扱い」）。

mod text;

use serde::{Deserialize, Serialize};

pub use crate::session::ApprovalOutcome;

/// 確認の分類。P2 以降にエフェクトなどの分類を加える。
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalKind {
    /// 通常モードの書ける場所の外への書き込み。
    WriteScope,
}

/// 信頼するループが作る確認の要求と結果。道具の JSON から答えを作らない。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ApprovalRecord {
    pub call_id: String,
    pub kind: ApprovalKind,
    pub summary: String,
    pub decision: Option<ApprovalOutcome>,
}

/// 利用者に代わって確認に答える境界。LLM の道具には公開しない。
pub trait Approver: Send {
    /// この一回の操作への答えを返す。
    fn approve(&mut self, request: &ApprovalRecord) -> ApprovalOutcome;
}

/// P1 の非対話の方針 never。範囲外の書き込みを許可しない。
#[derive(Debug, Default)]
pub struct NonInteractiveApprover;

impl Approver for NonInteractiveApprover {
    fn approve(&mut self, _request: &ApprovalRecord) -> ApprovalOutcome {
        ApprovalOutcome::Denied
    }
}

/// 拒否した操作について LLM に返す文。
pub fn denial_message() -> &'static str {
    text::DENIED
}
