//! 起動時の方針の選択と、使えないサンドボックスで続ける条件を確かめる（実装プラン P3-02）。

use std::cell::Cell;
use std::error::Error;
use std::fs;
use std::sync::atomic::AtomicBool;

use super::*;
use crate::provider::UserPartKind;

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Result<Self, Box<dyn Error>> {
        let id = SessionId::generate(SystemTime::now())?;
        let root = std::env::temp_dir().join(format!("benitoiteh-normal-startup-{id}"));
        fs::create_dir(&root)?;
        Ok(Self(fs::canonicalize(root)?))
    }

    fn paths(&self, normal: &str) -> Result<DataPaths, Box<dyn Error>> {
        for name in ["home", "project", "tmp", "config/benitoiteh"] {
            fs::create_dir_all(self.0.join(name))?;
        }
        let paths = DataPaths::from_values(
            Some(&self.0.join("home")),
            Some(&self.0.join("config")),
            Some(&self.0.join("state")),
            Some(&self.0.join("tmp")),
        )?;
        fs::write(
            &paths.config_file,
            format!(
                "default_mode = 'normal'\ndefault_provider = 'mock'\n[providers.mock]\nkind = 'openai-chat'\nmodel = 'mock'\n[normal]\n{normal}\n"
            ),
        )?;
        Ok(paths)
    }

    fn args(&self, options: &[&str]) -> Result<ExecArgs, Box<dyn Error>> {
        let mut args = ExecArgs::parse(options.iter().copied().chain(["request"]))?;
        args.project = Some(self.0.join("project"));
        Ok(args)
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _removed = fs::remove_dir_all(&self.0);
    }
}

// 作成時の関門: 実際の起動の設定解決とループを通す。OS の機能の欠落だけを差し替える
// （P3-02 の明示した指示）。方針の単体の検査では、起動時の渡し忘れを検出できない。
#[test]
fn startup_selects_policy_and_fails_closed_unless_explicitly_disabled() -> Result<(), Box<dyn Error>>
{
    for (normal, options, available, expected, probe_expected, disabled) in [
        ("", vec![], true, Some("workspace"), true, false),
        (
            "sandbox = 'read-only'\nnetwork = true",
            vec![],
            true,
            Some("read-only"),
            true,
            false,
        ),
        (
            "sandbox = 'read-only'\nnetwork = true",
            vec!["--sandbox", "workspace-write"],
            true,
            Some("network"),
            true,
            false,
        ),
        (
            "sandbox = 'danger-full-access'",
            vec![],
            false,
            Some("none"),
            false,
            false,
        ),
        ("", vec![], false, None, true, false),
        ("", vec!["--no-sandbox"], false, Some("none"), true, true),
        (
            "",
            vec!["--no-sandbox", "--approval", "never"],
            true,
            Some("workspace"),
            true,
            false,
        ),
    ] {
        let f = Fixture::new()?;
        // 利用者の approval は exec には適用しない。
        let paths = f.paths(&format!("{normal}\napproval = 'untrusted'"))?;
        let args = f.args(&options)?;
        let probed = Cell::new(false);
        let prepared = prepare(
            &args,
            None,
            &paths,
            |helper, scratch| {
                probed.set(true);
                assert!(helper.is_absolute());
                assert!(scratch.starts_with(&paths.sessions_dir));
                assert_eq!(scratch.file_name(), Some(std::ffi::OsStr::new("sandbox")));
                assert_eq!(
                    fs::metadata(scratch)
                        .map_err(SandboxError::Io)?
                        .permissions()
                        .mode()
                        & 0o7777,
                    0o700
                );
                if available {
                    Ok(())
                } else {
                    Err(SandboxError::Unavailable)
                }
            },
            &mut |_| false,
        );
        assert_eq!(probed.get(), probe_expected);
        let Some(expected) = expected else {
            assert!(matches!(
                prepared,
                Err(PreparationError {
                    error: StartupError::Sandbox(SandboxError::Unavailable),
                    ..
                })
            ));
            assert_eq!(fs::read_dir(&paths.temporary_root)?.count(), 0);
            continue;
        };
        let Prepared {
            mut agent, session, ..
        } = prepared.map_err(|e| io::Error::other(e.to_string()))?;
        let (sender, receiver) = mpsc::sync_channel(64);
        // 依頼と開始通知を記録した後、API を呼ぶ前に取り消す。
        agent.run("request".into(), Arc::new(AtomicBool::new(true)), sender)?;
        let events = receiver.into_iter().collect::<Vec<_>>();
        let start = events.first().ok_or("missing session_start")?;
        assert!(
            matches!(start, crate::agent::AgentEvent::SessionStart { notice: Some(notice), .. }
            if notice.contains(text::SANDBOX_DISABLED) == disabled)
        );
        let environment = agent
            .conversation()
            .items
            .iter()
            .find_map(|item| match item {
                Item::UserMessage(message) => message
                    .parts
                    .iter()
                    .find(|part| part.kind == UserPartKind::Environment)
                    .map(|part| part.text.as_str()),
                Item::AssistantTurn(_) | Item::ToolResult(_) => None,
            })
            .ok_or("missing environment")?;
        match expected {
            "none" => {
                assert!(environment.contains("<writable_roots>[]</writable_roots>"));
                assert!(environment.contains("<network>allowed</network>"));
            }
            "read-only" => {
                assert!(!environment.contains(&format!("\"{}\"", f.0.join("project").display())));
                assert!(environment.contains("<network>denied</network>"));
            }
            _ => {
                assert!(environment.contains(&format!("\"{}\"", f.0.join("project").display())));
                assert!(environment.contains(if expected == "network" {
                    "<network>allowed</network>"
                } else {
                    "<network>denied</network>"
                }));
            }
        }
        assert_eq!(
            fs::metadata(paths.data_dir.join("placeholders"))?
                .permissions()
                .mode()
                & 0o7777,
            0o700
        );
        drop(agent);
        drop(session);
        panic::install(None, None);
        assert_eq!(fs::read_dir(&paths.temporary_root)?.count(), 0);
    }
    Ok(())
}

