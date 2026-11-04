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

fn askpass(
    f: &mut Fixture,
    interactive: bool,
) -> Result<std::sync::Arc<benitoiteh::askpass::Broker>, Box<dyn Error>> {
    let shell = f.context.shell.as_mut().ok_or("missing shell")?;
    let broker = std::sync::Arc::new(benitoiteh::askpass::Broker::create(
        &f.root,
        f.session.id().as_str(),
        &shell.helper,
        &[f.project.clone(), f.session.temporary_dir().into()],
        interactive,
    )?);
    broker.environment(&mut shell.env);
    shell.askpass = Some(std::sync::Arc::clone(&broker));
    Ok(broker)
}

// 関門: 補助を本物の子として起動し、道具の結果と spill の秘密の除外を観察する。
// 窓口・環境・補助は差し替えず、利用者の回答だけを端末の代わりに送る。
#[test]
fn askpass_delivers_secrets_redacts_output_discards_spill_and_pauses_timeout() -> TestResult {
    let mut f = Fixture::new(Sandbox::DangerFullAccess)?;
    let broker = askpass(&mut f, true)?;
    let prompts = broker.take_requests().ok_or("missing prompts")?;
    std::thread::scope(|scope| -> TestResult {
        let responder = scope.spawn(move || -> Result<(), String> {
            let prompt = prompts
                .recv_timeout(Duration::from_secs(5))
                .map_err(|e| e.to_string())?;
            assert_eq!(prompt.facts.call_id, "secret");
            assert_eq!(prompt.kind, benitoiteh::askpass::Kind::Git);
            assert_eq!(prompt.prompt, "synthetic password");
            // 時間の上限 1 秒より長く利用者を待っても、コマンドの時間には数えない。
            std::thread::sleep(Duration::from_millis(1250));
            prompt
                .reply
                .send(benitoiteh::askpass::Answer::Secret(
                    zeroize::Zeroizing::new("synth秘密-value".into()),
                ))
                .map_err(|_| "answer failed".to_owned())?;
            Ok(())
        });
        let output = f.execute("secret", json!({"command":"value=$(\"$GIT_ASKPASS\" 'synthetic password') || exit 1; i=0; while [ $i -lt 2100 ]; do printf 'line\n'; i=$((i+1)); done; printf '%s|%s' \"$value\" \"$value\"", "timeout":1}), &AtomicBool::new(false))?;
        responder.join().map_err(|_| "responder panicked")??;
        assert!(!output.is_error, "{}", output.text);
        assert!(output.text.contains("[secret]|[secret]"), "{}", output.text);
        assert!(output.text.contains("Secret prompt (git): answered."));
        assert!(output.text.contains("Raw output was not saved"));
        assert!(!output.text.contains("synth秘密-value"));
        assert!(output.spill.is_none());
        assert_eq!(
            fs::read_dir(f.session.directory().join("spill"))?.count(),
            0
        );
        Ok(())
    })?;
    Ok(())
}

// 関門: 許された起動の条件と、補助の起動の名前の契約を本物のプロセスで守る。
#[test]
fn askpass_environment_only_authorizes_interactive_full_access_or_approved_escalation() -> TestResult
{
    for (sandbox, interactive, remove_policy, authorized) in [
        (Sandbox::DangerFullAccess, true, false, true),
        (Sandbox::WorkspaceWrite, true, false, true),
        (Sandbox::WorkspaceWrite, true, true, false),
        (Sandbox::DangerFullAccess, false, false, false),
    ] {
        let mut f = Fixture::new(sandbox)?;
        let broker = askpass(&mut f, interactive)?;
        if remove_policy {
            f.context.shell.as_mut().ok_or("missing shell")?.policy = None;
        }
        let command = "test -x \"$SSH_ASKPASS\" && test -x \"$GIT_ASKPASS\" && test -x \"$SUDO_ASKPASS\" && test \"$SSH_ASKPASS_REQUIRE\" = force && test \"$GIT_TERMINAL_PROMPT\" = 0 || exit 2; if [ -n \"$BENITOITEH_ASKPASS_TOKEN\" ] && [ -n \"$BENITOITEH_ASKPASS_SOCKET\" ]; then printf authorized; else printf unavailable; \"$GIT_ASKPASS\" prompt; fi";
        let output = f.execute(
            "env",
            json!({"command": command,"escalate":true}),
            &AtomicBool::new(false),
        )?;
        if authorized {
            assert!(output.text.starts_with("authorized"), "{}", output.text);
        } else {
            assert!(output.is_error);
            assert!(output.text.contains(benitoiteh::askpass::text::UNAVAILABLE));
        }
        let requests = broker.take_requests();
        assert_eq!(requests.is_some(), interactive);
    }
    Ok(())
}

