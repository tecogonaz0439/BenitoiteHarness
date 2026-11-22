//! `benitoiteh acp` と ACP 版 1 の通信層・確認台帳・5 段の終了手順の統合テスト
//! （設計書 05-03、07-01「持ち主の境界」）。

#[allow(dead_code)]
mod support;

use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fs;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, PoisonError, mpsc};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant, SystemTime};

use benitoiteh::agent::{Agent, AgentConfig, PreapprovalOverflow};
use benitoiteh::approval::{ApprovalPolicy, NonInteractiveApprover};
use benitoiteh::config::{Approval, Limits, Mode, ReadConfig, Sandbox, paths::DataPaths};
use benitoiteh::context::Environment;
use benitoiteh::http::HttpTimeouts;
use benitoiteh::provider::{AdapterKind, Capabilities, Destination};
use benitoiteh::safe_fs::ScopeRoot;
use benitoiteh::sandbox::proxy::{NetworkGate, ProxyRun, serve};
use benitoiteh::session::{Session, SessionId};
use benitoiteh::tools::{ShellContext, ToolContext, exclude::ExcludeRules};
use benitoiteh::ui::acp::{
    AcpBackend, AuthMethod, BackendError, InitializeInfo, PreparedSession, text as acp_text,
};
use serde_json::{Value, json};
use support::mock_provider::{Delivery, MockProvider, ResponseScript};

type TestResult = Result<(), Box<dyn Error>>;

struct TestDir(PathBuf);

impl TestDir {
    fn new(name: &str) -> Result<Self, Box<dyn Error>> {
        let id = SessionId::generate(SystemTime::now())?;
        let path = std::env::temp_dir().join(format!("bh-acp-{name}-{id}"));
        fs::create_dir(&path)?;
        Ok(Self(path.canonicalize()?))
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _removed = fs::remove_dir_all(&self.0);
    }
}

fn chat_sse_text(
    reasoning: Option<&str>,
    text: &str,
    input_tokens: u64,
    output_tokens: u64,
) -> Vec<u8> {
    let mut out = String::new();
    if let Some(reason) = reasoning {
        out.push_str(&format!(
            "data: {}\n\n",
            json!({
                "choices": [{"index": 0, "delta": {"reasoning_content": reason}, "finish_reason": null}]
            })
        ));
    }
    if !text.is_empty() {
        out.push_str(&format!(
            "data: {}\n\n",
            json!({
                "choices": [{"index": 0, "delta": {"content": text}, "finish_reason": null}]
            })
        ));
    }
    out.push_str(&format!(
        "data: {}\n\n",
        json!({
            "choices": [{"index": 0, "delta": {}, "finish_reason": "stop"}],
            "usage": {"prompt_tokens": input_tokens, "completion_tokens": output_tokens}
        })
    ));
    out.push_str("data: [DONE]\n\n");
    out.into_bytes()
}

fn chat_sse_tool_calls(
    reasoning: Option<&str>,
    calls: &[(&str, Value)],
    input_tokens: u64,
    output_tokens: u64,
) -> Vec<u8> {
    let mut out = String::new();
    if let Some(reason) = reasoning {
        out.push_str(&format!(
            "data: {}\n\n",
            json!({
                "choices": [{"index": 0, "delta": {"reasoning_content": reason}, "finish_reason": null}]
            })
        ));
    }
    for (idx, (name, args)) in calls.iter().enumerate() {
        out.push_str(&format!(
            "data: {}\n\n",
            json!({
                "choices": [{
                    "index": 0,
                    "delta": {
                        "tool_calls": [{
                            "index": idx,
                            "id": format!("tc_{idx}"),
                            "type": "function",
                            "function": {
                                "name": name,
                                "arguments": args.to_string(),
                            }
                        }]
                    },
                    "finish_reason": null
                }]
            })
        ));
    }
    out.push_str(&format!(
        "data: {}\n\n",
        json!({
            "choices": [{"index": 0, "delta": {}, "finish_reason": "tool_calls"}],
            "usage": {"prompt_tokens": input_tokens, "completion_tokens": output_tokens}
        })
    ));
    out.push_str("data: [DONE]\n\n");
    out.into_bytes()
}

fn chat_sse_many_deltas(count: usize) -> Vec<u8> {
    let mut out = String::new();
    for i in 0..count {
        out.push_str(&format!(
            "data: {}\n\n",
            json!({
                "choices": [{"index": 0, "delta": {"content": format!("chunk-{i}\n")}, "finish_reason": null}]
            })
        ));
    }
    out.push_str(&format!(
        "data: {}\n\n",
        json!({
            "choices": [{"index": 0, "delta": {}, "finish_reason": "stop"}],
            "usage": {"prompt_tokens": 20, "completion_tokens": count}
        })
    ));
    out.push_str("data: [DONE]\n\n");
    out.into_bytes()
}

struct SharedBuf(Arc<Mutex<Vec<u8>>>);

impl SharedBuf {
    fn new() -> (Self, Arc<Mutex<Vec<u8>>>) {
        let buf = Arc::new(Mutex::new(Vec::new()));
        (Self(Arc::clone(&buf)), buf)
    }
}