// 統合テストと同じ模擬の提供者を使う。この起動の検査では SSE の一括送信だけを使う。
#[allow(dead_code)]
#[path = "../../../tests/support/mock_provider.rs"]
mod mock_provider;

// 作成時の関門: --no-sandbox の起動で選んだ方針を落とす退行を捕まえる。
// P3-04 の指示に従い、OS の機能が使えないことだけを差し替える。
#[test]
fn unavailable_sandbox_keeps_write_approval_policy_and_configured_roots()
-> Result<(), Box<dyn Error>> {
    use mock_provider::{MockProvider, ResponseScript};
    use serde_json::json;
    for (sandbox, location, denied) in [
        ("workspace-write", "project/.benitoiteh/blocked", true),
        ("read-only", "project/blocked", true),
        ("workspace-write", "additional/allowed", false),
    ] {
        let f = Fixture::new()?;
        let path = f.0.join(location);
        let body = format!(
            "data: {}\n\ndata: {}\n\ndata: [DONE]\n\n",
            json!({"choices":[{"delta":{"tool_calls":[{"index":0,"id":"write","type":"function","function":{"name":"write","arguments":json!({"path":path,"content":"changed"}).to_string()}}]},"finish_reason":null}]}),
            json!({"choices":[{"delta":{},"finish_reason":"tool_calls"}]})
        );
        let mut server = MockProvider::start(vec![
            ResponseScript::sse(body),
            ResponseScript::sse(
                "data: {\"choices\":[{\"delta\":{\"content\":\"Done\"},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n",
            ),
        ])?;
        fs::create_dir(f.0.join("additional"))?;
        let paths = f.paths(&format!(
            "writable_roots = ['{}']",
            f.0.join("additional").display()
        ))?;
        let config = fs::read_to_string(&paths.config_file)?.replace(
            "model = 'mock'",
            &format!("model = 'mock'\nbase_url = '{}'", server.base_url()),
        );
        fs::write(&paths.config_file, config)?;
        let args = f.args(&["--sandbox", sandbox, "--no-sandbox"])?;
        let Prepared {
            mut agent, session, ..
        } = prepare(
            &args,
            None,
            &paths,
            |_, _| Err(SandboxError::Unavailable),
            &mut |_| false,
        )
        .map_err(|e| io::Error::other(e.to_string()))?;
        let (sender, receiver) = mpsc::sync_channel(64);
        let report = agent.run("request".into(), Arc::new(AtomicBool::new(false)), sender)?;
        assert_eq!(report.outcome, Outcome::Completed);
        assert_eq!(report.summary.approvals.policy_denied, u64::from(denied));
        assert_eq!(path.exists(), !denied);
        assert!(!receiver.into_iter().any(|e| matches!(
            e,
            crate::agent::AgentEvent::ApprovalRequest { .. }
                | crate::agent::AgentEvent::ApprovalResult { .. }
        )));
        drop(agent);
        drop(session);
        panic::install(None, None);
        server.shutdown()?;
    }
    Ok(())
}

