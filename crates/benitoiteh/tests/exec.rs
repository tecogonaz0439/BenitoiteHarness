//! 実行ファイルの入口で出力・記録・道具の変更を確かめる（設計書 07-01「持ち主の境界」）。

#[path = "exec/resume.rs"]
mod resume;
#[path = "exec/skills.rs"]
mod skills;
mod support;

#[allow(dead_code, unused_imports)]
#[path = "support/exec_fixture.rs"]
mod fixture;
use fixture::*;

// 関門: 設定の値が起動の準備から read とプロンプトとヘッダへ届く契約。
// 道具単体では見つけられない接続漏れを、普段の設定を使わない exec で捕まえる。
#[test]
fn evaluation_switches_reach_read_and_recorded_hashes_from_user_config() -> TestResult {
    let mut hashes = Vec::new();
    for enabled in [true, false] {
        let protocol = Protocol::Chat;
        let mut server = MockProvider::start(vec![
            ResponseScript::sse(protocol.response(
                Some(("read", "read", json!({"path":"a.bnt","limit":2}))),
                "",
                false,
            )),
            ResponseScript::sse(protocol.response(None, "done", false)),
        ])?;
        let f = Fixture::new(protocol, &server)?;
        fs::write(f.root.join("project/a.bnt"), "one\ntwo\nthree\n")?;
        if !enabled {
            let path = f.root.join("config/benitoiteh/config.toml");
            let contents = fs::read_to_string(&path)?;
            fs::write(
                path,
                format!(
                    "{contents}\n[restricted]\nafter_write_check=false\n[read]\nline_numbers=false\n"
                ),
            )?;
        }
        let output = f.command("read the file").arg("--json").output()?;
        assert_eq!(
            output.status.code(),
            Some(0),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let (_, records) = f.records()?;
        let header = records.first().ok_or("header")?;
        assert_eq!(header.get("format"), Some(&json!(2)));
        for key in ["after_write_check", "read_line_numbers"] {
            assert_eq!(header.get(key), (!enabled).then_some(&Value::Bool(false)));
        }
        hashes.push((
            header
                .get("system_prompt_sha256")
                .ok_or("prompt hash")?
                .clone(),
            header.get("tools_sha256").ok_or("tools hash")?.clone(),
        ));
        let result = records
            .iter()
            .find(|record| record.get("type").and_then(Value::as_str) == Some("tool_result"))
            .ok_or("tool result")?;
        assert_eq!(
            result.pointer("/result/text").and_then(Value::as_str),
            Some(if enabled {
                "     1\tone\n     2\ttwo\n[Showing lines 1-2 of 3. Use offset=3 to continue.]"
            } else {
                "one\ntwo\n[Showing lines 1-2 of 3. Use offset=3 to continue.]"
            })
        );
        let requests = server.requests()?;
        let request = &requests.first().ok_or("request")?.body;
        let read = request
            .get("tools")
            .and_then(Value::as_array)
            .and_then(|tools| {
                tools.iter().find(|tool| {
                    tool.pointer("/function/name").and_then(Value::as_str) == Some("read")
                })
            })
            .ok_or("read definition")?;
        assert_eq!(
            read.pointer("/function/description")
                .and_then(Value::as_str),
            Some(if enabled {
                "Read a text file with line numbers."
            } else {
                "Read a text file."
            })
        );
        server.shutdown()?;
    }
    let (numbered, plain) = (
        hashes.first().ok_or("numbered hashes")?,
        hashes.get(1).ok_or("plain hashes")?,
    );
    assert_ne!(numbered.0, plain.0);
    assert_ne!(numbered.1, plain.1);
    Ok(())
}

#[test]
fn startup_failure_preserves_earlier_project_diagnostics() -> TestResult {
    let mut server = MockProvider::start(Vec::new())?;
    let fixture = Fixture::new(Protocol::Chat, &server)?;
    fs::create_dir_all(fixture.root.join("project/.benitoiteh"))?;
    fs::write(
        fixture.root.join("project/.benitoiteh/config.toml"),
        "[invalid",
    )?;
    let output = fixture
        .command("request")
        .args(["--provider", "unknown"])
        .output()?;
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8(output.stderr)?;
    let ignored = stderr
        .find("Ignoring project configuration:")
        .ok_or("missing earlier diagnostic")?;
    let failure = stderr
        .find("benitoiteh:")
        .ok_or("missing startup failure")?;
    assert!(ignored < failure, "{stderr}");
    assert!(output.stdout.is_empty());
    assert!(server.requests()?.is_empty());
    // 診断の書き先が書けなくても、使い方の誤りの終了状態を上書きしない。
    let status = fixture
        .command("request")
        .args(["--provider", "unknown"])
        .stderr(Stdio::from(fs::File::open("/dev/null")?))
        .status()?;
    assert_eq!(status.code(), Some(2));
    server.shutdown()?;
    Ok(())
}

#[test]
fn exec_completes_tools_and_preserves_output_records_and_original_items() -> TestResult {
    for protocol in [Protocol::Chat, Protocol::Anthropic, Protocol::Responses] {
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
                .args([
                    "--mode",
                    "normal",
                    "--sandbox",
                    "danger-full-access",
                    "--max-turns",
                    "4",
                ])
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
                    "checkpoint",
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
                    "checkpoint",
                    "write_target",
                    "tool_result",
                    "turn_end",
                    "turn_start",
                    "assistant",
                    "tool_call",
                    "tool_start",
                    "checkpoint",
                    "write_target",
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
                        "checkpoint",
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
                        "checkpoint",
                        "tool_result",
                        "turn_end",
                        "turn_start",
                        "text_delta",
                        "reasoning_delta",
                        "tool_call",
                        "checkpoint",
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
                assert!(
                    events
                        .iter()
                        .filter(|event| event.get("type") == Some(&json!("tool_result")))
                        .all(|event| event.get("display").is_none())
                );
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
            // 設定から要求までの接続漏れは、アダプタ単体のテストでは捕まらない（設計書 02-01）。
            match protocol {
                Protocol::Anthropic => assert_eq!(
                    first.body.pointer("/output_config/effort"),
                    Some(&json!("low"))
                ),
                Protocol::Responses => assert_eq!(
                    first.body.pointer("/reasoning/effort"),
                    Some(&json!("high"))
                ),
                Protocol::Chat => {}
            }
            assert_eq!(
                first.path,
                match protocol {
                    Protocol::Chat => "/chat/completions",
                    Protocol::Responses => "/responses",
                    Protocol::Anthropic => "/v1/messages",
                }
            );
            assert_eq!(
                first
                    .headers
                    .get(match protocol {
                        Protocol::Chat | Protocol::Responses => "authorization",
                        Protocol::Anthropic => "x-api-key",
                    })
                    .map(String::as_str),
                Some(match protocol {
                    Protocol::Chat | Protocol::Responses => "Bearer synthetic-exec-key-never-log",
                    Protocol::Anthropic => KEY,
                })
            );
            let expected = match protocol {
                Protocol::Chat => {
                    json!({"role":"assistant","content":"Working.","reasoning_content":"Plan","tool_calls":[{"id":"read/original","type":"function","function":{"name":"read","arguments":"{\"path\":\"a.bnt\"}"}}]})
                }
                Protocol::Responses => {
                    json!({"type":"message","id":"msg_original","role":"assistant","content":[{"type":"output_text","text":"Working.","annotations":[]}],"status":"completed"})
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
                    Protocol::Responses => "/input/1",
                    Protocol::Anthropic => "/messages/1/content",
                };
                assert_eq!(request.body.pointer(pointer), Some(&expected));
                if matches!(protocol, Protocol::Responses) {
                    assert_eq!(
                        request.body.get("prompt_cache_key"),
                        records.first().and_then(|r| r.get("session_id"))
                    );
                    assert_eq!(
                        request.body.pointer("/input/3/call_id"),
                        Some(&json!("read/original"))
                    );
                    assert_eq!(
                        request.body.pointer("/input/4/type"),
                        Some(&json!("function_call_output"))
                    );
                    assert_eq!(
                        request.body.pointer("/input/4/call_id"),
                        Some(&json!("read/original"))
                    );
                    assert_eq!(
                        request.body.pointer("/input/2/encrypted_content"),
                        Some(&json!("encrypted+/="))
                    );
                }
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
                    Protocol::Responses => "event: response.completed",
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
// 作成時の関門: 起動を拒む場所と選択元の案内を、終了状態と同じ CLI の境界で守る。
fn startup_and_authentication_errors_have_distinct_exit_statuses() -> TestResult {
    for case in [
        "authentication",
        "option",
        "config",
        "home",
        "private-cwd",
        "private-project",
    ] {
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
            "private-cwd" | "private-project" => {
                let project = f.root.join("home/.config");
                fs::create_dir(&project)?;
                if case == "private-project" {
                    command.arg("--project").arg(&project);
                } else {
                    command.current_dir(&project);
                }
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
            "private-cwd" | "private-project" => {
                for expected in [
                    "The project directory ",
                    if case == "private-project" {
                        "(from --project)"
                    } else {
                        "(the current directory)"
                    },
                    "cannot be used:",
                    "writable root is inside a home dot directory",
                    "--project <dir>",
                ] {
                    assert!(stderr.contains(expected), "{stderr}");
                }
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
    let output = Running::start(f.command("Do it").args([
        "--mode",
        "normal",
        "--sandbox",
        "workspace-write",
    ]))?
    .finish()?;
    assert_eq!(output.status.code(), Some(0));
    assert!(!f.root.join("project/.benitoiteh/blocked.bnt").exists());
    let stderr = std::str::from_utf8(&output.stderr)?;
    assert!(stderr.contains("Ignoring project configuration:"));
    assert!(stderr.contains("Provider: mock; model: mock-model"));
    assert!(
        stderr.contains("denied by the never approval policy"),
        "{stderr}"
    );
    let (_, records) = f.records()?;
    assert_eq!(
        records
            .last()
            .and_then(|v| v.pointer("/approvals/policy_denied")),
        Some(&json!(1))
    );
    no_secret(&output, &records);
    server.shutdown()?;
    Ok(())
}

#[test]
fn exec_sends_displays_and_records_instructions_and_reads_legacy_records() -> TestResult {
    // 起動での収集忘れ、イベントへの転記漏れ、JSON と端末での表示漏れを入口で捕まえる。
    // 除外の個々の条件は context 側で確かめる（設計書 07-01「持ち主の境界」）。
    for json_output in [false, true] {
        let protocol = Protocol::Chat;
        let mut server = MockProvider::start(vec![ResponseScript::sse(
            protocol.response(None, "Done.", false),
        )])?;
        let f = Fixture::new(protocol, &server)?;
        fs::write(f.root.join("project/.git"), "worktree")?;
        fs::write(f.root.join("project/AGENTS.md"), "project <& instruction")?;
        fs::create_dir(f.root.join("project/sub"))?;
        let skipped_path = f.root.join("project/sub/AGENTS.md");
        fs::create_dir(&skipped_path)?;
        let user_path = f.root.join("config/benitoiteh/AGENTS.md");
        fs::write(&user_path, "user instruction")?;
        let expected_paths = json!([user_path, f.root.join("project/AGENTS.md")]);
        let mut command = f.command("Use project instructions.");
        command
            .args(["--mode", "normal", "--sandbox", "danger-full-access"])
            .current_dir(f.root.join("project/sub"));
        if json_output {
            command.arg("--json");
        }
        let output = Running::start(&mut command)?.finish()?;
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        server.shutdown()?;
        let (directory, mut records) = f.records()?;
        let start = records
            .iter()
            .find(|value| value.get("type") == Some(&json!("session_start")))
            .ok_or("missing session_start")?;
        assert_eq!(start.get("instruction_files"), Some(&expected_paths));
        let exclusions = start
            .get("skipped_instructions")
            .and_then(Value::as_array)
            .ok_or("missing exclusions")?;
        assert_eq!(exclusions.len(), 1);
        let skipped = exclusions.first().ok_or("missing exclusion diagnostic")?;
        assert_eq!(skipped.get("path"), Some(&json!(skipped_path)));
        assert!(
            skipped
                .get("reason")
                .and_then(Value::as_str)
                .is_some_and(|reason| reason.contains("regular file"))
        );
        if json_output {
            let events = lines(&output.stdout)?;
            assert_eq!(events.first(), Some(start));
        }
        let stderr = std::str::from_utf8(&output.stderr)?;
        for path in [user_path, f.root.join("project/AGENTS.md")] {
            assert!(
                stderr
                    .lines()
                    .any(|line| line == format!("Instruction file loaded: {}", path.display()))
            );
        }
        assert!(stderr.lines().any(|line| line.starts_with(&format!(
            "Instruction file skipped: {}:",
            skipped_path.display()
        ))));
        let requests = server.requests()?;
        let body = &requests.first().ok_or("missing request")?.body;
        let messages = body
            .get("messages")
            .and_then(Value::as_array)
            .ok_or("missing messages")?;
        let environment = messages
            .iter()
            .filter_map(|message| message.get("content").and_then(Value::as_array))
            .flatten()
            .filter_map(|part| part.get("text").and_then(Value::as_str))
            .find(|content| content.contains("<environment>"))
            .ok_or("missing environment")?;
        assert!(environment.contains("project &lt;&amp; instruction</project_instructions>"));
        assert!(
            environment
                .find("user instruction")
                .ok_or("missing user instructions")?
                < environment
                    .find("project &lt;&amp; instruction")
                    .ok_or("missing project instructions")?
        );
        assert_eq!(
            records.first().and_then(|value| value.get("format")),
            Some(&json!(2))
        );
        assert_eq!(
            benitoiteh::session::read_records(&directory)?.len(),
            records.len()
        );
        // 新しい欄がない旧形式を、実際の JSON Lines の読み手で読む。
        for record in &mut records {
            if record.get("type") == Some(&json!("session_start")) {
                let object = record.as_object_mut().ok_or("invalid record")?;
                object.remove("instruction_files");
                object.remove("skipped_instructions");
            }
        }
        let legacy = records
            .iter()
            .map(serde_json::to_string)
            .collect::<Result<Vec<_>, _>>()?
            .join("\n")
            + "\n";
        fs::write(directory.join("session.jsonl"), legacy)?;
        let restored = benitoiteh::session::read_records(&directory)?;
        assert!(restored.iter().any(|record| matches!(record,
            benitoiteh::session::Record::SessionStart { instruction_files, skipped_instructions, .. }
                if instruction_files.is_empty() && skipped_instructions.is_empty())));
        let start = restored
            .iter()
            .find(|record| matches!(record, benitoiteh::session::Record::SessionStart { .. }))
            .ok_or("missing restored start")?;
        let serialized = serde_json::to_value(start)?;
        assert!(serialized.get("instruction_files").is_none());
        assert!(serialized.get("skipped_instructions").is_none());
    }
    Ok(())
}

#[test]
fn exec_does_not_wait_for_instruction_fifo() -> TestResult {
    // FIFO の拒否は、起動を待ち続ける失敗も捕まえるため、時間上限付きの入口で守る。
    let protocol = Protocol::Chat;
    let mut server = MockProvider::start(vec![ResponseScript::sse(
        protocol.response(None, "Done.", false),
    )])?;
    let f = Fixture::new(protocol, &server)?;
    let candidate = f.root.join("project/AGENTS.md");
    assert!(
        Command::new("mkfifo")
            .arg(&candidate)
            .current_dir(&f.root)
            .status()?
            .success()
    );
    let mut command = f.command("Report skipped instructions.");
    command.args([
        "--mode",
        "normal",
        "--sandbox",
        "danger-full-access",
        "--json",
    ]);
    let output = Running::start(&mut command)?.finish()?;
    assert!(output.status.success());
    let events = lines(&output.stdout)?;
    let start = events.first().ok_or("missing session_start")?;
    assert!(start.get("instruction_files").is_none());
    let skipped = start
        .get("skipped_instructions")
        .and_then(Value::as_array)
        .ok_or("missing exclusions")?;
    assert_eq!(skipped.len(), 1);
    let skipped = skipped.first().ok_or("missing FIFO exclusion")?;
    assert_eq!(skipped.get("path"), Some(&json!(candidate)));
    assert!(
        skipped
            .get("reason")
            .and_then(Value::as_str)
            .is_some_and(|reason| reason.contains("regular file"))
    );
    server.shutdown()?;
    Ok(())
}

// 作成時の関門: exec が ToolContext に Session を渡し忘れる退行と、
// 設定で参照した提供者のキーが子へ漏れる退行を、入口と記録の境界で捕まえる。
#[test]
fn exec_bash_connects_session_and_preserves_spill_metadata_without_provider_keys() -> TestResult {
    let protocol = Protocol::Chat;
    let mut server = MockProvider::start(vec![
        ResponseScript::sse(protocol.response(
            Some(("large", "bash", json!({"command":"cat input"}))),
            "",
            false,
        )),
        ResponseScript::sse(protocol.response(
            Some(("env", "bash", json!({"command":"env"}))),
            "",
            false,
        )),
        ResponseScript::sse(protocol.response(None, "Done.", false)),
    ])?;
    let f = Fixture::new(protocol, &server)?;
    let all = (1..=2001)
        .map(|n| format!("line-{n}\n"))
        .collect::<String>();
    fs::write(f.root.join("project/input"), &all)?;
    let mut command = f.command("Run the commands.");
    command.args([
        "--mode",
        "normal",
        "--sandbox",
        "danger-full-access",
        "--json",
    ]);
    let output = Running::start(&mut command)?.finish()?;
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let events = lines(&output.stdout)?;
    assert_eq!(
        events
            .iter()
            .filter(|e| e.get("type") == Some(&json!("tool_result")))
            .count(),
        2
    );
    let (directory, records) = f.records()?;
    let results = records
        .iter()
        .filter(|r| r.get("type") == Some(&json!("tool_result")))
        .collect::<Vec<_>>();
    assert_eq!(results.len(), 2);
    let large = results.first().ok_or("missing bash result")?;
    assert_eq!(large.pointer("/result/is_error"), Some(&json!(false)));
    let path = large
        .pointer("/spill/path")
        .and_then(Value::as_str)
        .ok_or("missing spill path")?;
    assert_eq!(fs::read_to_string(directory.join(path))?, all);
    let env = results.get(1).ok_or("missing environment result")?;
    let text = env
        .pointer("/result/text")
        .and_then(Value::as_str)
        .ok_or("missing environment text")?;
    assert!(text.ends_with("[exit code: 0]"), "{text}");
    assert!(
        !text.contains("BENITOITEH_EXEC_TEST_KEY") && !text.contains(KEY),
        "{text}"
    );
    assert!(env.get("spill").is_none_or(Value::is_null));
    server.shutdown()?;
    Ok(())
}

// 作成時の関門: ループの圧縮イベントを、exec の出力が捨てたり別の形にする退行を捕まえる。
#[test]
fn exec_outputs_compaction_events_and_counts_on_json_and_terminal() -> TestResult {
    for json_output in [true, false] {
        let protocol = Protocol::Chat;
        let mut server = MockProvider::start(vec![
            ResponseScript::sse(protocol.response(
                Some(("read", "read", json!({"path":"a.bnt"}))),
                "Working.",
                false,
            )),
            ResponseScript::sse(protocol.response(None, "summary", false)),
            ResponseScript::sse(protocol.response(None, "Done.", false)),
        ])?;
        let f = Fixture::new(protocol, &server)?;
        // 大きい JSON の差分を出力せず、道具の結果で閾値を超える。
        // 親が出力を読む前の終了待ちを、OS ごとのパイプ容量に依存させない。
        fs::write(
            f.root.join("project/a.bnt"),
            "line-data-long-enough-to-exceed-the-budget\n".repeat(1_200),
        )?;
        let config_file = f.root.join("config/benitoiteh/config.toml");
        fs::write(&config_file, fs::read_to_string(&config_file)?.replace(
            "capabilities = { reasoning_replay = \"same-destination\" }",
            "capabilities = { reasoning_replay = \"same-destination\", tool_choice_none = true, context_window = 30000 }",
        ))?;
        let mut command = f.command("Read the project.");
        command.args(["--mode", "normal", "--sandbox", "danger-full-access"]);
        if json_output {
            command.arg("--json");
        }
        let output = Running::start(&mut command)?.finish()?;
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        if json_output {
            let values = lines(&output.stdout)?;
            let event = values
                .iter()
                .find(|value| value.get("type") == Some(&json!("compaction")))
                .ok_or("missing compaction event")?;
            assert_eq!(event.get("reason"), Some(&json!("threshold")));
            assert_eq!(event.get("summarized_items"), Some(&json!(3)));
            assert_eq!(event.get("kept_items"), Some(&json!(0)));
            assert!(
                event
                    .get("estimate_before")
                    .and_then(Value::as_u64)
                    .is_some_and(|n| n > 15_000)
            );
            assert_eq!(
                values.last().and_then(|value| value.get("compactions")),
                Some(&json!(1))
            );
            assert_eq!(
                values.last().and_then(|value| value.get("turns")),
                Some(&json!(2))
            );
        } else {
            assert!(
                std::str::from_utf8(&output.stderr)?.contains(
                    "Compacted the conversation (threshold): 3 items summarized, 0 kept."
                )
            );
            assert_eq!(std::str::from_utf8(&output.stdout)?, "Done.\n");
        }
        let (directory, records) = f.records()?;
        assert_eq!(
            records.last().and_then(|value| value.get("compactions")),
            Some(&json!(1))
        );
        let replayed = benitoiteh::session::replay(&directory)?;
        assert!(serde_json::to_string(&replayed)?.contains("<compacted_summary>"));
        assert_eq!(server.requests()?.len(), 3);
        server.shutdown()?;
    }
    Ok(())
}

// 関門: exec の二つの印の配線を落として、SIGINT で bash だけが残る退行を守る。
#[test]
fn sigint_stops_a_running_bash_and_records_run_cancellation() -> TestResult {
    let mut server = MockProvider::start(vec![ResponseScript::sse(Protocol::Chat.response(
        Some((
            "bash",
            "bash",
            json!({"command":"printf ready > started; exec sleep 30"}),
        )),
        "",
        false,
    ))])?;
    let f = Fixture::new(Protocol::Chat, &server)?;
    let child = Running::start(f.command("Wait for bash").args([
        "--json",
        "--mode",
        "normal",
        "--sandbox",
        "danger-full-access",
    ]))?;
    let started = Instant::now();
    while !f.root.join("project/started").exists() {
        if started.elapsed() > Duration::from_secs(5) {
            return Err("bash did not start".into());
        }
        thread::sleep(Duration::from_millis(5));
    }
    let pid = rustix::process::Pid::from_raw(i32::try_from(
        child.0.as_ref().ok_or("missing child")?.id(),
    )?)
    .ok_or("invalid pid")?;
    rustix::process::kill_process(pid, rustix::process::Signal::INT)?;
    let output = child.finish()?;
    assert_eq!(output.status.code(), Some(130));
    let (_, records) = f.records()?;
    assert!(
        records
            .iter()
            .any(|record| record.get("type") == Some(&json!("session_end"))
                && record.get("outcome") == Some(&json!("cancelled")))
    );
    assert_eq!(server.requests()?.len(), 1);
    server.shutdown()?;
    Ok(())
}

// 関門: --mode の明示値だけを見て、設定と再開の通常モードに --allow を通してしまう退行。
#[test]
fn allow_is_rejected_for_explicit_default_and_resumed_normal_modes() -> TestResult {
    let mut server = MockProvider::start(vec![ResponseScript::sse(
        Protocol::Chat.response(None, "done", false),
    )])?;
    let f = Fixture::new(Protocol::Chat, &server)?;
    for explicit in [false, true] {
        let mut command = f.command("request");
        command.args(["--allow", "File.Write"]);
        if explicit {
            command.args(["--mode", "normal"]);
        }
        let output = command.output()?;
        assert_eq!(output.status.code(), Some(2));
        assert!(
            String::from_utf8(output.stderr)?
                .contains("benitoiteh: --allow is available only in restricted mode.")
        );
    }
    assert!(server.requests()?.is_empty());
    let first = f.command("first").output()?;
    assert!(
        first.status.success(),
        "{}",
        String::from_utf8_lossy(&first.stderr)
    );
    let (directory, records) = f.records()?;
    let config = f.root.join("config/benitoiteh/config.toml");
    fs::write(
        &config,
        fs::read_to_string(&config)?
            .replace("default_mode = 'normal'", "default_mode = 'restricted'"),
    )?;
    let id = directory.file_name().and_then(|s| s.to_str()).ok_or("id")?;
    let output = f
        .command("resume")
        .args(["--resume", id, "--allow", "Assert.Check"])
        .output()?;
    assert_eq!(output.status.code(), Some(2));
    assert!(
        String::from_utf8(output.stderr)?.contains("--allow is available only in restricted mode.")
    );
    assert_eq!(server.requests()?.len(), 1);
    assert_eq!(f.records()?.1.len(), records.len());
    server.shutdown()?;
    Ok(())
}
