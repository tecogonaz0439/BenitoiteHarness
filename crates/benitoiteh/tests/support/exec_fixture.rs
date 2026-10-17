//! exec の入口のテストが共有する提供者と作業場所（設計書 07-01）。

pub(super) use std::error::Error;
pub(super) use std::fs;
pub(super) use std::io::{self, Write};
pub(super) use std::os::unix::fs::PermissionsExt;
pub(super) use std::path::PathBuf;
pub(super) use std::process::{Child, Command, Output, Stdio};
pub(super) use std::thread;
pub(super) use std::time::{Duration, Instant, SystemTime};

pub(super) use super::support::mock_provider::{Delivery, MockProvider, ResponseScript};
pub(super) use benitoiteh::session::SessionId;
pub(super) use serde_json::{Value, json};

pub(super) type TestResult = Result<(), Box<dyn Error>>;
pub(super) const KEY: &str = "synthetic-exec-key-never-log";
pub(super) const FINAL: &str =
    "Done.\n\t\0\x08\x1b[31m\r\x7f\u{80}\u{9f}\u{202a}\u{202e}\u{2066}\u{2069}";
pub(super) const DISPLAY: &str =
    "Done.\n\t\\x00\\x08\\x1b[31m\\x0d\\x7f\\x80\\x9f\\u{202a}\\u{202e}\\u{2066}\\u{2069}\n";

#[derive(Clone, Copy, Debug)]
pub(super) enum Protocol {
    Chat,
    Anthropic,
    Responses,
}

impl Protocol {
    pub(super) fn kind(self) -> &'static str {
        match self {
            Self::Chat => "openai-chat",
            Self::Responses => "openai-responses",
            Self::Anthropic => "anthropic-messages",
        }
    }

    // 手書きの SSE の形を組み立てる。本番のアダプタを生成に使わない。
    pub(super) fn response(
        self,
        call: Option<(&str, &str, Value)>,
        text: &str,
        limit: bool,
    ) -> Vec<u8> {
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
            Self::Responses => {
                let mut data = vec![
                    json!({"type":"response.output_item.added","output_index":0,"item":{"type":"message","id":"msg_original","role":"assistant","content":[],"status":"in_progress"}}),
                    json!({"type":"response.output_text.delta","output_index":0,"content_index":0,"item_id":"msg_original","delta":text}),
                    json!({"type":"response.output_item.done","output_index":0,"item":{"type":"message","id":"msg_original","role":"assistant","content":[{"type":"output_text","text":text,"annotations":[]}],"status":"completed"}}),
                ];
                if let Some((id, name, arguments)) = call {
                    data.extend([
                        json!({"type":"response.output_item.added","output_index":1,"item":{"type":"reasoning","id":"rs_original","summary":[]}}),
                        json!({"type":"response.reasoning_summary_text.delta","output_index":1,"summary_index":0,"item_id":"rs_original","delta":"Plan"}),
                        json!({"type":"response.output_item.done","output_index":1,"item":{"type":"reasoning","id":"rs_original","summary":[{"type":"summary_text","text":"Plan"}],"encrypted_content":"encrypted+/="}}),
                        json!({"type":"response.output_item.added","output_index":2,"item":{"type":"function_call","id":"fc_original","call_id":id,"name":name,"arguments":"","status":"in_progress"}}),
                        json!({"type":"response.function_call_arguments.delta","output_index":2,"item_id":"fc_original","delta":arguments.to_string()}),
                        json!({"type":"response.output_item.done","output_index":2,"item":{"type":"function_call","id":"fc_original","call_id":id,"name":name,"arguments":arguments.to_string(),"status":"completed"}}),
                    ]);
                }
                data.push(json!({"type":if limit {"response.incomplete"} else {"response.completed"},"response":{"status":if limit {"incomplete"} else {"completed"},"incomplete_details":if limit {json!({"reason":"max_output_tokens"})} else {Value::Null},"usage":{"input_tokens":10,"output_tokens":2,"input_tokens_details":{"cached_tokens":3}}}}));
                data.into_iter()
                    .enumerate()
                    .map(|(sequence, mut data)| {
                        if let Some(fields) = data.as_object_mut() {
                            fields.insert("sequence_number".into(), json!(sequence));
                        }
                        format!(
                            "event: {}\ndata: {data}\n\n",
                            data.get("type").and_then(Value::as_str).unwrap_or_default()
                        )
                    })
                    .collect::<String>()
                    .into_bytes()
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

pub(super) struct Fixture {
    pub(super) root: PathBuf,
    restricted: bool,
}

impl Fixture {
    pub(super) fn new(protocol: Protocol, server: &MockProvider) -> Result<Self, Box<dyn Error>> {
        let id = SessionId::generate(SystemTime::now())?;
        let root = std::env::temp_dir().join(format!("benitoiteh-exec-{id}"));
        fs::create_dir(&root)?;
        let fixture = Self {
            restricted: false,
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
        let config = fixture.root.join("config/benitoiteh/config.toml");
        let previous = fs::read_to_string(&config)?;
        fs::write(
            &config,
            format!(
                "default_mode = 'normal'\n{previous}\n[benitoite]\npath = {}\n",
                serde_json::to_string(&fixture.root.join("missing-benitoite"))?
            ),
        )?;
        fs::write(fixture.root.join("project/a.bnt"), "before \x1b[2J\n")?;
        Ok(fixture)
    }

    pub(super) fn restricted(
        protocol: Protocol,
        server: &MockProvider,
    ) -> Result<Self, Box<dyn Error>> {
        let mut fixture = Self::new(protocol, server)?;
        fixture.restricted = true;
        let config = fixture.root.join("config/benitoiteh/config.toml");
        let contents = fs::read_to_string(&config)?.replace("default_mode = 'normal'\n", "");
        let path = serde_json::to_string(&super::support::benitoite()?)?;
        let contents = contents
            .lines()
            .map(|line| {
                if line.starts_with("path = ") {
                    format!("path = {path}")
                } else {
                    line.into()
                }
            })
            .collect::<Vec<_>>()
            .join("\n");
        fs::write(config, contents)?;
        Ok(fixture)
    }

    pub(super) fn command(&self, request: &str) -> Command {
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
        if !self.restricted {
            command.arg("--no-sandbox");
        }
        command
    }

    pub(super) fn records(&self) -> Result<(PathBuf, Vec<Value>), Box<dyn Error>> {
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

pub(super) struct Running(pub(super) Option<Child>);

impl Running {
    pub(super) fn start(command: &mut Command) -> io::Result<Self> {
        Ok(Self(Some(command.spawn()?)))
    }

    pub(super) fn finish(mut self) -> Result<Output, Box<dyn Error>> {
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

pub(super) fn lines(bytes: &[u8]) -> Result<Vec<Value>, Box<dyn Error>> {
    Ok(std::str::from_utf8(bytes)?
        .lines()
        .map(serde_json::from_str)
        .collect::<Result<_, _>>()?)
}

pub(super) fn types(values: &[Value]) -> Vec<&str> {
    values
        .iter()
        .filter_map(|value| value.get("type").and_then(Value::as_str))
        .collect()
}

// 起動の途中で失敗しても、先に無視した設定の診断を失わないことを実行ファイルの入口で守る。
pub(super) fn no_secret(output: &Output, records: &[Value]) {
    for bytes in [&output.stdout, &output.stderr] {
        assert!(!String::from_utf8_lossy(bytes).contains(KEY));
    }
    assert!(!json!(records).to_string().contains(KEY));
}
