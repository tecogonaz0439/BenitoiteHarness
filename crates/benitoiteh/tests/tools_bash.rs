//! bash の確認、出力、停止、環境を道具の実行器で確かめる（設計書 01-03、07-01）。

#[path = "support/bash.rs"]
mod bash_support;
use bash_support::Fixture;
use benitoiteh::approval::ApprovalKind;
use benitoiteh::config::{Approval, Mode, Sandbox};
use benitoiteh::tools::{ApprovalStatus, Invocation, ToolSet};
use serde_json::json;
use std::error::Error;
use std::fs;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

type TestResult = Result<(), Box<dyn Error>>;

// 作成時の関門: JSON から確認と起動へ至る境界で、制限モードへの混入、
// 確認前の実行、escalate の種類の取り違えを捕まえる。既存の書く道具にはない契約である。
#[test]
fn bash_requires_approval_and_validates_before_asking() -> TestResult {
    let mut f = Fixture::new(Sandbox::WorkspaceWrite)?;
    assert!(
        ToolSet::new(Mode::Restricted)
            .validate("bash", json!({"command":"true"}))
            .is_err()
    );
    for policy in [Approval::Never, Approval::OnRequest, Approval::Untrusted] {
        f.context.shell.as_mut().ok_or("missing shell")?.approval = policy;
        for escalate in [false, true] {
            let output = f.tools.execute(&f.context, "bash", json!({"command":" printf unsafe > marker ", "escalate":escalate,"justification":"required reason"}));
            let request = output.approval.ok_or("missing approval")?;
            assert_eq!(
                request.kind,
                if escalate {
                    ApprovalKind::Escalate
                } else {
                    ApprovalKind::Command
                }
            );
            assert_eq!(
                request.facts.command.as_deref(),
                Some(" printf unsafe > marker ")
            );
            assert_eq!(request.facts.cwd.as_ref(), Some(&f.project));
            assert_eq!(
                request.facts.sandbox_policy.as_deref(),
                Some("workspace-write")
            );
            assert_eq!(request.justification.as_deref(), Some("required reason"));
            assert!(!f.project.join("marker").exists());
        }
    }
    for timeout in [0, 601, -1] {
        let output = f.tools.execute(
            &f.context,
            "bash",
            json!({"command":"true", "timeout":timeout}),
        );
        assert!(output.is_error);
        assert!(output.approval.is_none());
    }
    assert_eq!(
        fs::read_dir(f.session.directory().join("spill"))?.count(),
        0
    );
    Ok(())
}

#[test]
fn bash_returns_true_tail_and_saves_only_truncated_output() -> TestResult {
    let f = Fixture::new(Sandbox::DangerFullAccess)?;
    let cases = [
        ("short", "hello\n".to_owned(), "hello\n".to_owned(), None),
        (
            "lines",
            (1..=2005)
                .map(|n| format!("line-{n}\n"))
                .collect::<String>(),
            (6..=2005)
                .map(|n| format!("line-{n}\n"))
                .collect::<String>(),
            Some((6, 2005)),
        ),
        (
            "unterminated",
            (1..=2005)
                .map(|n| format!("line-{n}"))
                .collect::<Vec<_>>()
                .join("\n"),
            (6..=2005)
                .map(|n| format!("line-{n}"))
                .collect::<Vec<_>>()
                .join("\n"),
            Some((6, 2005)),
        ),
        (
            "bytes",
            format!("{}end", "日".repeat(18000)),
            format!("{}end", "日".repeat(17065)),
            Some((1, 1)),
        ),
        (
            "byte-lines",
            (1..=600)
                .map(|n| format!("{n:04}{}\n", "x".repeat(95)))
                .collect::<String>(),
            (89..=600)
                .map(|n| format!("{n:04}{}\n", "x".repeat(95)))
                .collect::<String>(),
            Some((89, 600)),
        ),
    ];
    for (id, all, tail, range) in cases {
        fs::write(f.project.join("input"), &all)?;
        let output = f.execute(id, json!({"command":"cat input"}), &AtomicBool::new(false))?;
        assert!(!output.is_error, "{}", output.text);
        let mut expected = tail;
        if !expected.ends_with('\n') {
            expected.push('\n');
        }
        if let Some((first, last)) = range {
            let spill = output.spill.ok_or("missing spill")?;
            let path = f.session.directory().join(&spill.path);
            assert_eq!(fs::read_to_string(&path)?, all);
            expected.push_str(&format!(
                "[Showing last lines {first}-{last} of {last}. Full output: {}]\n",
                path.display()
            ));
            let read = f
                .tools
                .execute(&f.context, "read", json!({"path":path,"limit":1}));
            assert!(!read.is_error, "{}", read.text);
        } else {
            assert!(output.spill.is_none());
            assert_eq!(
                fs::read_dir(f.session.directory().join("spill"))?.count(),
                0
            );
        }
        expected.push_str("[exit code: 0]");
        assert_eq!(output.text, expected, "{id}");
    }
    let all = format!("{}THE-END", "a".repeat(16 * 1024 * 1024 + 5));
    fs::write(f.project.join("input"), &all)?;
    let output = f.execute(
        "partial",
        json!({"command":"cat input"}),
        &AtomicBool::new(false),
    )?;
    assert!(
        output
            .text
            .contains("Partial output (first 16777216 of 16777228 bytes)")
    );
    assert!(output.text.contains("THE-END\n[Showing"));
    let spill = output.spill.ok_or("missing partial spill")?;
    assert_eq!(spill.written_bytes, 16777216);
    assert_eq!(spill.discarded_bytes, 12);
    Ok(())
}