// 関門: git 自身が起動する GIT_ASKPASS と stdin の資格情報の形式の結合を確かめる。
// helper、ネットワーク、利用者の git 設定と鍵は使わない。
// Linux の検査用 Alpine は git を持たない。全 OS での補助の直の起動は
// helper_names_confirm_decline_touch_and_missing_authority_follow_the_wire_contract で確かめる。
#[cfg(target_os = "macos")]
#[test]
fn git_credential_fill_uses_the_real_askpass_helper_without_network() -> TestResult {
    let mut f = Fixture::new(Sandbox::DangerFullAccess)?;
    let broker = askpass(&mut f, true)?;
    let shell = f.context.shell.as_mut().ok_or("missing shell")?;
    for (name, value) in [
        ("HOME", f.root.as_os_str()),
        ("XDG_CONFIG_HOME", f.root.as_os_str()),
        ("GIT_CONFIG_GLOBAL", std::ffi::OsStr::new("/dev/null")),
        ("GIT_CONFIG_NOSYSTEM", std::ffi::OsStr::new("1")),
    ] {
        shell.env.insert(name.into(), value.into());
    }
    let prompts = broker.take_requests().ok_or("missing prompts")?;
    std::thread::scope(|scope| -> TestResult {
        let responder = scope.spawn(move || -> Result<(), String> {
            let prompt = prompts
                .recv_timeout(Duration::from_secs(5))
                .map_err(|e| e.to_string())?;
            assert_eq!(prompt.kind, benitoiteh::askpass::Kind::Git);
            assert!(prompt.prompt.contains("Password"));
            prompt
                .reply
                .send(benitoiteh::askpass::Answer::Secret(
                    zeroize::Zeroizing::new("synthetic-git-answer".into()),
                ))
                .map_err(|_| "answer failed".to_owned())?;
            Ok(())
        });
        let output = f.execute("git", json!({"command":"printf 'protocol=https\nhost=example.invalid\nusername=synthetic\n\n' | git -c credential.helper= credential fill"}), &AtomicBool::new(false))?;
        responder.join().map_err(|_| "responder panicked")??;
        assert!(!output.is_error, "{}", output.text);
        assert!(output.text.contains("password=[secret]"), "{}", output.text);
        assert!(!output.text.contains("synthetic-git-answer"));
        Ok(())
    })?;
    Ok(())
}

#[test]
fn helper_names_confirm_decline_touch_and_missing_authority_follow_the_wire_contract() -> TestResult
{
    for (mode, command, responses, expected) in [
        (
            "names",
            "\"$SSH_ASKPASS\" ssh; \"$GIT_ASKPASS\" git; \"$SUDO_ASKPASS\" sudo",
            vec!["ssh", "git", "sudo"],
            "ssh-answer\ngit-answer\nsudo-answer",
        ),
        (
            "confirm",
            "SSH_ASKPASS_PROMPT=confirm \"$SSH_ASKPASS\" 'confirm?'; printf 'status=%s' $?",
            vec!["yes"],
            "status=0",
        ),
        (
            "decline",
            "SSH_ASKPASS_PROMPT=confirm \"$SSH_ASKPASS\" 'confirm?'; printf 'status=%s' $?",
            vec!["no"],
            "status=1",
        ),
        (
            "touch",
            "SSH_ASKPASS_PROMPT=none \"$SSH_ASKPASS\" touch; printf 'status=%s' $?",
            vec!["touch"],
            "status=0",
        ),
    ] {
        let mut f = Fixture::new(Sandbox::DangerFullAccess)?;
        let broker = askpass(&mut f, true)?;
        let requests = broker.take_requests().ok_or("missing prompts")?;
        std::thread::scope(|scope| -> TestResult {
            let responder = scope.spawn(move || -> Result<(), String> {
                for response in responses {
                    let prompt = requests
                        .recv_timeout(Duration::from_secs(5))
                        .map_err(|e| e.to_string())?;
                    let answer = match response {
                        "yes" => {
                            assert!(prompt.confirm());
                            benitoiteh::askpass::Answer::Yes
                        }
                        "no" => benitoiteh::askpass::Answer::Declined,
                        "touch" => {
                            assert!(prompt.touch());
                            benitoiteh::askpass::Answer::Cancelled
                        }
                        kind => {
                            assert_eq!(prompt.kind.name(), kind);
                            benitoiteh::askpass::Answer::Secret(zeroize::Zeroizing::new(format!(
                                "{kind}-answer"
                            )))
                        }
                    };
                    prompt
                        .reply
                        .send(answer)
                        .map_err(|_| "answer failed".to_owned())?;
                }
                Ok(())
            });
            let output = f.execute(mode, json!({"command":command}), &AtomicBool::new(false))?;
            responder.join().map_err(|_| "responder panicked")??;
            assert!(!output.is_error, "{}", output.text);
            if mode == "names" {
                assert!(
                    output.text.starts_with("[secret]\n[secret]\n[secret]"),
                    "{}",
                    output.text
                );
            } else {
                assert!(output.text.contains(expected), "{}", output.text);
            }
            Ok(())
        })?;
    }
    let mut f = Fixture::new(Sandbox::DangerFullAccess)?;
    let _broker = askpass(&mut f, false)?;
    let output = f.execute("unavailable", json!({"command":"for name in \"$SSH_ASKPASS\" \"$GIT_ASKPASS\" \"$SUDO_ASKPASS\"; do if \"$name\" prompt; then exit 2; fi; done"}), &AtomicBool::new(false))?;
    assert!(!output.is_error, "{}", output.text);
    assert_eq!(
        output
            .text
            .matches(benitoiteh::askpass::text::UNAVAILABLE)
            .count(),
        3
    );
    Ok(())
}

