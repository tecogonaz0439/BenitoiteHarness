//! `benitoiteh acp` の安全の性質を、本物の処理系（`v0.0.12`）と本物の OS のサンドボックス
//! （macOS の Seatbelt、Linux の bubblewrap + Landlock + seccomp）を通して確かめる統合テスト
//! （設計書 03-01、03-02、05-03、07-01「安全の性質のテスト」）。
//!
//! OS のサンドボックスを要するので、実装プラン 00-common「検査」の決まりにより `sandbox_*.rs` に置く。

#[allow(dead_code)]
mod support;

use std::error::Error;
use std::fs;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, ExitStatus, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant, SystemTime};

use benitoiteh::session::SessionId;
use serde_json::{Value, json};
use support::mock_provider::{Delivery, MockProvider, ResponseScript};

type TestResult = Result<(), Box<dyn Error>>;

const API_KEY_ENV: &str = "BENITOITEH_ACP_SANDBOX_TEST_KEY";
const API_KEY_VALUE: &str = "synthetic-acp-sandbox-key-never-leak";
const RECV_TIMEOUT: Duration = Duration::from_secs(30);

struct TestDir(PathBuf);

impl TestDir {
    fn new(name: &str) -> Result<Self, Box<dyn Error>> {
        let id = SessionId::generate(SystemTime::now())?;
        let path = std::env::temp_dir().join(format!("bh-sacp-{name}-{id}"));
        fs::create_dir(&path)?;
        let root = path.canonicalize()?;
        for child in ["home", "config/benitoiteh", "state", "tmp", "project"] {
            fs::create_dir_all(root.join(child))?;
        }
        Ok(Self(root))
    }

    fn project(&self) -> PathBuf {
        self.0.join("project")
    }

    fn write_config(&self, base_url: &str, default_mode: &str, extra: &str) -> TestResult {
        let benitoite_path = serde_json::to_string(&support::benitoite()?)?;
        let config = format!(
            "default_mode = \"{default_mode}\"\n\
             default_provider = \"mock\"\n\
             \n\
             [providers.mock]\n\
             kind = \"openai-chat\"\n\
             base_url = \"{base_url}\"\n\
             model = \"mock-model\"\n\
             api_key = {{ env = \"{API_KEY_ENV}\" }}\n\
             \n\
             [benitoite]\n\
             path = {benitoite_path}\n\
             {extra}"
        );
        fs::write(self.0.join("config/benitoiteh/config.toml"), config)?;
        Ok(())
    }

    fn session_jsonl(&self, session_id: &str) -> Result<String, Box<dyn Error>> {
        let path = self
            .0
            .join("state/benitoiteh/sessions")
            .join(session_id)
            .join("session.jsonl");
        Ok(fs::read_to_string(path)?)
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _removed = fs::remove_dir_all(&self.0);
    }
}

fn chat_sse_text(text: &str) -> Vec<u8> {
    format!(
        "data: {}\n\ndata: {}\n\ndata: [DONE]\n\n",
        json!({
            "choices": [{"index": 0, "delta": {"content": text}, "finish_reason": null}]
        }),
        json!({
            "choices": [{"index": 0, "delta": {}, "finish_reason": "stop"}],
            "usage": {"prompt_tokens": 10, "completion_tokens": 5}
        })
    )
    .into_bytes()
}

fn chat_sse_tool_call(call_id: &str, name: &str, args: Value) -> Vec<u8> {
    format!(
        "data: {}\n\ndata: {}\n\ndata: [DONE]\n\n",
        json!({
            "choices": [{
                "index": 0,
                "delta": {
                    "tool_calls": [{
                        "index": 0,
                        "id": call_id,
                        "type": "function",
                        "function": {
                            "name": name,
                            "arguments": args.to_string(),
                        }
                    }]
                },
                "finish_reason": null
            }]
        }),
        json!({
            "choices": [{"index": 0, "delta": {}, "finish_reason": "tool_calls"}],
            "usage": {"prompt_tokens": 20, "completion_tokens": 10}
        })
    )
    .into_bytes()
}

/// 本物の `benitoiteh acp` 子プロセスをパイプで駆動する補助。
/// 標準出力・標準エラー出力を別スレッドで読み、パイプ詰まりと無期限待機を防ぐ。
struct AcpProcess {
    child: Option<Child>,
    stdin: Option<ChildStdin>,
    stdout_rx: Receiver<io::Result<String>>,
    stdout_thread: Option<JoinHandle<()>>,
    stderr_buf: Arc<Mutex<Vec<u8>>>,
    stderr_thread: Option<JoinHandle<()>>,
    raw_stdout_lines: Vec<String>,
}

impl AcpProcess {
    fn spawn(
        dir: &TestDir,
        args: &[&str],
        extra_env: &[(&str, &str)],
    ) -> Result<Self, Box<dyn Error>> {
        let mut command = Command::new(env!("CARGO_BIN_EXE_benitoiteh"));
        command
            .arg("acp")
            .args(args)
            .current_dir(dir.project())
            .env("HOME", dir.0.join("home"))
            .env("XDG_CONFIG_HOME", dir.0.join("config"))
            .env("XDG_STATE_HOME", dir.0.join("state"))
            .env("TMPDIR", dir.0.join("tmp"))
            .env(API_KEY_ENV, API_KEY_VALUE)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        for (key, value) in extra_env {
            command.env(key, value);
        }
        let mut child = command.spawn()?;
        let stdin = child.stdin.take().ok_or("missing child stdin")?;
        let stdout = child.stdout.take().ok_or("missing child stdout")?;
        let mut stderr = child.stderr.take().ok_or("missing child stderr")?;

        let (stdout_tx, stdout_rx) = mpsc::channel();
        let stdout_thread = thread::Builder::new().spawn(move || {
            let mut reader = BufReader::new(stdout);
            loop {
                let mut line = String::new();
                match reader.read_line(&mut line) {
                    Ok(0) => break,
                    Ok(_) => {
                        if stdout_tx.send(Ok(line)).is_err() {
                            break;
                        }
                    }
                    Err(error) => {
                        let _sent = stdout_tx.send(Err(error));
                        break;
                    }
                }
            }
        })?;

        let stderr_buf = Arc::new(Mutex::new(Vec::new()));
        let stderr_sink = Arc::clone(&stderr_buf);
        let stderr_thread = thread::Builder::new().spawn(move || {
            let mut chunk = [0_u8; 4096];
            loop {
                match stderr.read(&mut chunk) {
                    Ok(0) | Err(_) => break,
                    Ok(count) => {
                        stderr_sink
                            .lock()
                            .unwrap_or_else(PoisonError::into_inner)
                            .extend_from_slice(chunk.get(..count).unwrap_or_default());
                    }
                }
            }
        })?;

        Ok(Self {
            child: Some(child),
            stdin: Some(stdin),
            stdout_rx,
            stdout_thread: Some(stdout_thread),
            stderr_buf,
            stderr_thread: Some(stderr_thread),
            raw_stdout_lines: Vec::new(),
        })
    }