// 関門: モードの変更を拒んだ後に cleanup や Agent の生成が記録を変える退行を守る。
#[test]
fn declined_normal_resume_preserves_record_bytes_and_releases_lock() -> Result<(), Box<dyn Error>> {
    let f = Fixture::new()?;
    let paths = f.paths("sandbox = 'danger-full-access'")?;
    let restricted = f.args(&["--mode", "restricted"])?;
    let Prepared {
        mut agent, session, ..
    } = prepare(&restricted, None, &paths, |_, _| Ok(()), &mut |_| false)
        .map_err(|e| io::Error::other(e.to_string()))?;
    let id = session.id().clone();
    let record = session.directory().join("session.jsonl");
    let (sender, _receiver) = mpsc::sync_channel(64);
    agent.run(
        "previous context".into(),
        Arc::new(AtomicBool::new(true)),
        sender,
    )?;
    drop(agent);
    drop(session);
    // 末尾の不完全な行も、書き手を呼ぶまでは切り詰めてはいけない。
    use std::io::Write;
    std::fs::OpenOptions::new()
        .append(true)
        .open(&record)?
        .write_all(b"{\"partial\":")?;
    let before = fs::read(&record)?;
    let mut args = f.args(&["--mode", "normal", "--resume", id.as_str()])?;
    args.interactive = true;
    let confirmed = Cell::new(false);
    let result = prepare(
        &args,
        None,
        &paths,
        |_, _| Err(SandboxError::Unavailable),
        &mut |change| {
            assert_eq!(change.session_id, id);
            confirmed.set(true);
            false
        },
    );
    assert!(matches!(
        result,
        Err(PreparationError {
            error: StartupError::Declined,
            ..
        })
    ));
    assert!(confirmed.get());
    assert_eq!(fs::read(&record)?, before);
    drop(Session::open(&paths, &id)?);
    assert_eq!(fs::read(&record)?, before);
    args.interactive = false;
    let result = prepare(&args, None, &paths, |_, _| Ok(()), &mut |_| {
        confirmed.set(false);
        true
    });
    assert!(matches!(
        result,
        Err(PreparationError {
            error: StartupError::Config(_),
            ..
        })
    ));
    assert!(confirmed.get());
    assert_eq!(fs::read(&record)?, before);
    Ok(())
}

// 関門: 起動で対話の方針を never に固定する退行と、制限モードで web_fetch の確認を落とす退行を守る。
#[test]
fn interactive_preparation_uses_configured_approval_and_restricted_web_confirmation()
-> Result<(), Box<dyn Error>> {
    use crate::approval::{ApprovalAnswer, ApprovalOutcome, ApprovalRecord, Approver};
    use mock_provider::{MockProvider, ResponseScript};
    struct Decline {
        seen: Arc<std::sync::Mutex<Vec<ApprovalRecord>>>,
    }
    impl Approver for Decline {
        fn approve(&mut self, request: &ApprovalRecord) -> ApprovalAnswer {
            self.seen
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push(request.clone());
            ApprovalAnswer {
                outcome: ApprovalOutcome::Denied,
                message: None,
            }
        }
    }
    for (mode, normal, option, tool, arguments, asks) in [
        (
            "normal",
            "",
            None,
            "bash",
            serde_json::json!({"command":"printf hello"}),
            false,
        ),
        (
            "normal",
            "approval = 'untrusted'",
            None,
            "bash",
            serde_json::json!({"command":"printf hello"}),
            true,
        ),
        (
            "normal",
            "approval = 'never'",
            Some("untrusted"),
            "bash",
            serde_json::json!({"command":"printf hello"}),
            true,
        ),
        (
            "normal",
            "",
            None,
            "write",
            serde_json::json!({"path":".benitoiteh/protected", "content":"bad"}),
            true,
        ),
        (
            "restricted",
            "",
            None,
            "web_fetch",
            serde_json::json!({"url":"https://example.com/"}),
            true,
        ),
    ] {
        let f = Fixture::new()?;
        let body = format!(
            "data: {}\n\ndata: [DONE]\n\n",
            serde_json::json!({"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call","type":"function","function":{"name":tool,"arguments":arguments.to_string()}}]},"finish_reason":"tool_calls"}]})
        );
        let mut server = MockProvider::start(vec![
            ResponseScript::sse(body),
            ResponseScript::sse(
                "data: {\"choices\":[{\"delta\":{\"content\":\"Done\"},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n",
            ),
        ])?;
        let paths = f.paths(normal)?;
        let contents = fs::read_to_string(&paths.config_file)?.replace(
            "model = 'mock'",
            &format!("model = 'mock'\nbase_url = '{}'", server.base_url()),
        );
        fs::write(&paths.config_file, contents)?;
        let mut args = f.args(&["--mode", mode])?;
        args.interactive = true;
        args.approval = option.map(|_| Approval::Untrusted);
        // OS の機能だけを差し替え、書く範囲の方針は workspace-write のままにする。
        args.no_sandbox = mode == "normal";
        let Prepared { agent, session, .. } = prepare(
            &args,
            None,
            &paths,
            |_, _| Err(SandboxError::Unavailable),
            &mut |_| false,
        )
        .map_err(|error| io::Error::other(error.to_string()))?;
        let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
        let mut agent = agent.with_approver(Box::new(Decline {
            seen: Arc::clone(&seen),
        }));
        let (sender, _receiver) = mpsc::sync_channel(64);
        let report = agent.run("request".into(), Arc::new(AtomicBool::new(false)), sender)?;
        assert_eq!(report.outcome, Outcome::Completed);
        assert_eq!(
            seen.lock().map_err(|_| "poisoned")?.len(),
            usize::from(asks)
        );
        drop(agent);
        drop(session);
        server.shutdown()?;
    }
    Ok(())
}

