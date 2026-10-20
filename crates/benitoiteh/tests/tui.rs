//! 画面の芯に模擬の端末と提供者をつなぐ（設計書 07-01、実装プラン P4-06）。

#[allow(dead_code)]
#[path = "support/benitoite_run.rs"]
mod benitoite_fixture;
#[path = "tui/commands.rs"]
mod commands;
#[path = "tui/interaction.rs"]
mod interaction;
#[path = "tui/pty.rs"]
mod pty;
#[allow(dead_code)]
mod support;
#[path = "support/tty.rs"]
#[allow(dead_code)]
mod tty;

use std::error::Error;
use std::fs;
use std::io;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant, SystemTime};

use benitoiteh::agent::{Agent, AgentConfig, AgentEvent, Outcome};
use benitoiteh::approval::{ApprovalFacts, ApprovalKind, ApprovalOutcome};
use benitoiteh::config::{Limits, Mode, ReadConfig, paths::DataPaths};
use benitoiteh::context::Environment;
use benitoiteh::http::HttpTimeouts;
use benitoiteh::provider::{AdapterKind, Capabilities, Destination, ProviderError};
use benitoiteh::safe_fs::ScopeRoot;
use benitoiteh::session::{
    Record, RecordedMode, Session, SessionId, SkippedInstruction, read_records,
};
use benitoiteh::tools::{ToolContext, exclude::ExcludeRules};
use benitoiteh::ui::tui::{InputSource, RunControl, Screen, Status, Worker, restoration, run};
use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use regex::Regex;
use serde_json::json;
use support::mock_provider::{Delivery, MockProvider, ResponseScript};

type TestResult = Result<(), Box<dyn Error>>;

fn status() -> Status {
    Status {
        mode: Mode::Normal,
        provider: "mock".into(),
        model: "mock-model".into(),
        host: "localhost".into(),
        sandbox: "workspace-write".into(),
        sandbox_disabled: false,
        context_window: Some(100_000),
        estimate: 123,
    }
}

fn controls() -> Result<Regex, regex::Error> {
    // 画面を消す CSI 2J、題名、クリップボード、別画面、端末への問い合わせは許さない。
    Regex::new(
        r"\x1b\[(?:[0-9;]*m|[0-9]+[AG]|0?J|[012]?K|\?(?:25|7|2026|2004)[hl]|>[0-9]+u|<[0-9]+u)",
    )
}

fn safe_output(bytes: &[u8]) -> Result<String, Box<dyn Error>> {
    let value = std::str::from_utf8(bytes)?;
    let visible = controls()?.replace_all(value, "").replace("\r\n", "\n");
    for ch in visible.chars() {
        assert!(
            !matches!(ch, '\u{0}'..='\u{9}' | '\u{b}'..='\u{1f}' | '\u{7f}'..='\u{9f}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}'),
            "unexpected control {ch:?}"
        );
    }
    Ok(visible)
}

