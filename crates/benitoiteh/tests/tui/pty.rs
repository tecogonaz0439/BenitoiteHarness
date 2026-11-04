//! 端末の復元と子の期限を、本物の擬似端末で確かめる（設計書 05-02、BH-097）。

use super::*;
use std::io::Write;
use std::os::unix::process::CommandExt;
use std::process::Command;

fn probe() -> Result<PathBuf, Box<dyn Error>> {
    let executable = std::env::current_exe()?;
    Ok(executable
        .parent()
        .and_then(std::path::Path::parent)
        .ok_or("target directory")?
        .join("sandbox-probe"))
}

// 関門: セッションの頭の exit と出力の排出が互いを待つ退行と、無期限の子を守る。
#[test]
fn pty_drains_session_output_during_exit_and_times_out_the_process_group() -> TestResult {
    let fixture = Fixture::new()?;
    let probe = probe()?;
    let source = fixture.root.join("input");
    for count in [1, 32768] {
        let mut pty = tty::Pty::new()?;
        fs::write(&source, b"output\n".repeat(count))?;
        let mut child = pty
            .wrapper(&probe)
            .arg(&probe)
            .arg("echo-stdin")
            .stdin(fs::File::open(&source)?)
            .stdout(pty.slave()?)
            .spawn()?;
        let (output, terminal) = pty.capture(&mut child)?;
        assert!(output.status.success(), "{output:?}");
        assert_eq!(terminal, b"output\r\n".repeat(count));
    }
    let mut pty = tty::Pty::new()?;
    let started = Instant::now();
    let marker = fixture.root.join("survived-timeout");
    let mut child = pty
        .wrapper(&probe)
        .arg("/bin/sh")
        .args(["-c", "/bin/sleep 0.5; printf alive > \"$1\"", "pty-timeout"])
        .arg(&marker)
        .spawn()?;
    let error = pty
        .capture_until(&mut child, Duration::from_millis(200))
        .err()
        .ok_or("missing deadline")?;
    assert_eq!(error.kind(), io::ErrorKind::TimedOut);
    assert!(started.elapsed() < Duration::from_secs(2));
    assert!(child.try_wait()?.is_some());
    thread::sleep(Duration::from_millis(600));
    assert!(
        !marker.exists(),
        "a descendant survived the process-group deadline"
    );
    Ok(())
}

// 関門: 期限以外の入力元の失敗でも子孫が生き残る退行を、実際の子の状態と書き込みで守る。
#[test]
fn pty_action_errors_kill_and_reap_the_child_and_its_group() -> TestResult {
    let f = Fixture::new()?;
    let probe = probe()?;
    let mut pty = tty::Pty::new()?;
    let marker = f.root.join("survived-error");
    let mut child = pty
        .wrapper(&probe)
        .arg("/bin/sh")
        .args(["-c", "/bin/sleep 0.5; printf alive > \"$1\"", "pty-error"])
        .arg(&marker)
        .spawn()?;
    let result = pty.interact(&mut child, Duration::from_secs(2), |_, _, elapsed| {
        if elapsed >= Duration::from_millis(200) {
            Err(io::Error::other("input failed"))
        } else {
            Ok(())
        }
    });
    let exited = child.try_wait()?.is_some();
    // 修正前の失敗を確かめる場合にも、テスト自身が子を残さない。
    if !exited {
        let _group = rustix::process::kill_process_group(
            rustix::process::Pid::from_child(&child),
            rustix::process::Signal::KILL,
        );
        child.kill()?;
        pty.capture_until(&mut child, Duration::from_secs(1))?;
    }
    assert!(result.is_err());
    assert!(exited, "input failure left the child running");
    thread::sleep(Duration::from_millis(600));
    assert!(!marker.exists());
    Ok(())
}