    fn send(&mut self, msg: &Value) -> TestResult {
        let stdin = self.stdin.as_mut().ok_or("child stdin already closed")?;
        let mut bytes = serde_json::to_vec(msg)?;
        bytes.push(b'\n');
        stdin.write_all(&bytes)?;
        stdin.flush()?;
        Ok(())
    }

    fn recv(&mut self) -> Result<Value, Box<dyn Error>> {
        let line = self
            .stdout_rx
            .recv_timeout(RECV_TIMEOUT)
            .map_err(|error| {
                format!(
                    "timed out waiting for ACP stdout ({error}); stdout={}; stderr={}",
                    self.stdout_text(),
                    self.stderr_text()
                )
            })??;
        self.raw_stdout_lines.push(line.clone());
        Ok(serde_json::from_str(&line)?)
    }

    fn initialize(&mut self) -> TestResult {
        self.send(&json!({
            "jsonrpc": "2.0",
            "id": "init",
            "method": "initialize",
            "params": {"protocolVersion": 1}
        }))?;
        let resp = self.recv()?;
        assert_eq!(resp.get("id"), Some(&json!("init")), "{resp}");
        assert_eq!(
            resp.pointer("/result/protocolVersion"),
            Some(&json!(1)),
            "{resp}"
        );
        Ok(())
    }

    fn new_session(&mut self, cwd: &Path, mcp_servers: Value) -> Result<String, Box<dyn Error>> {
        self.send(&json!({
            "jsonrpc": "2.0",
            "id": "new-session",
            "method": "session/new",
            "params": {
                "cwd": cwd,
                "mcpServers": mcp_servers,
            }
        }))?;
        let resp = self.recv()?;
        assert_eq!(resp.get("id"), Some(&json!("new-session")), "{resp}");
        let session_id = resp
            .pointer("/result/sessionId")
            .and_then(Value::as_str)
            .ok_or_else(|| format!("missing sessionId in {resp}; stderr={}", self.stderr_text()))?
            .to_owned();
        Ok(session_id)
    }

    fn send_prompt(&mut self, id: &str, session_id: &str, text: &str) -> TestResult {
        self.send(&json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "session/prompt",
            "params": {
                "sessionId": session_id,
                "prompt": [{"type": "text", "text": text}],
            }
        }))
    }

    fn reply_permission(&mut self, req_id: &Value, option_id: &str) -> TestResult {
        self.send(&json!({
            "jsonrpc": "2.0",
            "id": req_id,
            "result": {
                "outcome": {
                    "outcome": "selected",
                    "optionId": option_id,
                }
            }
        }))
    }

    fn send_cancel(&mut self, session_id: &str) -> TestResult {
        self.send(&json!({
            "jsonrpc": "2.0",
            "method": "session/cancel",
            "params": {"sessionId": session_id}
        }))
    }

    fn close_stdin(&mut self) {
        self.stdin.take();
    }

    fn stderr_text(&self) -> String {
        String::from_utf8_lossy(
            &self
                .stderr_buf
                .lock()
                .unwrap_or_else(PoisonError::into_inner),
        )
        .into_owned()
    }

    fn stdout_text(&self) -> String {
        self.raw_stdout_lines.concat()
    }

    fn wait_exit(
        mut self,
        timeout: Duration,
    ) -> Result<(ExitStatus, String, String), Box<dyn Error>> {
        self.stdin.take();
        let child = self.child.as_mut().ok_or("child already waited")?;
        let started = Instant::now();
        let status = loop {
            if let Some(status) = child.try_wait()? {
                break status;
            }
            if started.elapsed() > timeout {
                let _killed = child.kill();
                let _waited = child.wait();
                return Err(format!(
                    "benitoiteh acp did not exit within {timeout:?}; stderr={}",
                    self.stderr_text()
                )
                .into());
            }
            thread::sleep(Duration::from_millis(10));
        };
        self.child.take();
        if let Some(handle) = self.stdout_thread.take() {
            let _joined = handle.join();
        }
        if let Some(handle) = self.stderr_thread.take() {
            let _joined = handle.join();
        }
        let stdout = self.stdout_text();
        let stderr = self.stderr_text();
        Ok((status, stdout, stderr))
    }
}

impl Drop for AcpProcess {
    fn drop(&mut self) {
        self.stdin.take();
        if let Some(child) = &mut self.child {
            let _killed = child.kill();
            let _waited = child.wait();
        }
    }
}

fn permission_errno(message: &str) -> bool {
    [rustix::io::Errno::ACCESS, rustix::io::Errno::PERM]
        .iter()
        .any(|error| message.contains(&format!("(os error {})", error.raw_os_error())))
}

fn option_ids(perm: &Value) -> Vec<&str> {
    perm.pointer("/params/options")
        .and_then(Value::as_array)
        .map(|list| {
            list.iter()
                .filter_map(|opt| opt.get("optionId").and_then(Value::as_str))
                .collect()
        })
        .unwrap_or_default()
}

fn wait_until(timeout: Duration, mut predicate: impl FnMut() -> bool) -> TestResult {
    let started = Instant::now();
    while !predicate() {
        if started.elapsed() > timeout {
            return Err(format!("timed out after {timeout:?}").into());
        }
        thread::sleep(Duration::from_millis(20));
    }
    Ok(())
}

/// 名前空間の内外で PID が異なる Linux でも確実に子孫を照合できるよう、
/// コマンドラインに含まれる固有の目印文字列で生存中のプロセスを探す。
#[cfg(target_os = "linux")]
fn marked_processes(marker: &str) -> Result<Vec<rustix::process::Pid>, Box<dyn Error>> {
    let self_pid = std::process::id();
    let needle = marker.as_bytes();
    let mut found = Vec::new();
    for entry in fs::read_dir("/proc")? {
        let entry = entry?;
        let Some(raw) = entry
            .file_name()
            .to_str()
            .and_then(|s| s.parse::<i32>().ok())
        else {
            continue;
        };
        if raw <= 0 || raw.unsigned_abs() == self_pid {
            continue;
        }
        let Some(pid) = rustix::process::Pid::from_raw(raw) else {
            continue;
        };
        let stat = match fs::read_to_string(entry.path().join("stat")) {
            Ok(stat) => stat,
            Err(_) => continue,
        };
        if stat
            .rsplit_once(") ")
            .is_some_and(|(_, tail)| tail.starts_with('Z'))
        {
            continue;
        }
        let cmdline = match fs::read(entry.path().join("cmdline")) {
            Ok(bytes) => bytes,
            Err(_) => continue,
        };
        if !needle.is_empty() && cmdline.windows(needle.len()).any(|window| window == needle) {
            found.push(pid);
        }
    }
    Ok(found)
}

