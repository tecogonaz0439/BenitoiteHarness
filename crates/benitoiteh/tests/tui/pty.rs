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
            .args(["--mode", "normal", "--resume"])
            .arg(directory.file_name().ok_or("session ID")?)
            .current_dir(&f.project)
            .env_clear()
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
                "--approval",
                "never",
                "--json",
                "Check the controlling terminal",
            ])
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