// 作成時の関門: 値を可視化し忘れた欄から端末が操作される退行を、実際の書き先で検出する。
// 同じ可視化関数から期待値を作らず、許す制御の列を独立に検査する。
#[test]
fn every_untrusted_surface_emits_only_harness_controls() -> TestResult {
    let attack = "SGR\x1b[31m ESC\x1b[2J OSC52\x1b]52;c;AAAA\x07 OSC0\x1b]0;title\x07\r\x08\u{9b}2J\u{202e}\u{2066}";
    let mut state = status();
    for field in [
        &mut state.provider,
        &mut state.model,
        &mut state.host,
        &mut state.sandbox,
    ] {
        *field = attack.into();
    }
    let mut screen = Screen::new((240, 24), state);
    screen.startup([attack.into()]);
    let events = [
        AgentEvent::SessionStart {
            session_id: "id".into(),
            resumed: false,
            base_url: Some("https://user:DO_NOT_PRINT@host/path?DO_NOT_PRINT".into()),
            system_prompt_sha256: None,
            tools_sha256: None,
            mode: RecordedMode::Normal,
            provider: attack.into(),
            model: attack.into(),
            host: attack.into(),
            benitoiteh_version: "test".into(),
            notice: Some(attack.into()),
            plugins: vec![benitoiteh::session::RecordedPlugin {
                name: attack.into(),
                version: Some(attack.into()),
                path: attack.into(),
                sha256: attack.into(),
                source: benitoiteh::session::SkillSource::User,
            }],
            skills: vec![benitoiteh::session::RecordedSkill {
                name: attack.into(),
                path: attack.into(),
                sha256: "a".repeat(64),
                source: benitoiteh::session::SkillSource::User,
                plugin: Some(attack.into()),
            }],
            skipped_plugins: vec![SkippedInstruction {
                path: attack.into(),
                reason: attack.into(),
            }],
            plugin_notices: vec![SkippedInstruction {
                path: attack.into(),
                reason: attack.into(),
            }],
            skipped_skills: vec![SkippedInstruction {
                path: attack.into(),
                reason: attack.into(),
            }],
            instruction_files: vec![attack.into()],
            skipped_instructions: vec![SkippedInstruction {
                path: attack.into(),
                reason: attack.into(),
            }],
        },
        AgentEvent::TextDelta {
            turn: 1,
            text: format!("**{attack}**\n"),
        },
        AgentEvent::ReasoningDelta {
            turn: 1,
            text: attack.into(),
        },
        AgentEvent::ToolCall {
            turn: 1,
            call_id: "call".into(),
            name: attack.into(),
            arguments: json!({"path":attack}),
        },
        AgentEvent::ToolCall {
            turn: 1,
            call_id: "call".into(),
            name: "read".into(),
            arguments: json!({"path":attack}),
        },
        AgentEvent::ToolResult {
            turn: 1,
            call_id: "call".into(),
            name: "write".into(),
            is_error: false,
            summary: attack.into(),
            screen: Some(attack.into()),
            display: Some(format!("--- {attack}\n+++ {attack}\n+{attack}\n-{attack}")),
        },
        AgentEvent::ToolResult {
            turn: 1,
            call_id: "call".into(),
            name: "other".into(),
            is_error: true,
            summary: attack.into(),
            screen: None,
            display: None,
        },
        AgentEvent::ApprovalResult {
            call_id: "call".into(),
            kind: ApprovalKind::Command,
            facts: ApprovalFacts {
                command: Some(attack.into()),
                cwd: Some(attack.into()),
                path: Some(attack.into()),
                sandbox_policy: Some(attack.into()),
                url: Some(attack.into()),
                domain: Some(attack.into()),
                ..ApprovalFacts::default()
            },
            justification: Some(attack.into()),
            summary: attack.into(),
            decision: ApprovalOutcome::DeniedWithMessage,
            user_message: Some(attack.into()),
        },
        AgentEvent::Error {
            kind: "test".into(),
            message: attack.into(),
        },
        AgentEvent::Retry {
            attempt: 1,
            delay_ms: 10,
            reason: ProviderError::ContextOverflow,
        },
        AgentEvent::SessionEnd {
            outcome: Outcome::Cancelled,
            reason: attack.into(),
        },
    ];
    let mut output = Vec::new();
    for event in events {
        screen.event(&event);
        screen.draw(&mut output, None)?;
    }
    screen.input(Event::Paste(attack.into()), Duration::ZERO, false);
    screen.draw(&mut output, None)?;
    output.extend(restoration(screen.rows_below_cursor())?);
    let visible = safe_output(&output)?;
    assert!(!String::from_utf8_lossy(&output).contains("\x1b[31m"));
    for expected in [
        "\\x1b",
        "\\x07",
        "\\x0d",
        "\\x08",
        "\\x9b",
        "\\u{202e}",
        "\\u{2066}",
    ] {
        assert!(visible.contains(expected), "{expected}");
    }
    assert!(!visible.contains("DO_NOT_PRINT"));
    assert!(visible.contains("Interrupted."));
    Ok(())
}