#[test]
fn bash_reports_exit_timeout_and_cancellation() -> TestResult {
    let f = Fixture::new(Sandbox::DangerFullAccess)?;
    let output = f.execute(
        "exit",
        json!({"command":"printf out; printf err >&2; exit 7"}),
        &AtomicBool::new(false),
    )?;
    assert!(output.is_error);
    assert!(output.text.contains("out") && output.text.contains("err"));
    assert!(output.text.ends_with("[exit code: 7]"));
    let output = f.execute(
        "timeout",
        json!({"command":"sleep 30", "timeout":1}),
        &AtomicBool::new(false),
    )?;
    assert!(output.is_error);
    assert!(output.text.contains("timed out after 1 seconds"));
    let cancelled = AtomicBool::new(false);
    std::thread::scope(|scope| -> TestResult {
        scope.spawn(|| {
            std::thread::sleep(Duration::from_millis(150));
            cancelled.store(true, Ordering::Release);
        });
        let output = f.execute("cancelled", json!({"command":"sleep 30"}), &cancelled)?;
        assert!(output.is_error);
        assert!(output.text.contains("user cancelled the tool call"));
        Ok(())
    })?;
    assert_eq!(
        fs::read_dir(f.session.directory().join("spill"))?.count(),
        0
    );
    Ok(())
}

#[test]
fn bash_clears_keys_and_removes_sandbox_markers_when_escalated() -> TestResult {
    for sandbox in [Sandbox::WorkspaceWrite, Sandbox::DangerFullAccess] {
        let f = Fixture::new(sandbox)?;
        let output = f.execute(
            "env",
            json!({"command":"env", "escalate":true}),
            &AtomicBool::new(false),
        )?;
        assert!(!output.is_error, "{}", output.text);
        for name in [
            "OPENAI_API_KEY",
            "ANTHROPIC_API_KEY",
            "CUSTOM_PROVIDER_KEY",
            "BENITOITEH_SANDBOX",
            "synthetic-",
        ] {
            assert!(!output.text.contains(name), "{}", output.text);
        }
        assert!(output.text.contains("NO_COLOR=1"));
        assert!(
            output
                .text
                .contains(&format!("TMPDIR={}", f.session.temporary_dir().display()))
        );
    }
    Ok(())
}

#[test]
fn bash_never_falls_back_when_sandbox_preparation_fails() -> TestResult {
    let f = Fixture::new(Sandbox::WorkspaceWrite)?;
    fs::remove_dir(f.session.temporary_dir())?;
    let output = f.execute(
        "failed",
        json!({"command":"echo unsafe > marker"}),
        &AtomicBool::new(false),
    )?;
    assert!(output.is_error);
    assert!(!f.project.join("marker").exists());
    assert_eq!(
        fs::read_dir(f.session.directory().join("spill"))?.count(),
        0
    );
    for missing in ["shell", "session"] {
        let mut f = Fixture::new(Sandbox::DangerFullAccess)?;
        if missing == "shell" {
            f.context.shell = None;
        } else {
            f.context.session = None;
        }
        let call = f
            .tools
            .validate("bash", json!({"command":"echo unsafe > marker"}))?;
        let output = f.tools.execute_validated(
            &f.context,
            &call,
            ApprovalStatus::Approved,
            Invocation {
                confirmed_effects: &std::collections::BTreeMap::new(),
                session_domains: &Default::default(),
                call_id: "missing",
                cancelled: &AtomicBool::new(false),
            },
        );
        assert!(output.is_error);
        assert!(!f.project.join("marker").exists());
    }
    Ok(())
}