#[cfg(not(target_os = "linux"))]
fn marked_processes(marker: &str) -> Result<Vec<rustix::process::Pid>, Box<dyn Error>> {
    let self_pid = std::process::id();
    // macOS の `ps` では UID に `-2` が現れうるため UID 列は読まない（実装プラン 00-common）。
    let output = Command::new("/bin/ps")
        .args(["-axo", "pid=,stat=,command="])
        .output()?;
    if !output.status.success() {
        return Err(format!("ps failed: {}", output.status).into());
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let mut found = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim_start();
        let mut parts = trimmed.split_whitespace();
        let Some(pid_str) = parts.next() else {
            continue;
        };
        let Some(stat) = parts.next() else {
            continue;
        };
        if stat.starts_with('Z') {
            continue;
        }
        let Ok(raw) = pid_str.parse::<i32>() else {
            continue;
        };
        if raw <= 0 || raw.unsigned_abs() == self_pid {
            continue;
        }
        if trimmed.contains(marker)
            && let Some(pid) = rustix::process::Pid::from_raw(raw)
        {
            found.push(pid);
        }
    }
    Ok(found)
}

#[cfg(not(target_os = "linux"))]
fn pid_stopped(pid: rustix::process::Pid) -> bool {
    rustix::process::test_kill_process(pid) == Err(rustix::io::Errno::SRCH)
}

#[cfg(not(target_os = "linux"))]
fn read_pids_file(path: &Path) -> Result<Vec<rustix::process::Pid>, Box<dyn Error>> {
    let text = fs::read_to_string(path)?;
    let mut pids = Vec::new();
    for token in text.split_whitespace() {
        let raw = token.parse::<i32>()?;
        let pid = rustix::process::Pid::from_raw(raw).ok_or("invalid pid")?;
        pids.push(pid);
    }
    Ok(pids)
}

/// テストが途中で失敗しても目印プロセスを残さないための後始末ガード。
struct MarkedCleanup(String);

impl Drop for MarkedCleanup {
    fn drop(&mut self) {
        if let Ok(pids) = marked_processes(&self.0) {
            for pid in pids {
                if marked_processes(&self.0).is_ok_and(|current| current.contains(&pid)) {
                    let _sent = rustix::process::kill_process(pid, rustix::process::Signal::KILL);
                }
            }
        }
    }
}

