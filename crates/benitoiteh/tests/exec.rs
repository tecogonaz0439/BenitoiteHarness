//! 実行ファイルの入口で出力・記録・道具の変更を確かめる（設計書 07-01「持ち主の境界」）。

mod support;

use std::error::Error;
use std::fs;
use std::io::{self, Write};
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::{Child, Command, Output, Stdio};
use std::thread;
use std::time::{Duration, Instant, SystemTime};

use benitoiteh::session::SessionId;
use serde_json::{Value, json};
use support::mock_provider::{Delivery, MockProvider, ResponseScript};

type TestResult = Result<(), Box<dyn Error>>;
const KEY: &str = "synthetic-exec-key-never-log";
const FINAL: &str = "Done.\n\t\0\x08\x1b[31m\r\x7f\u{80}\u{9f}\u{202a}\u{202e}\u{2066}\u{2069}";
const DISPLAY: &str =
    "Done.\n\t\\x00\\x08\\x1b[31m\\x0d\\x7f\\x80\\x9f\\u{202a}\\u{202e}\\u{2066}\\u{2069}\n";

#[derive(Clone, Copy, Debug)]
enum Protocol {
    Chat,
    Anthropic,
}

impl Protocol {
    fn kind(self) -> &'static str {
        match self {
            Self::Chat => "openai-chat",
            Self::Anthropic => "anthropic-messages",
        }
    }

    // 手書きの SSE の形を組み立てる。本番のアダプタを生成に使わない。
    fn response(self, call: Option<(&str, &str, Value)>, text: &str, limit: bool) -> Vec<u8> {
        match self {
            Self::Chat => {
                let has_call = call.is_some();
                let delta = if let Some((id, name, arguments)) = call {
                    json!({"content":text,"reasoning_content":"Plan","tool_calls":[{"index":0,"id":id,"type":"function","function":{"name":name,"arguments":arguments.to_string()}}]})
                } else {
                    json!({"content":text})
                };
                let reason = if limit {
                    "length"
                } else if has_call {
                    "tool_calls"
                } else {
                    "stop"
                };
                format!("data: {}\n\ndata: {}\n\ndata: {}\n\ndata: [DONE]\n\n",
                    json!({"choices":[{"delta":delta,"finish_reason":null}]}),
                    json!({"choices":[{"delta":{},"finish_reason":reason}]}),
                    json!({"choices":[],"usage":{"prompt_tokens":10,"completion_tokens":2,"prompt_tokens_details":{"cached_tokens":3}}})
                ).into_bytes()
            }
            Self::Anthropic => {
                let has_call = call.is_some();
                let mut data = vec![
                    json!({"type":"message_start","message":{"id":"msg_original","usage":{"input_tokens":10,"output_tokens":0}}}),
                ];
                let mut blocks = vec![json!({"type":"text","text":text})];
                if let Some((id, name, arguments)) = call {
                    blocks
                        .push(json!({"type":"thinking","thinking":"Plan","signature":"opaque+/="}));
                    blocks.push(json!({"type":"tool_use","id":id,"name":name,"input":arguments}));
                    blocks.push(json!({"type":"redacted_thinking","data":"encrypted+/="}));
                }
                for (index, block) in blocks.into_iter().enumerate() {
                    data.push(
                        json!({"type":"content_block_start","index":index,"content_block":block}),
                    );
                    data.push(json!({"type":"content_block_stop","index":index}));
                }
                let reason = if limit {
                    "max_tokens"
                } else if has_call {
                    "tool_use"
                } else {
                    "end_turn"
                };
                data.push(json!({"type":"message_delta","delta":{"stop_reason":reason},"usage":{"output_tokens":2}}));
                data.push(json!({"type":"message_stop"}));
                data.into_iter()
                    .map(|data| {
                        format!(
                            "event: {}\ndata: {data}\n\n",
                            data.get("type").and_then(Value::as_str).unwrap_or_default()
                        )
                    })
                    .collect::<String>()
                    .into_bytes()
            }
        }
    }
}