// 関門: 取り消しと時間切れの猶予中に、古い合言葉で再度入力を求められないこと。
#[test]
fn askpass_closes_pending_requests_on_cancellation_and_revokes_tokens_before_timeout_grace()
-> TestResult {
    let mut f = Fixture::new(Sandbox::DangerFullAccess)?;
    let broker = askpass(&mut f, true)?;
    let mut requests = broker.take_requests().ok_or("missing prompts")?;
    let cancelled = AtomicBool::new(false);
    std::thread::scope(|scope| -> TestResult {
        let requests = &mut requests;
        let cancel = &cancelled;
        let responder = scope.spawn(move || -> Result<(), String> {
            let prompt = requests
                .recv_timeout(Duration::from_secs(5))
                .map_err(|e| e.to_string())?;
            cancel.store(true, Ordering::Release);
            let started = std::time::Instant::now();
            while prompt.is_open() && started.elapsed() < Duration::from_secs(2) {
                std::thread::sleep(Duration::from_millis(5));
            }
            assert!(!prompt.is_open());
            Ok(())
        });
        let output = f.execute(
            "cancel",
            json!({"command":"\"$GIT_ASKPASS\" waiting"}),
            &cancelled,
        )?;
        responder.join().map_err(|_| "responder panicked")??;
        assert!(output.is_error && output.text.contains("user cancelled"));
        assert!(!output.text.contains("waiting"));
        Ok(())
    })?;
    let output = f.execute("timeout-grace", json!({"command":"trap '\"$GIT_ASKPASS\" late; exit 0' TERM; while :; do sleep 1; done", "timeout":1}), &AtomicBool::new(false))?;
    assert!(output.text.contains("timed out"));
    assert!(
        output.text.contains(benitoiteh::askpass::text::REFUSED),
        "{}",
        output.text
    );
    assert!(requests.try_recv().is_err());
    Ok(())
}

// 関門: 子を起動する直前の実体の変更で、補助の名前と権限が使われないこと。
#[test]
fn changed_helper_directory_removes_all_askpass_environment_variables_for_the_call() -> TestResult {
    use std::os::unix::fs::PermissionsExt;
    for change in ["replace", "mode", "symlink"] {
        let mut f = Fixture::new(Sandbox::DangerFullAccess)?;
        let broker = askpass(&mut f, true)?;
        let helper = std::path::PathBuf::from(
            f.context
                .shell
                .as_ref()
                .ok_or("missing shell")?
                .env
                .get(std::ffi::OsStr::new("GIT_ASKPASS"))
                .ok_or("missing helper")?,
        );
        let directory = helper.parent().ok_or("missing parent")?.to_path_buf();
        let saved = f.root.join("saved");
        if change == "mode" {
            fs::set_permissions(&directory, fs::Permissions::from_mode(0o755))?;
        } else {
            fs::rename(&directory, &saved)?;
            if change == "symlink" {
                std::os::unix::fs::symlink(&saved, &directory)?;
            } else {
                fs::create_dir(&directory)?;
                fs::set_permissions(&directory, fs::Permissions::from_mode(0o700))?;
            }
        }
        let output = f.execute("changed", json!({"command":"env"}), &AtomicBool::new(false))?;
        assert!(!output.is_error, "{}", output.text);
        assert!(
            output
                .text
                .contains(benitoiteh::askpass::text::CHANGED_DIRECTORY)
        );
        for name in [
            "SSH_ASKPASS=",
            "GIT_ASKPASS=",
            "SUDO_ASKPASS=",
            "SSH_ASKPASS_REQUIRE=",
            "GIT_TERMINAL_PROMPT=",
            "BENITOITEH_ASKPASS_SOCKET=",
            "BENITOITEH_ASKPASS_TOKEN=",
        ] {
            assert!(!output.text.contains(name), "{}", output.text);
        }
        f.context.shell.as_mut().ok_or("missing shell")?.askpass = None;
        drop(broker);
        assert!(fs::symlink_metadata(&directory).is_ok());
        if change == "symlink" {
            fs::remove_file(&directory)?;
        } else {
            fs::remove_dir_all(&directory)?;
        }
    }
    Ok(())
}