impl Write for SharedBuf {
    fn write(&mut self, data: &[u8]) -> io::Result<usize> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .extend_from_slice(data);
        Ok(data.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

struct TestBackendConfig {
    paths: DataPaths,
    base_url: String,
    mode: Mode,
    approval: Approval,
    sandbox: Sandbox,
    sandboxed: bool,
    image_input: bool,
    context_window: Option<u64>,
    max_turns: u64,
    max_run_tokens: Option<u64>,
    with_benitoite_checks: bool,
    network_gate: Option<Arc<NetworkGate>>,
    exhaust_storage_budget: bool,
    read_exclude: Vec<String>,
    last_session_dir: Arc<Mutex<Option<PathBuf>>>,
}

impl TestBackendConfig {
    fn build_session(
        &self,
        cwd: &Path,
        resume_id: Option<&str>,
    ) -> Result<PreparedSession, BackendError> {
        let opened = match resume_id {
            Some(id) => {
                let id = id
                    .parse::<SessionId>()
                    .map_err(|e| BackendError::invalid_params(e.to_string()))?;
                let session = Session::open(&self.paths, &id)
                    .map_err(|e| BackendError::server(e.to_string(), Vec::new()))?;
                let state = session
                    .take_resume_state()
                    .map_err(|e| BackendError::server(e.to_string(), Vec::new()))?;
                let requested = fs::canonicalize(cwd)
                    .map_err(|e| BackendError::invalid_params(e.to_string()))?;
                let recorded = fs::canonicalize(&state.project_dir)
                    .map_err(|e| BackendError::server(e.to_string(), Vec::new()))?;
                if requested != recorded {
                    return Err(BackendError::invalid_params(
                        "cwd does not match the recorded session project directory.",
                    ));
                }
                Some((session, state))
            }
            None => None,
        };
        let session = match &opened {
            Some((session, _)) => session.clone(),
            None => {
                let id = SessionId::generate(SystemTime::now())
                    .map_err(|e| BackendError::server(e.to_string(), Vec::new()))?;
                Session::create(&self.paths, id)
                    .map_err(|e| BackendError::server(e.to_string(), Vec::new()))?
            }
        };
        if self.exhaust_storage_budget {
            let usage = benitoiteh::session::StorageUsage {
                total: benitoiteh::session::TOTAL_LIMIT,
                current: 0,
                current_outputs: 0,
            };
            session.configure_budget(&usage);
            let _over = session.reserve(1);
        }
        *self
            .last_session_dir
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = Some(session.directory().to_path_buf());
        let root =
            ScopeRoot::open(cwd).map_err(|e| BackendError::server(e.to_string(), Vec::new()))?;
        let spill = session.directory().join("spill");
        let exclude = ExcludeRules::new(
            root.real_path(),
            &self.paths,
            &spill,
            &ReadConfig {
                exclude: self.read_exclude.clone(),
            },
            &ReadConfig::default(),
        )
        .map_err(|e| BackendError::server(e.to_string(), Vec::new()))?;
        let mut context = ToolContext::new(
            self.mode,
            root,
            ScopeRoot::open(session.temporary_dir())
                .map_err(|e| BackendError::server(e.to_string(), Vec::new()))?,
            ScopeRoot::open(&spill).map_err(|e| BackendError::server(e.to_string(), Vec::new()))?,
            exclude,
        )
        .with_session(session.clone())
        .with_project_creation(
            self.paths.clone(),
            PathBuf::from(env!("CARGO_BIN_EXE_benitoiteh")),
        );
        if self.with_benitoite_checks {
            let project_real = context.project.real_path().to_path_buf();
            let checks = support::checks(project_real)
                .map_err(|e| BackendError::server(e.to_string(), Vec::new()))?;
            let skills =
                support::skills().map_err(|e| BackendError::server(e.to_string(), Vec::new()))?;
            context = context.with_check_query(checks).with_skill(skills);
        }
        if self.mode == Mode::Normal {
            let mut writable_roots = vec![
                context.project.real_path().to_path_buf(),
                session.temporary_dir().to_path_buf(),
            ];
            if self.sandbox == Sandbox::ReadOnly {
                writable_roots = vec![session.temporary_dir().to_path_buf()];
            }
            let policy = if self.sandboxed {
                let locations = benitoiteh::config::writable::WritableLocations {
                    project: context.project.real_path().to_path_buf(),
                    temporary: fs::canonicalize(session.temporary_dir())
                        .map_err(|e| BackendError::server(e.to_string(), Vec::new()))?,
                    additional: Vec::new(),
                };
                benitoiteh::sandbox::normal_policy(
                    self.sandbox,
                    &locations,
                    benitoiteh::config::NetworkAccess::Off,
                    &[],
                )
                .map_err(|e| BackendError::server(e.to_string(), Vec::new()))?
            } else {
                None
            };
            let policy_name = policy.as_ref().map(|_| self.sandbox.name());
            let mut env = BTreeMap::new();
            env.insert("PATH".into(), std::env::var_os("PATH").unwrap_or_default());
            context = context.with_shell(ShellContext {
                sandbox: self.sandbox,
                sandboxed: policy.is_some(),
                writable_roots,
                policy,
                policy_name,
                helper: PathBuf::from(env!("CARGO_BIN_EXE_benitoiteh")),
                bubblewrap: PathBuf::from("/usr/bin/bwrap"),
                scratch: session.temporary_dir().to_path_buf(),
                placeholders: session.temporary_dir().to_path_buf(),
                env,
                approval: self.approval,
                network: self.network_gate.as_ref().map(Arc::clone),
                askpass: None,
            });
        }
        let environment = Environment::capture(
            context.project.real_path(),
            self.mode,
            &[
                context.project.real_path().to_path_buf(),
                session.temporary_dir().to_path_buf(),
            ],
            false,
            SystemTime::now(),
        )
        .map_err(|e| BackendError::server(e.to_string(), Vec::new()))?;
        let capabilities = Capabilities {
            image_input: self.image_input,
            reasoning_effort: true,
            context_window: self.context_window,
            ..Capabilities::default()
        };
        let config = AgentConfig {
            project_parent: None,
            allow_effects: None,
            preapproval_overflow: PreapprovalOverflow::Ask,
            allow_agents_write: false,
            mode: self.mode,
            destination: Destination {
                provider: "mock".into(),
                base_url: self.base_url.clone(),
                model: "mock-model".into(),
            },
            kind: AdapterKind::OpenAiChat,
            capabilities,
            effort: "high".into(),
            limits: Limits {
                max_turns: self.max_turns,
                max_check_fix: 10,
                max_run_tokens: self.max_run_tokens,
            },
            agent: Default::default(),
            compaction: Default::default(),
            http_timeouts: HttpTimeouts::default(),
            retry_initial: Duration::from_millis(1),
        };
        let agent = match opened {
            Some((_, state)) => Agent::resume(
                config,
                String::new(),
                context,
                environment,
                session.clone(),
                state,
            ),
            None => Agent::new(config, String::new(), context, environment, session.clone()),
        }
        .map_err(|e| BackendError::server(e.to_string(), Vec::new()))?
        .with_approvals(
            Box::new(NonInteractiveApprover),
            ApprovalPolicy {
                approval: self.approval,
                sandbox: self.sandbox,
                sandboxed: self.sandboxed,
            },
        );
        Ok(PreparedSession {
            agent,
            session,
            image_input: self.image_input,
            supports_effort: true,
            context_window: self.context_window,
            interactive_approvals: self.approval != Approval::Never,
            diagnostics: Vec::new(),
            on_activate: None,
        })
    }
}

impl AcpBackend for TestBackendConfig {
    fn initialize_info(&mut self) -> Result<InitializeInfo, BackendError> {
        Ok(InitializeInfo {
            image_input: self.image_input,
            auth_methods: vec![AuthMethod {
                id: acp_text::AUTH_API_KEY_ID,
                name: acp_text::AUTH_API_KEY_NAME,
                description: acp_text::AUTH_API_KEY_DESCRIPTION,
            }],
        })
    }

    fn authenticate(&mut self, method_id: &str) -> Result<(), BackendError> {
        if method_id == acp_text::AUTH_API_KEY_ID {
            Ok(())
        } else {
            Err(BackendError::invalid_params(acp_text::UNKNOWN_AUTH_METHOD))
        }
    }

    fn new_session(&mut self, cwd: &Path) -> Result<PreparedSession, BackendError> {
        self.build_session(cwd, None)
    }

    fn load_session(
        &mut self,
        session_id: &str,
        cwd: &Path,
    ) -> Result<PreparedSession, BackendError> {
        self.build_session(cwd, Some(session_id))
    }
}

struct AcpHarness {
    client_write: Option<UnixStream>,
    client_read: BufReader<UnixStream>,
    diagnostics: Arc<Mutex<Vec<u8>>>,
    server_thread: Option<JoinHandle<io::Result<()>>>,
}

impl AcpHarness {
    fn start(mut backend: TestBackendConfig) -> Result<Self, Box<dyn Error>> {
        let (client_in, server_in) = UnixStream::pair()?;
        let (server_out, client_out) = UnixStream::pair()?;
        client_out.set_read_timeout(Some(Duration::from_secs(10)))?;
        let (diag_writer, diagnostics) = SharedBuf::new();
        let cancel = Arc::new(AtomicBool::new(false));
        let server_thread = thread::Builder::new().spawn(move || {
            benitoiteh::ui::acp::run(server_in, server_out, diag_writer, &mut backend, cancel)
        })?;
        Ok(Self {
            client_write: Some(client_in),
            client_read: BufReader::new(client_out),
            diagnostics,
            server_thread: Some(server_thread),
        })
    }

    fn send(&mut self, msg: &Value) -> Result<(), Box<dyn Error>> {
        let writer = self.client_write.as_mut().ok_or("client stdin closed")?;
        let mut bytes = serde_json::to_vec(msg)?;
        bytes.push(b'\n');
        writer.write_all(&bytes)?;
        writer.flush()?;
        Ok(())
    }

    fn recv(&mut self) -> Result<Value, Box<dyn Error>> {
        let mut line = String::new();
        let read = self.client_read.read_line(&mut line)?;
        if read == 0 {
            return Err("unexpected EOF from ACP server".into());
        }
        Ok(serde_json::from_str(&line)?)
    }

    /// 特定の応答 `id` が届くまでメッセージを集め、途中の通知・要求も返す。
    fn recv_until_id(&mut self, target_id: &Value) -> Result<(Vec<Value>, Value), Box<dyn Error>> {
        let mut other = Vec::new();
        loop {
            let msg = self.recv()?;
            if msg.get("id") == Some(target_id)
                && (msg.get("result").is_some() || msg.get("error").is_some())
            {
                return Ok((other, msg));
            }
            other.push(msg);
        }
    }

    fn diagnostics_text(&self) -> String {
        String::from_utf8_lossy(
            &self
                .diagnostics
                .lock()
                .unwrap_or_else(PoisonError::into_inner),
        )
        .into_owned()
    }

    fn close(mut self) -> Result<(), Box<dyn Error>> {
        self.client_write.take();
        if let Some(handle) = self.server_thread.take() {
            handle.join().map_err(|_| "server thread panicked")??;
        }
        Ok(())
    }
}

fn make_paths(root: &Path) -> Result<DataPaths, Box<dyn Error>> {
    for name in ["home", "config", "state", "tmp", "project"] {
        fs::create_dir_all(root.join(name))?;
    }
    Ok(DataPaths::from_values(
        Some(&root.join("home")),
        Some(&root.join("config")),
        Some(&root.join("state")),
        Some(&root.join("tmp")),
    )?)
}

// 作成時の関門: `benitoiteh acp --help`・不正オプションの終了状態 2・本物の実行ファイルでの
// パイプ経由の版 1 往復（stdout 純粋性）を CLI の境界で守る。
#[test]
fn acp_cli_help_usage_errors_and_subprocess_roundtrip() -> TestResult {
    let dir = TestDir::new("cli")?;
    let help = Command::new(env!("CARGO_BIN_EXE_benitoiteh"))
        .args(["acp", "--help"])
        .current_dir(&dir.0)
        .output()?;
    assert_eq!(help.status.code(), Some(0));
    assert!(help.stderr.is_empty());
    let help_text = String::from_utf8(help.stdout)?;
    assert!(
        help_text
            .lines()
            .any(|line| line.trim() == "benitoiteh acp [options]"),
        "{help_text}"
    );

    for bad in [
        vec!["acp", "--json"],
        vec!["acp", "--output-last-message", "out.txt"],
        vec!["acp", "--output-schema", "schema.json"],
        vec!["acp", "--resume", "id"],
        vec!["acp", "--continue"],
        vec!["acp", "request"],
        vec!["acp", "--mode", "restricted", "--approval", "untrusted"],
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_benitoiteh"))
            .args(&bad)
            .current_dir(&dir.0)
            .output()?;
        assert_eq!(output.status.code(), Some(2), "{bad:?}");
        assert!(output.stdout.is_empty(), "{bad:?}");
        assert!(!output.stderr.is_empty(), "{bad:?}");
    }

    // 本物の `benitoiteh acp` 子プロセスをパイプで起動し、stdout に JSON-RPC 2.0 だけが出ることを確かめる。
    let paths = make_paths(&dir.0)?;
    let mut server = MockProvider::start(vec![ResponseScript::sse(chat_sse_text(
        None,
        "hello from acp",
        10,
        5,
    ))])?;
    let config_dir = dir.0.join("config/benitoiteh");
    fs::create_dir_all(&config_dir)?;
    fs::write(
        config_dir.join("config.toml"),
        format!(
            "default_mode = 'normal'\ndefault_provider = 'mock'\n\n[providers.mock]\nkind = 'openai-chat'\nbase_url = '{}'\nmodel = 'mock-model'\n",
            server.base_url()
        ),
    )?;
    let project = dir.0.join("project");
    let mut child = Command::new(env!("CARGO_BIN_EXE_benitoiteh"))
        .args([
            "acp",
            "--mode",
            "normal",
            "--sandbox",
            "danger-full-access",
            "--no-sandbox",
        ])
        .env("HOME", &paths.home)
        .env("XDG_CONFIG_HOME", dir.0.join("config"))
        .env("XDG_STATE_HOME", dir.0.join("state"))
        .env("TMPDIR", &paths.temporary_root)
        .current_dir(&project)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let mut stdin = child.stdin.take().ok_or("stdin")?;
    let stdout = child.stdout.take().ok_or("stdout")?;
    let mut reader = BufReader::new(stdout);

    writeln!(
        stdin,
        "{}",
        json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":2,"unknownExtra":true}})
    )?;
    stdin.flush()?;
    let mut line = String::new();
    reader.read_line(&mut line)?;
    let init: Value = serde_json::from_str(&line)?;
    assert_eq!(init.pointer("/result/protocolVersion"), Some(&json!(1)));

    writeln!(
        stdin,
        "{}",
        json!({"jsonrpc":"2.0","id":2,"method":"session/new","params":{"cwd":project,"mcpServers":[]}})
    )?;
    stdin.flush()?;
    line.clear();
    reader.read_line(&mut line)?;
    let created: Value = serde_json::from_str(&line)?;
    let session_id = created
        .pointer("/result/sessionId")
        .and_then(Value::as_str)
        .ok_or("sessionId")?
        .to_owned();

    writeln!(
        stdin,
        "{}",
        json!({"jsonrpc":"2.0","id":"p1","method":"session/prompt","params":{"sessionId":session_id,"prompt":[{"type":"text","text":"hi"}]}})
    )?;
    stdin.flush()?;
    let mut saw_text = false;
    loop {
        line.clear();
        let n = reader.read_line(&mut line)?;
        assert!(n > 0, "unexpected EOF before prompt response");
        let msg: Value = serde_json::from_str(&line)?;
        if msg.pointer("/params/update/sessionUpdate") == Some(&json!("agent_message_chunk")) {
            saw_text = true;
        }
        if msg.get("id") == Some(&json!("p1")) {
            assert_eq!(msg.pointer("/result/stopReason"), Some(&json!("end_turn")));
            break;
        }
    }
    assert!(saw_text);
    drop(stdin);
    let output = child.wait_with_output()?;
    assert_eq!(output.status.code(), Some(0));
    server.shutdown()?;
    Ok(())
}

// 作成時の関門: 版 1 交渉（v2→v1、未知欄無視）、相対パス cwd・非空 mcpServers の資格情報秘匿、
// ContentBlock 検証（非対応ブロックで提供者を呼ばない）、session/update 遷移（pending→in_progress→completed、
// usage_update）、および非致命的な AgentEvent::Error（storage_limit）発生時も最終応答が
// 二重化・失敗化しない契約を守る。
#[test]
fn version_1_negotiation_roundtrip_content_blocks_and_nonfatal_error_event() -> TestResult {
    let dir = TestDir::new("roundtrip")?;
    let paths = make_paths(&dir.0)?;
    let project = dir.0.join("project");
    fs::write(project.join("note.txt"), "hello world\n")?;

    let mut server = MockProvider::start(vec![
        ResponseScript::sse(chat_sse_tool_calls(
            Some("thinking carefully"),
            &[("read", json!({"path": "note.txt"}))],
            40,
            15,
        )),
        ResponseScript::sse(chat_sse_text(None, "all done", 60, 12)),
    ])?;
    let last_session_dir = Arc::new(Mutex::new(None));
    let backend = TestBackendConfig {
        paths,
        base_url: server.base_url(),
        mode: Mode::Normal,
        approval: Approval::OnRequest,
        sandbox: Sandbox::WorkspaceWrite,
        sandboxed: false,
        image_input: false,
        context_window: Some(32_000),
        max_turns: 20,
        max_run_tokens: None,
        with_benitoite_checks: false,
        network_gate: None,
        exhaust_storage_budget: true,
        read_exclude: Vec::new(),
        last_session_dir: Arc::clone(&last_session_dir),
    };
    let mut acp = AcpHarness::start(backend)?;

    // initialize: protocolVersion: 2 + 未知欄 → protocolVersion: 1
    acp.send(&json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "protocolVersion": 2,
            "clientInfo": {"name": "test-client", "version": "1.0"},
            "unknownFutureField": {"nested": [1, 2, 3]}
        }
    }))?;
    let init = acp.recv()?;
    assert_eq!(init.get("id"), Some(&json!(1)));
    assert_eq!(init.pointer("/result/protocolVersion"), Some(&json!(1)));
    assert_eq!(
        init.pointer("/result/agentInfo/name"),
        Some(&json!("benitoiteh"))
    );
    assert_eq!(
        init.pointer("/result/agentCapabilities/loadSession"),
        Some(&json!(true))
    );
    assert_eq!(
        init.pointer("/result/agentCapabilities/promptCapabilities/image"),
        Some(&json!(false))
    );
    assert_eq!(
        init.pointer("/result/agentCapabilities/promptCapabilities/embeddedContext"),
        Some(&json!(true))
    );

    // authenticate: 正常と未知 methodId
    acp.send(&json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "authenticate",
        "params": {"methodId": "api-key"}
    }))?;
    assert_eq!(acp.recv()?.get("result"), Some(&json!({})));
    acp.send(&json!({
        "jsonrpc": "2.0",
        "id": 3,
        "method": "authenticate",
        "params": {"methodId": "unknown-method"}
    }))?;
    assert_eq!(acp.recv()?.pointer("/error/code"), Some(&json!(-32602)));

    // session/new: 相対パス cwd と非配列 mcpServers は -32602
    acp.send(&json!({
        "jsonrpc": "2.0",
        "id": 4,
        "method": "session/new",
        "params": {"cwd": "relative/path", "mcpServers": []}
    }))?;
    assert_eq!(acp.recv()?.pointer("/error/code"), Some(&json!(-32602)));
    acp.send(&json!({
        "jsonrpc": "2.0",
        "id": 5,
        "method": "session/new",
        "params": {"cwd": project}
    }))?;
    assert_eq!(acp.recv()?.pointer("/error/code"), Some(&json!(-32602)));

    // 非空 mcpServers: 資格情報を一切出力・記録せず件数だけ診断する。
    let decoy_token = "DECOY_MCP_BEARER_SECRET_999";
    let decoy_url = "http://127.0.0.1:49999/secret-mcp";
    let decoy_cmd = "/usr/local/bin/secret-mcp-bridge";
    acp.send(&json!({
        "jsonrpc": "2.0",
        "id": 6,
        "method": "session/new",
        "params": {
            "cwd": project,
            "mcpServers": [
                {
                    "type": "http",
                    "name": "t3-bridge",
                    "url": decoy_url,
                    "headers": [{"name": "Authorization", "value": decoy_token}]
                },
                {
                    "name": "stdio-bridge",
                    "command": decoy_cmd,
                    "args": ["--token", decoy_token],
                    "env": [{"name": "MCP_SECRET", "value": decoy_token}]
                }
            ]
        }
    }))?;
    let created = acp.recv()?;
    assert_eq!(created.get("id"), Some(&json!(6)));
    let session_id = created
        .pointer("/result/sessionId")
        .and_then(Value::as_str)
        .ok_or("sessionId")?
        .to_owned();
    let initial_usage = acp.recv()?;
    assert_eq!(
        initial_usage.pointer("/params/update/sessionUpdate"),
        Some(&json!("usage_update"))
    );
    assert_eq!(
        initial_usage.pointer("/params/update/size"),
        Some(&json!(32_000))
    );

    // session/set_mode は -32601、session/set_config_option は model と effort を更新する。
    acp.send(&json!({
        "jsonrpc": "2.0",
        "id": 7,
        "method": "session/set_mode",
        "params": {"sessionId": session_id, "modeId": "normal"}
    }))?;
    assert_eq!(acp.recv()?.pointer("/error/code"), Some(&json!(-32601)));

    acp.send(&json!({
        "jsonrpc": "2.0",
        "id": 8,
        "method": "session/set_config_option",
        "params": {"sessionId": session_id, "configId": "model", "value": "mock-model-2"}
    }))?;
    let updated_cfg = acp.recv()?;
    let cfg_opts = updated_cfg
        .pointer("/result/configOptions")
        .and_then(Value::as_array)
        .ok_or("configOptions")?;
    assert!(cfg_opts.iter().any(|opt| {
        opt.get("id") == Some(&json!("model"))
            && opt.get("currentValue") == Some(&json!("mock-model-2"))
    }));

    acp.send(&json!({
        "jsonrpc": "2.0",
        "id": 9,
        "method": "session/set_config_option",
        "params": {"sessionId": session_id, "configId": "effort", "value": "low"}
    }))?;
    let updated_effort = acp.recv()?;
    assert!(
        updated_effort
            .pointer("/result/configOptions")
            .and_then(Value::as_array)
            .ok_or("configOptions")?
            .iter()
            .any(|opt| {
                opt.get("id") == Some(&json!("effort"))
                    && opt.get("currentValue") == Some(&json!("low"))
            })
    );

    acp.send(&json!({
        "jsonrpc": "2.0",
        "id": 10,
        "method": "session/set_config_option",
        "params": {"sessionId": session_id, "configId": "mode", "value": "restricted"}
    }))?;
    assert_eq!(acp.recv()?.pointer("/error/code"), Some(&json!(-32602)));

    // 非対応 ContentBlock（audio・resource.blob・未知型・image_input==false の画像）は -32602 で拒み提供者へ送らない。
    for (req_id, bad_prompt) in [
        (
            11,
            json!([{"type": "audio", "data": "AA==", "mimeType": "audio/wav"}]),
        ),
        (
            12,
            json!([{"type": "resource", "resource": {"uri": "file:///a", "blob": "AA=="}}]),
        ),
        (13, json!([{"type": "unknown_future_block"}])),
        (
            14,
            json!([{"type": "image", "data": "iVBORw0KGgo=", "mimeType": "image/png"}]),
        ),
    ] {
        acp.send(&json!({
            "jsonrpc": "2.0",
            "id": req_id,
            "method": "session/prompt",
            "params": {"sessionId": session_id, "prompt": bad_prompt}
        }))?;
        let resp = acp.recv()?;
        assert_eq!(resp.get("id"), Some(&json!(req_id)));
        assert_eq!(resp.pointer("/error/code"), Some(&json!(-32602)));
    }
    assert!(
        server.requests()?.is_empty(),
        "rejected ContentBlocks must not contact provider"
    );

    // 正常な session/prompt（text + resource_link + resource.text）と storage_limit AgentEvent::Error の両立。
    acp.send(&json!({
        "jsonrpc": "2.0",
        "id": "prompt-ok",
        "method": "session/prompt",
        "params": {
            "sessionId": session_id,
            "prompt": [
                {"type": "text", "text": "Please read note.txt"},
                {"type": "resource_link", "uri": "file:///ref/doc.md", "name": "doc.md"},
                {"type": "resource", "resource": {"uri": "file:///embed.txt", "text": "embedded text"}}
            ]
        }
    }))?;
    let (notifications, prompt_resp) = acp.recv_until_id(&json!("prompt-ok"))?;
    assert_eq!(
        prompt_resp.pointer("/result/stopReason"),
        Some(&json!("end_turn"))
    );

    // 後続の ping 的要求（initialize）を送り、session/prompt の二重応答が来ていないことを確認する。
    acp.send(&json!({
        "jsonrpc": "2.0",
        "id": 99,
        "method": "initialize",
        "params": {"protocolVersion": 1}
    }))?;
    let next_msg = acp.recv()?;
    assert_eq!(
        next_msg.get("id"),
        Some(&json!(99)),
        "must not emit duplicate prompt response after AgentEvent::Error"
    );

    let updates = notifications
        .iter()
        .filter_map(|m| m.pointer("/params/update"))
        .collect::<Vec<_>>();
    assert!(
        updates
            .iter()
            .any(|u| u.get("sessionUpdate") == Some(&json!("agent_thought_chunk")))
    );
    assert!(
        updates
            .iter()
            .any(|u| u.get("sessionUpdate") == Some(&json!("agent_message_chunk")))
    );
    assert!(
        updates
            .iter()
            .any(|u| u.get("sessionUpdate") == Some(&json!("usage_update")))
    );
    // tool_call (pending) → tool_call_update (in_progress) → tool_call_update (completed) の順を確認。
    let statuses = updates
        .iter()
        .filter_map(|u| match u.get("sessionUpdate").and_then(Value::as_str) {
            Some("tool_call") | Some("tool_call_update") => u.get("status").and_then(Value::as_str),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(statuses, vec!["pending", "in_progress", "completed"]);

    let diag = acp.diagnostics_text();
    assert!(diag.contains("Ignoring 2 MCP server(s)"), "{diag}");
    for secret in [decoy_token, decoy_url, decoy_cmd] {
        assert!(!diag.contains(secret), "secret leaked to stderr: {diag}");
        for msg in &notifications {
            assert!(!msg.to_string().contains(secret), "secret leaked to stdout");
        }
    }
    // storage_limit の AgentEvent::Error が stderr に診断として出ていることも確認。
    assert!(
        diag.contains("benitoiteh:"),
        "expected storage_limit diagnostic on stderr: {diag}"
    );

    let session_dir = last_session_dir
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone()
        .ok_or("session dir")?;
    acp.close()?;
    let jsonl = fs::read_to_string(session_dir.join("session.jsonl"))?;
    for secret in [decoy_token, decoy_url, decoy_cmd] {
        assert!(!jsonl.contains(secret), "secret leaked to session.jsonl");
    }
    server.shutdown()?;
    Ok(())
}

// 作成時の関門: `session/load` で現在の枝の履歴が再生され過去の承認が復元されないこと、
// `session/new` でセッション切替後に前の許可や sessionId が隔離されること、
// 非アクティブ sessionId への通知 `session/cancel` は無応答無視・id 付き要求は -32602 になることを守る。
#[test]
fn session_load_replays_history_without_restoring_approvals_and_cancel_handles_inactive_ids()
-> TestResult {
    let dir = TestDir::new("load-and-switch")?;
    let paths = make_paths(&dir.0)?;
    let project = dir.0.join("project");
    let other_dir = dir.0.join("other");
    fs::create_dir_all(&other_dir)?;

    let mut server = MockProvider::start(vec![
        // セッション 1、1 回目の bash（allow-session で記憶）
        ResponseScript::sse(chat_sse_tool_calls(
            Some("plan step 1"),
            &[("bash", json!({"command": "echo first"}))],
            20,
            10,
        )),
        // セッション 1、2 回目の同じ bash（記憶により確認なし）
        ResponseScript::sse(chat_sse_tool_calls(
            None,
            &[("bash", json!({"command": "echo first"}))],
            25,
            10,
        )),
        ResponseScript::sse(chat_sse_text(None, "session 1 done", 30, 5)),
        // session/load 後の同じ bash（承認が復元されないため再び確認が要る！）
        ResponseScript::sse(chat_sse_tool_calls(
            None,
            &[("bash", json!({"command": "echo first"}))],
            35,
            10,
        )),
        ResponseScript::sse(chat_sse_text(None, "reloaded done", 40, 5)),
        // session/new で切り替えた新セッションでも同じ bash に確認が要る
        ResponseScript::sse(chat_sse_tool_calls(
            None,
            &[("bash", json!({"command": "echo first"}))],
            20,
            10,
        )),
        ResponseScript::sse(chat_sse_text(None, "new session done", 25, 5)),
    ])?;

    let backend = TestBackendConfig {
        paths,
        base_url: server.base_url(),
        mode: Mode::Normal,
        approval: Approval::Untrusted,
        sandbox: Sandbox::WorkspaceWrite,
        sandboxed: false,
        image_input: false,
        context_window: None,
        max_turns: 20,
        max_run_tokens: None,
        with_benitoite_checks: false,
        network_gate: None,
        exhaust_storage_budget: false,
        read_exclude: Vec::new(),
        last_session_dir: Arc::new(Mutex::new(None)),
    };
    let mut acp = AcpHarness::start(backend)?;

    acp.send(&json!({"jsonrpc":"2.0","id":1,"method":"session/new","params":{"cwd":project,"mcpServers":[]}}))?;
    let s1_id = acp
        .recv()?
        .pointer("/result/sessionId")
        .and_then(Value::as_str)
        .ok_or("s1")?
        .to_owned();

    // 1 回目の bash で allow-session、2 回目は確認なしで通ること。
    acp.send(&json!({
        "jsonrpc": "2.0",
        "id": "p1",
        "method": "session/prompt",
        "params": {"sessionId": s1_id, "prompt": [{"type": "text", "text": "run echo"}]}
    }))?;
    let mut perm_count = 0usize;
    loop {
        let msg = acp.recv()?;
        if msg.get("method") == Some(&json!("session/request_permission")) {
            perm_count = perm_count.saturating_add(1);
            let req_id = msg.get("id").cloned().ok_or("perm id")?;
            acp.send(&json!({
                "jsonrpc": "2.0",
                "id": req_id,
                "result": {"outcome": {"outcome": "selected", "optionId": "allow-session"}}
            }))?;
        } else if msg.get("id") == Some(&json!("p1")) {
            assert_eq!(msg.pointer("/result/stopReason"), Some(&json!("end_turn")));
            break;
        }
    }
    assert_eq!(
        perm_count, 1,
        "second identical bash in same session must use session memory"
    );

    // session/load: cwd が一致しなければ拒否。
    acp.send(&json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "session/load",
        "params": {"sessionId": s1_id, "cwd": other_dir, "mcpServers": []}
    }))?;
    assert!(acp.recv()?.get("error").is_some());

    // session/load: cwd が一致すれば履歴を再生してから { configOptions } を返す。
    acp.send(&json!({
        "jsonrpc": "2.0",
        "id": 3,
        "method": "session/load",
        "params": {"sessionId": s1_id, "cwd": project, "mcpServers": []}
    }))?;
    let (replayed, load_resp) = acp.recv_until_id(&json!(3))?;
    assert!(load_resp.pointer("/result/configOptions").is_some());
    let kinds = replayed
        .iter()
        .filter_map(|m| {
            m.pointer("/params/update/sessionUpdate")
                .and_then(Value::as_str)
        })
        .collect::<Vec<_>>();
    assert!(kinds.contains(&"user_message_chunk"), "{kinds:?}");
    assert!(kinds.contains(&"agent_thought_chunk"), "{kinds:?}");
    assert!(kinds.contains(&"tool_call"), "{kinds:?}");
    assert!(kinds.contains(&"tool_call_update"), "{kinds:?}");
    assert!(kinds.contains(&"agent_message_chunk"), "{kinds:?}");

    // 再開したセッションでは承認が復元されず、同じ bash に再び確認が求められること。
    acp.send(&json!({
        "jsonrpc": "2.0",
        "id": "p2",
        "method": "session/prompt",
        "params": {"sessionId": s1_id, "prompt": [{"type": "text", "text": "run echo again"}]}
    }))?;
    let mut asked_after_load = false;
    loop {
        let msg = acp.recv()?;
        if msg.get("method") == Some(&json!("session/request_permission")) {
            asked_after_load = true;
            let req_id = msg.get("id").cloned().ok_or("perm id")?;
            acp.send(&json!({
                "jsonrpc": "2.0",
                "id": req_id,
                "result": {"outcome": {"outcome": "selected", "optionId": "allow-session"}}
            }))?;
        } else if msg.get("id") == Some(&json!("p2")) {
            assert_eq!(msg.pointer("/result/stopReason"), Some(&json!("end_turn")));
            break;
        }
    }
    assert!(
        asked_after_load,
        "session/load must not restore prior approvals"
    );

    // session/new でセッションを切り替える。
    acp.send(&json!({"jsonrpc":"2.0","id":4,"method":"session/new","params":{"cwd":project,"mcpServers":[]}}))?;
    let s2_id = acp
        .recv()?
        .pointer("/result/sessionId")
        .and_then(Value::as_str)
        .ok_or("s2")?
        .to_owned();
    assert_ne!(s1_id, s2_id);

    // 非アクティブな s1_id への通知 session/cancel は無応答無視、id 付き要求は -32602。
    acp.send(&json!({
        "jsonrpc": "2.0",
        "method": "session/cancel",
        "params": {"sessionId": s1_id}
    }))?;
    acp.send(&json!({
        "jsonrpc": "2.0",
        "id": 5,
        "method": "session/cancel",
        "params": {"sessionId": s1_id}
    }))?;
    let cancel_req_resp = acp.recv()?;
    assert_eq!(
        cancel_req_resp.get("id"),
        Some(&json!(5)),
        "notification session/cancel for inactive session must not emit a response"
    );
    assert_eq!(cancel_req_resp.pointer("/error/code"), Some(&json!(-32602)));

    // 切り替えた新セッションでも前のセッションの許可は引き継がれない。
    acp.send(&json!({
        "jsonrpc": "2.0",
        "id": "p3",
        "method": "session/prompt",
        "params": {"sessionId": s2_id, "prompt": [{"type": "text", "text": "run echo in s2"}]}
    }))?;
    let mut asked_in_s2 = false;
    loop {
        let msg = acp.recv()?;
        if msg.get("method") == Some(&json!("session/request_permission")) {
            asked_in_s2 = true;
            let req_id = msg.get("id").cloned().ok_or("perm id")?;
            acp.send(&json!({
                "jsonrpc": "2.0",
                "id": req_id,
                "result": {"outcome": {"outcome": "selected", "optionId": "allow-once"}}
            }))?;
        } else if msg.get("id") == Some(&json!("p3")) {
            break;
        }
    }
    assert!(
        asked_in_s2,
        "new session must not inherit approvals from previous session"
    );

    acp.close()?;
    server.shutdown()?;
    Ok(())
}