struct Fixture {
    root: PathBuf,
}

impl Fixture {
    fn new(protocol: Protocol, server: &MockProvider) -> Result<Self, Box<dyn Error>> {
        let id = SessionId::generate(SystemTime::now())?;
        let root = std::env::temp_dir().join(format!("benitoiteh-exec-{id}"));
        fs::create_dir(&root)?;
        let fixture = Self {
            root: fs::canonicalize(root)?,
        };
        for path in ["project", "home", "config/benitoiteh", "tmp"] {
            fs::create_dir_all(fixture.root.join(path))?;
        }
        fs::write(
            fixture.root.join("config/benitoiteh/config.toml"),
            format!(
                "default_provider = \"mock\"\n[providers.mock]\nkind = \"{}\"\nbase_url = \"{}\"\nmodel = \"mock-model\"\napi_key = {{ env = \"BENITOITEH_EXEC_TEST_KEY\" }}\ncapabilities = {{ reasoning_replay = \"same-destination\" }}\n",
                protocol.kind(),
                server.base_url()
            ),
        )?;
        fs::write(fixture.root.join("project/a.bnt"), "before \x1b[2J\n")?;
        Ok(fixture)
    }

    fn command(&self, request: &str) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_benitoiteh"));
        command
            .args(["exec", request])
            .current_dir(self.root.join("project"))
            .env("HOME", self.root.join("home"))
            .env("XDG_CONFIG_HOME", self.root.join("config"))
            .env("XDG_STATE_HOME", self.root.join("state"))
            .env("TMPDIR", self.root.join("tmp"))
            .env("BENITOITEH_EXEC_TEST_KEY", KEY)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        command
    }

    fn records(&self) -> Result<(PathBuf, Vec<Value>), Box<dyn Error>> {
        let directories = fs::read_dir(self.root.join("state/benitoiteh/sessions"))?
            .collect::<Result<Vec<_>, _>>()?;
        assert_eq!(directories.len(), 1);
        let directory = directories
            .first()
            .ok_or_else(|| io::Error::from(io::ErrorKind::NotFound))?
            .path();
        let records = lines(&fs::read(directory.join("session.jsonl"))?)?;
        Ok((directory, records))
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _removed = fs::remove_dir_all(&self.root);
    }
}

struct Running(Option<Child>);

impl Running {
    fn start(command: &mut Command) -> io::Result<Self> {
        Ok(Self(Some(command.spawn()?)))
    }

    fn finish(mut self) -> Result<Output, Box<dyn Error>> {
        let started = Instant::now();
        while self
            .0
            .as_mut()
            .ok_or_else(|| io::Error::from(io::ErrorKind::NotFound))?
            .try_wait()?
            .is_none()
        {
            if started.elapsed() > Duration::from_secs(12) {
                return Err(io::Error::from(io::ErrorKind::TimedOut).into());
            }
            thread::sleep(Duration::from_millis(5));
        }
        Ok(self
            .0
            .take()
            .ok_or_else(|| io::Error::from(io::ErrorKind::NotFound))?
            .wait_with_output()?)
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        if let Some(child) = &mut self.0 {
            let _killed = child.kill();
            let _waited = child.wait();
        }
    }
}

fn lines(bytes: &[u8]) -> Result<Vec<Value>, Box<dyn Error>> {
    Ok(std::str::from_utf8(bytes)?
        .lines()
        .map(serde_json::from_str)
        .collect::<Result<_, _>>()?)
}

fn types(values: &[Value]) -> Vec<&str> {
    values
        .iter()
        .filter_map(|value| value.get("type").and_then(Value::as_str))
        .collect()
}

fn no_secret(output: &Output, records: &[Value]) {
    for bytes in [&output.stdout, &output.stderr] {
        assert!(!String::from_utf8_lossy(bytes).contains(KEY));
    }
    assert!(!json!(records).to_string().contains(KEY));
}

