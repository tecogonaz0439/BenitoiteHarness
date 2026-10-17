//! 承認表と完全一致の許可を、判定を所有する境界で確かめる（実装プラン P3-04）。

use super::*;

// 作成時の関門: bash は P3-05 なので、公開の判定を表で検査する。
// 方針や --no-sandbox の分岐の取り違えを捕まえる。書く道具の配線はループで別に確かめる。
#[test]
fn policy_table_covers_every_kind_approval_and_sandbox_state() {
    use Decision::{Allow, Ask, Deny};
    let facts = ApprovalFacts {
        command: Some("cargo test".into()),
        ..ApprovalFacts::default()
    };
    // 行は read-only、workspace-write、danger-full-access、--no-sandbox。
    // 各行は WriteScope、Command、Escalate の順。
    for (approval, expected) in [
        (
            Approval::Untrusted,
            [
                [Ask, Ask, Ask],
                [Ask, Ask, Ask],
                [Allow, Ask, Ask],
                [Ask, Ask, Ask],
            ],
        ),
        (
            Approval::OnRequest,
            [
                [Ask, Allow, Ask],
                [Ask, Allow, Ask],
                [Allow, Allow, Allow],
                [Ask, Allow, Allow],
            ],
        ),
        (
            Approval::Never,
            [
                [Deny, Allow, Deny],
                [Deny, Allow, Deny],
                [Allow, Allow, Allow],
                [Deny, Allow, Allow],
            ],
        ),
    ] {
        for ((sandbox, sandboxed), row) in [
            (Sandbox::ReadOnly, true),
            (Sandbox::WorkspaceWrite, true),
            (Sandbox::DangerFullAccess, false),
            (Sandbox::WorkspaceWrite, false),
        ]
        .into_iter()
        .zip(expected)
        {
            let policy = ApprovalPolicy {
                approval,
                sandbox,
                sandboxed,
            };
            for (kind, expected) in [
                ApprovalKind::WriteScope,
                ApprovalKind::Command,
                ApprovalKind::Escalate,
            ]
            .into_iter()
            .zip(row)
            {
                assert_eq!(
                    decide(&policy, kind, &facts, &BTreeSet::new()),
                    expected,
                    "{policy:?} {kind:?}"
                );
            }
        }
    }
}

#[test]
fn only_command_answers_remember_exact_strings_and_escalations_still_require_approval() {
    let policy = ApprovalPolicy {
        approval: Approval::Untrusted,
        sandbox: Sandbox::WorkspaceWrite,
        sandboxed: true,
    };
    let answer = ApprovalAnswer {
        outcome: ApprovalOutcome::ApprovedForSession,
        message: None,
    };
    for kind in [
        ApprovalKind::Command,
        ApprovalKind::Escalate,
        ApprovalKind::WriteScope,
    ] {
        let request = ApprovalRecord {
            call_id: "call".into(),
            kind,
            facts: ApprovalFacts {
                command: Some("cargo test".into()),
                ..ApprovalFacts::default()
            },
            summary: "command".into(),
            justification: None,
            decision: None,
            user_message: None,
        };
        let mut allowed = BTreeSet::new();
        answer.remember_command(&request, &mut allowed);
        assert_eq!(
            decide(&policy, kind, &request.facts, &allowed),
            if kind == ApprovalKind::Command {
                Decision::Allow
            } else {
                Decision::Ask
            }
        );
        let changed = ApprovalFacts {
            command: Some("cargo test ".into()),
            ..ApprovalFacts::default()
        };
        assert_eq!(
            decide(&policy, ApprovalKind::Command, &changed, &allowed),
            Decision::Ask
        );
        assert_eq!(
            decide(
                &policy,
                ApprovalKind::Command,
                &ApprovalFacts::default(),
                &allowed
            ),
            Decision::Ask
        );
        assert_eq!(
            decide(&policy, ApprovalKind::Escalate, &request.facts, &allowed),
            Decision::Ask
        );
        let no_sandbox = ApprovalPolicy {
            sandboxed: false,
            ..policy
        };
        assert_eq!(
            decide(
                &no_sandbox,
                ApprovalKind::Escalate,
                &request.facts,
                &allowed
            ),
            if kind == ApprovalKind::Command {
                Decision::Allow
            } else {
                Decision::Ask
            }
        );
        // 新しい Agent の空の集合には、前のセッションの許可がない。
        assert_eq!(
            decide(
                &policy,
                ApprovalKind::Command,
                &request.facts,
                &BTreeSet::new()
            ),
            Decision::Ask
        );
    }
}