// 作成時の関門: 通常の Effects/TestEffects の allow-once（1 回目通過・2 回目再確認）と allow-session
// （2 回目以降確認なし）、一回限りの確認（Escalate・WriteScope・CreateProject・agents_write・
// unread_imports）の選択肢制限、未提示 optionId の Denied 化、遅延応答の無視、および
// rawInput.command と content への全事実格納・制御文字と空白可視化・justification 除外・kind 制限を守る。
#[test]
fn approval_options_memory_scope_fact_rendering_and_stale_responses() -> TestResult {
    // 1. 制限モード: Effects（通常・agents_write・unread_imports）、TestEffects、CreateProject
    {
        let dir = TestDir::new("restricted-approvals")?;
        let paths = make_paths(&dir.0)?;
        let project = dir.0.join("project");
        let read_script = "import Benitoite.Unofficial.IO.File\nfunction main() -> Unit uses File.Read\n bind _ <- File.readText(\"input\")\n return ()\nend function\n";
        let write_script = "import Benitoite.Unofficial.IO.File\nfunction main() -> Unit uses File.Write\n bind _ <- File.writeText(path: \"out.txt\", text: \"x\")\nend function\n";
        let read_test = "import Benitoite.Unofficial.IO.File\n@test(\"read\")\nfunction testRead() -> Unit uses File.Read\n bind _ <- File.readText(\"input\")\n return ()\nend function\n";
        fs::write(project.join("a.bnt"), read_script)?;
        fs::write(project.join("w.bnt"), write_script)?;
        fs::write(project.join("t.bnt"), read_test)?;
        let new_project = dir.0.join("created-proj");

        let mut server = MockProvider::start(vec![
            // 1-3: Effects（a.bnt）を 3 回呼ぶ: 1 回目 allow-once → 2 回目再確認で allow-session → 3 回目確認なし
            ResponseScript::sse(chat_sse_tool_calls(
                None,
                &[
                    ("benitoite_run", json!({"path": "a.bnt"})),
                    ("benitoite_run", json!({"path": "a.bnt"})),
                    ("benitoite_run", json!({"path": "a.bnt"})),
                ],
                20,
                10,
            )),
            // 4-6: TestEffects（t.bnt）を 3 回呼ぶ: 1 回目 allow-once → 2 回目再確認で allow-session → 3 回目確認なし
            ResponseScript::sse(chat_sse_tool_calls(
                None,
                &[
                    ("benitoite_test", json!({"paths": ["t.bnt"]})),
                    ("benitoite_test", json!({"paths": ["t.bnt"]})),
                    ("benitoite_test", json!({"paths": ["t.bnt"]})),
                ],
                20,
                10,
            )),
            // 7: agents_write == true の Effects（write_agents: true）→ 一回限り選択肢、未提示 allow-session で Denied
            ResponseScript::sse(chat_sse_tool_calls(
                None,
                &[(
                    "benitoite_run",
                    json!({"path": "w.bnt", "write_agents": true}),
                )],
                20,
                10,
            )),
            // 8: CreateProject → 一回限り選択肢、kind == "edit"
            ResponseScript::sse(chat_sse_tool_calls(
                None,
                &[("create_project", json!({"path": new_project}))],
                20,
                10,
            )),
            ResponseScript::sse(chat_sse_text(None, "done restricted", 30, 5)),
        ])?;

        let backend = TestBackendConfig {
            paths,
            base_url: server.base_url(),
            mode: Mode::Restricted,
            approval: Approval::OnRequest,
            sandbox: Sandbox::ReadOnly,
            sandboxed: true,
            image_input: false,
            context_window: None,
            max_turns: 20,
            max_run_tokens: None,
            with_benitoite_checks: true,
            network_gate: None,
            exhaust_storage_budget: false,
            read_exclude: Vec::new(),
            last_session_dir: Arc::new(Mutex::new(None)),
        };
        let mut acp = AcpHarness::start(backend)?;
        acp.send(&json!({"jsonrpc":"2.0","id":1,"method":"session/new","params":{"cwd":project,"mcpServers":[]}}))?;
        let sid = acp
            .recv()?
            .pointer("/result/sessionId")
            .and_then(Value::as_str)
            .ok_or("sid")?
            .to_owned();

        acp.send(&json!({
            "jsonrpc": "2.0",
            "id": "p-restricted",
            "method": "session/prompt",
            "params": {"sessionId": sid, "prompt": [{"type": "text", "text": "test effects"}]}
        }))?;

        let mut perm_records: Vec<(String, Value)> = Vec::new();
        let mut first_perm_id: Option<String> = None;
        loop {
            let msg = acp.recv()?;
            if msg.get("method") == Some(&json!("session/request_permission")) {
                let req_id = msg
                    .get("id")
                    .and_then(Value::as_str)
                    .ok_or("perm id string")?
                    .to_owned();
                assert!(req_id.starts_with("bh-perm-"), "{req_id}");
                let params = msg.get("params").cloned().ok_or("params")?;
                let option_ids = params
                    .get("options")
                    .and_then(Value::as_array)
                    .ok_or("options")?
                    .iter()
                    .filter_map(|o| o.get("optionId").and_then(Value::as_str))
                    .collect::<Vec<_>>();
                let step = perm_records.len();
                let reply_option = match step {
                    // 0: Effects 1 回目 (allow-once)
                    0 => {
                        assert_eq!(
                            option_ids,
                            vec!["allow-once", "allow-session", "reject-once"]
                        );
                        first_perm_id = Some(req_id.clone());
                        "allow-once"
                    }
                    // 1: Effects 2 回目（再確認！ここで allow-session を返す → 3 回目は聞かれない）
                    1 => {
                        assert_eq!(
                            option_ids,
                            vec!["allow-once", "allow-session", "reject-once"]
                        );
                        "allow-session"
                    }
                    // 2: TestEffects 1 回目 (allow-once)
                    2 => {
                        assert_eq!(
                            option_ids,
                            vec!["allow-once", "allow-session", "reject-once"]
                        );
                        "allow-once"
                    }
                    // 3: TestEffects 2 回目 (allow-session → 3 回目は聞かれない)
                    3 => {
                        assert_eq!(
                            option_ids,
                            vec!["allow-once", "allow-session", "reject-once"]
                        );
                        "allow-session"
                    }
                    // 4: write_agents: true の Effects（一回限り選択肢のみ！未提示 allow-session を送って Denied を確認）
                    4 => {
                        assert_eq!(option_ids, vec!["allow-once", "reject-once"]);
                        let cmd = params
                            .pointer("/toolCall/rawInput/command")
                            .and_then(Value::as_str)
                            .unwrap_or_default();
                        let content = params
                            .pointer("/toolCall/content/0/content/text")
                            .and_then(Value::as_str)
                            .unwrap_or_default();
                        assert_eq!(cmd, content);
                        assert!(cmd.contains("File.Write"), "{cmd}");
                        assert!(
                            cmd.contains(
                                "creates, changes, moves, or deletes files and directories"
                            ),
                            "{cmd}"
                        );
                        assert!(
                            cmd.contains(
                                "This call may write agent skills and settings under .agents/."
                            ),
                            "{cmd}"
                        );
                        assert_eq!(params.pointer("/toolCall/kind"), Some(&json!("execute")));
                        "allow-session"
                    }
                    // 5: CreateProject（一回限り選択肢、kind == "edit"）
                    5 => {
                        assert_eq!(option_ids, vec!["allow-once", "reject-once"]);
                        assert_eq!(params.pointer("/toolCall/kind"), Some(&json!("edit")));
                        "reject-once"
                    }
                    _ => "reject-once",
                };
                perm_records.push((req_id.clone(), params));
                acp.send(&json!({
                    "jsonrpc": "2.0",
                    "id": req_id,
                    "result": {"outcome": {"outcome": "selected", "optionId": reply_option}}
                }))?;
                // 1 回目の完了済み ID へ遅れて allow-session を送っても無視されることを確かめる。
                if step == 0
                    && let Some(stale) = &first_perm_id
                {
                    acp.send(&json!({
                        "jsonrpc": "2.0",
                        "id": stale,
                        "result": {"outcome": {"outcome": "selected", "optionId": "allow-session"}}
                    }))?;
                }
            } else if msg.get("id") == Some(&json!("p-restricted")) {
                assert_eq!(msg.pointer("/result/stopReason"), Some(&json!("end_turn")));
                break;
            }
        }
        // 3 回目の Effects と 3 回目の TestEffects が確認を飛ばしたため、合計 6 回の確認になる。
        assert_eq!(perm_records.len(), 6);
        assert!(!new_project.exists());
        // すべての要求 ID が一意で単調増加していること。
        let unique_ids: BTreeSet<&str> = perm_records.iter().map(|(id, _)| id.as_str()).collect();
        assert_eq!(unique_ids.len(), perm_records.len());

        // 未提示 optionId で Denied になった write_agents の結果を検証。
        let reqs = server.requests()?;
        let body_after_write_agents = reqs
            .get(3)
            .ok_or("request after write_agents")?
            .body
            .to_string();
        assert!(
            body_after_write_agents.contains("declined"),
            "unpresented allow-session must result in Denied: {body_after_write_agents}"
        );
        acp.close()?;
        server.shutdown()?;
    }

    // 2. 通常モード: Escalate（制御文字・8 文字以上の空白・justification 除外・kind == "execute"）と
    // WriteScope（範囲外書き込み・kind == "edit"）の確認を検証。
    {
        let dir = TestDir::new("normal-approvals")?;
        let paths = make_paths(&dir.0)?;
        let project = dir.0.join("project");
        let outside_file = dir.0.join("outside.txt");
        let attack_cmd = "echo \x1b[31mred\u{202e}          hidden";
        let decoy_justification = "DECOY_LLM_JUSTIFICATION_MUST_NOT_APPEAR";

        let mut server = MockProvider::start(vec![
            ResponseScript::sse(chat_sse_tool_calls(
                None,
                &[
                    (
                        "bash",
                        json!({
                            "command": attack_cmd,
                            "escalate": true,
                            "justification": decoy_justification
                        }),
                    ),
                    (
                        "write",
                        json!({
                            "path": outside_file,
                            "content": "should not be written"
                        }),
                    ),
                ],
                20,
                10,
            )),
            ResponseScript::sse(chat_sse_text(None, "done normal", 30, 5)),
        ])?;

        let backend = TestBackendConfig {
            paths,
            base_url: server.base_url(),
            mode: Mode::Normal,
            approval: Approval::OnRequest,
            sandbox: Sandbox::WorkspaceWrite,
            sandboxed: true,
            image_input: false,
            context_window: None,
            max_turns: 20,
            max_run_tokens: None,
            with_benitoite_checks: false,
            network_gate: None,
            exhaust_storage_budget: false,
            read_exclude: Vec::new(),
            last_session_dir: Arc::new(Mutex::new(None)),
        };
        let mut acp = AcpHarness::start(backend)?;
        acp.send(&json!({"jsonrpc":"2.0","id":1,"method":"session/new","params":{"cwd":project,"mcpServers":[]}}))?;
        let sid = acp
            .recv()?
            .pointer("/result/sessionId")
            .and_then(Value::as_str)
            .ok_or("sid")?
            .to_owned();

        acp.send(&json!({
            "jsonrpc": "2.0",
            "id": "p-normal",
            "method": "session/prompt",
            "params": {"sessionId": sid, "prompt": [{"type": "text", "text": "escalate and write outside"}]}
        }))?;

        let mut seen = Vec::new();
        loop {
            let msg = acp.recv()?;
            if msg.get("method") == Some(&json!("session/request_permission")) {
                let req_id = msg.get("id").cloned().ok_or("perm id")?;
                let params = msg.get("params").cloned().ok_or("params")?;
                let kind = params
                    .pointer("/toolCall/kind")
                    .and_then(Value::as_str)
                    .ok_or("kind")?
                    .to_owned();
                assert!(
                    matches!(kind.as_str(), "execute" | "edit" | "fetch"),
                    "forbidden auto-approved kind: {kind}"
                );
                let cmd = params
                    .pointer("/toolCall/rawInput/command")
                    .and_then(Value::as_str)
                    .ok_or("rawInput.command")?
                    .to_owned();
                let content = params
                    .pointer("/toolCall/content/0/content/text")
                    .and_then(Value::as_str)
                    .ok_or("content text")?
                    .to_owned();
                assert_eq!(cmd, content);
                assert!(
                    !params.to_string().contains(decoy_justification),
                    "LLM justification leaked into permission request: {params}"
                );
                let option_ids = params
                    .get("options")
                    .and_then(Value::as_array)
                    .ok_or("options")?
                    .iter()
                    .filter_map(|o| o.get("optionId").and_then(Value::as_str))
                    .collect::<Vec<_>>();
                // Escalate と WriteScope はどちらも一回限り（allow-session なし）。
                assert_eq!(option_ids, vec!["allow-once", "reject-once"]);
                if seen.is_empty() {
                    assert_eq!(kind, "execute");
                    assert!(cmd.contains("\\x1b[31m"), "ESC not escaped: {cmd}");
                    assert!(cmd.contains("\\u{202e}"), "bidi not escaped: {cmd}");
                    assert!(cmd.contains("␠×10"), "10 spaces not compacted: {cmd}");
                    assert!(cmd.contains("outside the OS sandbox"), "{cmd}");
                } else {
                    assert_eq!(kind, "edit");
                    assert!(cmd.contains(&outside_file.display().to_string()), "{cmd}");
                }
                seen.push(kind);
                // 未提示の allow-session を返して両方とも拒否されることを確認。
                acp.send(&json!({
                    "jsonrpc": "2.0",
                    "id": req_id,
                    "result": {"outcome": {"outcome": "selected", "optionId": "allow-session"}}
                }))?;
            } else if msg.get("id") == Some(&json!("p-normal")) {
                assert_eq!(msg.pointer("/result/stopReason"), Some(&json!("end_turn")));
                break;
            }
        }
        assert_eq!(seen, vec!["execute", "edit"]);
        assert!(!outside_file.exists());
        acp.close()?;
        server.shutdown()?;
    }

    // 3. 制限モードで unread_imports > 0 のスクリプトは一回限り選択肢のみになり、件数が事実に出る。
    {
        let dir = TestDir::new("unread-imports")?;
        let paths = make_paths(&dir.0)?;
        let project = dir.0.join("project");
        fs::write(
            project.join("Secret.bnt"),
            "public function token() -> String\n return \"secret\"\nend function\n",
        )?;
        fs::write(
            project.join("unread.bnt"),
            "import Secret\nimport Benitoite.Unofficial.IO.File\nfunction main() -> Unit uses File.Read\n bind _ <- File.readText(\"input\")\n return ()\nend function\n",
        )?;
        let mut server = MockProvider::start(vec![
            ResponseScript::sse(chat_sse_tool_calls(
                None,
                &[("benitoite_run", json!({"path": "unread.bnt"}))],
                20,
                10,
            )),
            ResponseScript::sse(chat_sse_text(None, "done unread", 20, 5)),
        ])?;
        let backend = TestBackendConfig {
            paths,
            base_url: server.base_url(),
            mode: Mode::Restricted,
            approval: Approval::OnRequest,
            sandbox: Sandbox::ReadOnly,
            sandboxed: true,
            image_input: false,
            context_window: None,
            max_turns: 20,
            max_run_tokens: None,
            with_benitoite_checks: true,
            network_gate: None,
            exhaust_storage_budget: false,
            read_exclude: vec!["Secret.bnt".into()],
            last_session_dir: Arc::new(Mutex::new(None)),
        };
        let mut acp = AcpHarness::start(backend)?;
        acp.send(&json!({"jsonrpc":"2.0","id":1,"method":"session/new","params":{"cwd":project,"mcpServers":[]}}))?;
        let sid = acp
            .recv()?
            .pointer("/result/sessionId")
            .and_then(Value::as_str)
            .ok_or("sid")?
            .to_owned();
        acp.send(&json!({
            "jsonrpc": "2.0",
            "id": "p-unread",
            "method": "session/prompt",
            "params": {"sessionId": sid, "prompt": [{"type": "text", "text": "run unread"}]}
        }))?;
        let mut saw_unread_perm = false;
        loop {
            let msg = acp.recv()?;
            if msg.get("method") == Some(&json!("session/request_permission")) {
                saw_unread_perm = true;
                let req_id = msg.get("id").cloned().ok_or("perm id")?;
                let params = msg.get("params").cloned().ok_or("params")?;
                let option_ids = params
                    .get("options")
                    .and_then(Value::as_array)
                    .ok_or("options")?
                    .iter()
                    .filter_map(|o| o.get("optionId").and_then(Value::as_str))
                    .collect::<Vec<_>>();
                assert_eq!(option_ids, vec!["allow-once", "reject-once"]);
                let cmd = params
                    .pointer("/toolCall/rawInput/command")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                assert!(
                    cmd.contains("Imports files that benitoiteh does not read"),
                    "{cmd}"
                );
                acp.send(&json!({
                    "jsonrpc": "2.0",
                    "id": req_id,
                    "result": {"outcome": {"outcome": "selected", "optionId": "reject-once"}}
                }))?;
            } else if msg.get("id") == Some(&json!("p-unread")) {
                assert_eq!(msg.pointer("/result/stopReason"), Some(&json!("end_turn")));
                break;
            }
        }
        assert!(saw_unread_perm);
        acp.close()?;
        server.shutdown()?;
    }

    Ok(())
}

