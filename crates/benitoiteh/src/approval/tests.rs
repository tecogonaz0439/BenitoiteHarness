//! 承認表と完全一致の許可を、判定を所有する境界で確かめる（実装プラン P3-04）。

use super::*;

// 作成時の関門: bash は P3-05 なので、公開の判定を表で検査する。
// 方針や --no-sandbox の分岐の取り違えを捕まえる。書く道具の配線はループで別に確かめる。
#[test]
fn policy_table_covers_every_kind_approval_and_sandbox_state() {
    use Decision::{Allow, Ask, Deny};
    let facts = ApprovalFacts {
        command: Some("cargo test".into()),
        domain: Some("mock.invalid".into()),
        ..ApprovalFacts::default()
    };
    // 行は read-only、workspace-write、danger-full-access、--no-sandbox。
    // 各行は WriteScope、Command、Escalate、WebFetch の順。
    for (approval, expected) in [
        (
            Approval::Untrusted,
            [
                [Ask, Ask, Ask, Ask],
                [Ask, Ask, Ask, Ask],
                [Allow, Ask, Ask, Ask],
                [Ask, Ask, Ask, Ask],
            ],
        ),
        (
            Approval::OnRequest,
            [
                [Ask, Allow, Ask, Ask],
                [Ask, Allow, Ask, Ask],
                [Allow, Allow, Allow, Ask],
                [Ask, Allow, Allow, Ask],
            ],
        ),
        (
            Approval::Never,
            [
                [Deny, Allow, Deny, Deny],
                [Deny, Allow, Deny, Deny],
                [Allow, Allow, Allow, Deny],
                [Deny, Allow, Allow, Deny],
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
                ApprovalKind::WebFetch,
            ]
            .into_iter()
            .zip(row)
            {
                assert_eq!(
                    decide(&policy, kind, &facts, &BTreeSet::new(), &BTreeSet::new()),
                    expected,
                    "{policy:?} {kind:?}"
                );
            }
            for (domain, expected) in [
                ("mock.invalid", Allow),
                ("sub.mock.invalid", row.get(3).copied().unwrap_or(Deny)),
            ] {
                let domains = BTreeSet::from([domain.to_owned()]);
                assert_eq!(
                    decide(
                        &policy,
                        ApprovalKind::WebFetch,
                        &facts,
                        &BTreeSet::new(),
                        &domains
                    ),
                    expected,
                    "{policy:?} {domains:?}"
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
            decide(&policy, kind, &request.facts, &allowed, &BTreeSet::new()),
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
            decide(
                &policy,
                ApprovalKind::Command,
                &changed,
                &allowed,
                &BTreeSet::new()
            ),
            Decision::Ask
        );
        assert_eq!(
            decide(
                &policy,
                ApprovalKind::Command,
                &ApprovalFacts::default(),
                &allowed,
                &BTreeSet::new()
            ),
            Decision::Ask
        );
        assert_eq!(
            decide(
                &policy,
                ApprovalKind::Escalate,
                &request.facts,
                &allowed,
                &BTreeSet::new()
            ),
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
                &allowed,
                &BTreeSet::new()
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
                &BTreeSet::new(),
                &BTreeSet::new()
            ),
            Decision::Ask
        );
    }
}

// 関門: 制限モードで誤って確認を許す退行を、通常モードの同じ方針と比較して検出する。
#[test]
fn restricted_mode_denies_every_non_web_approval_even_with_full_access_policy() {
    let policy = ApprovalPolicy {
        approval: Approval::OnRequest,
        sandbox: Sandbox::DangerFullAccess,
        sandboxed: false,
    };
    for kind in [
        ApprovalKind::WriteScope,
        ApprovalKind::Command,
        ApprovalKind::Escalate,
        ApprovalKind::WebFetch,
    ] {
        let decision = decide_in_mode(
            crate::config::Mode::Restricted,
            &policy,
            kind,
            &ApprovalFacts::default(),
            &BTreeSet::new(),
            &BTreeSet::new(),
        );
        assert_eq!(
            decision,
            if kind == ApprovalKind::WebFetch {
                Decision::Ask
            } else {
                Decision::Deny
            }
        );
    }
}

#[test]
fn interactive_replies_belong_to_one_request_and_disconnection_cancels()
-> Result<(), Box<dyn std::error::Error>> {
    let (mut approver, requests) = InteractiveApprover::channel();
    let record = ApprovalRecord {
        call_id: "one".into(),
        kind: ApprovalKind::Command,
        facts: ApprovalFacts::default(),
        justification: None,
        summary: "command".into(),
        decision: None,
        user_message: None,
    };
    std::thread::scope(|scope| -> Result<(), Box<dyn std::error::Error>> {
        let worker = scope.spawn(|| {
            let first = approver.approve(&record);
            let mut second = record.clone();
            second.call_id = "two".into();
            let second = approver.approve(&second);
            let third = approver.approve(&record);
            (first, second, third)
        });
        let first = requests.recv()?;
        first.reply.send(ApprovalAnswer {
            outcome: ApprovalOutcome::Approved,
            message: None,
        })?;
        let second = requests.recv()?;
        assert_eq!(second.request.call_id, "two");
        assert!(
            first
                .reply
                .send(ApprovalAnswer {
                    outcome: ApprovalOutcome::Approved,
                    message: None
                })
                .is_err()
        );
        second.reply.send(ApprovalAnswer {
            outcome: ApprovalOutcome::Denied,
            message: None,
        })?;
        drop(requests);
        let (first, second, third) = worker.join().map_err(|_| "approver panicked")?;
        assert_eq!(first.outcome, ApprovalOutcome::Approved);
        assert_eq!(second.outcome, ApprovalOutcome::Denied);
        assert_eq!(third.outcome, ApprovalOutcome::Cancelled);
        Ok(())
    })
}