#[test]
fn exec_completes_tools_and_preserves_output_records_and_original_items() -> TestResult {
    for protocol in [Protocol::Chat, Protocol::Anthropic] {
        for json_output in [false, true] {
            let mut final_response = ResponseScript::sse(protocol.response(None, FINAL, false));
            final_response.delivery = Delivery::Chunks {
                size: 37,
                delay: Duration::from_micros(20),
            };
            let mut server = MockProvider::start(vec![
                ResponseScript::sse(protocol.response(
                    Some(("read/original", "read", json!({"path":"a.bnt"}))),
                    "Working.",
                    false,
                )),
                ResponseScript::sse(protocol.response(
                    Some((
                        "write:original",
                        "write",
                        json!({"path":"b.bnt","content":"created\n"}),
                    )),
                    "Working.",
                    false,
                )),
                ResponseScript::sse(protocol.response(
                    Some((
                        "edit+original",
                        "edit",
                        json!({"path":"a.bnt","old_string":"before","new_string":"after"}),
                    )),
                    "Working.",
                    false,
                )),
                final_response,
            ])?;
            let f = Fixture::new(protocol, &server)?;
            fs::create_dir(f.root.join("project/.benitoiteh"))?;
            fs::write(
                f.root.join("project/.benitoiteh/config.toml"),
                "model = \"project-model\"\n",
            )?;
            let request = if json_output {
                "-"
            } else {
                "Read, write, and edit."
            };
            let mut command = f.command(request);
            command
                .args(["--mode", "normal", "--max-turns", "4"])
                .arg("--output-last-message")
                .arg(f.root.join("last-message.txt"));
            if json_output {
                command.arg("--json").stdin(Stdio::piped());
            }
            let mut child = Running::start(&mut command)?;
            if json_output {
                let mut stdin = child
                    .0
                    .as_mut()
                    .and_then(|child| child.stdin.take())
                    .ok_or_else(|| io::Error::from(io::ErrorKind::NotFound))?;
                stdin.write_all(b"Read, write, and edit.")?;
            }
            let output = child.finish()?;
            assert_eq!(
                output.status.code(),
                Some(0),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            assert_eq!(
                fs::read_to_string(f.root.join("project/a.bnt"))?,
                "after \x1b[2J\n"
            );
            assert_eq!(
                fs::read_to_string(f.root.join("project/b.bnt"))?,
                "created\n"
            );
            assert_eq!(fs::read_to_string(f.root.join("last-message.txt"))?, FINAL);
            let stderr = std::str::from_utf8(&output.stderr)?;
            assert!(stderr.contains("Loaded project configuration:"));
            assert!(stderr.contains("Provider: mock; model: project-model"));
            if !json_output {
                assert_eq!(std::str::from_utf8(&output.stdout)?, DISPLAY);
                assert!(
                    stderr.contains("read: ok:      1 before \\x1b[2J"),
                    "{stderr}"
                );
                assert!(!stderr.contains('\x1b'));
            }
            let (directory, records) = f.records()?;
            assert_eq!(
                types(&records),
                [
                    "header",
                    "session_start",
                    "user",
                    "turn_start",
                    "assistant",
                    "tool_call",
                    "tool_result",
                    "turn_end",
                    "turn_start",
                    "assistant",
                    "tool_call",
                    "tool_start",
                    "tool_result",
                    "turn_end",
                    "turn_start",
                    "assistant",
                    "tool_call",
                    "tool_start",
                    "tool_result",
                    "turn_end",
                    "turn_start",
                    "assistant",
                    "turn_end",
                    "session_end",
                    "summary",
                ]
            );
            let summary = records
                .last()
                .ok_or_else(|| io::Error::from(io::ErrorKind::NotFound))?;
            assert_eq!(summary.get("turns"), Some(&json!(4)));
            assert_eq!(
                summary.get("tool_calls"),
                Some(&json!({"read":1,"write":1,"edit":1}))
            );
            assert_eq!(summary.pointer("/tokens/input"), Some(&json!(40)));
            assert_eq!(summary.pointer("/tokens/output"), Some(&json!(8)));
            assert_eq!(summary.get("reason"), Some(&json!("The run completed.")));
            if json_output {
                let events = lines(&output.stdout)?;
                assert_eq!(
                    types(&events),
                    [
                        "session_start",
                        "turn_start",
                        "text_delta",
                        "reasoning_delta",
                        "tool_call",
                        "tool_result",
                        "turn_end",
                        "turn_start",
                        "text_delta",
                        "reasoning_delta",
                        "tool_call",
                        "tool_result",
                        "turn_end",
                        "turn_start",
                        "text_delta",
                        "reasoning_delta",
                        "tool_call",
                        "tool_result",
                        "turn_end",
                        "turn_start",
                        "text_delta",
                        "turn_end",
                        "session_end",
                        "summary",
                    ]
                );
                assert_eq!(events.last(), Some(summary));
                assert!(events.iter().any(|e| e.get("text") == Some(&json!(FINAL))));
            }
            for (path, mode) in [
                (&directory, 0o700),
                (&directory.join("session.jsonl"), 0o600),
            ] {
                assert_eq!(fs::metadata(path)?.permissions().mode() & 0o7777, mode);
            }
            assert_eq!(fs::read_dir(f.root.join("tmp"))?.count(), 0);
            no_secret(&output, &records);
            let requests = server.requests()?;
            assert_eq!(requests.len(), 4);
            let first = requests
                .first()
                .ok_or_else(|| io::Error::from(io::ErrorKind::NotFound))?;
            assert_eq!(first.method, "POST");
            assert_eq!(
                first.path,
                match protocol {
                    Protocol::Chat => "/chat/completions",
                    Protocol::Anthropic => "/v1/messages",
                }
            );
            assert_eq!(
                first
                    .headers
                    .get(match protocol {
                        Protocol::Chat => "authorization",
                        Protocol::Anthropic => "x-api-key",
                    })
                    .map(String::as_str),
                Some(match protocol {
                    Protocol::Chat => "Bearer synthetic-exec-key-never-log",
                    Protocol::Anthropic => KEY,
                })
            );
            let expected = match protocol {
                Protocol::Chat => {
                    json!({"role":"assistant","content":"Working.","reasoning_content":"Plan","tool_calls":[{"id":"read/original","type":"function","function":{"name":"read","arguments":"{\"path\":\"a.bnt\"}"}}]})
                }
                Protocol::Anthropic => json!([
                    {"type":"text","text":"Working."},
                    {"type":"thinking","thinking":"Plan","signature":"opaque+/="},
                    {"type":"tool_use","id":"read/original","name":"read","input":{"path":"a.bnt"}},
                    {"type":"redacted_thinking","data":"encrypted+/="},
                ]),
            };
            for request in requests.iter().skip(1) {
                let pointer = match protocol {
                    Protocol::Chat => "/messages/2",
                    Protocol::Anthropic => "/messages/1/content",
                };
                assert_eq!(request.body.pointer(pointer), Some(&expected));
                assert_eq!(request.body.get("model"), Some(&json!("project-model")));
            }
            server.shutdown()?;
        }
    }
    Ok(())
}

#[test]
fn unfinished_and_output_limited_turns_do_not_execute_tools() -> TestResult {
    for protocol in [Protocol::Chat, Protocol::Anthropic] {
        for disconnect in [false, true] {
            let body = protocol.response(
                Some((
                    "untrusted",
                    "write",
                    json!({"path":"a.bnt","content":"changed"}),
                )),
                "Working.",
                !disconnect,
            );
            let mut script = ResponseScript::sse(body);
            if disconnect {
                let marker = match protocol {
                    Protocol::Chat => "data: [DONE]",
                    Protocol::Anthropic => "event: message_stop",
                };
                let after = std::str::from_utf8(&script.body)?
                    .find(marker)
                    .ok_or_else(|| io::Error::from(io::ErrorKind::InvalidData))?;
                script.delivery = Delivery::Disconnect { after };
            }
            let scripts = if disconnect {
                vec![
                    script,
                    ResponseScript::json(401, &json!({"error":{"message":KEY}}))?,
                ]
            } else {
                vec![script]
            };
            let mut server = MockProvider::start(scripts)?;
            let f = Fixture::new(protocol, &server)?;
            let output = Running::start(f.command("Do it").args(["--max-turns", "1", "--json"]))?
                .finish()?;
            assert_eq!(
                output.status.code(),
                Some(1),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            assert_eq!(
                fs::read_to_string(f.root.join("project/a.bnt"))?,
                "before \x1b[2J\n"
            );
            let (_, records) = f.records()?;
            assert!(!types(&records).contains(&"tool_start"));
            assert_eq!(
                records.iter().rev().nth(1).and_then(|v| v.get("outcome")),
                Some(&json!("failed"))
            );
            assert_eq!(
                lines(&output.stdout)?.last().and_then(|v| v.get("type")),
                Some(&json!("summary"))
            );
            if disconnect {
                let requests = server.requests()?;
                assert_eq!(requests.len(), 2);
                assert_eq!(
                    requests.first().map(|r| &r.body),
                    requests.get(1).map(|r| &r.body)
                );
            }
            no_secret(&output, &records);
            server.shutdown()?;
        }
    }
    Ok(())
}

#[test]
fn startup_and_authentication_errors_have_distinct_exit_statuses() -> TestResult {
    for case in ["authentication", "option", "config", "home"] {
        let mut server = MockProvider::start(vec![ResponseScript::json(
            401,
            &json!({"error":{"message":KEY}}),
        )?])?;
        let f = Fixture::new(Protocol::Chat, &server)?;
        let mut command = f.command("Do it");
        match case {
            "option" => {
                command.arg("--unknown");
            }
            "config" => {
                fs::write(
                    f.root.join("config/benitoiteh/config.toml"),
                    format!("unknown_setting = \"{KEY}\"\n"),
                )?;
            }
            "home" => {
                command.arg("--project").arg(f.root.join("home"));
            }
            _ => {}
        }
        let output = Running::start(&mut command)?.finish()?;
        assert_eq!(
            output.status.code(),
            Some(if case == "authentication" { 1 } else { 2 }),
            "{case}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(output.stdout.is_empty());
        let stderr = std::str::from_utf8(&output.stderr)?;
        match case {
            "authentication" => {
                let (_, records) = f.records()?;
                no_secret(&output, &records);
                assert_eq!(server.requests()?.len(), 1);
            }
            "config" => {
                assert!(stderr.contains("unknown_setting"));
                assert_eq!(stderr.lines().count(), 1);
            }
            "option" => {
                assert!(stderr.contains("invalid arguments"));
                assert_eq!(stderr.lines().count(), 1);
            }
            "home" => {
                assert!(stderr.contains("home"));
            }
            _ => {}
        }
        if case != "authentication" {
            assert!(server.requests()?.is_empty());
            no_secret(&output, &[]);
        }
        server.shutdown()?;
    }
    Ok(())
}

#[test]
fn signals_cancel_the_loop_and_leave_summary_after_session_end() -> TestResult {
    for signal in [rustix::process::Signal::INT, rustix::process::Signal::TERM] {
        let mut script = ResponseScript::sse(Vec::new());
        script.delivery = Delivery::HeadersOnly;
        let mut server = MockProvider::start(vec![script])?;
        let f = Fixture::new(Protocol::Chat, &server)?;
        let child = Running::start(f.command("Wait").arg("--json"))?;
        let started = Instant::now();
        while server.requests()?.is_empty() {
            if started.elapsed() > Duration::from_secs(5) {
                return Err(io::Error::from(io::ErrorKind::TimedOut).into());
            }
            thread::sleep(Duration::from_millis(5));
        }
        let raw = i32::try_from(
            child
                .0
                .as_ref()
                .ok_or_else(|| io::Error::from(io::ErrorKind::NotFound))?
                .id(),
        )?;
        let pid = rustix::process::Pid::from_raw(raw)
            .ok_or_else(|| io::Error::from(io::ErrorKind::InvalidInput))?;
        rustix::process::kill_process(pid, signal)?;
        let output = child.finish()?;
        assert_eq!(output.status.code(), Some(130));
        let (_, records) = f.records()?;
        assert_eq!(
            records.last().and_then(|v| v.get("type")),
            Some(&json!("summary"))
        );
        let end = records
            .iter()
            .rev()
            .nth(1)
            .ok_or_else(|| io::Error::from(io::ErrorKind::NotFound))?;
        assert_eq!(end.get("type"), Some(&json!("session_end")));
        assert_eq!(end.get("outcome"), Some(&json!("cancelled")));
        assert_eq!(lines(&output.stdout)?.last(), records.last());
        assert_eq!(fs::read_dir(f.root.join("tmp"))?.count(), 0);
        no_secret(&output, &records);
        server.shutdown()?;
    }
    Ok(())
}

#[test]
fn closed_event_output_still_finishes_the_session_record() -> TestResult {
    let mut script = ResponseScript::sse(Vec::new());
    script.delivery = Delivery::HeadersOnly;
    let mut server = MockProvider::start(vec![script])?;
    let f = Fixture::new(Protocol::Chat, &server)?;
    let mut child = Running::start(f.command("Wait").arg("--json"))?;
    drop(child.0.as_mut().and_then(|child| child.stdout.take()));
    let output = child.finish()?;
    assert_eq!(output.status.code(), Some(1));
    let (_, records) = f.records()?;
    assert_eq!(
        records.last().and_then(|v| v.get("type")),
        Some(&json!("summary"))
    );
    assert_eq!(
        records.iter().rev().nth(1).and_then(|v| v.get("type")),
        Some(&json!("session_end"))
    );
    assert_eq!(fs::read_dir(f.root.join("tmp"))?.count(), 0);
    no_secret(&output, &records);
    server.shutdown()?;
    Ok(())
}

#[test]
fn invalid_project_configuration_is_reported_and_noninteractive_confirmation_is_denied()
-> TestResult {
    let mut server = MockProvider::start(vec![
        ResponseScript::sse(Protocol::Chat.response(
            Some((
                "outside",
                "write",
                json!({"path":".benitoiteh/blocked.bnt","content":"changed"}),
            )),
            "Working.",
            false,
        )),
        ResponseScript::sse(Protocol::Chat.response(None, "Done.", false)),
    ])?;
    let f = Fixture::new(Protocol::Chat, &server)?;
    fs::create_dir(f.root.join("project/.benitoiteh"))?;
    fs::write(
        f.root.join("project/.benitoiteh/config.toml"),
        "model = \"discard-me\"\nprovider = \"attacker\"\n",
    )?;
    let output = Running::start(f.command("Do it").args(["--mode", "normal"]))?.finish()?;
    assert_eq!(output.status.code(), Some(0));
    assert!(!f.root.join("project/.benitoiteh/blocked.bnt").exists());
    let stderr = std::str::from_utf8(&output.stderr)?;
    assert!(stderr.contains("Ignoring project configuration:"));
    assert!(stderr.contains("Provider: mock; model: mock-model"));
    assert!(stderr.contains("Approval denied:"), "{stderr}");
    let (_, records) = f.records()?;
    assert_eq!(
        records.last().and_then(|v| v.pointer("/approvals/denied")),
        Some(&json!(1))
    );
    no_secret(&output, &records);
    server.shutdown()?;
    Ok(())
}
