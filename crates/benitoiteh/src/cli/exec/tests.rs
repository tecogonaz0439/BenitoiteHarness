//! 起動時の方針の選択と、使えないサンドボックスで続ける条件を確かめる（実装プラン P3-02）。

use std::cell::Cell;
use std::error::Error;
use std::fs;
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};

use super::*;
use crate::provider::UserPartKind;

fn test_processor() -> Result<PathBuf, Box<dyn Error>> {
    Ok(fs::canonicalize(
        std::env::var_os("BENITOITEH_TEST_BENITOITE")
            .filter(|path| !path.is_empty())
            .ok_or(
                "BENITOITEH_TEST_BENITOITE is required: set it to a benitoite v0.0.7 executable.",
            )?,
    )?)
}
fn prepare(
    args: &ExecArgs,
    overrides: Option<&SessionOverrides>,
    paths: &DataPaths,
    mut check: impl FnMut(&Path, &Path) -> Result<(), SandboxError>,
    confirm: &mut dyn FnMut(&ModeChange) -> bool,
) -> Result<Prepared, PreparationError> {
    // 並行するテストの fork が継承した錠は exec まで残る。再開時の Locked だけを
    // 短時間待ち、本当に錠を放さない退行とほかの誤りは失敗にする（設計書 07-01）。
    let started = Instant::now();
    loop {
        match super::prepare(
            args,
            overrides,
            paths,
            &mut check,
            QueryLauncher::TestDirect,
            confirm,
        ) {
            Err(PreparationError {
                error: StartupError::Session(SessionError::Locked),
                ..
            }) if args.resume.is_some() && started.elapsed() < Duration::from_secs(2) => {
                thread::sleep(Duration::from_millis(20));
            }
            result => return result,
        }
    }
}
fn prepare_new(
    args: &ExecArgs,
    current: &crate::agent::AgentStatus,
    paths: &DataPaths,
    check: impl FnOnce(&Path, &Path) -> Result<(), SandboxError>,
) -> Result<Prepared, PreparationError> {
    super::prepare_new(args, current, paths, check, QueryLauncher::TestDirect)
}

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
                "default_mode = 'normal'\ndefault_provider = 'mock'\n[providers.mock]\nkind = 'openai-chat'\nmodel = 'mock'\n[benitoite]\npath = {}\n[normal]\n{normal}\n",
                serde_json::to_string(&test_processor()?)?
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