// 作成時の関門: 本物の `benitoiteh acp` 子プロセスと本物の処理系・OS サンドボックスを通して、
// (1) 親環境に置いた `T3_ACP_MCP_*`・`T3CODE_*` が制限モード `benitoite_run` と通常モード `bash` の
// どちらの子プロセス環境にも渡らないこと（対照の通常環境変数は届くこと）、
// (2) `session/new` の `mcpServers` の資格情報・URL・env が stdout・stderr・session.jsonl のどこにも
// 漏れず、stderr に無視件数のみが出ることを守る（設計書 05-03「信頼の境界と安全の原則」）。
#[test]
fn acp_isolates_mcp_env_and_redacts_mcp_servers() -> TestResult {
    let decoy_env = [
        (
            "T3_ACP_MCP_ENDPOINT",
            "http://127.0.0.1:41111/decoy-mcp-endpoint",
        ),
        (
            "T3_ACP_MCP_AUTHORIZATION",
            "Bearer DECOY_T3_ENV_AUTH_TOKEN_777",
        ),
        ("T3_ACP_MCP_NODE", "/usr/local/bin/decoy-t3-node-888"),
        ("T3CODE_THREAD_ID", "decoy-t3-thread-id-999"),
        // 正の対照: 既定の環境規則で通常の環境変数は子プロセスへ届くことを確かめる。
        ("BH_ACP_CONTROL_ENV", "CONTROL_ENV_PRESENT_123"),
    ];
    let decoy_mcp_token = "DECOY_MCP_SERVER_TOKEN_555";
    let decoy_mcp_url = "http://127.0.0.1:49998/decoy-mcp-server-url";
    let decoy_mcp_cmd = "/usr/local/bin/decoy-mcp-stdio-cmd";
    let decoy_mcp_env_key = "MCP_SERVER_SECRET_KEY";
    let decoy_mcp_env_val = "DECOY_MCP_SERVER_ENV_666";
    let mcp_servers = json!([
        {
            "type": "http",
            "name": "t3-bridge",
            "url": decoy_mcp_url,
            "headers": [{"name": "Authorization", "value": decoy_mcp_token}]
        },
        {
            "name": "stdio-bridge",
            "command": decoy_mcp_cmd,
            "args": ["--token", decoy_mcp_token],
            "env": [{"name": decoy_mcp_env_key, "value": decoy_mcp_env_val}]
        }
    ]);
    let forbidden_secrets = [
        "decoy-mcp-endpoint",
        "DECOY_T3_ENV_AUTH_TOKEN_777",
        "decoy-t3-node-888",
        "decoy-t3-thread-id-999",
        decoy_mcp_token,
        decoy_mcp_url,
        decoy_mcp_cmd,
        decoy_mcp_env_key,
        decoy_mcp_env_val,
        API_KEY_VALUE,
    ];
    let forbidden_env_names = [
        "T3_ACP_MCP_ENDPOINT",
        "T3_ACP_MCP_AUTHORIZATION",
        "T3_ACP_MCP_NODE",
        "T3CODE_THREAD_ID",
    ];

    // 1. 制限モードの `benitoite_run`（Process.Environment を allow-once で確認）
    {
        let dir = TestDir::new("mcp-restricted")?;
        let script = r#"import Benitoite.Unofficial.IO.Process
import Benitoite.Unofficial.IO.Console
function checkVar(name: String) -> Unit uses Process.Environment, Console.Write
 match Process.environmentVariable(name) with
 case Result.Ok(Option.Some(value)) -> Console.writeLine("FOUND:${name}=${value}")
 case Result.Ok(Option.None) -> Console.writeLine("ABSENT:${name}")
 case Result.Error(error) -> Console.writeLine("ERR:${name}:${IOError.message(error)}")
 end match
end function
function main() -> Unit uses Process.Environment, Console.Write
 checkVar("T3_ACP_MCP_ENDPOINT")
 checkVar("T3_ACP_MCP_AUTHORIZATION")
 checkVar("T3_ACP_MCP_NODE")
 checkVar("T3CODE_THREAD_ID")
 checkVar("BH_ACP_CONTROL_ENV")
end function
"#;
        fs::write(dir.project().join("check_env.bnt"), script)?;

        let mut server = MockProvider::start(vec![
            ResponseScript::sse(chat_sse_tool_call(
                "call-env-bnt",
                "benitoite_run",
                json!({"path": "check_env.bnt"}),
            )),
            ResponseScript::sse(chat_sse_text("restricted env checked")),
        ])?;
        dir.write_config(&server.base_url(), "restricted", "")?;

        let mut acp = AcpProcess::spawn(&dir, &["--mode", "restricted"], &decoy_env)?;
        acp.initialize()?;
        let session_id = acp.new_session(&dir.project(), mcp_servers.clone())?;
        acp.send_prompt("p-env-bnt", &session_id, "Check environment")?;

        let mut saw_perm = false;
        loop {
            let msg = acp.recv()?;
            if msg.get("method") == Some(&json!("session/request_permission")) {
                saw_perm = true;
                let req_id = msg.get("id").cloned().ok_or("missing permission id")?;
                assert_eq!(
                    msg.pointer("/params/toolCall/kind"),
                    Some(&json!("execute")),
                    "{msg}"
                );
                let cmd = msg
                    .pointer("/params/toolCall/rawInput/command")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                assert!(cmd.contains("Process.Environment"), "{cmd}");
                assert_eq!(
                    option_ids(&msg),
                    vec!["allow-once", "allow-session", "reject-once"]
                );
                acp.reply_permission(&req_id, "allow-once")?;
            } else if msg.get("id") == Some(&json!("p-env-bnt")) {
                assert_eq!(
                    msg.pointer("/result/stopReason"),
                    Some(&json!("end_turn")),
                    "{msg}"
                );
                break;
            }
        }
        assert!(saw_perm, "Process.Environment must require confirmation");
        let (status, stdout, stderr) = acp.wait_exit(Duration::from_secs(15))?;
        assert!(status.success(), "stderr={stderr}");

        let requests = server.requests()?;
        let tool_turn = serde_json::to_string(&requests.get(1).ok_or("missing second turn")?.body)?;
        assert!(
            tool_turn.contains("FOUND:BH_ACP_CONTROL_ENV=CONTROL_ENV_PRESENT_123"),
            "positive control missing: {tool_turn}"
        );
        for name in forbidden_env_names {
            assert!(
                tool_turn.contains(&format!("ABSENT:{name}")),
                "{name} was not absent in benitoite_run: {tool_turn}"
            );
            assert!(
                !tool_turn.contains(&format!("FOUND:{name}")),
                "{name} leaked into benitoite_run: {tool_turn}"
            );
        }

        let session_log = dir.session_jsonl(&session_id)?;
        assert!(
            stderr.contains("Ignoring 2 MCP server(s) from ACP client; benitoiteh does not connect to MCP servers."),
            "missing ignored count in stderr: {stderr}"
        );
        for secret in forbidden_secrets {
            assert!(
                !stdout.contains(secret),
                "secret {secret} leaked in stdout: {stdout}"
            );
            assert!(
                !stderr.contains(secret),
                "secret {secret} leaked in stderr: {stderr}"
            );
            assert!(
                !session_log.contains(secret),
                "secret {secret} leaked in session.jsonl: {session_log}"
            );
            assert!(
                !tool_turn.contains(secret),
                "secret {secret} leaked in provider request: {tool_turn}"
            );
        }
        server.shutdown()?;
    }

    // 2. 通常モードの `bash`（OS サンドボックス内で `env` を出力）
    {
        let dir = TestDir::new("mcp-normal")?;
        let mut server = MockProvider::start(vec![
            ResponseScript::sse(chat_sse_tool_call(
                "call-env-bash",
                "bash",
                json!({"command": "env"}),
            )),
            ResponseScript::sse(chat_sse_text("normal env checked")),
        ])?;
        dir.write_config(&server.base_url(), "normal", "")?;

        let mut acp = AcpProcess::spawn(
            &dir,
            &[
                "--mode",
                "normal",
                "--sandbox",
                "workspace-write",
                "--approval",
                "on-request",
            ],
            &decoy_env,
        )?;
        acp.initialize()?;
        let session_id = acp.new_session(&dir.project(), mcp_servers)?;
        acp.send_prompt("p-env-bash", &session_id, "Print environment")?;

        loop {
            let msg = acp.recv()?;
            if msg.get("id") == Some(&json!("p-env-bash")) {
                assert_eq!(
                    msg.pointer("/result/stopReason"),
                    Some(&json!("end_turn")),
                    "{msg}"
                );
                break;
            }
        }
        let (status, stdout, stderr) = acp.wait_exit(Duration::from_secs(15))?;
        assert!(status.success(), "stderr={stderr}");

        let requests = server.requests()?;
        let tool_turn = serde_json::to_string(&requests.get(1).ok_or("missing second turn")?.body)?;
        assert!(
            tool_turn.contains("BH_ACP_CONTROL_ENV=CONTROL_ENV_PRESENT_123"),
            "positive control missing in bash env: {tool_turn}"
        );
        for name in forbidden_env_names {
            assert!(
                !tool_turn.contains(name),
                "forbidden env name {name} leaked into bash output: {tool_turn}"
            );
        }

        let session_log = dir.session_jsonl(&session_id)?;
        assert!(
            stderr.contains("Ignoring 2 MCP server(s) from ACP client; benitoiteh does not connect to MCP servers."),
            "missing ignored count in stderr: {stderr}"
        );
        for secret in forbidden_secrets {
            assert!(
                !stdout.contains(secret),
                "secret {secret} leaked in stdout: {stdout}"
            );
            assert!(
                !stderr.contains(secret),
                "secret {secret} leaked in stderr: {stderr}"
            );
            assert!(
                !session_log.contains(secret),
                "secret {secret} leaked in session.jsonl: {session_log}"
            );
            assert!(
                !tool_turn.contains(secret),
                "secret {secret} leaked in provider request: {tool_turn}"
            );
        }
        server.shutdown()?;
    }

    Ok(())
}