// 関門: 末尾を切る境と、互いに重なる出現から秘密の一部が漏れないこと。
#[test]
fn secret_redaction_precedes_tail_truncation_and_covers_overlapping_occurrences() -> TestResult {
    for (id, secret, command, forbidden) in [
        (
            "boundary",
            "prefixSUFFIX",
            "value=$(\"$GIT_ASKPASS\" prompt); printf '%s' \"$value\"; i=0; while [ $i -lt 51194 ]; do printf x; i=$((i+1)); done",
            "SUFFIX",
        ),
        (
            "overlap",
            "abab",
            "\"$GIT_ASKPASS\" prompt >/dev/null; printf 'abababTAIL'",
            "abab",
        ),
        (
            "streams",
            "prefixSUFFIX",
            "value=$(\"$GIT_ASKPASS\" prompt); printf '%s stdout\n' \"$value\"; printf '%s stderr\n' \"$value\" >&2",
            "SUFFIX",
        ),
    ] {
        let mut f = Fixture::new(Sandbox::DangerFullAccess)?;
        let broker = askpass(&mut f, true)?;
        let prompts = broker.take_requests().ok_or("missing prompts")?;
        std::thread::scope(|scope| -> TestResult {
            let responder = scope.spawn(move || -> Result<(), String> {
                let prompt = prompts.recv().map_err(|e| e.to_string())?;
                prompt
                    .reply
                    .send(benitoiteh::askpass::Answer::Secret(
                        zeroize::Zeroizing::new(secret.into()),
                    ))
                    .map_err(|_| "answer failed".to_owned())?;
                Ok(())
            });
            let output = f.execute(id, json!({"command":command}), &AtomicBool::new(false))?;
            responder.join().map_err(|_| "responder panicked")??;
            assert!(!output.is_error, "{}", output.text);
            assert!(
                !output.text.contains(secret) && !output.text.contains(forbidden),
                "{}",
                output.text
            );
            assert!(output.spill.is_none());
            assert_eq!(
                fs::read_dir(f.session.directory().join("spill"))?.count(),
                0
            );
            if id == "overlap" {
                assert!(output.text.starts_with("[secret]TAIL"), "{}", output.text);
            } else if id == "streams" {
                assert_eq!(output.text.matches("[secret]").count(), 2);
                assert!(output.text.contains("stdout") && output.text.contains("stderr"));
            }
            Ok(())
        })?;
    }
    Ok(())
}

// 関門: 応答前に既にある spill の一時ファイルも許さない。求めの受信で実行の順番を決める。
#[test]
fn mediated_commands_never_create_spill_files_even_before_the_secret_answer() -> TestResult {
    for answer in [Some("synthetic-output-secret"), None] {
        let mut f = Fixture::new(Sandbox::DangerFullAccess)?;
        let broker = askpass(&mut f, true)?;
        let prompts = broker.take_requests().ok_or("missing prompts")?;
        let spill_directory = f.session.directory().join("spill");
        std::thread::scope(|scope| -> TestResult {
            let responder = scope.spawn(move || -> Result<(), String> {
                let prompt = prompts.recv().map_err(|e| e.to_string())?;
                assert_eq!(
                    fs::read_dir(&spill_directory)
                        .map_err(|e| e.to_string())?
                        .count(),
                    0
                );
                prompt
                    .reply
                    .send(match answer {
                        Some(value) => benitoiteh::askpass::Answer::Secret(
                            zeroize::Zeroizing::new(value.into()),
                        ),
                        None => benitoiteh::askpass::Answer::Cancelled,
                    })
                    .map_err(|_| "answer failed".to_owned())?;
                Ok(())
            });
            let output = f.execute("no-spill", json!({"command":"i=0; while [ $i -lt 2100 ]; do printf 'before-input\n'; i=$((i+1)); done; \"$GIT_ASKPASS\" password"}), &AtomicBool::new(false))?;
            responder.join().map_err(|_| "responder panicked")??;
            assert!(output.spill.is_none());
            assert_eq!(
                fs::read_dir(f.session.directory().join("spill"))?.count(),
                0
            );
            assert!(!output.text.contains("synthetic-output-secret"));
            Ok(())
        })?;
    }
    Ok(())
}