// 関門: モードの確認中のシグナルを通常の拒否と同じ終了状態にする退行と、
// raw のモードのまま診断を出す退行を、実行ファイルと端末の復元で守る。
#[test]
fn mode_confirmation_signals_exit_130_and_decline_is_printed_after_terminal_restore() -> TestResult
{
    for signal in [
        None,
        Some(rustix::process::Signal::TERM),
        Some(rustix::process::Signal::HUP),
        Some(rustix::process::Signal::INT),
    ] {
        let f = Fixture::new()?;
        let mut server = MockProvider::start(Vec::new())?;
        let (worker, directory) = f.worker(&server)?;
        worker.close()?;
        let record = directory.join("session.jsonl");
        let mut records: Vec<serde_json::Value> = fs::read_to_string(&record)?
            .lines()
            .map(serde_json::from_str)
            .collect::<Result<_, _>>()?;
        let header = records.first_mut().ok_or("missing header")?;
        *header.get_mut("mode").ok_or("missing mode")? = json!("restricted");
        let mut bytes = Vec::new();
        for record in records {
            serde_json::to_writer(&mut bytes, &record)?;
            bytes.push(b'\n');
        }
        fs::write(&record, &bytes)?;
        fs::create_dir_all(f.paths.config_file.parent().ok_or("config parent")?)?;
        fs::write(
            &f.paths.config_file,
            "default_provider = 'mock'\n[providers.mock]\nkind = 'openai-chat'\nbase_url = 'http://127.0.0.1:1/v1'\nmodel = 'mock-model'\ncontext_window = 4096\n",
        )?;
        let mut pty = tty::Pty::new()?;
        let original = rustix::termios::tcgetattr(pty.slave()?)?;
        let mut input = pty.input()?;
        let mut command = Command::new(env!("CARGO_BIN_EXE_benitoiteh"));
        command
            .args(["--mode", "normal", "--no-sandbox", "--resume"])
            .arg(directory.file_name().ok_or("session ID")?)
            .current_dir(&f.project)
            .env_clear()
            .env("PATH", "")
            .env("HOME", f.root.join("home"))
            .env("XDG_CONFIG_HOME", f.root.join("config"))
            .env("XDG_STATE_HOME", f.root.join("state"))
            .env("TMPDIR", f.root.join("tmp"))
            .process_group(0)
            .stdin(pty.slave()?)
            .stdout(pty.slave()?)
            .stderr(pty.slave()?);
        let mut child = command.spawn()?;
        let mut shown = None;
        let mut sent = false;
        let (output, terminal) = pty.interact(
            &mut child,
            Duration::from_secs(10),
            |child, terminal, elapsed| {
                if shown.is_none() && String::from_utf8_lossy(terminal).contains("[n] decline") {
                    shown = Some(elapsed);
                }
                if !sent
                    && shown
                        .is_some_and(|at| elapsed.saturating_sub(at) >= Duration::from_millis(400))
                {
                    sent = true;
                    if let Some(signal) = signal {
                        let pid = rustix::process::Pid::from_raw(
                            i32::try_from(child.id()).map_err(io::Error::other)?,
                        )
                        .ok_or_else(|| io::Error::other("pid"))?;
                        rustix::process::kill_process(pid, signal)?;
                    } else {
                        input.write_all(b"n")?;
                    }
                }
                Ok(())
            },
        )?;
        assert!(sent, "{output:?}: {}", String::from_utf8_lossy(&terminal));
        assert_eq!(
            output.status.code(),
            Some(if signal.is_some() { 130 } else { 0 }),
            "{output:?}"
        );
        let restored = rustix::termios::tcgetattr(pty.slave()?)?;
        assert_eq!(restored.local_modes, original.local_modes);
        assert_eq!(restored.input_modes, original.input_modes);
        assert_eq!(restored.output_modes, original.output_modes);
        assert_eq!(restored.control_modes, original.control_modes);
        let visible = String::from_utf8_lossy(&terminal);
        assert!(visible.contains("\x1b[?2004l"));
        if signal.is_none() {
            let restored_at = visible.find("\x1b[?2004l").ok_or("missing restoration")?;
            let declined_at = visible
                .find("Resume in normal mode was declined.")
                .ok_or("missing decline")?;
            assert!(restored_at < declined_at);
        } else {
            assert!(!visible.contains("was declined"));
        }
        assert_eq!(fs::read(&record)?, bytes);
        assert!(server.requests()?.is_empty());
        server.shutdown()?;
    }
    Ok(())
}