// 関門: cleanup の報告だけでは起動処理による通知の取り落としを捕まえない。
// 他の起動テストと同じ準備の入口から、成功と画面へ渡す診断を確かめる。
#[test]
fn startup_continues_with_a_diagnostic_when_session_cleanup_fails() -> Result<(), Box<dyn Error>> {
    if rustix::process::geteuid().is_root() {
        eprintln!("Skipped cleanup startup check: root can read mode 000 directories.");
        return Ok(());
    }
    struct Restore(PathBuf);
    impl Drop for Restore {
        fn drop(&mut self) {
            let _restored = fs::set_permissions(&self.0, fs::Permissions::from_mode(0o700));
        }
    }
    let fixture = Fixture::new()?;
    let paths = fixture.paths("sandbox = 'danger-full-access'")?;
    let stale = Session::create(&paths, SessionId::generate(SystemTime::now())?)?;
    let id = stale.id().clone();
    let directory = stale.directory().to_owned();
    fs::File::open(directory.join("session.jsonl"))?.set_modified(SystemTime::UNIX_EPOCH)?;
    let blocked = directory.join("blocked");
    fs::create_dir(&blocked)?;
    fs::write(blocked.join("keep"), b"keep")?;
    fs::set_permissions(&blocked, fs::Permissions::from_mode(0o000))?;
    let _restore = Restore(blocked);
    drop(stale);
    let prepared = prepare(
        &fixture.args(&[])?,
        None,
        &paths,
        |_, _| Ok(()),
        &mut |_| false,
    )
    .map_err(|error| error.to_string())?;
    assert_ne!(prepared.agent.status().session_id, id.as_str());
    assert!(
        prepared
            .diagnostics
            .iter()
            .any(|message| message.contains("Could not clean up session")
                && message.contains(id.as_str()))
    );
    assert!(directory.join("lock").exists());
    Ok(())
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
            true,
            false,
        ),
        ("", vec![], false, None, true, false),
        ("sandbox = 'read-only'", vec![], false, None, true, false),
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
    // prepare と同じく、ほかのテストの fork が継承した錠だけを短時間待つ。
    let started = Instant::now();
    let reopened = loop {
        match Session::open(&paths, &id) {
            Err(SessionError::Locked) if started.elapsed() < Duration::from_secs(2) => {
                thread::sleep(Duration::from_millis(20));
            }
            result => break result?,
        }
    };
    drop(reopened);
    assert_eq!(fs::read(&record)?, before);
    args.interactive = false;
    let result = prepare(&args, None, &paths, |_, _| Ok(()), &mut |_| {
        confirmed.set(false);
        true
    });
    let actual = result.as_ref().err().map(ToString::to_string);
    assert!(
        matches!(
            result,
            Err(PreparationError {
                error: StartupError::Config(_),
                ..
            })
        ),
        "unexpected preparation result: {actual:?}"
    );
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
            |_, _| {
                if mode == "restricted" {
                    Ok(())
                } else {
                    Err(SandboxError::Unavailable)
                }
            },
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
        config.push_str(
            "[restricted]\nallowed_programs = []\nscratch_auto_effects = ['File.Read']\n",
        );
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
            assert!(old_safety.allowed_programs.is_empty());
            assert_eq!(old_safety.scratch_auto_effects, ["File.Read"]);
        }
        // 元の起動の依頼を送らず、設定の変更のうち provider/model は読み直す。
        fs::write(
            &paths.config_file,
            format!(
                "default_mode = 'normal'\ndefault_provider = 'changed'\n[providers.changed]\nkind = 'openai-chat'\nmodel = 'new-model'\n[normal]\nsandbox = 'danger-full-access'\napproval = 'never'\nnetwork = true\nwritable_roots = [{}]\n[restricted]\nallowed_programs = ['/bin/ls']\nscratch_auto_effects = ['Clock.Time']\n",
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
        fs::copy(test_processor()?, path)?;
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

// 関門: モードごとの失敗の扱いと、プロジェクトの設定からの処理系のすり替えを起動の境界で守る。
#[test]
fn processor_startup_fails_closed_in_restricted_mode_and_disables_tools_in_normal_mode()
-> Result<(), Box<dyn Error>> {
    for (mode, failure) in [
        ("restricted", "sandbox"),
        ("normal", "sandbox"),
        ("restricted", "processor"),
        ("normal", "processor"),
    ] {
        let f = Fixture::new()?;
        let paths = f.paths("sandbox = 'danger-full-access'")?;
        let mut args = f.args(&["--mode", mode])?;
        if failure == "processor" {
            let config = fs::read_to_string(&paths.config_file)?.replace(
                &serde_json::to_string(&test_processor()?)?,
                &serde_json::to_string(&f.0.join("missing"))?,
            );
            fs::write(&paths.config_file, config)?;
        }
        let prepared = prepare(
            &args,
            None,
            &paths,
            |_, _| {
                if failure == "sandbox" {
                    Err(SandboxError::Unavailable)
                } else {
                    Ok(())
                }
            },
            &mut |_| false,
        );
        if mode == "restricted" {
            let error = prepared.err().ok_or("restricted startup must fail")?.error;
            if failure == "sandbox" {
                assert!(matches!(error, StartupError::Sandbox(_)), "{error}");
            } else {
                assert!(matches!(error, StartupError::Benitoite(_)), "{error}");
            }
        } else {
            let prepared = prepared.map_err(|e| e.to_string())?;
            assert!(prepared.agent.status().benitoite_path.is_none());
            assert!(
                prepared
                    .diagnostics
                    .iter()
                    .any(|line| line.contains("Benitoite tools are disabled"))
            );
        }
        // 信頼しないプロジェクトの path は使われず、利用者の処理系で起動する。
        args.mode = Some(Mode::Restricted);
        let config = fs::read_to_string(&paths.config_file)?.replace(
            &serde_json::to_string(&f.0.join("missing"))?,
            &serde_json::to_string(&test_processor()?)?,
        );
        fs::write(&paths.config_file, config)?;
        fs::create_dir(f.0.join("project/.benitoiteh"))?;
        fs::write(
            f.0.join("project/.benitoiteh/config.toml"),
            "[benitoite]\npath = '/missing-project-processor'\n",
        )?;
        let prepared = prepare(&args, None, &paths, |_, _| Ok(()), &mut |_| false)
            .map_err(|e| e.to_string())?;
        assert_eq!(
            prepared.agent.status().benitoite_path,
            Some(test_processor()?)
        );
        assert!(
            prepared
                .diagnostics
                .iter()
                .any(|line| line.contains("Ignoring project configuration"))
        );
    }
    Ok(())
}

// 関門: ヘッダの比較に錠を使う、今のセッション自身を比較する、再開時にヘッダを上書きする退行を守る。
#[test]
fn changed_processor_is_reported_in_diagnostics_and_notice_without_rewriting_resume_header()
-> Result<(), Box<dyn Error>> {
    use std::io::Write;
    let f = Fixture::new()?;
    let paths = f.paths("sandbox = 'danger-full-access'")?;
    let args = f.args(&[])?;
    let old =
        prepare(&args, None, &paths, |_, _| Ok(()), &mut |_| false).map_err(|e| e.to_string())?;
    let id = old.session.id().clone();
    let record_path = old.session.directory().join("session.jsonl");
    let original_header = fs::read_to_string(&record_path)?
        .lines()
        .next()
        .ok_or("header")?
        .to_owned();
    let copy = f.0.join("different-benitoite");
    fs::copy(test_processor()?, &copy)?;
    OpenOptions::new()
        .append(true)
        .open(&copy)?
        .write_all(b"changed executable\n")?;
    let config = fs::read_to_string(&paths.config_file)?.replace(
        &serde_json::to_string(&test_processor()?)?,
        &serde_json::to_string(&copy)?,
    );
    fs::write(&paths.config_file, config)?;
    // 直前のセッションを開いたままでも、比較にはその錠を取らない。
    let current =
        prepare(&args, None, &paths, |_, _| Ok(()), &mut |_| false).map_err(|e| e.to_string())?;
    assert!(
        current
            .diagnostics
            .iter()
            .any(|line| line.contains("executable changed"))
    );
    let same =
        prepare(&args, None, &paths, |_, _| Ok(()), &mut |_| false).map_err(|e| e.to_string())?;
    assert!(
        !same
            .diagnostics
            .iter()
            .any(|line| line.contains("executable changed"))
    );
    drop(old);
    let mut resumed_args = args;
    resumed_args.resume = Some(id.to_string());
    let mut resumed = prepare(&resumed_args, None, &paths, |_, _| Ok(()), &mut |_| false)
        .map_err(|e| e.to_string())?;
    let (sender, receiver) = mpsc::sync_channel(64);
    resumed
        .agent
        .run("request".into(), Arc::new(AtomicBool::new(true)), sender)?;
    assert!(receiver.into_iter().any(|event| matches!(event, crate::agent::AgentEvent::SessionStart { notice: Some(notice), .. } if notice.contains("executable changed") && notice.contains("0.0.7"))));
    assert_eq!(
        fs::read_to_string(record_path)?.lines().next(),
        Some(original_header.as_str())
    );
    Ok(())
}

#[test]
fn new_pins_the_verified_processor_even_when_configuration_changes() -> Result<(), Box<dyn Error>> {
    let f = Fixture::new()?;
    let paths = f.paths("sandbox = 'danger-full-access'")?;
    let args = f.args(&[])?;
    let old =
        prepare(&args, None, &paths, |_, _| Ok(()), &mut |_| false).map_err(|e| e.to_string())?;
    let config = fs::read_to_string(&paths.config_file)?.replace(
        &serde_json::to_string(&test_processor()?)?,
        "'/missing-new-processor'",
    );
    fs::write(&paths.config_file, config)?;
    let new = prepare_new(&args, &old.agent.status(), &paths, |_, _| Ok(()))
        .map_err(|e| e.to_string())?;
    assert_eq!(
        new.agent.status().benitoite_path,
        old.agent.status().benitoite_path
    );
    let header = crate::session::read_records(new.session.directory())?;
    assert!(
        matches!(header.first(), Some(Record::Header(header)) if header.benitoite.as_ref().is_some_and(|processor| processor.sha256.len() == 64))
    );
    Ok(())
}

// 関門: 共通の検証関数が正しくても、起動側が allowed_programs の検証を呼び忘れると制限が抜ける。
#[test]
fn restricted_startup_rejects_interpreter_names_after_resolving_aliases()
-> Result<(), Box<dyn Error>> {
    for name in ["python3.12", "bash", "tclsh8.5", "vim.basic", "alias"] {
        let f = Fixture::new()?;
        let paths = f.paths("")?;
        let real = f.0.join(if name == "alias" { "python3.12" } else { name });
        fs::copy(test_processor()?, &real)?;
        let path = f.0.join(name);
        if name == "alias" {
            std::os::unix::fs::symlink(&real, &path)?;
        }
        let mut config = fs::read_to_string(&paths.config_file)?;
        config.push_str(&format!(
            "[restricted]\nallowed_programs = [{}]\n",
            serde_json::to_string(&path)?
        ));
        fs::write(&paths.config_file, config)?;
        let args = f.args(&["--mode", "restricted"])?;
        assert!(matches!(
            prepare(&args, None, &paths, |_, _| Ok(()), &mut |_| false),
            Err(PreparationError {
                error: StartupError::Benitoite(benitoite::BenitoiteError::Invalid(
                    benitoite::text::BANNED_PROGRAM
                )),
                ..
            })
        ));
        assert!(!paths.sessions_dir.exists());
    }
    Ok(())
}

// 関門: 起動時に未使用だった解除の許可を /new が復活させる退行を起動の境界で守る。
#[test]
fn new_does_not_reuse_unused_no_sandbox_permission() -> Result<(), Box<dyn Error>> {
    let f = Fixture::new()?;
    let paths = f.paths("")?;
    let args = f.args(&["--no-sandbox"])?;
    let old =
        prepare(&args, None, &paths, |_, _| Ok(()), &mut |_| false).map_err(|e| e.to_string())?;
    assert!(!old.agent.status().sandbox_disabled);
    assert!(matches!(
        prepare_new(&args, &old.agent.status(), &paths, |_, _| Err(
            SandboxError::Unavailable
        )),
        Err(PreparationError {
            error: StartupError::Sandbox(SandboxError::Unavailable),
            ..
        })
    ));
    Ok(())
}

// 関門: 過去の処理系の型だけが壊れていても、現在の有効な処理系で起動できる契約を守る。
#[test]
fn unreadable_previous_processor_only_adds_a_diagnostic() -> Result<(), Box<dyn Error>> {
    let f = Fixture::new()?;
    let paths = f.paths("")?;
    let args = f.args(&[])?;
    let old =
        prepare(&args, None, &paths, |_, _| Ok(()), &mut |_| false).map_err(|e| e.to_string())?;
    let record = old.session.directory().join("session.jsonl");
    let mut header: serde_json::Value = serde_json::from_str(&fs::read_to_string(&record)?)?;
    header.as_object_mut().ok_or("header")?.insert(
        "benitoite".into(),
        serde_json::json!({"path":123,"version":false}),
    );
    fs::write(&record, format!("{header}\n"))?;
    let mut current =
        prepare(&args, None, &paths, |_, _| Ok(()), &mut |_| false).map_err(|e| e.to_string())?;
    assert_eq!(
        current.agent.status().benitoite_path,
        Some(test_processor()?)
    );
    assert_eq!(current.diagnostics.iter().filter(|line| line.contains("Could not read the previous session's benitoite identity")).count(), 1);
    let (sender, receiver) = mpsc::sync_channel(64);
    current
        .agent
        .run("request".into(), Arc::new(AtomicBool::new(true)), sender)?;
    let notice = receiver
        .into_iter()
        .find_map(|event| {
            if let crate::agent::AgentEvent::SessionStart { notice, .. } = event {
                notice
            } else {
                None
            }
        })
        .ok_or("notice")?;
    assert!(
        !notice.contains("executable changed") && !notice.contains("Could not read the previous")
    );
    Ok(())
}

// 関門: TestDirect に替える前の本番の選択を、実際の解除状態だけから確かめる。
#[test]
fn query_launcher_and_environment_follow_actual_sandbox_state() -> Result<(), Box<dyn Error>> {
    for (mode, disabled, expected) in [
        (Mode::Restricted, false, QueryLauncher::Sandboxed),
        (Mode::Normal, false, QueryLauncher::Sandboxed),
        (Mode::Normal, true, QueryLauncher::Unsandboxed),
    ] {
        let launcher = select_query_launcher(mode, disabled);
        assert_eq!(launcher, expected);
        let env = child_environment(
            [
                ("BENITOITEH_SANDBOX".into(), "inherited".into()),
                ("BENITOITEH_SANDBOX_NETWORK_DISABLED".into(), "1".into()),
                ("OPENAI_API_KEY".into(), "secret".into()),
            ],
            &Default::default(),
            Path::new("/temporary"),
            launcher.environment_policy(),
        );
        assert!(!env.contains_key(std::ffi::OsStr::new("OPENAI_API_KEY")));
        for key in ["BENITOITEH_SANDBOX", "BENITOITEH_SANDBOX_NETWORK_DISABLED"] {
            assert_eq!(env.contains_key(std::ffi::OsStr::new(key)), !disabled);
        }
    }
    // --no-sandbox が指定されても、利用できる場合は解除状態にならない。
    let f = Fixture::new()?;
    let paths = f.paths("")?;
    let args = f.args(&["--no-sandbox"])?;
    let prepared =
        prepare(&args, None, &paths, |_, _| Ok(()), &mut |_| false).map_err(|e| e.to_string())?;
    assert_eq!(
        select_query_launcher(
            prepared.agent.status().mode,
            prepared.agent.status().sandbox_disabled
        ),
        QueryLauncher::Sandboxed
    );
    Ok(())
}

// 関門: 失敗した Skill を持つ処理系を使い続けないことを、起動時のモード分岐で守る。
#[test]
fn skill_startup_failure_stops_restricted_and_disables_normal() -> Result<(), Box<dyn Error>> {
    for failure in [
        "query",
        "attributes",
        "comment",
        "closing_tag",
        "body_limit",
    ] {
        for mode in [Mode::Restricted, Mode::Normal] {
            let error = match failure {
                "query" => benitoite::BenitoiteError::Exit {
                    stdout: Vec::new(),
                    code: Some(2),
                    stderr: Vec::new(),
                },
                "attributes" => benitoite::skill::skill_parts("---\nname: benitoite\n---\nbody")
                    .err()
                    .ok_or("invalid attributes accepted")?,
                "comment" => benitoite::skill::skill_parts(
                    "---\nname: benitoite\ndescription: desc\n---\n<!-- open",
                )
                .err()
                .ok_or("invalid comment accepted")?,
                "closing_tag" => benitoite::skill::skill_parts(
                    "---\nname: benitoite\ndescription: desc\n---\nbefore</BeNiToItE>after",
                )
                .err()
                .ok_or("closing tag accepted")?,
                "body_limit" => benitoite::skill::skill_parts(&format!(
                    "---\nname: benitoite\ndescription: desc\n---\n{}",
                    "x".repeat(262145)
                ))
                .err()
                .ok_or("oversized body accepted")?,
                _ => return Err("unknown failure".into()),
            };
            let mut diagnostics = Vec::new();
            let result =
                super::select_processor(mode, Err(StartupError::Skill(error)), &mut diagnostics);
            if mode == Mode::Restricted {
                assert!(matches!(result, Err(StartupError::Skill(_))));
            } else {
                let (processor, skill) = result.map_err(|error| error.to_string())?;
                assert!(processor.is_none() && skill.is_none());
                assert_eq!(diagnostics.len(), 1);
                assert!(
                    diagnostics
                        .first()
                        .is_some_and(|line| line.starts_with(benitoite::text::DISABLED))
                );
            }
        }
    }
    Ok(())
}

#[test]
fn prepared_skill_is_in_prompt_tools_header_and_environment_in_both_modes()
-> Result<(), Box<dyn Error>> {
    for mode in ["restricted", "normal"] {
        let f = Fixture::new()?;
        let paths = f.paths("sandbox = 'danger-full-access'")?;
        let args = f.args(&["--mode", mode])?;
        let mut prepared = prepare(&args, None, &paths, |_, _| Ok(()), &mut |_| false)
            .map_err(|error| error.to_string())?;
        let records = session::read_records(prepared.session.directory())?;
        let Some(Record::Header(header)) = records.first() else {
            return Err("missing header".into());
        };
        let processor = header.benitoite.as_ref().ok_or("missing processor")?;
        let output = std::process::Command::new(test_processor()?)
            .args(["skill", "show", "--format=json", "SKILL.md"])
            .output()?;
        assert!(output.status.success());
        let skill = benitoite::skill::SkillDocument::from_json("SKILL.md", &output.stdout)?;
        assert_eq!(
            processor.skill_content_hash.as_deref(),
            Some(skill.content_hash())
        );
        assert!(serde_json::to_value(header)?.get("skill_version").is_none());
        let (system, tools) = if mode == "restricted" {
            (
                include_str!("../../../tests/expected/system-restricted.txt"),
                include_str!("../../../tests/expected/tools-restricted.json"),
            )
        } else {
            (
                include_str!("../../../tests/expected/system-normal.txt"),
                include_str!("../../../tests/expected/tools-normal.json"),
            )
        };
        // 全文と定義の固定値への一致で、起動処理から Agent への渡し忘れを捕まえる。
        assert_eq!(
            header.system_prompt_sha256,
            crate::context::system_prompt_hash(system)
        );
        let tools: serde_json::Value = serde_json::from_str(tools)?;
        assert_eq!(
            header.tools_sha256,
            crate::context::system_prompt_hash(&serde_json::to_string(&tools)?)
        );
        let (sender, receiver) = mpsc::sync_channel(64);
        prepared
            .agent
            .run("request".into(), Arc::new(AtomicBool::new(true)), sender)?;
        let _events = receiver.into_iter().collect::<Vec<_>>();
        assert!(prepared.agent.conversation().items.iter().any(|item| matches!(item, Item::UserMessage(message) if message.parts.iter().any(|part| part.text.contains("<benitoite_version>0.0.7</benitoite_version>")))));
    }
    Ok(())
}

// 関門: 起動時の Skill の誤りにも、道具と同じ文書向けの理由を示す。
#[test]
fn skill_query_failures_use_document_wording_at_startup() -> Result<(), Box<dyn Error>> {
    for mode in [Mode::Restricted, Mode::Normal] {
        for (error, expected) in [
            (
                benitoite::BenitoiteError::OutputLimit,
                "the Skill document from benitoite exceeded 16 MiB.",
            ),
            (
                benitoite::BenitoiteError::Exit {
                    stdout: Vec::new(),
                    code: Some(2),
                    stderr: b"untrusted".to_vec(),
                },
                "benitoite exited with status 2.",
            ),
            (
                benitoite::BenitoiteError::Exit {
                    stdout: Vec::new(),
                    code: None,
                    stderr: Vec::new(),
                },
                "benitoite exited without an exit status.",
            ),
        ] {
            let mut diagnostics = Vec::new();
            let result =
                super::select_processor(mode, Err(StartupError::Skill(error)), &mut diagnostics);
            if mode == Mode::Restricted {
                assert_eq!(
                    result.err().ok_or("startup continued")?.to_string(),
                    expected
                );
            } else {
                let (processor, skill) = result.map_err(|error| error.to_string())?;
                assert!(processor.is_none() && skill.is_none());
                assert_eq!(
                    diagnostics,
                    [format!("Benitoite tools are disabled: {expected}")]
                );
            }
        }
    }
    Ok(())
}

// 関門: 構文の解釈だけでは、設定で解いたモードと実の親の検証・Agent への渡し忘れを捕まえない。
#[test]
fn project_parent_is_resolved_only_for_restricted_exec_and_used_by_the_agent()
-> Result<(), Box<dyn Error>> {
    use crate::agent::AgentEvent;
    use crate::provider::Item;
    use crate::provider::UserPartKind;
    use crate::session::Record;
    use crate::session::ResumeState;
    #[allow(dead_code, clippy::duplicate_mod)]
    #[path = "../../../tests/support/mock_provider.rs"]
    mod mock;
    use mock::{MockProvider, ResponseScript};
    let f = Fixture::new()?;
    let paths = f.paths("")?;
    fs::create_dir(f.0.join("parent"))?;
    std::os::unix::fs::symlink(f.0.join("parent"), f.0.join("alias"))?;
    for (mode, interactive, parent, reason) in [
        ("normal", false, "parent", text::PROJECT_PARENT_RESTRICTED),
        (
            "restricted",
            true,
            "parent",
            text::PROJECT_PARENT_RESTRICTED,
        ),
        (
            "restricted",
            false,
            "missing",
            text::PROJECT_PARENT_DIRECTORY,
        ),
    ] {
        let mut args = f.args(&["--mode", mode])?;
        args.interactive = interactive;
        args.project_parent = Some(f.0.join(parent));
        let error = prepare(&args, None, &paths, |_, _| Ok(()), &mut |_| false)
            .err()
            .ok_or("accepted invalid parent")?;
        assert_eq!(error.to_string(), reason);
        let shown = text::startup_error(&error);
        assert_eq!(shown.matches("benitoiteh:").count(), 1, "{shown}");
        assert_eq!(shown, format!("benitoiteh: {reason}"));
        assert!(!paths.sessions_dir.exists());
    }
    let body = format!(
        "data: {}\n\ndata: [DONE]\n\n",
        serde_json::json!({"choices":[{"delta":{"tool_calls":[{"index":0,"id":"create","type":"function","function":{"name":"create_project","arguments":serde_json::json!({"path":f.0.join("parent/new")}).to_string()}}]},"finish_reason":"tool_calls"}]})
    );
    let done = format!(
        "data: {}\n\ndata: [DONE]\n\n",
        serde_json::json!({"choices":[{"delta":{"content":"Done"},"finish_reason":"stop"}]})
    );
    let mut server =
        MockProvider::start(vec![ResponseScript::sse(body), ResponseScript::sse(done)])?;
    let config = fs::read_to_string(&paths.config_file)?.replace(
        "kind = 'openai-chat'",
        &format!("kind = 'openai-chat'\nbase_url = '{}'", server.base_url()),
    );
    fs::write(&paths.config_file, config)?;
    let mut args = f.args(&["--mode", "restricted"])?;
    args.project_parent = Some(f.0.join("alias"));
    let mut prepared = prepare(&args, None, &paths, |_, _| Ok(()), &mut |_| false)
        .map_err(|error| error.to_string())?;
    let (sender, receiver) = mpsc::sync_channel(64);
    assert_eq!(
        prepared
            .agent
            .run("Create".into(), Arc::new(AtomicBool::new(false)), sender)?
            .outcome,
        crate::agent::Outcome::Completed
    );
    assert!(receiver.into_iter().any(|event| matches!(event, AgentEvent::ProjectDirChanged { project_dir } if project_dir == f.0.join("parent/new"))));
    assert!(f.0.join("parent/new/main.bnt").is_file());
    let records = session::read_records(prepared.session.directory())?;
    assert!(
        records
            .iter()
            .any(|r| matches!(r, Record::ProjectDirChanged { .. }))
    );
    // イベントを記録した後、環境の通知を送る前に終わっても、再開が不足分を補う。
    let record_path = prepared.session.directory().join("session.jsonl");
    let changed = records
        .iter()
        .position(|r| matches!(r, Record::ProjectDirChanged { .. }))
        .ok_or("changed")?;
    let prefix = records.get(..=changed).ok_or("prefix")?;
    let mut serialized = String::new();
    for record in prefix {
        serialized.push_str(&serde_json::to_string(record)?);
        serialized.push('\n');
    }
    fs::write(&record_path, serialized)?;
    let id = prepared.session.id().to_string();
    drop(prepared);
    args.resume = Some(id);
    args.project = None;
    let mut resumed = prepare(&args, None, &paths, |_, _| Ok(()), &mut |_| false)
        .map_err(|error| error.to_string())?;
    assert_eq!(
        ResumeState::read(resumed.session.directory())?.project_dir,
        f.0.join("parent/new")
    );
    let (sender, _receiver) = mpsc::sync_channel(64);
    resumed
        .agent
        .run("Resume".into(), Arc::new(AtomicBool::new(true)), sender)?;
    assert!(resumed.agent.conversation().items.iter().any(|item| matches!(item, Item::UserMessage(message) if message.parts.iter().any(|part| part.kind == UserPartKind::Environment && part.text.contains(&format!("<project_directory>{}</project_directory>", f.0.join("parent/new").display()))))));
    server.shutdown()?;
    Ok(())
}

// 関門: 写した記録のモードで今の安全の値を上書きする退行と、通常への変更確認を
// 誤って求める退行を、再開の準備の入口で捕まえる。LLM は呼ばない。
#[test]
fn fork_resumes_with_current_safety_and_retains_copy_if_preparation_fails()
-> Result<(), Box<dyn Error>> {
    for mode in [Mode::Restricted, Mode::Normal] {
        let f = Fixture::new()?;
        let paths = f.paths("sandbox = 'read-only'\napproval = 'untrusted'")?;
        let mut args = f.args(&["--mode", "restricted"])?;
        args.interactive = true;
        let mut source = prepare(&args, None, &paths, |_, _| Ok(()), &mut |_| false)
            .map_err(|e| e.to_string())?;
        let (tx, _rx) = mpsc::sync_channel(64);
        source
            .agent
            .run("saved request".into(), Arc::new(AtomicBool::new(true)), tx)?;
        let (tx, _rx_second) = mpsc::sync_channel(64);
        source.agent.run(
            "/restore this request".into(),
            Arc::new(AtomicBool::new(true)),
            tx,
        )?;
        let last_request = session::read_records(source.session.directory())?
            .iter()
            .find(|record| {
                session::request_text(record).as_deref() == Some("/restore this request")
            })
            .and_then(Record::node)
            .ok_or("request node")?;
        let original = fs::read(source.session.directory().join("session.jsonl"))?;
        let mut current = source.agent.status();
        current.mode = mode;
        current.sandbox = Sandbox::ReadOnly;
        current.approval = if mode == Mode::Normal {
            Approval::Untrusted
        } else {
            Approval::OnRequest
        };
        current.safety.network = mode == Mode::Normal;
        let (copied, request) = super::prepare_fork(
            &args,
            &current,
            &source.session,
            &paths,
            session::ForkTarget::Current,
            |_, _| Ok(()),
            QueryLauncher::TestDirect,
        )
        .map_err(|e| e.to_string())?;
        let status = copied.agent.status();
        assert_eq!(status.mode, current.mode);
        if mode == Mode::Normal {
            assert_eq!(status.sandbox, current.sandbox);
        }
        assert_eq!(status.approval, current.approval);
        assert_eq!(status.safety, current.safety);
        assert_eq!(status.project, current.project);
        assert_ne!(status.session_id, current.session_id);
        assert_eq!(status.provider, current.provider);
        assert_eq!(status.model, current.model);
        assert!(!copied.agent.conversation().items.is_empty());
        assert!(request.is_none());
        assert!(
            copied
                .diagnostics
                .iter()
                .any(|line| line.contains("Forked session")
                    && line.contains(&current.session_id)
                    && line.contains(&status.session_id))
        );
        assert!(
            !copied
                .diagnostics
                .iter()
                .any(|line| line.contains("Resuming"))
        );
        assert_eq!(
            fs::read(source.session.directory().join("session.jsonl"))?,
            original
        );
        let (before_request, restored) = super::prepare_fork(
            &args,
            &current,
            &source.session,
            &paths,
            session::ForkTarget::BeforeRequest(last_request),
            |_, _| Ok(()),
            QueryLauncher::TestDirect,
        )
        .map_err(|e| e.to_string())?;
        assert_eq!(restored.as_deref(), Some("/restore this request"));
        assert_eq!(
            session::read_records(before_request.session.directory())?
                .iter()
                .filter_map(session::request_text)
                .collect::<Vec<_>>(),
            ["saved request"]
        );
        fs::write(&paths.config_file, "invalid = [")?;
        let before = session::list_sessions(&paths)?.len();
        let error = super::prepare_fork(
            &args,
            &current,
            &source.session,
            &paths,
            session::ForkTarget::Current,
            |_, _| Ok(()),
            QueryLauncher::TestDirect,
        )
        .err()
        .ok_or("preparation should fail")?;
        let entries = session::list_sessions(&paths)?;
        assert_eq!(entries.len(), before + 1);
        assert!(
            entries
                .iter()
                .filter(|entry| entry.id != status.session_id)
                .any(|entry| error
                    .diagnostics
                    .iter()
                    .any(|line| line.contains(&entry.id) && line.contains("retained")))
        );
        assert_eq!(
            fs::read(source.session.directory().join("session.jsonl"))?,
            original
        );
    }
    Ok(())
}

// 関門: 過去の枝の提供者への逆戻りと、新しいキーを古い URL へ送る退行を実際の要求で守る。
#[test]
fn fork_after_returning_to_an_old_branch_keeps_the_current_destination()
-> Result<(), Box<dyn Error>> {
    use mock_provider::{MockProvider, ResponseScript};
    let response = "data: {\"choices\":[{\"delta\":{\"content\":\"Done\"},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n";
    // 古い送り先にも二回目の応答を用意し、誤送信を待ち時間ではなく受信内容で検出する。
    let mut old = MockProvider::start(vec![
        ResponseScript::sse(response),
        ResponseScript::sse(response),
    ])?;
    let mut new = MockProvider::start(vec![ResponseScript::sse(response)])?;
    let f = Fixture::new()?;
    let paths = f.paths("sandbox = 'read-only'\napproval = 'untrusted'")?;
    let config = fs::read_to_string(&paths.config_file)?;
    fs::write(
        &paths.config_file,
        format!(
            "{config}\n[providers.other]\nkind = 'openai-chat'\nmodel = 'configured-b'\nbase_url = '{}'\napi_key = {{ command = ['/bin/echo', 'old-secret'] }}\n",
            old.base_url()
        ),
    )?;
    let mut original = f.args(&[])?;
    original.interactive = true;
    let mut first = prepare(&original, None, &paths, |_, _| Ok(()), &mut |_| false)
        .map_err(|e| e.to_string())?;
    let (events, _received) = mpsc::sync_channel(64);
    first
        .agent
        .run("A request".into(), Arc::new(AtomicBool::new(true)), events)?;
    let first_tip = session::ResumeState::read(first.session.directory())?
        .tree
        .tip;
    let mut args = original.clone();
    args.resume = Some(first.session.id().to_string());
    args.provider = Some("other".into());
    args.model = Some("model-b".into());
    drop(first);
    let mut current =
        prepare(&args, None, &paths, |_, _| Ok(()), &mut |_| false).map_err(|e| e.to_string())?;
    let (events, _received_second) = mpsc::sync_channel(64);
    let report = current
        .agent
        .run("B request".into(), Arc::new(AtomicBool::new(false)), events)?;
    assert_eq!(report.outcome, Outcome::Completed);
    let requests = old.requests()?;
    assert_eq!(requests.len(), 1);
    let request = requests.first().ok_or("missing original B request")?;
    assert_eq!(
        request.body.get("model").and_then(|v| v.as_str()),
        Some("model-b")
    );
    assert_eq!(
        request.headers.get("authorization").map(String::as_str),
        Some("Bearer old-secret")
    );
    let tip = session::ResumeState::read(current.session.directory())?
        .tree
        .tip;
    current.session.writer()?.append(&Record::Branch {
        to: first_tip,
        from: tip,
        checkpoint: None,
    })?;
    // 名前とモデルは今のセッションを引き継ぎ、URL とキーは今の設定から一緒に解く。
    fs::write(
        &paths.config_file,
        fs::read_to_string(&paths.config_file)?
            .replace(&old.base_url(), &new.base_url())
            .replace("old-secret", "new-secret")
            .replace("configured-b", "changed-default"),
    )?;
    let (mut copied, _) = super::prepare_fork(
        &original,
        &current.agent.status(),
        &current.session,
        &paths,
        session::ForkTarget::Current,
        |_, _| Ok(()),
        QueryLauncher::TestDirect,
    )
    .map_err(|e| e.to_string())?;
    assert_eq!(copied.agent.status().provider, "other");
    assert_eq!(copied.agent.status().model, "model-b");
    let (events, _received_copy) = mpsc::sync_channel(64);
    let report = copied.agent.run(
        "new B request".into(),
        Arc::new(AtomicBool::new(false)),
        events,
    )?;
    assert_eq!(report.outcome, Outcome::Completed);
    assert_eq!(old.requests()?.len(), 1);
    let requests = new.requests()?;
    assert_eq!(requests.len(), 1);
    let request = requests.first().ok_or("missing forked B request")?;
    assert_eq!(
        request.body.get("model").and_then(|v| v.as_str()),
        Some("model-b")
    );
    assert_eq!(
        request.headers.get("authorization").map(String::as_str),
        Some("Bearer new-secret")
    );
    let state = session::ResumeState::read(copied.session.directory())?;
    assert_eq!(state.destination.provider, "other");
    assert_eq!(state.destination.model, "model-b");
    assert_eq!(state.destination.base_url, new.base_url());
    old.shutdown()?;
    new.shutdown()?;
    Ok(())
}

// 関門: 設定から提供者が消えたとき、既定の別の提供者へ送らず元のセッションを保つ。
#[test]
fn fork_retains_copy_when_current_provider_was_removed() -> Result<(), Box<dyn Error>> {
    let f = Fixture::new()?;
    let paths = f.paths("sandbox = 'read-only'")?;
    let mut args = f.args(&[])?;
    args.interactive = true;
    let mut source = prepare(&args, None, &paths, |_, _| Ok(()), &mut |_| false)
        .map_err(|error| error.to_string())?;
    let (events, _received) = mpsc::sync_channel(64);
    source.agent.run(
        "source request".into(),
        Arc::new(AtomicBool::new(true)),
        events,
    )?;
    let original = fs::read(source.session.directory().join("session.jsonl"))?;
    fs::write(
        &paths.config_file,
        fs::read_to_string(&paths.config_file)?
            .replace("[providers.mock]", "[providers.replacement]")
            .replace(
                "default_provider = 'mock'",
                "default_provider = 'replacement'",
            ),
    )?;
    let error = super::prepare_fork(
        &args,
        &source.agent.status(),
        &source.session,
        &paths,
        session::ForkTarget::Current,
        |_, _| Ok(()),
        QueryLauncher::TestDirect,
    )
    .err()
    .ok_or("removed provider was accepted")?;
    assert!(matches!(
        error.error,
        StartupError::Invalid(text::FORK_PROVIDER_MISSING)
    ));
    let entries = session::list_sessions(&paths)?;
    assert_eq!(entries.len(), 2);
    let copied = entries
        .iter()
        .find(|entry| entry.id != source.session.id().as_str())
        .ok_or("missing retained fork")?;
    assert!(copied.error.is_none());
    let id = copied.id.parse::<SessionId>()?;
    assert!(error.diagnostics.contains(&text::fork_retained(&id)));
    let retained = Session::open(&paths, &id)?;
    assert_eq!(retained.take_resume_state()?.destination.provider, "mock");
    assert_eq!(
        copied
            .forked_from
            .as_ref()
            .map(|from| from.session.as_str()),
        Some(source.session.id().as_str())
    );
    assert_eq!(
        fs::read(source.session.directory().join("session.jsonl"))?,
        original
    );
    let (events, _received_continue) = mpsc::sync_channel(64);
    let report = source.agent.run(
        "continue source".into(),
        Arc::new(AtomicBool::new(true)),
        events,
    )?;
    assert_eq!(report.outcome, Outcome::Cancelled);
    assert_eq!(
        source.agent.status().session_id,
        source.session.id().as_str()
    );
    assert!(
        session::read_records(source.session.directory())?
            .iter()
            .any(|record| session::request_text(record).as_deref() == Some("continue source"))
    );
    Ok(())
}

// 関門: 写した Session を開き直して錠や一時領域を失う退行を、準備の入口で守る。
#[test]
fn fork_preparation_reuses_the_locked_session_and_cached_resume_state() -> Result<(), Box<dyn Error>>
{
    let f = Fixture::new()?;
    let paths = f.paths("sandbox = 'read-only'")?;
    let mut args = f.args(&[])?;
    args.interactive = true;
    let mut source = prepare(&args, None, &paths, |_, _| Ok(()), &mut |_| false)
        .map_err(|error| error.to_string())?;
    let (events, _received) = mpsc::sync_channel(64);
    source.agent.run(
        "source request".into(),
        Arc::new(AtomicBool::new(true)),
        events,
    )?;
    let forked = source.session.fork(
        &paths,
        &source.agent.status().project,
        session::ForkTarget::Current,
    )?;
    let id = forked.session.id().clone();
    let marker = forked.session.temporary_dir().join("kept-while-preparing");
    fs::write(&marker, b"kept")?;
    let mut fixed = SessionOverrides::from(&source.agent.status());
    assert_eq!(forked.from.session, source.session.id().to_string());
    assert!(forked.request.is_none());
    fixed.forked_from = Some(forked.from);
    fixed.fork_session = Some(forked.session);
    args.resume = Some(id.to_string());
    let prepared = super::prepare(
        &args,
        Some(&fixed),
        &paths,
        |_, _| {
            assert!(matches!(
                Session::open(&paths, &id),
                Err(SessionError::Locked)
            ));
            Ok(())
        },
        QueryLauncher::TestDirect,
        &mut |_| false,
    )
    .map_err(|error| error.to_string())?;
    assert_eq!(prepared.session.id(), &id);
    assert_eq!(fs::read(&marker)?, b"kept");
    assert!(prepared.agent.conversation().items.iter().any(|item| matches!(item, Item::UserMessage(message) if message.parts.iter().any(|part| part.text == "source request"))));
    assert!(prepared.session.take_resume_state().is_err());
    Ok(())
}