struct Fixture {
    root: PathBuf,
    project: PathBuf,
    paths: DataPaths,
}
impl Fixture {
    fn new() -> Result<Self, Box<dyn Error>> {
        let root = std::env::temp_dir().join(format!(
            "benitoiteh-tui-{}",
            SessionId::generate(SystemTime::now())?
        ));
        fs::create_dir(&root)?;
        let root = fs::canonicalize(root)?;
        let project = root.join("project");
        for path in [&project, &root.join("home"), &root.join("tmp")] {
            fs::create_dir(path)?;
        }
        let paths = DataPaths::from_values(
            Some(&root.join("home")),
            Some(&root.join("config")),
            Some(&root.join("state")),
            Some(&root.join("tmp")),
        )?;
        fs::write(project.join("a.bnt"), "before\n")?;
        Ok(Self {
            root,
            project,
            paths,
        })
    }

    fn worker(&self, server: &MockProvider) -> Result<(Worker, PathBuf), Box<dyn Error>> {
        let prepared = self.new_session(server)?;
        let directory = prepared.session.directory().to_owned();
        Ok((Worker::start(prepared.agent, prepared.session)?, directory))
    }

    fn new_session(
        &self,
        server: &MockProvider,
    ) -> Result<benitoiteh::ui::tui::NewSession, Box<dyn Error>> {
        self.new_session_in_mode(server, Mode::Normal)
    }

    fn new_session_in_mode(
        &self,
        server: &MockProvider,
        mode: Mode,
    ) -> Result<benitoiteh::ui::tui::NewSession, Box<dyn Error>> {
        self.session_in_mode(server, mode, None)
    }

    fn session_in_mode(
        &self,
        server: &MockProvider,
        mode: Mode,
        resumed: Option<Session>,
    ) -> Result<benitoiteh::ui::tui::NewSession, Box<dyn Error>> {
        let state = resumed
            .as_ref()
            .map(|session| benitoiteh::session::ResumeState::read(session.directory()))
            .transpose()?;
        let session = match resumed {
            Some(session) => session,
            None => Session::create(&self.paths, SessionId::generate(SystemTime::now())?)?,
        };
        let directory = session.directory().to_owned();
        let spill = directory.join("spill");
        let config = AgentConfig {
            agent: Default::default(),
            project_parent: None,
            allow_effects: None,
            mode,
            destination: Destination {
                provider: "mock".into(),
                base_url: server.base_url(),
                model: "mock-model".into(),
            },
            kind: AdapterKind::OpenAiChat,
            capabilities: Capabilities::default(),
            effort: "high".into(),
            limits: Limits::default(),
            http_timeouts: HttpTimeouts {
                connect: Duration::from_secs(1),
                headers: Duration::from_secs(2),
                idle: Duration::from_secs(2),
                total: Duration::from_secs(4),
            },
            retry_initial: Duration::from_millis(2),
        };
        let helper = fs::canonicalize(env!("CARGO_BIN_EXE_benitoiteh"))?;
        let askpass = if mode == Mode::Normal {
            Some(std::sync::Arc::new(benitoiteh::askpass::Broker::create(
                &self.paths.temporary_root,
                session.id().as_str(),
                &helper,
                &[self.project.clone(), session.temporary_dir().into()],
                true,
            )?))
        } else {
            None
        };
        let mut env = benitoiteh::config::child_environment(
            [("PATH".into(), "/bin:/usr/bin".into())],
            &Default::default(),
            session.temporary_dir(),
            None,
        );
        if let Some(broker) = &askpass {
            broker.environment(&mut env);
        }
        let mut context = ToolContext::new(
            mode,
            ScopeRoot::open(&self.project)?,
            ScopeRoot::open(session.temporary_dir())?,
            ScopeRoot::open(&spill)?,
            ExcludeRules::new(
                &self.project,
                &self.paths,
                &spill,
                &ReadConfig::default(),
                &ReadConfig::default(),
            )?,
        )
        .with_session(session.clone())
        .with_project_creation(
            self.paths.clone(),
            fs::canonicalize(env!("CARGO_BIN_EXE_benitoiteh"))?,
        )
        .with_shell(benitoiteh::tools::ShellContext {
            askpass,
            policy: None,
            policy_name: None,
            helper,
            scratch: directory.join("sandbox"),
            placeholders: self.paths.data_dir.join("placeholders"),
            env,
            writable_roots: vec![self.project.clone(), session.temporary_dir().into()],
            sandbox: benitoiteh::config::Sandbox::DangerFullAccess,
            sandboxed: false,
            approval: benitoiteh::config::Approval::Never,
        });
        if mode == Mode::Restricted {
            context = context.with_skill(support::skills()?);
        }
        let environment = Environment::capture(
            &self.project,
            mode,
            &[self.project.clone(), session.temporary_dir().to_owned()],
            false,
            SystemTime::now(),
        )?;
        let agent = match state {
            Some(state) => Agent::resume(
                config,
                "synthetic-tui-key".into(),
                context,
                environment,
                session.clone(),
                state,
            ),
            None => Agent::new(
                config,
                "synthetic-tui-key".into(),
                context,
                environment,
                session.clone(),
            ),
        }?;
        assert!(agent.context_estimate() > 0);
        assert_eq!(agent.context_window(), None);
        Ok(benitoiteh::ui::tui::NewSession::new(
            agent,
            session,
            status(),
            Vec::new(),
        ))
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _removed = fs::remove_dir_all(&self.root);
    }
}