// 作成時の関門: (1) 通常の確認待ち中、(2) NetworkGate（通信の中継）の確認待ち中、
// (3) AgentEvent チャネル（容量 64）が満杯の状態での `session/cancel` が、
// いずれもデッドロックせずに最後の session/update の後に一度だけ `stopReason: "cancelled"` を返す契約を守る。
#[test]
fn cancel_unblocks_tool_approval_network_approval_and_full_event_channel() -> TestResult {
    // (1) 通常の確認待ち中の session/cancel
    {
        let dir = TestDir::new("cancel-tool-perm")?;
        let paths = make_paths(&dir.0)?;
        let project = dir.0.join("project");
        let mut server = MockProvider::start(vec![ResponseScript::sse(chat_sse_tool_calls(
            None,
            &[("bash", json!({"command": "echo wait"}))],
            20,
            10,
        ))])?;
        let backend = TestBackendConfig {
            paths,
            base_url: server.base_url(),
            mode: Mode::Normal,
            approval: Approval::Untrusted,
            sandbox: Sandbox::WorkspaceWrite,
            sandboxed: false,
            image_input: false,
            context_window: None,
            max_turns: 20,
            max_run_tokens: None,
            with_benitoite_checks: false,
            network_gate: None,
            exhaust_storage_budget: false,
            read_exclude: Vec::new(),
            last_session_dir: Arc::new(Mutex::new(None)),
        };
        let mut acp = AcpHarness::start(backend)?;
        acp.send(&json!({"jsonrpc":"2.0","id":1,"method":"session/new","params":{"cwd":project,"mcpServers":[]}}))?;
        let sid = acp
            .recv()?
            .pointer("/result/sessionId")
            .and_then(Value::as_str)
            .ok_or("sid")?
            .to_owned();

        acp.send(&json!({
            "jsonrpc": "2.0",
            "id": "p-cancel-1",
            "method": "session/prompt",
            "params": {"sessionId": sid, "prompt": [{"type": "text", "text": "run"}]}
        }))?;
        loop {
            let msg = acp.recv()?;
            if msg.get("method") == Some(&json!("session/request_permission")) {
                break;
            }
        }
        // 確認に答えず session/cancel 通知を送る。
        acp.send(&json!({
            "jsonrpc": "2.0",
            "method": "session/cancel",
            "params": {"sessionId": sid}
        }))?;
        let (_, resp) = acp.recv_until_id(&json!("p-cancel-1"))?;
        assert_eq!(
            resp.pointer("/result/stopReason"),
            Some(&json!("cancelled"))
        );
        acp.close()?;
        server.shutdown()?;
    }

    // (2) NetworkGate（通信の中継）の確認待ち中の id 付き session/cancel
    {
        let dir = TestDir::new("cancel-network-perm")?;
        let paths = make_paths(&dir.0)?;
        let project = dir.0.join("project");
        let gate = Arc::new(NetworkGate::new(BTreeSet::new(), true));
        let (entered_tx, entered_rx) = mpsc::channel();
        let (_release_tx, release_rx) = mpsc::channel();
        let mut server = MockProvider::start(vec![ResponseScript {
            status: 200,
            headers: vec![("content-type".into(), "text/event-stream".into())],
            body: chat_sse_text(None, "never reached", 10, 5),
            delivery: Delivery::Controlled {
                entered: entered_tx,
                release: release_rx,
            },
        }])?;
        let backend = TestBackendConfig {
            paths,
            base_url: server.base_url(),
            mode: Mode::Normal,
            approval: Approval::OnRequest,
            sandbox: Sandbox::WorkspaceWrite,
            sandboxed: false,
            image_input: false,
            context_window: None,
            max_turns: 20,
            max_run_tokens: None,
            with_benitoite_checks: false,
            network_gate: Some(Arc::clone(&gate)),
            exhaust_storage_budget: false,
            read_exclude: Vec::new(),
            last_session_dir: Arc::new(Mutex::new(None)),
        };
        let mut acp = AcpHarness::start(backend)?;
        acp.send(&json!({"jsonrpc":"2.0","id":1,"method":"session/new","params":{"cwd":project,"mcpServers":[]}}))?;
        let sid = acp
            .recv()?
            .pointer("/result/sessionId")
            .and_then(Value::as_str)
            .ok_or("sid")?
            .to_owned();

        // プロンプト実行中に NetworkGate / ProxyRun が通信確認を上げる。
        acp.send(&json!({
            "jsonrpc": "2.0",
            "id": "p-cancel-net",
            "method": "session/prompt",
            "params": {"sessionId": sid, "prompt": [{"type": "text", "text": "fetch via proxy"}]}
        }))?;
        entered_rx.recv_timeout(Duration::from_secs(5))?;

        let listener = TcpListener::bind("127.0.0.1:0")?;
        let proxy_addr = listener.local_addr()?;
        let proxy_run = ProxyRun::new(
            Arc::clone(&gate),
            "curl https://example.test".into(),
            project.clone(),
        );
        let _proxy_server = serve(vec![listener], Arc::clone(&proxy_run))?;
        let proxy_client = thread::spawn(move || -> io::Result<String> {
            let mut stream = TcpStream::connect(proxy_addr)?;
            stream.set_read_timeout(Some(Duration::from_secs(5)))?;
            stream.write_all(
                b"CONNECT example.test:443 HTTP/1.1\r\nHost: example.test:443\r\n\r\n",
            )?;
            stream.flush()?;
            let mut resp = String::new();
            let _read = stream.read_to_string(&mut resp);
            Ok(resp)
        });

        let perm = loop {
            let msg = acp.recv()?;
            if msg.get("method") == Some(&json!("session/request_permission")) {
                break msg;
            }
        };
        assert_eq!(perm.pointer("/params/toolCall/kind"), Some(&json!("fetch")));
        let cmd = perm
            .pointer("/params/toolCall/rawInput/command")
            .and_then(Value::as_str)
            .unwrap_or_default();
        assert!(cmd.contains("example.test"), "{cmd}");
        assert!(cmd.contains("every port of example.test"), "{cmd}");

        // id 付き要求の session/cancel を送る → {} と stopReason: "cancelled" が返り、ProxyRun も即解放される。
        acp.send(&json!({
            "jsonrpc": "2.0",
            "id": "cancel-req",
            "method": "session/cancel",
            "params": {"sessionId": sid}
        }))?;
        let mut saw_cancel_ack = false;
        let mut saw_prompt_cancelled = false;
        while !(saw_cancel_ack && saw_prompt_cancelled) {
            let msg = acp.recv()?;
            if msg.get("id") == Some(&json!("cancel-req")) {
                assert_eq!(msg.get("result"), Some(&json!({})));
                saw_cancel_ack = true;
            } else if msg.get("id") == Some(&json!("p-cancel-net")) {
                assert_eq!(msg.pointer("/result/stopReason"), Some(&json!("cancelled")));
                saw_prompt_cancelled = true;
            }
        }
        let proxy_resp = proxy_client.join().map_err(|_| "proxy client")??;
        assert!(proxy_resp.starts_with("HTTP/1.1 403"), "{proxy_resp}");
        proxy_run.finish();
        acp.close()?;
        server.shutdown()?;
    }

    // (3) AgentEvent チャネル（容量 64）が満杯になりうる大量チャンク配信中の session/cancel
    {
        let dir = TestDir::new("cancel-full-events")?;
        let paths = make_paths(&dir.0)?;
        let project = dir.0.join("project");
        let mut server = MockProvider::start(vec![ResponseScript {
            status: 200,
            headers: vec![("content-type".into(), "text/event-stream".into())],
            body: chat_sse_many_deltas(160),
            delivery: Delivery::Chunks {
                size: 40,
                delay: Duration::from_millis(2),
            },
        }])?;
        let backend = TestBackendConfig {
            paths,
            base_url: server.base_url(),
            mode: Mode::Normal,
            approval: Approval::OnRequest,
            sandbox: Sandbox::WorkspaceWrite,
            sandboxed: false,
            image_input: false,
            context_window: None,
            max_turns: 20,
            max_run_tokens: None,
            with_benitoite_checks: false,
            network_gate: None,
            exhaust_storage_budget: false,
            read_exclude: Vec::new(),
            last_session_dir: Arc::new(Mutex::new(None)),
        };
        let mut acp = AcpHarness::start(backend)?;
        acp.send(&json!({"jsonrpc":"2.0","id":1,"method":"session/new","params":{"cwd":project,"mcpServers":[]}}))?;
        let sid = acp
            .recv()?
            .pointer("/result/sessionId")
            .and_then(Value::as_str)
            .ok_or("sid")?
            .to_owned();

        acp.send(&json!({
            "jsonrpc": "2.0",
            "id": "p-cancel-stream",
            "method": "session/prompt",
            "params": {"sessionId": sid, "prompt": [{"type": "text", "text": "stream lots"}]}
        }))?;
        // 最初のチャンクを受信した直後に cancel を送る。
        let first = acp.recv()?;
        assert_eq!(first.get("method"), Some(&json!("session/update")));
        acp.send(&json!({
            "jsonrpc": "2.0",
            "method": "session/cancel",
            "params": {"sessionId": sid}
        }))?;
        let (_, resp) = acp.recv_until_id(&json!("p-cancel-stream"))?;
        assert_eq!(
            resp.pointer("/result/stopReason"),
            Some(&json!("cancelled"))
        );
        // 二重応答がないことを確認。
        acp.send(
            &json!({"jsonrpc":"2.0","id":77,"method":"initialize","params":{"protocolVersion":1}}),
        )?;
        assert_eq!(acp.recv()?.get("id"), Some(&json!(77)));
        acp.close()?;
        server.shutdown()?;
    }

    Ok(())
}