// 作成時の関門: 本物の `benitoiteh acp --mode restricted`、本物の処理系、本物の OS サンドボックスで、
// (1) `File.Write` スクリプトへの `reject-once` がファイル作成を阻止すること、
// (2) `allow-once` で 1 回目は成功しつつセッションに記憶されず、2 回目で再び確認が届くこと、
// (3) `/bin/sh` を起動するスクリプトは `allow-once` で許可しても OS サンドボックスが起動を拒否すること
// を守る（設計書 03-01、03-02、05-03）。
#[test]
fn acp_restricted_effect_approval_once_and_sandbox_denies_shell() -> TestResult {
    let dir = TestDir::new("restricted-sandbox")?;
    fs::write(
        dir.project().join("write_file.bnt"),
        "import Benitoite.Unofficial.IO.File\n\
         import Benitoite.Unofficial.IO.Console\n\
         function main() -> Unit uses File.Write, Console.Write\n\
          bind _ <- File.writeText(path: \"created.txt\", text: \"written-by-bnt\")\n\
          Console.writeLine(\"WRITE_OK\")\n\
         end function\n",
    )?;
    fs::write(
        dir.project().join("spawn_sh.bnt"),
        "import Benitoite.Unofficial.IO.Process\n\
         import Benitoite.Unofficial.IO.Console\n\
         function main() -> Unit uses Process.Run, Console.Write\n\
          bind result <- Process.run(Process.command(program: \"/bin/sh\", arguments: [\"-c\", \"echo ESCAPED_TO_SHELL\"]))\n\
          match result with\n\
          case Result.Ok(output) ->\n\
           Console.writeLine(\"SHELL_RAN:${Process.Output.exitCode(output)}:${Process.Output.standardOutput(output)}\")\n\
          case Result.Error(error) ->\n\
           Console.writeLine(\"SHELL_DENIED:${IOError.message(error)}\")\n\
          end match\n\
         end function\n",
    )?;

    let mut server = MockProvider::start(vec![
        // Prompt 1: write_file.bnt -> reject-once
        ResponseScript::sse(chat_sse_tool_call(
            "call-write-1",
            "benitoite_run",
            json!({"path": "write_file.bnt"}),
        )),
        ResponseScript::sse(chat_sse_text("first write declined")),
        // Prompt 2: write_file.bnt -> allow-once
        ResponseScript::sse(chat_sse_tool_call(
            "call-write-2",
            "benitoite_run",
            json!({"path": "write_file.bnt"}),
        )),
        ResponseScript::sse(chat_sse_text("second write ran")),
        // Prompt 3: write_file.bnt again -> must ask permission again (allow-once not remembered)
        ResponseScript::sse(chat_sse_tool_call(
            "call-write-3",
            "benitoite_run",
            json!({"path": "write_file.bnt"}),
        )),
        ResponseScript::sse(chat_sse_text("third write declined")),
        // Prompt 4: spawn_sh.bnt -> allow-once, OS sandbox must deny /bin/sh
        ResponseScript::sse(chat_sse_tool_call(
            "call-spawn-sh",
            "benitoite_run",
            json!({"path": "spawn_sh.bnt"}),
        )),
        ResponseScript::sse(chat_sse_text("shell attempt finished")),
    ])?;
    dir.write_config(&server.base_url(), "restricted", "")?;

    let mut acp = AcpProcess::spawn(&dir, &["--mode", "restricted"], &[])?;
    acp.initialize()?;
    let session_id = acp.new_session(&dir.project(), json!([]))?;
    let created = dir.project().join("created.txt");

    // 1. reject-once -> 実行阻止・ファイル未作成
    acp.send_prompt("p1", &session_id, "Run write_file 1")?;
    let mut perm_count = 0usize;
    loop {
        let msg = acp.recv()?;
        if msg.get("method") == Some(&json!("session/request_permission")) {
            perm_count = perm_count.saturating_add(1);
            let req_id = msg.get("id").cloned().ok_or("perm id")?;
            assert_eq!(
                msg.pointer("/params/toolCall/kind"),
                Some(&json!("execute"))
            );
            let cmd = msg
                .pointer("/params/toolCall/rawInput/command")
                .and_then(Value::as_str)
                .unwrap_or_default();
            assert!(cmd.contains("File.Write"), "{cmd}");
            acp.reply_permission(&req_id, "reject-once")?;
        } else if msg.get("id") == Some(&json!("p1")) {
            assert_eq!(msg.pointer("/result/stopReason"), Some(&json!("end_turn")));
            break;
        }
    }
    assert_eq!(perm_count, 1);
    assert!(
        !created.exists(),
        "reject-once must prevent write_file.bnt from creating created.txt"
    );

    // 2. allow-once -> 1 回目は実行されファイル作成
    acp.send_prompt("p2", &session_id, "Run write_file 2")?;
    let mut perm_count = 0usize;
    loop {
        let msg = acp.recv()?;
        if msg.get("method") == Some(&json!("session/request_permission")) {
            perm_count = perm_count.saturating_add(1);
            let req_id = msg.get("id").cloned().ok_or("perm id")?;
            acp.reply_permission(&req_id, "allow-once")?;
        } else if msg.get("id") == Some(&json!("p2")) {
            assert_eq!(msg.pointer("/result/stopReason"), Some(&json!("end_turn")));
            break;
        }
    }
    assert_eq!(perm_count, 1);
    assert_eq!(fs::read_to_string(&created)?, "written-by-bnt");
    fs::remove_file(&created)?;

    // 3. 同じスクリプトの 2 回目の実行 -> allow-once は記憶されず再び確認が届く
    acp.send_prompt("p3", &session_id, "Run write_file 3")?;
    let mut perm_count = 0usize;
    loop {
        let msg = acp.recv()?;
        if msg.get("method") == Some(&json!("session/request_permission")) {
            perm_count = perm_count.saturating_add(1);
            let req_id = msg.get("id").cloned().ok_or("perm id")?;
            acp.reply_permission(&req_id, "reject-once")?;
        } else if msg.get("id") == Some(&json!("p3")) {
            assert_eq!(msg.pointer("/result/stopReason"), Some(&json!("end_turn")));
            break;
        }
    }
    assert_eq!(
        perm_count, 1,
        "allow-once must not remember effects for subsequent calls"
    );
    assert!(!created.exists());

    // 4. `/bin/sh` を起動するスクリプト -> allow-once でも OS サンドボックスに拒否される
    acp.send_prompt("p4", &session_id, "Run spawn_sh")?;
    let mut perm_count = 0usize;
    loop {
        let msg = acp.recv()?;
        if msg.get("method") == Some(&json!("session/request_permission")) {
            perm_count = perm_count.saturating_add(1);
            let req_id = msg.get("id").cloned().ok_or("perm id")?;
            let cmd = msg
                .pointer("/params/toolCall/rawInput/command")
                .and_then(Value::as_str)
                .unwrap_or_default();
            assert!(cmd.contains("Process.Run"), "{cmd}");
            acp.reply_permission(&req_id, "allow-once")?;
        } else if msg.get("id") == Some(&json!("p4")) {
            assert_eq!(msg.pointer("/result/stopReason"), Some(&json!("end_turn")));
            break;
        }
    }
    assert_eq!(perm_count, 1);

    let (status, _, stderr) = acp.wait_exit(Duration::from_secs(15))?;
    assert!(status.success(), "stderr={stderr}");

    let requests = server.requests()?;
    let sh_turn = serde_json::to_string(&requests.last().ok_or("missing final turn")?.body)?;
    assert!(
        sh_turn.contains("SHELL_DENIED:") && permission_errno(&sh_turn),
        "OS sandbox must deny /bin/sh with EACCES/EPERM: {sh_turn}"
    );
    assert!(!sh_turn.contains("ESCAPED_TO_SHELL"), "{sh_turn}");
    assert!(!sh_turn.contains("SHELL_RAN:"), "{sh_turn}");
    server.shutdown()?;
    Ok(())
}