fn response(delta: serde_json::Value, reason: &str) -> String {
    format!(
        "data: {}\n\ndata: [DONE]\n\n",
        json!({"choices":[{"delta":delta,"finish_reason":reason}]})
    )
}

fn finish(worker: &mut Worker, screen: &mut Screen, output: &mut Vec<u8>) -> TestResult {
    let deadline = Instant::now();
    while worker.is_running() {
        assert!(deadline.elapsed() < Duration::from_secs(5));
        worker.drain(screen)?;
        screen.draw(output, None)?;
        thread::sleep(Duration::from_millis(1));
    }
    Ok(())
}

// 作成時の関門: ワーカーと画面と記録の接続に固有の退行を捕まえる。ネットワークだけを置き換える。
#[test]
fn requests_show_real_tool_summaries_diffs_and_append_a_summary_each_time() -> TestResult {
    let mut server = MockProvider::start(vec![
        ResponseScript::sse(response(
            json!({"tool_calls":[{"index":0,"id":"read","type":"function","function":{"name":"read","arguments":"{\"path\":\"a.bnt\"}"}}]}),
            "tool_calls",
        )),
        ResponseScript::sse(response(
            json!({"tool_calls":[{"index":0,"id":"write","type":"function","function":{"name":"write","arguments":"{\"path\":\"a.bnt\",\"content\":\"changed\\n\"}"}}]}),
            "tool_calls",
        )),
        ResponseScript::sse(response(json!({"content":"First answer\n"}), "stop")),
        ResponseScript::sse(response(json!({"content":"Second answer\n"}), "stop")),
    ])?;
    let fixture = Fixture::new()?;
    let (mut worker, directory) = fixture.worker(&server)?;
    let mut screen = Screen::new((100, 24), status());
    let mut bytes = Vec::new();
    for request in ["Read and write", "Continue"] {
        screen.request(request);
        worker.submit(request.into(), RunControl::default())?;
        finish(&mut worker, &mut screen, &mut bytes)?;
    }
    worker.close()?;
    let visible = safe_output(&bytes)?;
    for expected in [
        "read a.bnt",
        "1 lines",
        "write a.bnt",
        "+1 -1",
        "-before",
        "+changed",
        "First answer",
        "Second answer",
    ] {
        assert!(visible.contains(expected), "{expected}: {visible}");
    }
    assert_eq!(
        fs::read_to_string(fixture.project.join("a.bnt"))?,
        "changed\n"
    );
    assert_eq!(
        read_records(&directory)?
            .iter()
            .filter(|record| matches!(record, Record::Summary { .. }))
            .count(),
        2
    );
    server.shutdown()?;
    Ok(())
}

