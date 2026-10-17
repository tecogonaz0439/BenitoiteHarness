//! 道具から独立した承認の判定と確認の受け手（設計書 03-03「通常モードの承認の方針」）。

mod text;

use crate::config::{Approval, Sandbox};
pub use crate::session::ApprovalOutcome;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::path::PathBuf;

/// 確認の分類。
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalKind {
    /// 書ける場所の外、または守る場所への書き込み。
    WriteScope,
    /// サンドボックスの外への要求でないコマンド。
    Command,
    /// サンドボックスの外で動かすコマンド。
    Escalate,
    /// ウェブのドメインの取得。
    WebFetch,
}

/// ハーネスが確かめた操作の事実。LLM の説明とは別に渡す。
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct ApprovalFacts {
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub domain: Option<String>,
    pub command: Option<String>,
    pub cwd: Option<PathBuf>,
    pub path: Option<PathBuf>,
    pub sandbox_policy: Option<String>,
}

/// 起動時に選んだ方針と、実際のサンドボックスの有無。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ApprovalPolicy {
    pub approval: Approval,
    pub sandbox: Sandbox,
    pub sandboxed: bool,
}

/// 問い合わせの前にハーネスが決める扱い。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Decision {
    /// 確認なしで実行してよい。
    Allow,
    /// 利用者の答えを要する。
    Ask,
    /// 方針により実行しない。
    Deny,
}

/// 種類、方針、セッション内の完全一致の許可から扱いを決める。
pub fn decide(
    policy: &ApprovalPolicy,
    kind: ApprovalKind,
    facts: &ApprovalFacts,
    allowed: &BTreeSet<String>,
    domains: &BTreeSet<String>,
) -> Decision {
    let command = || match policy.approval {
        Approval::Untrusted => {
            if facts
                .command
                .as_ref()
                .is_some_and(|text| allowed.contains(text))
            {
                Decision::Allow
            } else {
                Decision::Ask
            }
        }
        Approval::OnRequest | Approval::Never => Decision::Allow,
    };
    match kind {
        ApprovalKind::WebFetch => {
            if facts
                .domain
                .as_ref()
                .is_some_and(|domain| domains.contains(domain))
            {
                Decision::Allow
            } else if policy.approval == Approval::Never {
                Decision::Deny
            } else {
                Decision::Ask
            }
        }
        ApprovalKind::WriteScope if policy.sandbox == Sandbox::DangerFullAccess => Decision::Allow,
        ApprovalKind::Command => command(),
        ApprovalKind::Escalate
            if !policy.sandboxed || policy.sandbox == Sandbox::DangerFullAccess =>
        {
            command()
        }
        ApprovalKind::WriteScope | ApprovalKind::Escalate => match policy.approval {
            Approval::Untrusted | Approval::OnRequest => Decision::Ask,
            Approval::Never => Decision::Deny,
        },
    }
}

/// 信頼するループが作る確認の要求と結果。道具の JSON から答えを作らない。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ApprovalRecord {
    pub call_id: String,
    pub kind: ApprovalKind,
    pub facts: ApprovalFacts,
    pub justification: Option<String>,
    pub summary: String,
    pub decision: Option<ApprovalOutcome>,
    pub user_message: Option<String>,
}

/// 確認の受け手だけが作る答えと、拒否に添える利用者の文。
#[derive(Clone, Debug, PartialEq)]
pub struct ApprovalAnswer {
    pub outcome: ApprovalOutcome,
    pub message: Option<String>,
}

impl ApprovalAnswer {
    /// 通常のコマンドのセッション内許可だけを覚える。再開の記録からは呼ばない。
    pub(crate) fn remember_command(
        &self,
        request: &ApprovalRecord,
        allowed: &mut BTreeSet<String>,
    ) {
        if self.outcome == ApprovalOutcome::ApprovedForSession
            && request.kind == ApprovalKind::Command
            && let Some(command) = &request.facts.command
        {
            allowed.insert(command.clone());
        }
    }
    /// 今回の答えだけからドメインを覚える。再開した記録は許可を復元しない。
    pub(crate) fn remember_domain(&self, request: &ApprovalRecord, domains: &mut BTreeSet<String>) {
        if self.outcome == ApprovalOutcome::ApprovedForSession
            && request.kind == ApprovalKind::WebFetch
            && let Some(domain) = &request.facts.domain
        {
            domains.insert(domain.clone());
        }
    }
}

/// 利用者に代わって確認に答える境界。LLM の道具には公開しない。
pub trait Approver: Send {
    /// この一回の操作への答えを返す。
    fn approve(&mut self, request: &ApprovalRecord) -> ApprovalAnswer;
}

/// 対話できないときの確認の受け手。操作を許可しない。
#[derive(Debug, Default)]
pub struct NonInteractiveApprover;

impl Approver for NonInteractiveApprover {
    fn approve(&mut self, _request: &ApprovalRecord) -> ApprovalAnswer {
        ApprovalAnswer {
            outcome: ApprovalOutcome::Denied,
            message: None,
        }
    }
}

/// 方針が拒否した操作について LLM に返す文。
pub fn policy_denial_message(kind: ApprovalKind) -> &'static str {
    match kind {
        ApprovalKind::WriteScope => text::POLICY_WRITE_DENIED,
        ApprovalKind::Command => text::POLICY_COMMAND_DENIED,
        ApprovalKind::Escalate => text::POLICY_ESCALATE_DENIED,
        ApprovalKind::WebFetch => text::POLICY_WEB_DENIED,
    }
}

/// 利用者が拒否した操作について LLM に返す文。
pub fn user_denial_message(kind: ApprovalKind, message: Option<&str>) -> String {
    let denial = match kind {
        ApprovalKind::WriteScope => text::USER_WRITE_DENIED,
        ApprovalKind::Command => text::USER_COMMAND_DENIED,
        ApprovalKind::Escalate => text::USER_ESCALATE_DENIED,
        ApprovalKind::WebFetch => text::USER_WEB_DENIED,
    };
    match message {
        Some(message) => text::with_user_message(denial, message),
        None => denial.to_owned(),
    }
}

#[cfg(test)]
mod tests;