// 関門: danger-full-access 相当の起動でも制御端末を引き継がないことを、対照付きで守る。
#[test]
fn unsandboxed_session_launch_has_no_controlling_terminal() -> TestResult {
    let probe = probe()?;
    for operation in ["tty-open", "tty-write", "tty-ioctl"] {
        let f = Fixture::new()?;
        let mut pty = tty::Pty::new()?;
        let mut control = pty.wrapper(&probe).arg(&probe).arg("tty-open").spawn()?;
        let (output, _) = pty.capture(&mut control)?;
        assert!(output.status.success(), "{output:?}");
        let command = format!(
            "'{}' {operation}",
            probe.to_string_lossy().replace('\'', "'\\''")
        );
        let mut server = MockProvider::start(vec![
            ResponseScript::sse(response(
                json!({"tool_calls":[{"index":0,"id":"tty","type":"function","function":{"name":"bash","arguments":serde_json::to_string(&json!({"command":command}))?}}]}),
                "tool_calls",
            )),
            ResponseScript::sse(response(json!({"content":"done"}), "stop")),
        ])?;
        fs::create_dir_all(f.paths.config_file.parent().ok_or("config parent")?)?;
        fs::write(
            &f.paths.config_file,
            format!(
                "default_provider = 'mock'\n[providers.mock]\nkind = 'openai-chat'\nbase_url = '{}'\nmodel = 'mock-model'\ncontext_window = 100000\n",
                server.base_url()
            ),
        )?;
        let mut child = pty
            .wrapper(&probe)
            .arg(env!("CARGO_BIN_EXE_benitoiteh"))
            .args([
                "exec",
                "--mode",
                "normal",
                "--sandbox",
                "danger-full-access",
                "--no-sandbox",
                "--approval",
                "never",
                "--json",
                "Check the controlling terminal",
            ])
            .env("PATH", "")
            .env("HOME", f.root.join("home"))
            .env("XDG_CONFIG_HOME", f.root.join("config"))
            .env("XDG_STATE_HOME", f.root.join("state"))
            .env("TMPDIR", f.root.join("tmp"))
            .current_dir(&f.project)
            .spawn()?;
        let (output, terminal) = pty.capture(&mut child)?;
        assert!(output.status.success(), "{output:?}");
        assert!(terminal.is_empty());
        let requests = server.requests()?;
        let results = requests
            .get(1)
            .ok_or("missing tool result request")?
            .body
            .to_string();
        if operation == "tty-open" {
            assert!(results.contains("[exit code: 15]"), "{results}");
        } else {
            assert!(
                results.contains("[exit code: 10]") || results.contains("[exit code: 11]"),
                "{results}"
            );
        }
        server.shutdown()?;
    }
    Ok(())
}

// 関門: /quit が終了状態を変える退行と、処理中の受信を止めて端末を戻せなくなる退行を本物の端末で守る。
#[test]
fn slash_quit_exits_zero_and_restores_terminal_while_idle_and_processing() -> TestResult {
    for processing in [false, true] {
        let f = Fixture::new()?;
        let mut blocked = ResponseScript::sse(Vec::new());
        blocked.delivery = Delivery::HeadersOnly;
        let mut server = MockProvider::start(if processing {
            vec![blocked]
        } else {
            Vec::new()
        })?;
        fs::create_dir_all(f.paths.config_file.parent().ok_or("config parent")?)?;
        fs::write(
            &f.paths.config_file,
            format!(
                "default_provider = 'mock'\n[providers.mock]\nkind = 'openai-chat'\nbase_url = '{}'\nmodel = 'mock-model'\n",
                server.base_url()
            ),
        )?;
        let mut pty = tty::Pty::new()?;
        let original = rustix::termios::tcgetattr(pty.slave()?)?;
        let mut input = pty.input()?;
        let mut command = Command::new(env!("CARGO_BIN_EXE_benitoiteh"));
        command.args([
            "--mode",
            "normal",
            "--sandbox",
            "danger-full-access",
            "--no-sandbox",
            "--approval",
            "never",
        ]);
        if processing {
            command.arg("Wait for a response");
        }
        command
            .current_dir(&f.project)
            .env_clear()
            .env("PATH", "")
            .env("HOME", f.root.join("home"))
            .env("XDG_CONFIG_HOME", f.root.join("config"))
            .env("XDG_STATE_HOME", f.root.join("state"))
            .env("TMPDIR", f.root.join("tmp"))
            .process_group(0)
            .stdin(pty.slave()?)
            .stdout(pty.slave()?)
            .stderr(pty.slave()?);
        let mut child = command.spawn()?;
        let mut sent = false;
        let (output, terminal) =
            pty.interact(&mut child, Duration::from_secs(10), |_, terminal, _| {
                let started = String::from_utf8_lossy(terminal).contains("Sandbox:");
                if !sent && started && (!processing || !server.requests()?.is_empty()) {
                    sent = true;
                    input.write_all(b"/quit\r")?;
                }
                Ok(())
            })?;
        assert!(sent, "{}", String::from_utf8_lossy(&terminal));
        assert_eq!(
            output.status.code(),
            Some(0),
            "{output:?}: {}",
            String::from_utf8_lossy(&terminal)
        );
        let restored = rustix::termios::tcgetattr(pty.slave()?)?;
        assert_eq!(restored.local_modes, original.local_modes);
        assert_eq!(restored.input_modes, original.input_modes);
        assert_eq!(restored.output_modes, original.output_modes);
        assert_eq!(restored.control_modes, original.control_modes);
        let terminal = String::from_utf8_lossy(&terminal);
        for restored in ["\x1b[?2004l", "\x1b[?25h", "\x1b[?7h"] {
            assert!(terminal.contains(restored));
        }
        assert_eq!(server.requests()?.len(), usize::from(processing));
        server.shutdown()?;
    }
    Ok(())
}