#[test]
fn a_cancelled_request_does_not_cancel_the_next_request() -> TestResult {
    let mut server = MockProvider::start(vec![ResponseScript::sse(response(
        json!({"content":"After cancellation"}),
        "stop",
    ))])?;
    let fixture = Fixture::new()?;
    let (mut worker, directory) = fixture.worker(&server)?;
    let mut screen = Screen::new((100, 24), status());
    let mut output = Vec::new();
    let control = RunControl::default();
    control.cancel();
    worker.submit("Cancel before networking".into(), control)?;
    finish(&mut worker, &mut screen, &mut output)?;
    worker.submit("New request".into(), RunControl::default())?;
    finish(&mut worker, &mut screen, &mut output)?;
    worker.close()?;
    let records = read_records(&directory)?;
    let outcomes: Vec<_> = records
        .iter()
        .filter_map(|record| {
            if let Record::SessionEnd { outcome, .. } = record {
                Some(outcome.as_str())
            } else {
                None
            }
        })
        .collect();
    assert_eq!(outcomes, ["cancelled", "completed"]);
    assert_eq!(
        records
            .iter()
            .filter(|record| matches!(record, Record::Summary { .. }))
            .count(),
        2
    );
    assert!(safe_output(&output)?.contains("After cancellation"));
    assert_eq!(server.requests()?.len(), 1);
    server.shutdown()?;
    Ok(())
}

struct StopInput<'a> {
    signal: &'a AtomicBool,
    provider: &'a MockProvider,
    external: bool,
    sent: bool,
    failure: Option<io::Error>,
}
impl InputSource for StopInput<'_> {
    fn poll(&mut self, wait: Duration) -> io::Result<Vec<Event>> {
        if !self.sent && !self.provider.requests()?.is_empty() {
            self.sent = true;
            if self.external {
                self.signal.store(true, Ordering::Release);
                if let Some(failure) = self.failure.take() {
                    return Err(failure);
                }
            } else {
                return Ok(vec![Event::Key(KeyEvent::new(
                    KeyCode::Char('d'),
                    KeyModifiers::CONTROL,
                ))]);
            }
        }
        thread::sleep(wait);
        Ok(Vec::new())
    }
}

// シグナルと Ctrl+D のどちらでも、イベントを捨てず session_end と Summary まで受け取って戻る。
#[test]
fn shutdown_drains_the_worker_and_preserves_the_restoration_position() -> TestResult {
    for (external, failure, output_fails) in [
        (false, None, false),
        (true, None, false),
        (true, Some(io::Error::from_raw_os_error(5)), false),
        (true, Some(io::ErrorKind::UnexpectedEof.into()), false),
        (true, None, true),
    ] {
        let mut response = ResponseScript::sse("unused");
        response.delivery = Delivery::HeadersOnly;
        let mut server = MockProvider::start(vec![response])?;
        let fixture = Fixture::new()?;
        let (mut worker, directory) = fixture.worker(&server)?;
        worker.submit("Stop".into(), RunControl::default())?;
        let mut screen = Screen::new((100, 24), status());
        let signal = AtomicBool::new(false);
        let mut input = StopInput {
            signal: &signal,
            provider: &server,
            external,
            sent: false,
            failure,
        };
        struct Output<'a> {
            bytes: Vec<u8>,
            signal: &'a AtomicBool,
            fails: bool,
        }
        impl io::Write for Output<'_> {
            fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
                if self.fails && self.signal.load(Ordering::Acquire) {
                    return Err(io::Error::from_raw_os_error(5));
                }
                self.bytes.extend_from_slice(bytes);
                Ok(bytes.len())
            }
            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }
        let mut output = Output {
            bytes: Vec::new(),
            signal: &signal,
            fails: output_fails,
        };
        let mut below = 0;
        assert_eq!(
            run(
                &mut screen,
                worker,
                &mut input,
                &mut output,
                &signal,
                |rows| below = rows,
                &mut |_, _, _| Err("unexpected new session".into()),
            )?,
            if external { 130 } else { 0 }
        );
        output.bytes.extend(restoration(below)?);
        let visible = safe_output(&output.bytes)?;
        if !output_fails {
            assert!(visible.contains("Interrupted."));
        }
        let records = read_records(&directory)?;
        assert!(matches!(records.last(), Some(Record::Summary { .. })));
        assert_eq!(below, screen.rows_below_cursor());
        server.shutdown()?;
    }
    Ok(())
}