/// プラットフォーム差を吸収して、OS サンドボックス内のプロキシ経由で未許可ドメインへ接続する
/// `bash` コマンドを作る（既存の `sandbox_bash.rs` と同じ手段を使う）。
fn proxy_probe_command(domain: &str) -> String {
    #[cfg(target_os = "linux")]
    {
        format!(
            "exec 3<>/dev/tcp/127.0.0.1/3128\n\
             printf 'CONNECT {domain}:443 HTTP/1.1\\r\\n\\r\\n' >&3\n\
             head -c 12 <&3; echo"
        )
    }
    #[cfg(not(target_os = "linux"))]
    {
        format!(
            "curl -s --max-time 30 -o /dev/null -w '%{{http_code}}\\n' http://{domain}/ || true"
        )
    }
}

/// プロキシ確認要求が上がった後に、外部の合図ファイル（`release_file`）でコマンド自体を終了させ、
/// `ProxyRun::finish` の後に遅れて `allow-session` を返す競合を決定的に起こす `bash` コマンド。
fn proxy_release_on_file_command(domain: &str, release_file: &str) -> String {
    #[cfg(target_os = "linux")]
    {
        format!(
            "(exec 3<>/dev/tcp/127.0.0.1/3128; printf 'CONNECT {domain}:443 HTTP/1.1\\r\\n\\r\\n' >&3; read -r _line <&3) >/dev/null 2>&1 &\n\
             cpid=$!\n\
             while [ ! -f '{release_file}' ]; do sleep 0.05 2>/dev/null || sleep 1; done\n\
             kill \"$cpid\" 2>/dev/null || true\n\
             wait \"$cpid\" 2>/dev/null || true"
        )
    }
    #[cfg(not(target_os = "linux"))]
    {
        format!(
            "curl -s --max-time 30 -o /dev/null http://{domain}/ >/dev/null 2>&1 &\n\
             cpid=$!\n\
             while [ ! -f '{release_file}' ]; do sleep 0.05 2>/dev/null || sleep 1; done\n\
             kill \"$cpid\" 2>/dev/null || true\n\
             wait \"$cpid\" 2>/dev/null || true"
        )
    }
}