// 関門: 巻き戻し中の二度目の合図が待ち続ける退行と、復元前の時点の ID を失う退行を守る。
#[test]
fn second_signal_prints_checkpoint_after_terminal_restore_only_during_rewind() -> TestResult {
    for during in [true, false] {
        let count = if during { 8000 } else { 1 };
        let f = Fixture::new()?;
        // 十分な数の実ファイルを戻し、通知を受けてから合図を送る間も巻き戻しを続ける。
        // 時刻やテスト専用の待ち合わせを本番の書き戻しへ差し込まない。
        for index in 0..count {
            fs::write(f.project.join(format!("file-{index:05}")), "saved")?;
        }
        let mut server = MockProvider::start(vec![ResponseScript::sse(response(
            json!({"content":"recorded"}),
            "stop",
        ))])?;
        let (mut worker, directory) = f.worker(&server)?;
        let mut screen = Screen::new((160, 30), status());
        worker.submit("Record initial files".into(), RunControl::default())?;
        finish(&mut worker, &mut screen, &mut Vec::new())?;
        worker.close()?;
        for index in 0..count {
            fs::write(f.project.join(format!("file-{index:05}")), "changed")?;
        }
        fs::create_dir_all(f.paths.config_file.parent().ok_or("config parent")?)?;
        fs::write(
            &f.paths.config_file,
            format!(
                "default_provider = 'mock'\n[providers.mock]\nkind = 'openai-chat'\nbase_url = '{}'\nmodel = 'mock-model'\ncontext_window = 100000\n",
                server.base_url()
            ),
        )?;
        let mut pty = tty::Pty::new()?;
        let original = rustix::termios::tcgetattr(pty.slave()?)?;
        let mut input = pty.input()?;
        let mut child = Command::new(env!("CARGO_BIN_EXE_benitoiteh"))
            .args([
                "--mode",
                "normal",
                "--sandbox",
                "danger-full-access",
                "--no-sandbox",
                "--approval",
                "never",
                "--resume",
            ])
            .arg(directory.file_name().ok_or("session ID")?)
            .current_dir(&f.project)
            .env_clear()
            .env("PATH", "")
            .env("HOME", f.root.join("home"))
            .env("XDG_CONFIG_HOME", f.root.join("config"))
            .env("XDG_STATE_HOME", f.root.join("state"))
            .env("TMPDIR", f.root.join("tmp"))
            .process_group(0)
            .stdin(pty.slave()?)
            .stdout(pty.slave()?)
            .stderr(pty.slave()?)
            .spawn()?;
        let mut stage = 0;
        let mut confirmation_at = None;
        let (output, terminal) = pty.interact(
            &mut child,
            Duration::from_secs(20),
            |child, terminal, elapsed| {
                let visible = String::from_utf8_lossy(terminal);
                match stage {
                    0 if visible.contains("Sandbox:") => {
                        input.write_all(b"/rewind\r")?;
                        stage = 1;
                    }
                    1 if visible.contains("Select a checkpoint") => {
                        input.write_all(b"\r")?;
                        stage = 2;
                    }
                    2 if visible.contains("Select rewind scope") => {
                        input.write_all(b"\r")?;
                        stage = 3;
                    }
                    3 if visible.contains("[y] rewind") => {
                        let shown = *confirmation_at.get_or_insert(elapsed);
                        if elapsed.saturating_sub(shown) >= Duration::from_millis(400) {
                            input.write_all(b"y")?;
                            stage = 4;
                        }
                    }
                    4 if visible.contains(if during {
                        "Checkpoint cp-2 recorded."
                    } else {
                        "Rewind completed:"
                    }) =>
                    {
                        let pid = rustix::process::Pid::from_child(child);
                        rustix::process::kill_process(pid, rustix::process::Signal::TERM)?;
                        rustix::process::kill_process(pid, rustix::process::Signal::HUP)?;
                        stage = 5;
                    }
                    _ => {}
                }
                Ok(())
            },
        )?;
        let visible = String::from_utf8_lossy(&terminal);
        assert_eq!(stage, 5, "{output:?}: {visible}");
        assert_eq!(output.status.code(), Some(130), "{output:?}: {visible}");
        let restored = rustix::termios::tcgetattr(pty.slave()?)?;
        assert_eq!(restored.local_modes, original.local_modes);
        assert_eq!(restored.input_modes, original.input_modes);
        assert_eq!(restored.output_modes, original.output_modes);
        assert_eq!(restored.control_modes, original.control_modes);
        let restored_at = visible.find("\x1b[?2004l").ok_or("missing restoration")?;
        if during {
            let checkpoint_at = visible
                .find("Checkpoint before rewind: cp-2")
                .ok_or("missing checkpoint after interruption")?;
            assert!(restored_at < checkpoint_at);
        } else {
            assert!(!visible.contains("Checkpoint before rewind:"));
        }
        let records = read_records(&directory)?;
        assert!(records.iter().any(|record| matches!(record, Record::Checkpoint(point) if point.id == "cp-2" && point.reason == benitoiteh::rewind::CheckpointReason::BeforeRewind)));
        assert_eq!(
            records
                .iter()
                .any(|record| matches!(record, Record::Branch { .. })),
            !during,
            "rewind completion and signal timing"
        );
        if during {
            assert!(
                fs::read_dir(&f.project)?
                    .filter_map(Result::ok)
                    .any(|entry| fs::read_to_string(entry.path())
                        .is_ok_and(|content| content == "changed"))
            );
        }
        assert_eq!(server.requests()?.len(), 1);
        server.shutdown()?;
    }
    Ok(())
}