// 関門: resume の記録と設定の既定値が違っても、/new が安全の方針を変えないことを起動の境界で守る。
#[test]
fn new_after_restricted_resume_preserves_mode_and_project_and_normal_preserves_policies()
-> Result<(), Box<dyn Error>> {
    for (mode, sandbox) in [
        (Mode::Restricted, Sandbox::ReadOnly),
        (Mode::Normal, Sandbox::ReadOnly),
        (Mode::Normal, Sandbox::WorkspaceWrite),
    ] {
        let f = Fixture::new()?;
        let old_root = f.0.join("old-writable");
        let alias = f.0.join("writable-alias");
        let extra_root = f.0.join("new-writable");
        for path in [&old_root, &extra_root] {
            fs::create_dir(path)?;
        }
        std::os::unix::fs::symlink(&old_root, &alias)?;
        let paths = f.paths(&format!(
            "sandbox = '{}'\napproval = 'untrusted'\nnetwork = false\nwritable_roots = [{}]",
            sandbox.name(),
            serde_json::to_string(&alias)?
        ))?;
        let mut config = fs::read_to_string(&paths.config_file)?;
        config.push_str("[restricted]\nallowed_programs = ['/bin/sh']\n");
        fs::write(&paths.config_file, config)?;
        let mut original = f.args(&[
            "--mode",
            if mode == Mode::Restricted {
                "restricted"
            } else {
                "normal"
            },
        ])?;
        original.interactive = true;
        let mut initial = prepare(&original, None, &paths, |_, _| Ok(()), &mut |_| false)
            .map_err(|error| io::Error::other(error.to_string()))?;
        let id = initial.session.id().to_string();
        let (tx, _rx) = mpsc::sync_channel(64);
        initial
            .agent
            .run("old request".into(), Arc::new(AtomicBool::new(true)), tx)?;
        drop(initial);
        let mut resumed_args = original.clone();
        resumed_args.mode = None;
        resumed_args.project = None;
        resumed_args.resume = Some(id.clone());
        let resumed = prepare(&resumed_args, None, &paths, |_, _| Ok(()), &mut |_| false)
            .map_err(|error| io::Error::other(error.to_string()))?;
        assert_eq!(resumed.agent.status().mode, mode);
        let old_safety = resumed.agent.status().safety;
        if mode == Mode::Normal {
            assert_eq!(old_safety.writable_roots, std::slice::from_ref(&old_root));
        } else {
            assert_eq!(old_safety.allowed_programs, [fs::canonicalize("/bin/sh")?]);
        }
        // 元の起動の依頼を送らず、設定の変更のうち provider/model は読み直す。
        fs::write(
            &paths.config_file,
            format!(
                "default_mode = 'normal'\ndefault_provider = 'changed'\n[providers.changed]\nkind = 'openai-chat'\nmodel = 'new-model'\n[normal]\nsandbox = 'danger-full-access'\napproval = 'never'\nnetwork = true\nwritable_roots = [{}]\n[restricted]\nallowed_programs = ['/bin/ls']\n",
                serde_json::to_string(&extra_root)?
            ),
        )?;
        let new = prepare_new(
            &resumed_args,
            &resumed.agent.status(),
            &paths,
            |_, _| Ok(()),
        )
        .map_err(|error| io::Error::other(error.to_string()))?;
        let status = new.agent.status();
        assert_ne!(status.session_id, id);
        assert_eq!(status.mode, mode);
        assert_eq!(status.project, fs::canonicalize(f.0.join("project"))?);
        assert_eq!(status.provider, "changed");
        assert_eq!(status.model, "new-model");
        assert!(new.agent.conversation().items.is_empty());
        assert!(
            !crate::session::read_records(new.session.directory())?
                .iter()
                .any(|record| matches!(record, Record::User { .. }))
        );
        if mode == Mode::Normal {
            assert_eq!(status.sandbox, sandbox);
            assert_eq!(status.approval, Approval::Untrusted);
        }
        assert_eq!(status.safety, old_safety);
        assert_eq!(resumed.agent.status().session_id, id);
        if mode == Mode::Normal {
            // 解決済みのパスにリンクが差し込まれたら、再検査で別の場所への拡大を拒む。
            fs::remove_dir(&old_root)?;
            std::os::unix::fs::symlink(&extra_root, &old_root)?;
            assert!(matches!(
                prepare_new(
                    &resumed_args,
                    &resumed.agent.status(),
                    &paths,
                    |_, _| Ok(())
                ),
                Err(PreparationError {
                    error: StartupError::Invalid(text::SESSION_PATH_CHANGED),
                    ..
                })
            ));
        }
    }
    Ok(())
}