// 作成時の関門: (1) stdout の Broken pipe（stdin は開いたまま）と、
// (2) stdout を開いたまま一切読まず出力キュー（256 行）を満杯にして stdin を閉じた状態（EOF）で、
// 主スレッドがブロックせず後始末を終えて終了する契約を守る。
#[test]
fn broken_pipe_and_full_output_queue_plus_eof_exit_without_deadlock() -> TestResult {
    struct FailAfterN {
        allowed: usize,
        count: AtomicUsize,
        failed_signal: mpsc::SyncSender<()>,
    }
    impl Write for FailAfterN {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            let n = self.count.fetch_add(1, Ordering::AcqRel);
            if n >= self.allowed {
                let _sent = self.failed_signal.try_send(());
                return Err(io::Error::from(io::ErrorKind::BrokenPipe));
            }
            Ok(buf.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    // (1) Broken pipe: stdin を開いたままでも即座に後始末して戻る。
    {
        let dir = TestDir::new("broken-pipe")?;
        let paths = make_paths(&dir.0)?;
        let mut server = MockProvider::start(vec![ResponseScript::sse(chat_sse_many_deltas(10))])?;
        let mut backend = TestBackendConfig {
            paths,
            base_url: server.base_url(),
            mode: Mode::Normal,
            approval: Approval::OnRequest,
            sandbox: Sandbox::WorkspaceWrite,
            sandboxed: false,
            image_input: false,
            context_window: None,
            max_turns: 20,
            max_run_tokens: None,
            with_benitoite_checks: false,
            network_gate: None,
            exhaust_storage_budget: false,
            read_exclude: Vec::new(),
            last_session_dir: Arc::new(Mutex::new(None)),
        };
        let (mut client_in, server_in) = UnixStream::pair()?;
        let (fail_tx, _fail_rx) = mpsc::sync_channel(1);
        let writer = FailAfterN {
            allowed: 1, // session/new の応答のあと、session/prompt の最初の update で BrokenPipe
            count: AtomicUsize::new(0),
            failed_signal: fail_tx,
        };
        let (diag, _) = SharedBuf::new();
        let cancel = Arc::new(AtomicBool::new(false));
        let (done_tx, done_rx) = mpsc::sync_channel(1);
        let handle = thread::spawn(move || {
            let res = benitoiteh::ui::acp::run(server_in, writer, diag, &mut backend, cancel);
            let _sent = done_tx.try_send(());
            res
        });

        // session/new → session/prompt（sessionId は固定できないので 1 回だけ read する代わりに
        // `initialize` を 2 回送って 2 回目で BrokenPipe を起こし、さらに開いた stdin を放置しても終了することを確認）
        writeln!(
            client_in,
            "{}",
            json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":1}})
        )?;
        writeln!(
            client_in,
            "{}",
            json!({"jsonrpc":"2.0","id":2,"method":"initialize","params":{"protocolVersion":1}})
        )?;
        client_in.flush()?;
        // client_in は閉じない（stdin スレッドが開いたままブロックしている状態）！
        let started = Instant::now();
        done_rx
            .recv_timeout(Duration::from_secs(5))
            .map_err(|_| "broken pipe must exit without waiting for open stdin")?;
        assert!(started.elapsed() < Duration::from_secs(5));
        handle.join().map_err(|_| "server panicked")??;
        drop(client_in);
        server.shutdown()?;
    }

    // (2) stdout が一切読まれずブロックして出力キュー（256 行）が満杯 + stdin EOF でデッドロックしない。
    {
        struct BlockingWriter {
            allow_first: AtomicBool,
            release: mpsc::Receiver<()>,
            first_written: mpsc::SyncSender<()>,
        }
        impl Write for BlockingWriter {
            fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
                if self.allow_first.swap(false, Ordering::AcqRel) {
                    let _sent = self.first_written.try_send(());
                    return Ok(buf.len());
                }
                // クライアントが stdout を開いたまま一切読まない状態を模擬する。
                let _waited = self.release.recv();
                Ok(buf.len())
            }
            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }

        let dir = TestDir::new("full-queue-eof")?;
        let paths = make_paths(&dir.0)?;
        let project = dir.0.join("project");
        let mut server = MockProvider::start(vec![ResponseScript::sse(chat_sse_many_deltas(400))])?;
        let last_dir = Arc::new(Mutex::new(None));
        let mut backend = TestBackendConfig {
            paths,
            base_url: server.base_url(),
            mode: Mode::Normal,
            approval: Approval::OnRequest,
            sandbox: Sandbox::WorkspaceWrite,
            sandboxed: false,
            image_input: false,
            context_window: None,
            max_turns: 20,
            max_run_tokens: None,
            with_benitoite_checks: false,
            network_gate: None,
            exhaust_storage_budget: false,
            read_exclude: Vec::new(),
            last_session_dir: Arc::clone(&last_dir),
        };
        let (mut client_in, server_in) = UnixStream::pair()?;
        let (release_tx, release_rx) = mpsc::channel();
        let (first_tx, first_rx) = mpsc::sync_channel(1);
        let writer = BlockingWriter {
            allow_first: AtomicBool::new(true),
            release: release_rx,
            first_written: first_tx,
        };
        let (diag, _) = SharedBuf::new();
        let cancel = Arc::new(AtomicBool::new(false));
        let (done_tx, done_rx) = mpsc::sync_channel(1);
        let handle = thread::spawn(move || {
            let res = benitoiteh::ui::acp::run(server_in, writer, diag, &mut backend, cancel);
            let _sent = done_tx.try_send(());
            res
        });

        writeln!(
            client_in,
            "{}",
            json!({"jsonrpc":"2.0","id":1,"method":"session/new","params":{"cwd":project,"mcpServers":[]}})
        )?;
        client_in.flush()?;
        first_rx.recv_timeout(Duration::from_secs(5))?;
        let session_dir = last_dir
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
            .ok_or("session dir")?;
        let sid = session_dir
            .file_name()
            .and_then(|s| s.to_str())
            .ok_or("sid")?
            .to_owned();

        // 400 件の TextDelta を出す prompt を送り、続けて stdin を閉じて EOF にする。
        // stdout の書き手はブロックしたままだが、256 行キューが満杯になるか EOF で即座に終了すること。
        writeln!(
            client_in,
            "{}",
            json!({"jsonrpc":"2.0","id":"p-flood","method":"session/prompt","params":{"sessionId":sid,"prompt":[{"type":"text","text":"flood"}]}})
        )?;
        client_in.flush()?;
        drop(client_in);

        done_rx
            .recv_timeout(Duration::from_secs(5))
            .map_err(|_| "blocked stdout + EOF must not deadlock")?;
        handle.join().map_err(|_| "server panicked")??;
        // テスト後始末のためだけに背景のブロック中ライタを解放する。
        let _released = release_tx.send(());
        server.shutdown()?;
    }

    Ok(())
}

// 作成時の関門: StopReason（max_turns・max_run_tokens）の ACP stopReason 変換と、
// image_input == true での base64 画像の検証・拒否（不正 base64・21 枚超過・有効 PNG）を守る。
#[test]
fn stop_reasons_and_inline_image_validation() -> TestResult {
    let dir = TestDir::new("stop-and-image")?;
    let paths = make_paths(&dir.0)?;
    let project = dir.0.join("project");
    fs::write(project.join("a.txt"), "x")?;

    // 最小の有効な 1x1 PNG（67 bytes）
    let tiny_png_b64 = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII=";
    let mut server = MockProvider::start(vec![
        // 1: 有効な PNG 画像を受け取って応答完了
        ResponseScript::sse(chat_sse_text(None, "saw image", 20, 5)),
        // 2: max_turns=1 に達する（1 番目で道具を呼ぶので 2 番目に入れず MaxTurns）
        ResponseScript::sse(chat_sse_tool_calls(
            None,
            &[("read", json!({"path": "a.txt"}))],
            20,
            5,
        )),
    ])?;
    let backend = TestBackendConfig {
        paths,
        base_url: server.base_url(),
        mode: Mode::Normal,
        approval: Approval::OnRequest,
        sandbox: Sandbox::WorkspaceWrite,
        sandboxed: false,
        image_input: true,
        context_window: None,
        max_turns: 1,
        max_run_tokens: None,
        with_benitoite_checks: false,
        network_gate: None,
        exhaust_storage_budget: false,
        read_exclude: Vec::new(),
        last_session_dir: Arc::new(Mutex::new(None)),
    };
    let mut acp = AcpHarness::start(backend)?;
    acp.send(&json!({"jsonrpc":"2.0","id":1,"method":"session/new","params":{"cwd":project,"mcpServers":[]}}))?;
    let sid = acp
        .recv()?
        .pointer("/result/sessionId")
        .and_then(Value::as_str)
        .ok_or("sid")?
        .to_owned();

    // 不正 base64、非対応 MIME、21 枚超過は -32602 で拒み提供者へ送らない。
    let too_many: Vec<Value> = (0..21)
        .map(|_| json!({"type": "image", "data": tiny_png_b64, "mimeType": "image/png"}))
        .collect();
    for (req_id, bad) in [
        (
            2,
            json!([{"type": "image", "data": "not!base64", "mimeType": "image/png"}]),
        ),
        (
            3,
            json!([{"type": "image", "data": tiny_png_b64, "mimeType": "image/svg+xml"}]),
        ),
        (4, Value::Array(too_many)),
    ] {
        acp.send(&json!({
            "jsonrpc": "2.0",
            "id": req_id,
            "method": "session/prompt",
            "params": {"sessionId": sid, "prompt": bad}
        }))?;
        assert_eq!(acp.recv()?.pointer("/error/code"), Some(&json!(-32602)));
    }
    assert!(server.requests()?.is_empty());

    // 有効な PNG base64 は objects/ に保存されて提供者へ届く。
    acp.send(&json!({
        "jsonrpc": "2.0",
        "id": "p-img",
        "method": "session/prompt",
        "params": {
            "sessionId": sid,
            "prompt": [
                {"type": "text", "text": "describe"},
                {"type": "image", "data": tiny_png_b64, "mimeType": "image/png"}
            ]
        }
    }))?;
    let (_, resp) = acp.recv_until_id(&json!("p-img"))?;
    assert_eq!(resp.pointer("/result/stopReason"), Some(&json!("end_turn")));
    assert_eq!(server.requests()?.len(), 1);

    // max_turns 超過 → stopReason: "max_turn_requests"
    acp.send(&json!({
        "jsonrpc": "2.0",
        "id": "p-turns",
        "method": "session/prompt",
        "params": {"sessionId": sid, "prompt": [{"type": "text", "text": "loop"}]}
    }))?;
    let (_, resp) = acp.recv_until_id(&json!("p-turns"))?;
    assert_eq!(
        resp.pointer("/result/stopReason"),
        Some(&json!("max_turn_requests"))
    );

    acp.close()?;
    server.shutdown()?;

    // max_run_tokens 超過 → stopReason: "max_tokens"
    {
        let dir2 = TestDir::new("stop-max-tokens")?;
        let paths2 = make_paths(&dir2.0)?;
        let project2 = dir2.0.join("project");
        let mut server2 = MockProvider::start(vec![ResponseScript::sse(chat_sse_text(
            None,
            "token heavy",
            100,
            50,
        ))])?;
        let backend2 = TestBackendConfig {
            paths: paths2,
            base_url: server2.base_url(),
            mode: Mode::Normal,
            approval: Approval::OnRequest,
            sandbox: Sandbox::WorkspaceWrite,
            sandboxed: false,
            image_input: false,
            context_window: None,
            max_turns: 20,
            max_run_tokens: Some(100),
            with_benitoite_checks: false,
            network_gate: None,
            exhaust_storage_budget: false,
            read_exclude: Vec::new(),
            last_session_dir: Arc::new(Mutex::new(None)),
        };
        let mut acp2 = AcpHarness::start(backend2)?;
        acp2.send(&json!({"jsonrpc":"2.0","id":1,"method":"session/new","params":{"cwd":project2,"mcpServers":[]}}))?;
        let sid2 = acp2
            .recv()?
            .pointer("/result/sessionId")
            .and_then(Value::as_str)
            .ok_or("sid2")?
            .to_owned();
        acp2.send(&json!({
            "jsonrpc": "2.0",
            "id": "p-tokens",
            "method": "session/prompt",
            "params": {"sessionId": sid2, "prompt": [{"type": "text", "text": "spend tokens"}]}
        }))?;
        let (_, resp2) = acp2.recv_until_id(&json!("p-tokens"))?;
        assert_eq!(
            resp2.pointer("/result/stopReason"),
            Some(&json!("max_tokens"))
        );
        acp2.close()?;
        server2.shutdown()?;
    }
    Ok(())
}