// 作成時の関門: 本物の端末を通した確認・通知・外部のエディタ（設計書 05-02「通知」「入力の操作」、
// 03-03「.agents への書き込み」）。画面の部品のテストは時刻と端末を差し替えるので、擬似端末の
// バイト列からのキーと前面の報告の読み取り、表示の直後のキーの扱い、エディタのための端末の
// 明け渡しと復元を、実行ファイルと端末の境界で一度通す。エディタと通知のコマンドはサンドボックスの外で
// 動くので、提供者のキー（固定の名前と設定で参照したもの）を受け取らないことも確かめる。
#[test]
fn terminal_confirmation_notification_and_external_editor_work_end_to_end() -> TestResult {
    let f = Fixture::new()?;
    let tool = response(
        json!({"tool_calls":[{"index":0,"id":"call_1","type":"function","function":{"name":"write","arguments":json!({"path":".agents/note.md","content":"approved"}).to_string()}}]}),
        "tool_calls",
    );
    let mut server = MockProvider::start(vec![
        ResponseScript::sse(tool.into_bytes()),
        ResponseScript::sse(response(json!({"content":"Wrote it."}), "stop").into_bytes()),
        ResponseScript::sse(response(json!({"content":"Read it."}), "stop").into_bytes()),
    ])?;
    // 通知のコマンドは、受け取った提供者のキーと知らせの文を記録に足す。
    let log = f.root.join("notified.log");
    let notify = f.root.join("notify.sh");
    fs::write(
        &notify,
        format!(
            "printf '%s/%s %s\\n' \"${{OPENAI_API_KEY:-none}}\" \"${{BENITOITEH_TUI_TEST_KEY:-none}}\" \"$1\" >> '{}'\n",
            log.display()
        ),
    )?;
    fs::create_dir_all(f.paths.config_file.parent().ok_or("config parent")?)?;
    fs::write(
        &f.paths.config_file,
        format!(
            "default_provider = 'mock'\n[providers.mock]\nkind = 'openai-chat'\nbase_url = '{}'\nmodel = 'mock-model'\napi_key = {{ env = 'BENITOITEH_TUI_TEST_KEY' }}\n[notifications]\ncommand = ['/bin/sh', '{}']\n",
            server.base_url(),
            notify.display()
        ),
    )?;
    // 外部のエディタは、下書きのファイルに決まった文と、受け取った提供者のキーを書いて終える。
    let editor = f.root.join("editor.sh");
    fs::write(
        &editor,
        "#!/bin/sh\nprintf 'edited request keys=%s/%s' \"${OPENAI_API_KEY:-none}\" \"${BENITOITEH_TUI_TEST_KEY:-none}\" > \"$1\"\n",
    )?;
    fs::set_permissions(&editor, std::os::unix::fs::PermissionsExt::from_mode(0o700))?;
    let mut pty = tty::Pty::new()?;
    let original = rustix::termios::tcgetattr(pty.slave()?)?;
    let mut input = pty.input()?;
    let mut command = Command::new(env!("CARGO_BIN_EXE_benitoiteh"));
    command
        .args(["--mode", "normal", "--no-sandbox", "Write the note"])
        .current_dir(&f.project)
        .env_clear()
        .env("PATH", "")
        .env("HOME", f.root.join("home"))
        .env("XDG_CONFIG_HOME", f.root.join("config"))
        .env("XDG_STATE_HOME", f.root.join("state"))
        .env("TMPDIR", f.root.join("tmp"))
        .env("VISUAL", &editor)
        .env("OPENAI_API_KEY", "fixed-provider-secret")
        .env("BENITOITEH_TUI_TEST_KEY", "referenced-provider-secret")
        .process_group(0)
        .stdin(pty.slave()?)
        .stdout(pty.slave()?)
        .stderr(pty.slave()?);
    let mut child = command.spawn()?;
    let mut stage = 0;
    let mut shown = None;
    let mut early_write = false;
    let project = f.project.clone();
    let (output, terminal) = pty.interact(
        &mut child,
        Duration::from_secs(20),
        |_, terminal, elapsed| {
            let visible = String::from_utf8_lossy(terminal);
            match stage {
                // 前面の報告を有効にしたら、前面を離れたと報告する。
                0 if visible.contains("\x1b[?1004h") => {
                    input.write_all(b"\x1b[O")?;
                    stage = 1;
                }
                1 if visible.contains("[y] write") => {
                    // 表示の直後のキーは読み捨てる。すぐの y では書かず、少し待った y で書く。
                    if shown.is_none() {
                        input.write_all(b"y")?;
                    }
                    let at = *shown.get_or_insert(elapsed);
                    if elapsed.saturating_sub(at) >= Duration::from_millis(400) {
                        early_write = project.join(".agents/note.md").exists();
                        input.write_all(b"y")?;
                        stage = 2;
                    }
                }
                2 if visible.contains("Wrote it.") => {
                    input.write_all(b"\x07")?;
                    stage = 3;
                }
                3 if visible.contains("edited request") => {
                    input.write_all(b"\r")?;
                    stage = 4;
                }
                4 if visible.contains("Read it.") => {
                    input.write_all(b"/quit\r")?;
                    stage = 5;
                }
                _ => {}
            }
            Ok(())
        },
    )?;
    let visible = String::from_utf8_lossy(&terminal);
    assert_eq!(stage, 5, "{visible}");
    assert!(
        !early_write,
        "the key sent as the confirmation appeared was accepted"
    );
    assert_eq!(output.status.code(), Some(0), "{output:?}: {visible}");
    assert_eq!(
        fs::read_to_string(f.project.join(".agents/note.md"))?,
        "approved"
    );
    // 前面にない間に、確認を開いたときと処理を終えたときに知らせた。
    for message in [
        "\x1b]9;benitoiteh is waiting for your confirmation.\x07",
        "\x1b]9;benitoiteh finished the request.\x07",
    ] {
        assert!(visible.contains(message), "{message:?}: {visible}");
    }
    let requests = server.requests()?;
    assert_eq!(requests.len(), 3);
    assert!(
        requests
            .last()
            .ok_or("edited request")?
            .body
            .to_string()
            .contains("edited request keys=none/none")
    );
    // 通知のコマンドは終わりを待たずに起動するので、確認と終わりの記録がそろうまで待つ。
    let started = std::time::Instant::now();
    let notified = loop {
        let notified = fs::read_to_string(&log).unwrap_or_default();
        if notified.lines().count() >= 2 || started.elapsed() > Duration::from_secs(5) {
            break notified;
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    assert!(notified.lines().count() >= 2, "{notified}");
    assert!(
        notified.lines().all(|line| line.starts_with("none/none ")),
        "{notified}"
    );
    let restored = rustix::termios::tcgetattr(pty.slave()?)?;
    assert_eq!(restored.local_modes, original.local_modes);
    assert_eq!(restored.input_modes, original.input_modes);
    assert!(visible.contains("\x1b[?1004l"));
    server.shutdown()?;
    Ok(())
}