// 関門: /new のとき OS の機能が使えるようになっても、外したサンドボックスの状態を勝手に替えない。
#[test]
fn new_preserves_explicitly_disabled_sandbox_without_reprobing() -> Result<(), Box<dyn Error>> {
    let f = Fixture::new()?;
    let paths = f.paths("sandbox = 'workspace-write'\napproval = 'untrusted'")?;
    let mut args = f.args(&["--no-sandbox"])?;
    args.interactive = true;
    let old = prepare(
        &args,
        None,
        &paths,
        |_, _| Err(SandboxError::Unavailable),
        &mut |_| false,
    )
    .map_err(|error| io::Error::other(error.to_string()))?;
    assert!(old.agent.status().sandbox_disabled);
    let probed = Cell::new(false);
    let new = prepare_new(&args, &old.agent.status(), &paths, |_, _| {
        probed.set(true);
        Ok(())
    })
    .map_err(|error| io::Error::other(error.to_string()))?;
    assert!(!probed.get());
    assert!(new.agent.status().sandbox_disabled);
    assert!(new.status.sandbox_disabled);
    assert_eq!(new.agent.status().sandbox, Sandbox::WorkspaceWrite);
    Ok(())
}

// 関門: 固定した許可プログラムのすり替えを、セッションを作る前に同じ誤りで拒む。
#[test]
fn new_rejects_changed_allowed_program_path_before_creating_a_session() -> Result<(), Box<dyn Error>>
{
    let f = Fixture::new()?;
    let paths = f.paths("")?;
    let program = f.0.join("program-a");
    let other = f.0.join("program-b");
    for path in [&program, &other] {
        fs::write(path, "#!/bin/sh\nexit 0\n")?;
        fs::set_permissions(path, fs::Permissions::from_mode(0o755))?;
    }
    let mut config = fs::read_to_string(&paths.config_file)?;
    config.push_str(&format!(
        "[restricted]\nallowed_programs = [{}]\n",
        serde_json::to_string(&program)?
    ));
    fs::write(&paths.config_file, config)?;
    let mut args = f.args(&["--mode", "restricted"])?;
    args.interactive = true;
    let old = prepare(&args, None, &paths, |_, _| Ok(()), &mut |_| false)
        .map_err(|error| io::Error::other(error.to_string()))?;
    let count = fs::read_dir(&paths.sessions_dir)?.count();
    fs::remove_file(&program)?;
    std::os::unix::fs::symlink(&other, &program)?;
    assert!(matches!(
        prepare_new(&args, &old.agent.status(), &paths, |_, _| Ok(())),
        Err(PreparationError {
            error: StartupError::Invalid(text::SESSION_PATH_CHANGED),
            ..
        })
    ));
    assert_eq!(fs::read_dir(&paths.sessions_dir)?.count(), count);
    Ok(())
}
