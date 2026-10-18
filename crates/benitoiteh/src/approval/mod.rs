//! 道具から独立した承認の判定と確認の受け手（設計書 03-03「通常モードの承認の方針」）。

mod interactive;
mod text;
pub use interactive::{ApprovalPrompt, InteractiveApprover};

pub use crate::benitoite::report::{ApprovalSource, ContentHash};
use crate::config::{Approval, Sandbox};
pub use crate::session::ApprovalOutcome;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

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
    /// スクリプトのエフェクトの確認。
    Effects,
    /// テストのエフェクトの確認。
    TestEffects,
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
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub targets: Vec<PathBuf>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub effects: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub added_effects: Vec<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub shell_reference: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sources: Vec<ApprovalSource>,
    /// 読まない利用者のソースの件数。パスは画面に示さない。
    #[serde(default, skip_serializing_if = "is_zero")]
    pub unread_imports: usize,
}

fn is_zero(n: &usize) -> bool {
    *n == 0
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
        ApprovalKind::Effects | ApprovalKind::TestEffects => match policy.approval {
            Approval::Never => Decision::Deny,
            Approval::OnRequest | Approval::Untrusted => Decision::Ask,
        },
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

/// 制限モードの多重の守り。既存の通常モードの判定と同じ入口を共有する。
pub fn decide_in_mode(
    mode: crate::config::Mode,
    policy: &ApprovalPolicy,
    kind: ApprovalKind,
    facts: &ApprovalFacts,
    allowed: &BTreeSet<String>,
    domains: &BTreeSet<String>,
) -> Decision {
    if mode == crate::config::Mode::Restricted
        && !matches!(
            kind,
            ApprovalKind::WebFetch | ApprovalKind::Effects | ApprovalKind::TestEffects
        )
    {
        Decision::Deny
    } else {
        decide(policy, kind, facts, allowed, domains)
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
    /// 承認した今回の集合だけを広げる。記録の再開では呼ばない。
    pub(crate) fn remember_effects(
        &self,
        request: &ApprovalRecord,
        confirmed: &mut std::collections::BTreeMap<
            crate::tools::ConfirmKey,
            BTreeSet<crate::benitoite::Effect>,
        >,
    ) {
        if self.outcome == ApprovalOutcome::Approved
            && let Some(key) = crate::tools::ConfirmKey::from_facts(request.kind, &request.facts)
        {
            confirmed.entry(key).or_default().extend(
                request
                    .facts
                    .effects
                    .iter()
                    .filter_map(|name| crate::benitoite::Effect::from_name(name)),
            );
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
        ApprovalKind::Effects => text::POLICY_EFFECTS_DENIED,
        ApprovalKind::TestEffects => text::POLICY_TEST_EFFECTS_DENIED,
    }
}

/// 利用者が拒否した操作について LLM に返す文。
pub fn user_denial_message(
    kind: ApprovalKind,
    facts: &ApprovalFacts,
    project: &Path,
    message: Option<&str>,
) -> String {
    let denial = match kind {
        ApprovalKind::WriteScope => text::USER_WRITE_DENIED.to_owned(),
        ApprovalKind::Command => text::USER_COMMAND_DENIED.to_owned(),
        ApprovalKind::Escalate => text::USER_ESCALATE_DENIED.to_owned(),
        ApprovalKind::WebFetch => text::USER_WEB_DENIED.to_owned(),
        ApprovalKind::Effects | ApprovalKind::TestEffects => {
            text::effects_denied(kind, facts, project)
        }
    };
    match message {
        Some(message) => text::with_user_message(&denial, message),
        None => denial,
    }
}

/// 確認の事実から、LLM の説明と独立した要約を作る。
pub fn effects_summary(facts: &ApprovalFacts, project: &Path) -> String {
    text::effects_summary(ApprovalKind::Effects, facts, project)
}

#[cfg(test)]
mod tests;

/// テストのすべての対象を含む確認の要約。
pub fn test_effects_summary(facts: &ApprovalFacts, project: &Path) -> String {
    text::effects_summary(ApprovalKind::TestEffects, facts, project)
}
/// 非対話で許した範囲を超える実行の説明。
pub fn allow_denied(
    kind: ApprovalKind,
    facts: &ApprovalFacts,
    project: &Path,
    allowed: &BTreeSet<crate::benitoite::Effect>,
) -> String {
    text::allow_denied(kind, facts, project, allowed)
}

/// 非対話ではエフェクトの許可と独立に、読まないソースの取り込みを断る。
pub(crate) fn unread_imports_denied(facts: &ApprovalFacts, project: &Path) -> String {
    text::unread_imports_denied(facts, project)
}