// 作成時の関門: 本物の `benitoiteh acp --mode normal`（`network = "proxy"`、`sandbox = "workspace-write"`）と
// 本物の OS サンドボックス・プロキシ中継で、未許可ドメインへの `bash` 接続が `kind: "fetch"` の
// `session/request_permission` を上げ、(1) コマンドが自然終了した後に遅れて `allow-session` を返した経路、
// (2) `session/cancel` で取り消された後に遅れて `allow-session` を返した経路のどちらでも、
// 次の `bash` コマンドで同じドメインがセッション許可されず再び確認になることを守る
// （設計書 03-02「通信の中継」、05-03「通信の中継と秘密の入力の扱い」）。
#[test]
fn acp_normal_proxy_late_session_approval_does_not_allow_next_command() -> TestResult {
    let dir = TestDir::new("proxy-race")?;
    let (turn2_entered_tx, turn2_entered_rx) = mpsc::channel();
    let (turn2_release_tx, turn2_release_rx) = mpsc::channel();

    let mut server = MockProvider::start(vec![
        // Prompt 1 Turn 1: exit-race.invalid へ接続し、release-cmd1 ファイルで終了する。
        ResponseScript::sse(chat_sse_tool_call(
            "call-proxy-exit-1",
            "bash",
            json!({"command": proxy_release_on_file_command("exit-race.invalid", "release-cmd1")}),
        )),
        // Prompt 1 Turn 2: 遅れた allow-session を送り終えるまで待ってから同じドメインへ再接続。
        ResponseScript {
            status: 200,
            headers: vec![("content-type".into(), "text/event-stream".into())],
            body: chat_sse_tool_call(
                "call-proxy-exit-2",
                "bash",
                json!({"command": proxy_probe_command("exit-race.invalid")}),
            ),
            delivery: Delivery::Controlled {
                entered: turn2_entered_tx,
                release: turn2_release_rx,
            },
        },
        ResponseScript::sse(chat_sse_text("exit race done")),
        // Prompt 2 Turn 1: cancel-race.invalid へ接続中に session/cancel する。
        ResponseScript::sse(chat_sse_tool_call(
            "call-proxy-cancel-1",
            "bash",
            json!({"command": proxy_probe_command("cancel-race.invalid")}),
        )),
        // Prompt 3 Turn 1: cancel 後に遅れた allow-session を送った後、同じドメインへ再接続。
        ResponseScript::sse(chat_sse_tool_call(
            "call-proxy-cancel-2",
            "bash",
            json!({"command": proxy_probe_command("cancel-race.invalid")}),
        )),
        ResponseScript::sse(chat_sse_text("cancel race done")),
    ])?;
    dir.write_config(
        &server.base_url(),
        "normal",
        "\n[normal]\nnetwork = \"proxy\"\nallowed_domains = [\"allowed.example\"]\n",
    )?;

    let mut acp = AcpProcess::spawn(
        &dir,
        &[
            "--mode",
            "normal",
            "--sandbox",
            "workspace-write",
            "--approval",
            "on-request",
        ],
        &[],
    )?;
    acp.initialize()?;
    let session_id = acp.new_session(&dir.project(), json!([]))?;

    // --- 経路 1: コマンドの自然終了後に遅れて allow-session を返す ---
    acp.send_prompt("p-exit-race", &session_id, "Run proxy exit race")?;
    let first_perm_id = loop {
        let msg = acp.recv()?;
        if msg.get("method") == Some(&json!("session/request_permission")) {
            assert_eq!(
                msg.pointer("/params/toolCall/kind"),
                Some(&json!("fetch")),
                "{msg}"
            );
            let cmd = msg
                .pointer("/params/toolCall/rawInput/command")
                .and_then(Value::as_str)
                .unwrap_or_default();
            assert!(cmd.contains("exit-race.invalid"), "{cmd}");
            assert!(cmd.contains("every port of exit-race.invalid"), "{cmd}");
            assert_eq!(
                option_ids(&msg),
                vec!["allow-once", "allow-session", "reject-once"]
            );
            break msg.get("id").cloned().ok_or("missing first perm id")?;
        }
    };
    // 確認に答えないままコマンド 1 を終了させ、ProxyRun::finish を完了させる。
    fs::write(dir.project().join("release-cmd1"), "go\n")?;
    loop {
        let msg = acp.recv()?;
        if msg.pointer("/params/update/sessionUpdate") == Some(&json!("tool_call_update"))
            && matches!(
                msg.pointer("/params/update/status").and_then(Value::as_str),
                Some("completed" | "failed")
            )
        {
            break;
        }
    }
    turn2_entered_rx
        .recv_timeout(RECV_TIMEOUT)
        .map_err(|e| format!("turn 2 did not start after bash 1 finished: {e}"))?;
    // コマンド 1 が終わった後に、遅れて allow-session を返す。
    acp.reply_permission(&first_perm_id, "allow-session")?;
    thread::sleep(Duration::from_millis(150));
    turn2_release_tx.send(())?;

    // コマンド 2 で同じドメインへ接続すると、セッション許可されておらず再び確認が届く。
    let mut saw_second_perm = false;
    loop {
        let msg = acp.recv()?;
        if msg.get("method") == Some(&json!("session/request_permission")) {
            saw_second_perm = true;
            assert_eq!(msg.pointer("/params/toolCall/kind"), Some(&json!("fetch")));
            let cmd = msg
                .pointer("/params/toolCall/rawInput/command")
                .and_then(Value::as_str)
                .unwrap_or_default();
            assert!(cmd.contains("exit-race.invalid"), "{cmd}");
            let req_id = msg.get("id").cloned().ok_or("missing second perm id")?;
            acp.reply_permission(&req_id, "reject-once")?;
        } else if msg.get("id") == Some(&json!("p-exit-race")) {
            assert_eq!(msg.pointer("/result/stopReason"), Some(&json!("end_turn")));
            break;
        }
    }
    assert!(
        saw_second_perm,
        "late allow-session after bash exit must not grant session permission"
    );

    // --- 経路 2: session/cancel で取り消された後に遅れて allow-session を返す ---
    acp.send_prompt("p-cancel-race-1", &session_id, "Run proxy cancel race 1")?;
    let cancel_perm_id = loop {
        let msg = acp.recv()?;
        if msg.get("method") == Some(&json!("session/request_permission")) {
            assert_eq!(msg.pointer("/params/toolCall/kind"), Some(&json!("fetch")));
            let cmd = msg
                .pointer("/params/toolCall/rawInput/command")
                .and_then(Value::as_str)
                .unwrap_or_default();
            assert!(cmd.contains("cancel-race.invalid"), "{cmd}");
            break msg.get("id").cloned().ok_or("missing cancel perm id")?;
        }
    };
    acp.send_cancel(&session_id)?;
    loop {
        let msg = acp.recv()?;
        if msg.get("id") == Some(&json!("p-cancel-race-1")) {
            assert_eq!(
                msg.pointer("/result/stopReason"),
                Some(&json!("cancelled")),
                "{msg}"
            );
            break;
        }
    }
    // 取り消し完了後に遅れて allow-session を送る。
    acp.reply_permission(&cancel_perm_id, "allow-session")?;
    thread::sleep(Duration::from_millis(100));

    // 同じセッションの次のプロンプトで同じドメインへ接続すると再び確認になる。
    acp.send_prompt("p-cancel-race-2", &session_id, "Run proxy cancel race 2")?;
    let mut saw_after_cancel_perm = false;
    loop {
        let msg = acp.recv()?;
        if msg.get("method") == Some(&json!("session/request_permission")) {
            saw_after_cancel_perm = true;
            assert_eq!(msg.pointer("/params/toolCall/kind"), Some(&json!("fetch")));
            let cmd = msg
                .pointer("/params/toolCall/rawInput/command")
                .and_then(Value::as_str)
                .unwrap_or_default();
            assert!(cmd.contains("cancel-race.invalid"), "{cmd}");
            let req_id = msg
                .get("id")
                .cloned()
                .ok_or("missing after-cancel perm id")?;
            acp.reply_permission(&req_id, "reject-once")?;
        } else if msg.get("id") == Some(&json!("p-cancel-race-2")) {
            assert_eq!(msg.pointer("/result/stopReason"), Some(&json!("end_turn")));
            break;
        }
    }
    assert!(
        saw_after_cancel_perm,
        "late allow-session after session/cancel must not grant session permission"
    );

    let (status, _, stderr) = acp.wait_exit(Duration::from_secs(15))?;
    assert!(status.success(), "stderr={stderr}");

    let requests = server.requests()?;
    let exit_turn =
        serde_json::to_string(&requests.get(2).ok_or("missing exit race result")?.body)?;
    assert!(
        exit_turn.contains("benitoiteh blocked network access to exit-race.invalid")
            && exit_turn.contains("the user declined"),
        "{exit_turn}"
    );
    let cancel_turn =
        serde_json::to_string(&requests.last().ok_or("missing cancel race result")?.body)?;
    assert!(
        cancel_turn.contains("benitoiteh blocked network access to cancel-race.invalid")
            && cancel_turn.contains("the user declined"),
        "{cancel_turn}"
    );
    server.shutdown()?;
    Ok(())
}