// 作成時の関門: 終了待ち中の入力を捨てる退行と、二度目でもワーカーを待つ退行を守る。
#[test]
fn repeated_shutdown_returns_immediately_and_resize_is_kept_while_exiting() -> TestResult {
    for second in [Some('c'), Some('d'), None] {
        let mut response = ResponseScript::sse("unused");
        response.delivery = Delivery::HeadersOnly;
        let mut server = MockProvider::start(vec![response])?;
        let fixture = Fixture::new()?;
        let (mut worker, _) = fixture.worker(&server)?;
        worker.submit("Stop twice".into(), RunControl::default())?;
        let signal = AtomicBool::new(false);
        struct Input<'a> {
            provider: &'a MockProvider,
            signal: &'a AtomicBool,
            second: Option<char>,
            delivered: bool,
        }
        impl InputSource for Input<'_> {
            fn poll(&mut self, wait: Duration) -> io::Result<Vec<Event>> {
                if !self.delivered && !self.provider.requests()?.is_empty() {
                    self.delivered = true;
                    if let Some(second) = self.second {
                        return Ok(vec![
                            Event::Key(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::CONTROL)),
                            Event::Resize(12, 4),
                            Event::Key(KeyEvent::new(KeyCode::Char(second), KeyModifiers::CONTROL)),
                        ]);
                    }
                    self.signal.store(true, Ordering::Release);
                }
                thread::sleep(wait);
                Ok(Vec::new())
            }
            fn termination_count(&mut self) -> io::Result<usize> {
                Ok(if self.delivered && self.second.is_none() {
                    2
                } else {
                    0
                })
            }
        }
        let mut input = Input {
            provider: &server,
            signal: &signal,
            second,
            delivered: false,
        };
        let mut screen = Screen::new((100, 24), status());
        let mut bytes = Vec::new();
        assert_eq!(
            run(
                &mut screen,
                worker,
                &mut input,
                &mut bytes,
                &signal,
                |_| {},
                &mut |_, _, _| Err("unexpected new session".into())
            )?,
            130
        );
        if second.is_some() {
            let mut after = Vec::new();
            screen.draw(&mut after, None)?;
            let visible = safe_output(&after)?;
            assert!(!visible.contains("context"), "{visible}");
            assert!(visible.contains("normal | moc"), "{visible}");
        }
        server.shutdown()?;
    }
    Ok(())
}

// 関門: Worker の早い破棄では Agent が残っていても、補助の名前と窓口を直ちに消す。
#[test]
fn dropping_worker_removes_helpers_even_while_secret_input_is_pending() -> TestResult {
    for running in [false, true] {
        let f = Fixture::new()?;
        let server = MockProvider::start(if running {
            vec![ResponseScript::sse(response(
                json!({"tool_calls":[{"index":0,"id":"secret","type":"function","function":{"name":"bash","arguments":"{\"command\":\"\\\"$GIT_ASKPASS\\\" password\"}"}}]}),
                "tool_calls",
            ))]
        } else {
            vec![]
        })?;
        let prepared = f.new_session(&server)?;
        let id = prepared.session.id().to_string();
        let name = if cfg!(target_os = "macos") {
            id.rsplit_once('-')
                .map_or(id.as_str(), |(_, random)| random)
        } else {
            id.as_str()
        };
        let runtime = if cfg!(target_os = "linux") {
            std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from)
        } else {
            None
        };
        let directory = [f.paths.temporary_root.clone(), PathBuf::from("/tmp")]
            .into_iter()
            .chain(runtime)
            .map(|root| {
                root.join(format!("bha-{}", rustix::process::geteuid().as_raw()))
                    .join(name)
            })
            .find(|path| path.exists())
            .ok_or("missing helpers")?;
        let requests = prepared
            .agent
            .take_secret_requests()
            .ok_or("missing requests")?;
        let mut worker = Worker::start(prepared.agent, prepared.session)?;
        let prompt = if running {
            worker.submit("request".into(), RunControl::default())?;
            Some(requests.recv()?)
        } else {
            None
        };
        drop(worker);
        assert!(!directory.exists(), "{}", directory.display());
        if let Some(prompt) = prompt {
            assert!(!prompt.is_open());
        }
    }
    Ok(())
}