// 作成時の関門: 本物の `benitoiteh acp` で長いコマンド（通常モード `bash`）と
// 長いスクリプト（制限モード `benitoite_run`）を OS サンドボックス内で実行している途中に、
// `session/cancel` または `stdin` の EOF を送ったとき、子プロセスと子孫プロセスが確実に止まり、
// `stopReason: "cancelled"`（取消時）またはプロセス終了（EOF 時）となって子孫が残らないことを守る
// （設計書 03-02「子プロセスの後始末」、05-03「スレッドの構成と終了の手順」）。
#[test]
fn acp_cancel_and_eof_stop_child_processes_without_leftovers() -> TestResult {
    let run_id = SessionId::generate(SystemTime::now())?;

    // 1. 通常モード `bash`: session/cancel と stdin EOF の両方で子・孫プロセスが残らない。
    for use_eof in [false, true] {
        let label = if use_eof { "bash-eof" } else { "bash-cancel" };
        let marker = format!("bh-acp-{label}-{run_id}");
        let _cleanup = MarkedCleanup(marker.clone());
        let dir = TestDir::new(label)?;
        let pid_file = dir.project().join("descendants.pid");
        let command = format!(
            "sh -c 'sleep 30 & gpid=$!; printf \"%s %s\\n\" \"$$\" \"$gpid\" > \"$1\"; wait' _ \"$PWD/descendants.pid\" '{marker}' &\nwait"
        );

        let mut server = MockProvider::start(vec![ResponseScript::sse(chat_sse_tool_call(
            "call-long-bash",
            "bash",
            json!({"command": command}),
        ))])?;
        dir.write_config(&server.base_url(), "normal", "")?;

        let mut acp = AcpProcess::spawn(
            &dir,
            &[
                "--mode",
                "normal",
                "--sandbox",
                "workspace-write",
                "--approval",
                "on-request",
            ],
            &[],
        )?;
        acp.initialize()?;
        let session_id = acp.new_session(&dir.project(), json!([]))?;
        acp.send_prompt("p-long-bash", &session_id, "Run long bash")?;

        wait_until(Duration::from_secs(15), || {
            fs::read_to_string(&pid_file)
                .map(|s| s.split_whitespace().count() == 2)
                .unwrap_or(false)
        })?;
        // 正の対照: 取消・EOF の前に子孫プロセスが実際に生きていることを観測する。
        assert!(
            !marked_processes(&marker)?.is_empty(),
            "{label}: expected descendant marker process to be running"
        );
        #[cfg(not(target_os = "linux"))]
        let recorded_pids = {
            let pids = read_pids_file(&pid_file)?;
            assert_eq!(pids.len(), 2, "{label}: {pids:?}");
            for &pid in &pids {
                assert!(!pid_stopped(pid), "{label}: pid {pid:?} already stopped");
            }
            pids
        };

        if use_eof {
            acp.close_stdin();
            let (status, _, stderr) = acp.wait_exit(Duration::from_secs(15))?;
            assert!(status.success(), "{label}: stderr={stderr}");
        } else {
            acp.send_cancel(&session_id)?;
            loop {
                let msg = acp.recv()?;
                if msg.get("id") == Some(&json!("p-long-bash")) {
                    assert_eq!(
                        msg.pointer("/result/stopReason"),
                        Some(&json!("cancelled")),
                        "{label}: {msg}"
                    );
                    break;
                }
            }
            let (status, _, stderr) = acp.wait_exit(Duration::from_secs(15))?;
            assert!(status.success(), "{label}: stderr={stderr}");
        }

        wait_until(Duration::from_secs(5), || {
            marked_processes(&marker)
                .map(|pids| pids.is_empty())
                .unwrap_or(false)
        })?;
        #[cfg(not(target_os = "linux"))]
        for pid in recorded_pids {
            wait_until(Duration::from_secs(5), || pid_stopped(pid))?;
        }
        server.shutdown()?;
    }

    // 2. 制限モード `benitoite_run`: session/cancel と stdin EOF の両方で処理系の子プロセスが残らない。
    for use_eof in [false, true] {
        let label = if use_eof { "bnt-eof" } else { "bnt-cancel" };
        let marker = format!("bh-acp-{label}-{run_id}");
        let _cleanup = MarkedCleanup(marker.clone());
        let dir = TestDir::new(label)?;
        let script_name = format!("sleep_{marker}.bnt");
        let ready_file = dir.project().join("bnt-ready.txt");
        fs::write(
            dir.project().join(&script_name),
            "import Benitoite.Unofficial.IO.Clock\n\
             import Benitoite.Unofficial.IO.File\n\
             function main() -> Unit uses Clock.Time, File.Write\n\
              bind _ <- File.writeText(path: \"bnt-ready.txt\", text: \"ready\")\n\
              Clock.sleep(30000)\n\
             end function\n",
        )?;

        let mut server = MockProvider::start(vec![ResponseScript::sse(chat_sse_tool_call(
            "call-long-bnt",
            "benitoite_run",
            json!({"path": script_name}),
        ))])?;
        dir.write_config(&server.base_url(), "restricted", "")?;

        let mut acp = AcpProcess::spawn(&dir, &["--mode", "restricted"], &[])?;
        acp.initialize()?;
        let session_id = acp.new_session(&dir.project(), json!([]))?;
        acp.send_prompt("p-long-bnt", &session_id, "Run long script")?;

        // 確認要求を allow-once で許可してスクリプトの実行を開始させる。
        loop {
            let msg = acp.recv()?;
            if msg.get("method") == Some(&json!("session/request_permission")) {
                let req_id = msg.get("id").cloned().ok_or("perm id")?;
                acp.reply_permission(&req_id, "allow-once")?;
                break;
            }
        }
        wait_until(Duration::from_secs(15), || ready_file.is_file())?;
        assert!(
            !marked_processes(&marker)?.is_empty(),
            "{label}: expected benitoite process with marker {marker} to be running"
        );

        if use_eof {
            acp.close_stdin();
            let (status, _, stderr) = acp.wait_exit(Duration::from_secs(15))?;
            assert!(status.success(), "{label}: stderr={stderr}");
        } else {
            acp.send_cancel(&session_id)?;
            loop {
                let msg = acp.recv()?;
                if msg.get("id") == Some(&json!("p-long-bnt")) {
                    assert_eq!(
                        msg.pointer("/result/stopReason"),
                        Some(&json!("cancelled")),
                        "{label}: {msg}"
                    );
                    break;
                }
            }
            let (status, _, stderr) = acp.wait_exit(Duration::from_secs(15))?;
            assert!(status.success(), "{label}: stderr={stderr}");
        }

        wait_until(Duration::from_secs(5), || {
            marked_processes(&marker)
                .map(|pids| pids.is_empty())
                .unwrap_or(false)
        })?;
        server.shutdown()?;
    }

    Ok(())
}
