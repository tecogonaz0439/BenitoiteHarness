//! 判定の実行器の引数から、OS の拒否と結果の JSON までを確かめる（設計書 07-02）。

#![cfg(target_os = "macos")]

#[path = "support/socket_directory.rs"]
mod socket_directory;

#[path = "support/macos_process_info.rs"]
mod macos_process_info;

use std::error::Error;
use std::fs;
use std::net::{TcpListener, TcpStream};
use std::os::unix::fs::{PermissionsExt, symlink};
use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use serde_json::Value;
use socket_directory::SocketDirectory;

type TestResult = Result<(), Box<dyn Error>>;

// 連続出力の 8 プロセスと 4 スレッドの計算が、ほかの判定の起動・停止を
// 時間切れにしないよう、このファイルの全テストを直列にする。上限は緩めない。
// ロックの待ちは、各テストが測る時間と実行器の timeout の外に置く（設計書 07-01）。
static JUDGE_TEST_LOCK: Mutex<()> = Mutex::new(());

fn serial_judgment() -> MutexGuard<'static, ()> {
    // 一件の assert の失敗で、残りがロックの poison だけを理由に落ちるのを避ける。
    JUDGE_TEST_LOCK
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
}

struct Fixture {
    root: SocketDirectory,
    cwd: PathBuf,
    temporary: PathBuf,
    denied: Vec<PathBuf>,
    runner: PathBuf,
    helper: PathBuf,
    probe: PathBuf,
}

impl Fixture {
    fn new() -> Result<Self, Box<dyn Error>> {
        let root = SocketDirectory::new()?;
        let target = std::env::current_exe()?
            .parent()
            .and_then(Path::parent)
            .ok_or("Missing target directory")?
            .to_owned();
        let fixture = Self {
            cwd: root.path().join("judge"),
            temporary: root.path().join("temporary"),
            denied: vec![
                root.path().join("repository"),
                root.path().join("workspace"),
            ],
            runner: target.join("examples/skill_eval_judge"),
            helper: fs::canonicalize(env!("CARGO_BIN_EXE_benitoiteh"))?,
            probe: target.join("sandbox-probe"),
            root,
        };
        if !fixture.runner.is_file() || !fixture.probe.is_file() {
            return Err("Build skill_eval_judge and sandbox-probe before testing".into());
        }
        for directory in [&fixture.cwd, &fixture.temporary]
            .into_iter()
            .chain(&fixture.denied)
        {
            fs::create_dir(directory)?;
        }
        fs::create_dir(fixture.root.path().join("home"))?;
        Ok(fixture)
    }

    fn command(&self, program: &Path, timeout: u64) -> Command {
        self.command_with_port(program, timeout, None)
    }

    fn command_with_port(&self, program: &Path, timeout: u64, port: Option<u16>) -> Command {
        let mut command = Command::new(&self.runner);
        command
            .env_clear()
            .env("HOME", self.root.path().join("home"))
            .env("XDG_CONFIG_HOME", self.root.path().join("config"))
            .env("XDG_STATE_HOME", self.root.path().join("state"))
            .env("TMPDIR", &self.temporary)
            .arg("--helper")
            .arg(&self.helper)
            .arg("--cwd")
            .arg(&self.cwd)
            .arg("--temp")
            .arg(&self.temporary)
            .arg("--timeout")
            .arg(timeout.to_string());
        for directory in &self.denied {
            command.arg("--deny-read").arg(directory);
        }
        if let Some(port) = port {
            command.arg("--allow-loopback-port").arg(port.to_string());
        }
        command.arg("--").arg(program);
        command
    }

    fn probe(&self, args: &[&str]) -> Result<(Output, Value), Box<dyn Error>> {
        result(self.command(&self.probe, 10).args(args))
    }

    fn restricted_command(&self, roots: &[PathBuf], programs: &[PathBuf]) -> Command {
        let base = self.command(&self.probe, 10);
        let mut command = Command::new(base.get_program());
        command
            .env_clear()
            .envs(
                base.get_envs()
                    .filter_map(|(name, value)| value.map(|value| (name, value))),
            )
            .args(base.get_args().take_while(|value| *value != "--"));
        for root in roots {
            command.arg("--read-root").arg(root);
        }
        for program in programs {
            command.arg("--allow-program").arg(program);
        }
        command.arg("--").arg(&self.probe);
        command
    }
}

fn result(command: &mut Command) -> Result<(Output, Value), Box<dyn Error>> {
    let output = command.output()?;
    let report: Value = serde_json::from_slice(&output.stdout).map_err(|error| {
        format!(
            "{error}: stdout={:?}, stderr={:?}",
            output.stdout, output.stderr
        )
    })?;
    assert_eq!(
        report.get("schema_version"),
        Some(&Value::from(1)),
        "{report}"
    );
    assert_eq!(
        report.get("success"),
        Some(&Value::Bool(output.status.success())),
        "{report}; runner status={:?}",
        output.status
    );
    assert!(
        output.stderr.is_empty(),
        "{report}; runner stderr={:?}",
        output.stderr
    );
    Ok((output, report))
}

fn field<'a>(report: &'a Value, name: &str) -> Result<&'a str, Box<dyn Error>> {
    report
        .get(name)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("Missing {name}: {report}").into())
}

// 作成時の関門: 実行器が二つの writable と繰り返す denied_read を取り違える退行を捕まえる。
// OS ごとの方針の細部は既存の sandbox_macos に任せ、この入口固有の組み立てだけを守る。
#[test]
fn judge_applies_both_writable_roots_and_every_denied_read() -> TestResult {
    let _serial = serial_judgment();
    for kind in ["cwd", "temporary", "outside"] {
        let f = Fixture::new()?;
        let directory = match kind {
            "cwd" => f.cwd.as_path(),
            "temporary" => f.temporary.as_path(),
            _ => f.root.path(),
        };
        let path = directory.join("result ; literal.txt");
        let (output, report) = result(f.command(&f.probe, 10).arg("write").arg(&path))?;
        if directory == f.root.path() {
            assert!(!output.status.success(), "{report}");
            assert_eq!(report.get("exit_code"), Some(&Value::from(10)), "{report}");
            assert!(!path.exists(), "{report}");
        } else {
            assert!(output.status.success(), "{report}");
            assert_eq!(fs::read_to_string(path)?, "sandbox-probe\n", "{report}");
        }
    }
    for index in 0..2 {
        let f = Fixture::new()?;
        let directory = f.denied.get(index).ok_or("Missing denied root")?;
        let path = directory.join("secret");
        fs::write(&path, "hidden")?;
        let (output, report) = result(f.command(&f.probe, 10).arg("read").arg(&path))?;
        assert!(!output.status.success(), "{report}");
        assert_eq!(report.get("exit_code"), Some(&Value::from(10)), "{report}");
        assert!(
            field(&report, "stderr")?.contains("PermissionDenied"),
            "{report}"
        );
    }
    let f = Fixture::new()?;
    let allowed = f.cwd.join("input");
    fs::write(&allowed, "readable")?;
    let (output, report) = result(f.command(&f.probe, 10).arg("read").arg(&allowed))?;
    assert!(output.status.success(), "{report}");
    Ok(())
}

// 作成時の関門: 実行器が allow_ip を誤って有効にする退行を、実際につながる宛先で捕まえる。
#[test]
fn judge_denies_loopback_communication() -> TestResult {
    let _serial = serial_judgment();
    let f = Fixture::new()?;
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let address = listener.local_addr()?;
    drop(TcpStream::connect(address)?);
    let (output, report) = f.probe(&["tcp-connect", &address.to_string()])?;
    assert!(!output.status.success(), "{report}");
    assert_eq!(report.get("exit_code"), Some(&Value::from(10)), "{report}");
    assert!(
        field(&report, "stderr")?.contains("PermissionDenied"),
        "{report}"
    );
    Ok(())
}

// 関門: ポートの例外が全 IP・待ち受け・Unix IPC を許す退行を、本物の接続と対にする。
#[test]
fn judge_loopback_exception_allows_only_the_selected_outbound_port() -> TestResult {
    let _serial = serial_judgment();
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let address = listener.local_addr()?;
    drop(TcpStream::connect(address)?);
    let f = Fixture::new()?;
    let (output, report) = result(
        f.command_with_port(&f.probe, 10, Some(address.port()))
            .args(["tcp-connect", &address.to_string()]),
    )?;
    assert!(output.status.success(), "{report}");
    let other = TcpListener::bind("127.0.0.1:0")?;
    let target = other.local_addr()?;
    drop(TcpStream::connect(target)?);
    let f = Fixture::new()?;
    let (output, report) = result(
        f.command_with_port(&f.probe, 10, Some(address.port()))
            .args(["tcp-connect", &target.to_string()]),
    )?;
    assert!(!output.status.success(), "{report}");
    assert!(
        field(&report, "stderr")?.contains("PermissionDenied"),
        "{report}"
    );
    // 外の結果は接続できる宛先かどうかの参考であり、別名がなければ時間切れもある。
    // 確かめるのは内の PermissionDenied である（実装プラン P5-34）。
    let alternate = std::net::SocketAddr::from(([127, 0, 0, 2], address.port()));
    match TcpStream::connect_timeout(&alternate, Duration::from_secs(1)) {
        Ok(stream) => drop(stream),
        Err(error)
            if matches!(
                error.kind(),
                std::io::ErrorKind::ConnectionRefused
                    | std::io::ErrorKind::AddrNotAvailable
                    | std::io::ErrorKind::NetworkUnreachable
                    | std::io::ErrorKind::HostUnreachable
                    | std::io::ErrorKind::TimedOut
                    | std::io::ErrorKind::WouldBlock
            ) => {}
        Err(error) => return Err(error.into()),
    }
    let f = Fixture::new()?;
    let (output, report) = result(
        f.command_with_port(&f.probe, 10, Some(address.port()))
            .args(["tcp-connect", &alternate.to_string()]),
    )?;
    assert!(!output.status.success(), "{report}");
    assert!(
        field(&report, "stderr")?.contains("PermissionDenied"),
        "{report}"
    );
    let f = Fixture::new()?;
    let socket = f.root.path().join("outside.sock");
    let _unix = UnixListener::bind(&socket)?;
    let (output, report) = result(
        f.command_with_port(&f.probe, 10, Some(address.port()))
            .arg("unix-connect")
            .arg(&socket),
    )?;
    assert!(!output.status.success(), "{report}");
    assert!(
        field(&report, "stderr")?.contains("PermissionDenied"),
        "{report}"
    );
    // 空いているポートを使い、アドレス使用中による失敗と区別する。
    let free = TcpListener::bind("127.0.0.1:0")?.local_addr()?;
    let f = Fixture::new()?;
    let program = Path::new("/usr/bin/perl");
    let source = format!(
        "use Socket; socket(S, PF_INET, SOCK_STREAM, 0) or die $!; bind(S, sockaddr_in({}, inet_aton('127.0.0.1'))) or die $!; listen(S, 1) or die $!;",
        free.port()
    );
    let (output, report) = result(
        f.command_with_port(program, 10, Some(free.port()))
            .args(["-e", &source]),
    )?;
    assert!(!output.status.success(), "{report}");
    let stderr = field(&report, "stderr")?;
    assert!(
        stderr.contains("Operation not permitted") || stderr.contains("Permission denied"),
        "{report}"
    );
    Ok(())
}

// 関門: 0 と範囲外を別の許可に丸めず、起動前に拒む。
#[test]
fn judge_rejects_invalid_loopback_ports() -> TestResult {
    let _serial = serial_judgment();
    for ports in [
        vec!["0"],
        vec!["65536"],
        vec!["-1"],
        vec!["invalid"],
        vec!["12345", "12346"],
    ] {
        let f = Fixture::new()?;
        // 不正な引数は -- の前に置く必要があるので、入口から組み直す。
        let base = f.command(&f.probe, 10);
        let mut command = Command::new(base.get_program());
        command
            .env_clear()
            .envs(
                base.get_envs()
                    .filter_map(|(name, value)| value.map(|value| (name, value))),
            )
            .args(base.get_args().take_while(|value| *value != "--"));
        for port in ports {
            command.args(["--allow-loopback-port", port]);
        }
        command.arg("--").arg(&f.probe).arg("environment");
        let (output, report) = result(&mut command)?;
        assert!(!output.status.success(), "{report}");
        assert_eq!(report.get("exit_code"), Some(&Value::Null), "{report}");
    }
    Ok(())
}

// 関門: 起動に必要な / の literal の欠落と、subpath で全体を許す退行を対で捕まえる。
#[test]
fn judge_read_roots_allow_startup_and_deny_home_files() -> TestResult {
    let _serial = serial_judgment();
    for inside in [true, false] {
        let f = Fixture::new()?;
        let home = f.root.path().join("home");
        let secret = home.join("ordinary-file");
        let allowed = f.cwd.join("input");
        fs::write(&secret, b"private")?;
        fs::write(&allowed, b"permitted")?;
        let mut roots = vec![f.cwd.clone(), f.temporary.clone()];
        roots.extend(
            [
                "/System",
                "/Library",
                "/usr",
                "/bin",
                "/sbin",
                "/private/etc",
                "/dev",
            ]
            .into_iter()
            .map(PathBuf::from)
            .filter(|path| path.is_dir()),
        );
        let (output, report) = result(
            f.restricted_command(&roots, std::slice::from_ref(&f.probe))
                .arg("read")
                .arg(if inside { &allowed } else { &secret }),
        )?;
        assert_eq!(output.status.success(), inside, "{report}");
        if !inside {
            assert_eq!(report.get("exit_code"), Some(&Value::from(10)), "{report}");
            assert!(
                field(&report, "stderr")?.contains("PermissionDenied"),
                "{report}"
            );
        }
    }
    Ok(())
}

// 関門: HOME を包含する根と、All のままのプログラムの列挙を、引数の入口で拒む。
#[test]
fn judge_rejects_broad_read_roots_and_programs_without_roots() -> TestResult {
    let _serial = serial_judgment();
    for kind in ["filesystem", "home", "ancestor", "program-only"] {
        let f = Fixture::new()?;
        let roots = match kind {
            "filesystem" => vec![PathBuf::from("/")],
            "home" => vec![f.root.path().join("home")],
            "ancestor" => vec![f.root.path().to_owned()],
            _ => vec![],
        };
        let (output, report) = result(
            f.restricted_command(&roots, std::slice::from_ref(&f.probe))
                .arg("environment"),
        )?;
        assert!(!output.status.success(), "{report}");
        assert_eq!(report.get("exit_code"), Some(&Value::Null), "{report}");
        assert!(
            field(&report, "error")?.contains(if kind == "program-only" {
                "--allow-program requires --read-root"
            } else {
                "Read roots must not contain HOME"
            }),
            "{report}"
        );
    }
    Ok(())
}

// 作成時の関門: 親のキー・設定・Python の探索先と包みの PATH の漏れ、
// シェル文字列への再構成、書ける場所での実行の拒否をこの入口で捕まえる。
#[test]
fn judge_uses_private_environment_and_preserves_literal_arguments() -> TestResult {
    let _serial = serial_judgment();
    let f = Fixture::new()?;
    let (output, report) = result(
        f.command(&f.probe, 10)
            .arg("environment")
            .current_dir(&f.cwd)
            .env("OPENAI_API_KEY", "dummy-secret")
            .env("PYTHONPATH", "/original/python")
            .env("PYTHONHOME", "/original/python-home")
            .env("SKILL_EVAL_WORKSPACE", "/original/workspace")
            .env("OLDPWD", "/original/repository")
            .env("PATH", "/original/wrappers")
            .env("SKILL_EVAL_JUDGE_RUN", "parent-marker-must-not-be-reused")
            .env("BASH_ENV", "/original/bash-settings"),
    )?;
    assert!(output.status.success(), "{report}");
    assert_eq!(
        report.get("possibly_remaining"),
        Some(&Value::Bool(false)),
        "{report}"
    );
    assert_eq!(
        report.get("stop_required"),
        Some(&Value::Bool(false)),
        "{report}"
    );
    let environment = field(&report, "stdout")?;
    for forbidden in [
        "dummy-secret",
        "/original/",
        "PYTHONPATH=",
        "PYTHONHOME=",
        "BASH_ENV=",
        "parent-marker-must-not-be-reused",
    ] {
        assert!(!environment.contains(forbidden), "{report}");
    }
    let marker = environment
        .lines()
        .find_map(|line| line.strip_prefix("SKILL_EVAL_JUDGE_RUN="))
        .ok_or("Missing judge run marker")?;
    assert_eq!(marker.len(), 32, "{report}");
    assert!(
        marker
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)),
        "{report}"
    );
    for expected in [
        format!("HOME={}/home", f.temporary.display()),
        format!("XDG_CONFIG_HOME={}/config", f.temporary.display()),
        format!("XDG_STATE_HOME={}/state", f.temporary.display()),
        format!("TMPDIR={}", f.temporary.display()),
        format!("PWD={}", f.cwd.display()),
        "PYTHONNOUSERSITE=1".into(),
        "PYTHONDONTWRITEBYTECODE=1".into(),
        "LC_ALL=C".into(),
        "PATH=/usr/bin:/bin".into(),
    ] {
        assert!(environment.lines().any(|line| line == expected), "{report}");
    }
    drop(f);
    let f = Fixture::new()?;
    let script = f.cwd.join("literal arguments.sh");
    fs::write(&script, "#!/bin/sh\nprintf '%s\\n' \"$@\"\n")?;
    fs::set_permissions(&script, fs::Permissions::from_mode(0o700))?;
    let arguments = [
        "space value",
        "$(touch injected)",
        "; touch injected",
        "--helper",
        "",
    ];
    let (output, report) = result(f.command(&script, 10).args(arguments))?;
    assert!(output.status.success(), "{report}");
    assert_eq!(
        field(&report, "stdout")?,
        "space value\n$(touch injected)\n; touch injected\n--helper\n\n",
        "{report}"
    );
    assert!(!f.cwd.join("injected").exists(), "{report}");
    Ok(())
}

// 作成時の関門: 時間切れで code=0 が返っても合格させず、子が停止要求を受けて終わったことを守る。
#[test]
fn judge_reports_timeout_even_when_child_handles_term_and_exits_zero() -> TestResult {
    let _serial = serial_judgment();
    let f = Fixture::new()?;
    let marker = f.cwd.join("signal");
    let start = Instant::now();
    let (output, report) = result(
        f.command(&f.probe, 1)
            .args(["signal", "handle"])
            .arg(&marker),
    )?;
    assert!(!output.status.success(), "{report}");
    assert_eq!(report.get("exit_code"), Some(&Value::from(0)), "{report}");
    assert_eq!(
        report.get("timed_out"),
        Some(&Value::Bool(true)),
        "{report}"
    );
    assert_eq!(field(&report, "end")?, "timed_out", "{report}");
    assert_eq!(fs::read_to_string(marker)?, "SIGTERM", "{report}");
    assert!(start.elapsed().as_secs() < 20, "{report}");
    assert_eq!(
        report.get("possibly_remaining"),
        Some(&Value::Bool(false)),
        "{report}"
    );
    Ok(())
}

// 作成時の関門: 上限ちょうどは合格し、超過は読み捨てて子を詰まらせず失敗へ変換する契約。
#[test]
fn judge_caps_each_stream_without_blocking_the_child() -> TestResult {
    let _serial = serial_judgment();
    let limit = 1024_usize * 1024;
    let stderr_footer = "query-output: ok\n".len();
    for (stdout_size, stderr_size, stdout_truncated, stderr_truncated) in [
        (limit, limit.saturating_sub(stderr_footer), false, false),
        (5 * limit, 0, true, false),
        (0, 5 * limit, false, true),
        (5 * limit, 5 * limit, true, true),
    ] {
        let f = Fixture::new()?;
        let started = Instant::now();
        let (output, report) = f.probe(&[
            "query-output",
            &stdout_size.to_string(),
            &stderr_size.to_string(),
            "0",
        ])?;
        assert_eq!(
            output.status.success(),
            !stdout_truncated && !stderr_truncated,
            "{report}"
        );
        assert_eq!(report.get("exit_code"), Some(&Value::from(0)), "{report}");
        assert_eq!(
            report.get("timed_out"),
            Some(&Value::Bool(false)),
            "{report}"
        );
        assert_eq!(field(&report, "end")?, "exited", "{report}");
        assert_eq!(
            report.get("possibly_remaining"),
            Some(&Value::Bool(false)),
            "{report}"
        );
        assert!(started.elapsed().as_secs() < 10, "{report}");
        assert_eq!(
            report.get("stdout_truncated"),
            Some(&Value::Bool(stdout_truncated)),
            "{report}"
        );
        assert_eq!(
            report.get("stderr_truncated"),
            Some(&Value::Bool(stderr_truncated)),
            "{report}"
        );
        assert_eq!(
            field(&report, "stdout")?.len(),
            stdout_size.min(limit),
            "{report}"
        );
        assert_eq!(
            field(&report, "stderr")?.len(),
            stderr_size.saturating_add(stderr_footer).min(limit),
            "{report}"
        );
    }
    Ok(())
}

// 作成時の関門: 後始末の結果を捨てて code=0 だけで合格させる、実行器固有の退行を捕まえる。
#[test]
fn judge_requires_evaluation_to_stop_when_descendants_may_remain() -> TestResult {
    let _serial = serial_judgment();
    for (session, remaining) in [("group", false), ("session", true)] {
        let f = Fixture::new()?;
        let marker = f.cwd.join(session);
        let (output, report) = result(
            f.command(&f.probe, 10)
                .args(["descendant", session, "open"])
                .arg(&marker),
        )?;
        assert_eq!(report.get("exit_code"), Some(&Value::from(0)), "{report}");
        assert_eq!(output.status.success(), !remaining, "{report}");
        assert_eq!(
            report.get("possibly_remaining"),
            Some(&Value::Bool(remaining)),
            "{report}"
        );
        assert_eq!(
            report.get("stop_required"),
            Some(&Value::Bool(remaining)),
            "{report}"
        );
        assert!(fs::read_to_string(marker)?.parse::<i32>()? > 0, "{report}");
    }
    // 既存の探査の子は 30 秒で自ら終わる。使い回されうる PID へ外から合図を送らない。
    Ok(())
}

// 作成時の関門: 準備の失敗でサンドボックスなしへ進むこと、非零や合図を合格にすることを入口で防ぐ。
#[test]
fn judge_fails_closed_on_setup_errors_and_preserves_child_status() -> TestResult {
    let _serial = serial_judgment();
    let mut f = Fixture::new()?;
    let marker = f.cwd.join("must-not-run");
    let helper = f.helper.clone();
    f.helper = f.root.path().join("missing-helper");
    let (output, report) = result(f.command(&f.probe, 10).arg("write").arg(&marker))?;
    assert!(!output.status.success(), "{report}");
    assert_eq!(field(&report, "end")?, "error", "{report}");
    assert_eq!(report.get("exit_code"), Some(&Value::Null), "{report}");
    assert!(!marker.exists(), "{report}");
    f.helper = helper;
    f.denied.push(f.cwd.clone());
    let (output, report) = result(f.command(&f.probe, 10).arg("write").arg(&marker))?;
    assert!(!output.status.success(), "{report}");
    assert_eq!(field(&report, "end")?, "error", "{report}");
    assert!(!marker.exists(), "{report}");
    f.denied.pop();
    drop(f);
    for code in [0, 7, 125] {
        let f = Fixture::new()?;
        let (output, report) = f.probe(&["exit", &code.to_string()])?;
        assert_eq!(
            report.get("exit_code"),
            Some(&Value::from(code)),
            "{report}"
        );
        assert_eq!(output.status.success(), code == 0, "{report}");
        assert_eq!(field(&report, "end")?, "exited", "{report}");
    }
    let f = Fixture::new()?;
    let (output, report) = result(
        f.command(&fs::canonicalize("/bin/sh")?, 10)
            .args(["-c", "kill -TERM $$"]),
    )?;
    assert!(!output.status.success(), "{report}");
    assert_eq!(report.get("exit_code"), Some(&Value::Null), "{report}");
    assert_eq!(report.get("signal"), Some(&Value::from(15)), "{report}");
    Ok(())
}

// 関門: 引数に書かなくても、通常モードの秘密と Keychains が判定から読めない契約。
// 偽の HOME と設定だけを作り、本物の利用者の秘密は開かない（レビュー 中-1）。
#[test]
fn judge_denies_normal_private_paths_and_keychains() -> TestResult {
    let _serial = serial_judgment();
    for relative in [
        "home/.ssh/id_test",
        "home/.gnupg/private",
        "home/.aws/credentials",
        "home/Library/Keychains/login.keychain-db",
        "config/benitoiteh/config.toml",
        "state/benitoiteh/auth/chatgpt.json",
        "state/benitoiteh/sessions/record",
    ] {
        let f = Fixture::new()?;
        let secret = f.root.path().join(relative);
        fs::create_dir_all(secret.parent().ok_or("Missing private directory")?)?;
        fs::write(&secret, "FAKE-PRIVATE-DATA")?;
        let (output, report) = result(f.command(&f.probe, 10).arg("read").arg(&secret))?;
        assert!(!output.status.success(), "{relative}: {report}");
        assert_eq!(report.get("exit_code"), Some(&Value::from(10)), "{report}");
        assert!(
            field(&report, "stderr")?.contains("PermissionDenied"),
            "{report}"
        );
    }
    Ok(())
}

fn data_alias(path: &Path) -> Result<PathBuf, Box<dyn Error>> {
    let alias = Path::new("/System/Volumes/Data").join(path.strip_prefix("/")?);
    if !alias.exists() {
        return Err(format!("Missing macOS firmlink alias: {}", alias.display()).into());
    }
    Ok(alias)
}

// 関門: realpath では残る firmlink の別名で、読み取りの拒否を抜けられない契約。
// 書ける根の別名は、意図しない範囲を許さないために拒む（レビュー2 低-B）。
#[test]
fn judge_resolves_denied_firmlinks_and_rejects_writable_firmlinks() -> TestResult {
    let _serial = serial_judgment();
    let mut f = Fixture::new()?;
    let secret = f
        .denied
        .first()
        .ok_or("Missing denied root")?
        .join("secret");
    fs::write(&secret, "hidden")?;
    f.denied = f
        .denied
        .iter()
        .map(|path| data_alias(path))
        .collect::<Result<_, _>>()?;
    let (output, report) = result(f.command(&f.probe, 10).arg("read").arg(&secret))?;
    assert!(!output.status.success(), "{report}");
    assert_eq!(report.get("exit_code"), Some(&Value::from(10)), "{report}");
    drop(f);
    let mut f = Fixture::new()?;
    let target = f.cwd.join("written");
    f.cwd = data_alias(&f.cwd)?;
    f.temporary = data_alias(&f.temporary)?;
    f.helper = data_alias(&f.helper)?;
    let (output, report) = result(f.command(&f.probe, 10).arg("write").arg(&target))?;
    assert!(!output.status.success(), "{report}");
    assert_eq!(field(&report, "end")?, "error", "{report}");
    assert!(
        field(&report, "error")?.contains("kernel paths"),
        "{report}"
    );
    assert!(!target.exists(), "{report}");
    Ok(())
}

// 関門: 読ませない根を含む writable や、HOME・実行器を含む根を起動前に拒む。
// noop を使い、OS による別の拒否で通ってしまう否定テストを避ける（レビュー 中-3）。
#[test]
fn judge_rejects_overlapping_and_overbroad_writable_roots() -> TestResult {
    let _serial = serial_judgment();
    for case in [
        "cwd-contains-denied",
        "temp-contains-denied",
        "denied-contains-cwd",
        "denied-contains-temp",
        "root",
        "home",
        "home-parent",
        "helper-parent",
        "runner-parent",
    ] {
        let mut f = Fixture::new()?;
        match case {
            "cwd-contains-denied" | "temp-contains-denied" => {
                let parent = if case == "cwd-contains-denied" {
                    &f.cwd
                } else {
                    &f.temporary
                };
                let denied = parent.join("hidden");
                fs::create_dir(&denied)?;
                f.denied.push(denied);
            }
            "denied-contains-cwd" => f.denied.push(f.cwd.clone()),
            "denied-contains-temp" => f.denied.push(f.temporary.clone()),
            "root" => f.cwd = "/".into(),
            "home" => f.cwd = f.root.path().join("home"),
            "home-parent" => f.cwd = f.root.path().into(),
            "helper-parent" => f.cwd = f.helper.parent().ok_or("Missing helper parent")?.into(),
            "runner-parent" => f.cwd = f.runner.parent().ok_or("Missing runner parent")?.into(),
            _ => return Err("Unknown test case".into()),
        }
        let (output, report) = f.probe(&["noop"])?;
        assert!(!output.status.success(), "{case}: {report}");
        assert_eq!(field(&report, "end")?, "error", "{case}: {report}");
        assert!(
            field(&report, "error")?.contains("Writable roots overlap"),
            "{case}: {report}"
        );
    }
    Ok(())
}

// 関門: writable の末尾のリンクは拒み、上位の /tmp の別名は保つ（レビュー 中-3）。
#[test]
fn judge_rejects_final_writable_symlinks_but_accepts_parent_aliases() -> TestResult {
    let _serial = serial_judgment();
    for temporary in [false, true] {
        let mut f = Fixture::new()?;
        let link = f.root.path().join("linked-root");
        let target = if temporary { &f.temporary } else { &f.cwd };
        symlink(target, &link)?;
        if temporary {
            f.temporary = link;
        } else {
            f.cwd = link;
        }
        let (output, report) = f.probe(&["noop"])?;
        assert!(!output.status.success(), "{report}");
        assert!(
            field(&report, "error")?.contains("symbolic link"),
            "{report}"
        );
    }
    let mut f = Fixture::new()?;
    f.cwd = Path::new("/tmp").join(f.cwd.strip_prefix("/private/tmp")?);
    f.temporary = Path::new("/tmp").join(f.temporary.strip_prefix("/private/tmp")?);
    let (output, report) = f.probe(&["noop"])?;
    assert!(output.status.success(), "{report}");
    Ok(())
}

// 関門: パイプの EOF だけでは見つからない孤児を、回収後の一覧で検出して止める（レビュー 中-4）。
#[test]
fn judge_detects_marked_probe_descendants_with_closed_output() -> TestResult {
    let _serial = serial_judgment();
    let f = Fixture::new()?;
    let (output, report) = result(
        f.command(&f.probe, 10)
            .args(["descendant", "marked-session", "closed"])
            .arg(f.cwd.join("orphan")),
    )?;
    assert!(!output.status.success(), "{report}");
    assert_eq!(report.get("exit_code"), Some(&Value::from(0)), "{report}");
    assert_eq!(field(&report, "end")?, "exited", "{report}");
    assert_eq!(
        report.get("possibly_remaining"),
        Some(&Value::Bool(true)),
        "{report}"
    );
    assert_eq!(
        report.get("stop_required"),
        Some(&Value::Bool(true)),
        "{report}"
    );
    Ok(())
}

// 関門: 環境が ps -E に出ない Apple の子も、書ける場所に残した cwd で検出して停止する。
#[test]
fn judge_detects_unmarked_apple_shell_descendants_by_writable_cwd() -> TestResult {
    let _serial = serial_judgment();
    let f = Fixture::new()?;
    let (output, report) = result(
        f.command(&f.probe, 10)
            .args(["descendant", "apple-session", "closed"])
            .arg(f.cwd.join("apple-orphan")),
    )?;
    assert!(!output.status.success(), "{report}");
    assert_eq!(report.get("exit_code"), Some(&Value::from(0)), "{report}");
    assert_eq!(field(&report, "end")?, "exited", "{report}");
    assert_eq!(
        report.get("possibly_remaining"),
        Some(&Value::Bool(true)),
        "{report}"
    );
    assert_eq!(
        report.get("stop_required"),
        Some(&Value::Bool(true)),
        "{report}"
    );
    Ok(())
}

// 関門: 実行器自身の CPU 上限によって後始末と JSON が飛ぶ退行を、同時の書き手で再現する。
#[test]
fn judge_returns_timeout_json_under_multiple_continuous_writers() -> TestResult {
    let _serial = serial_judgment();
    let f = Fixture::new()?;
    let (output, report) = result(f.command(Path::new("/bin/sh"), 6).args([
        "-c",
        "for i in 1 2 3 4; do /usr/bin/yes & /usr/bin/yes >&2 & done; wait",
    ]))?;
    assert!(!output.status.success(), "{report}");
    assert_eq!(field(&report, "end")?, "timed_out", "{report}");
    assert_eq!(
        report.get("timed_out"),
        Some(&Value::Bool(true)),
        "{report}"
    );
    assert_eq!(
        report.get("possibly_remaining"),
        Some(&Value::Bool(false)),
        "{report}"
    );
    assert_eq!(
        report.get("stop_required"),
        Some(&Value::Bool(false)),
        "{report}"
    );
    assert_eq!(
        report.get("stdout_truncated"),
        Some(&Value::Bool(true)),
        "{report}"
    );
    assert_eq!(
        report.get("stderr_truncated"),
        Some(&Value::Bool(true)),
        "{report}"
    );
    Ok(())
}

// 関門: 全スレッドの CPU 時間を壁時計と同じ上限にして、正しい計算を落とす退行を捕まえる。
#[test]
fn judge_accepts_cpu_work_on_four_threads_within_wall_timeout() -> TestResult {
    let _serial = serial_judgment();
    let f = Fixture::new()?;
    let (output, report) = result(f.command(&f.probe, 5).arg("cpu-four-threads"))?;
    assert!(output.status.success(), "{report}");
    assert_eq!(field(&report, "end")?, "exited", "{report}");
    assert_eq!(
        report.get("possibly_remaining"),
        Some(&Value::Bool(false)),
        "{report}"
    );
    Ok(())
}

// 関門: 異なる資源の制限を一般の終了失敗へ潰さず、呼び出し元が理由を区別できる。
#[test]
fn judge_distinguishes_cpu_and_file_size_limit_signals() -> TestResult {
    let _serial = serial_judgment();
    for (signal, number, reason) in [
        ("XCPU", 24, "CPU time limit (SIGXCPU)"),
        ("XFSZ", 25, "file size limit (SIGXFSZ)"),
    ] {
        let f = Fixture::new()?;
        let (output, report) = result(
            f.command(Path::new("/bin/sh"), 10)
                .args(["-c", &format!("kill -{signal} $$")]),
        )?;
        assert!(!output.status.success(), "{report}");
        assert_eq!(report.get("signal"), Some(&Value::from(number)), "{report}");
        assert!(field(&report, "error")?.contains(reason), "{report}");
    }
    Ok(())
}

// 関門: 通常の方針の修正だけでなく、実行器の環境を持つ親にも process-info の拒否が届く。
#[test]
fn judge_denies_parent_and_unrelated_process_environments() -> TestResult {
    let _serial = serial_judgment();
    let f = Fixture::new()?;
    let probe = macos_process_info::ProcessInfoProbe::new(f.root.path())?;
    let mut command = f.command(probe.python(), 10);
    command.args(probe.args("denied"));
    let (output, report) = result(&mut probe.wrap(&command))?;
    assert!(output.status.success(), "{report}");
    assert_eq!(
        field(&report, "stdout")?,
        "process-info: denied\n",
        "{report}"
    );
    Ok(())
}

// 関門: 正規化で消える '..' と中間のリンクで、利用者が指定した根と違う範囲を許さない。
#[test]
fn judge_rejects_parent_components_and_interior_writable_symlinks() -> TestResult {
    let _serial = serial_judgment();
    for (temporary, linked) in [(false, false), (true, false), (false, true), (true, true)] {
        let mut f = Fixture::new()?;
        let root = if temporary { &f.temporary } else { &f.cwd };
        let input = if linked {
            let alias = f.root.path().join("alias");
            symlink(f.root.path(), &alias)?;
            alias.join(root.file_name().ok_or("Missing root name")?)
        } else {
            root.join("..")
                .join(root.file_name().ok_or("Missing root name")?)
        };
        if temporary {
            f.temporary = input;
        } else {
            f.cwd = input;
        }
        let (output, report) = f.probe(&["noop"])?;
        assert!(!output.status.success(), "{report}");
        assert_eq!(field(&report, "end")?, "error", "{report}");
        assert!(
            field(&report, "error")?.contains("kernel paths"),
            "{report}"
        );
    }
    Ok(())
}

// 関門: CPU とファイルの上限を子孫へ継がせ、子が上限を広げられない契約（レビュー 中-4）。
#[test]
fn judge_children_inherit_cpu_and_file_size_hard_limits() -> TestResult {
    let _serial = serial_judgment();
    let f = Fixture::new()?;
    let mut script = "/bin/sh -c 'ulimit -Ht; ulimit -Hf'".to_owned();
    if !rustix::process::geteuid().is_root() {
        script.push_str("; if (ulimit -Ht unlimited) 2>/dev/null; then exit 41; fi; if (ulimit -Hf unlimited) 2>/dev/null; then exit 42; fi");
    }
    let (output, report) = result(f.command(Path::new("/bin/sh"), 10).args(["-c", &script]))?;
    assert!(output.status.success(), "{report}");
    // この macOS 専用のテストで使う同梱の sh（bash 3.2）は 1024 バイト単位。
    let cpu_limit = 10 * u64::try_from(std::thread::available_parallelism()?.get())? + 5;
    assert_eq!(
        field(&report, "stdout")?,
        format!("{cpu_limit}\n65536\n"),
        "{report}"
    );
    drop(f);
    let f = Fixture::new()?;
    let target = f.cwd.join("large");
    let (output, report) = result(
        f.command(Path::new("/bin/dd"), 10)
            .arg("if=/dev/zero")
            .arg(format!("of={}", target.display()))
            .args(["bs=1048576", "count=65"]),
    )?;
    assert!(!output.status.success(), "{report}");
    assert_eq!(field(&report, "end")?, "exited", "{report}");
    assert_eq!(fs::metadata(target)?.len(), 64 * 1024 * 1024, "{report}");
    Ok(())
}

// 関門: 親から渡された fd=7 を補助まで素通しにしない（レビュー 低-1）。
#[test]
fn judge_rejects_inherited_nonstandard_descriptors() -> TestResult {
    let _serial = serial_judgment();
    let f = Fixture::new()?;
    let outside = f.root.path().join("outside");
    fs::write(&outside, "inherited descriptor")?;
    let command = f.command(&f.probe, 10);
    let mut wrapper = Command::new("/bin/sh");
    wrapper
        .env_clear()
        .envs(
            command
                .get_envs()
                .filter_map(|(name, value)| value.map(|v| (name, v))),
        )
        .args(["-c", "exec 7<\"$1\"; shift; exec \"$@\"", "inherited-fd"])
        .arg(&outside)
        .arg(command.get_program())
        .args(command.get_args())
        .arg("noop");
    let (output, report) = result(&mut wrapper)?;
    assert!(!output.status.success(), "{report}");
    assert_eq!(field(&report, "end")?, "error", "{report}");
    assert!(
        field(&report, "error")?.contains("file descriptors"),
        "{report}"
    );
    Ok(())
}

// 関門: true の取り違えを成功にせず、引数で明示された不存在の拒否先も黙って除かない（レビュー 低-2）。
#[test]
fn judge_rejects_wrong_helpers_and_missing_explicit_denials() -> TestResult {
    let _serial = serial_judgment();
    for wrong_helper in [true, false] {
        let mut f = Fixture::new()?;
        if wrong_helper {
            f.helper = "/usr/bin/true".into();
        } else {
            f.denied.push(f.root.path().join("missing-denial"));
        }
        let (output, report) = f.probe(&["noop"])?;
        assert!(!output.status.success(), "{report}");
        assert_eq!(field(&report, "end")?, "error", "{report}");
        if wrong_helper {
            assert!(field(&report, "error")?.contains("helper"), "{report}");
        }
    }
    Ok(())
}

// 関門: sort の空入力でも出る sysconf の警告を許容せず、標準の道具が動くことを守る。
// 名前の解決・書式の問い合わせも行う sysctl の CLI では、個別の名前の許可を検査できない。
// 共通の方針の範囲と二名を除いた対照は、製品の方針の単体テストと sandbox_macos が持つ。
#[test]
fn judge_runs_sort_without_warnings() -> TestResult {
    let _serial = serial_judgment();
    let sort = fs::canonicalize("/usr/bin/sort")?;
    for (input, expected) in [("", ""), ("z.txt\na space.txt\n", "a space.txt\nz.txt\n")] {
        let f = Fixture::new()?;
        let path = f.cwd.join("input.txt");
        fs::write(&path, input)?;
        let (output, report) = result(f.command(&sort, 10).arg(&path))?;
        assert!(output.status.success(), "{report}");
        assert_eq!(report.get("exit_code"), Some(&Value::from(0)), "{report}");
        assert_eq!(field(&report, "stdout")?, expected, "{report}");
        assert_eq!(field(&report, "stderr")?, "", "{report}");
    }
    Ok(())
}

// 関門: 前の判定が残した設定を読み込む前に、使い回した一時領域を拒む（レビュー 低-3）。
#[test]
fn judge_rejects_nonempty_temporary_directories() -> TestResult {
    let _serial = serial_judgment();
    let f = Fixture::new()?;
    fs::write(f.temporary.join("previous-settings"), "untrusted")?;
    let (output, report) = f.probe(&["noop"])?;
    assert!(!output.status.success(), "{report}");
    assert_eq!(field(&report, "end")?, "error", "{report}");
    assert!(
        field(&report, "error")?.contains("must be empty"),
        "{report}"
    );
    assert!(!f.temporary.join("home").exists(), "{report}");
    Ok(())
}

// 関門: 結果と診断の書き込みの両方に失敗しても、診断で panic しない（レビュー 情報）。
#[test]
fn judge_does_not_panic_when_output_and_diagnostics_are_unwritable() -> TestResult {
    let _serial = serial_judgment();
    let f = Fixture::new()?;
    let command = f.command(&f.probe, 10);
    let readonly = f.root.path().join("readonly-output");
    fs::write(&readonly, "cannot write through read-only descriptors")?;
    let output = Command::new("/bin/sh")
        .env_clear()
        .envs(
            command
                .get_envs()
                .filter_map(|(name, value)| value.map(|v| (name, v))),
        )
        // 閉じた標準記述子は Rust の起動時に補われうるので、開いた読み取り専用記述子にする。
        .args([
            "-c",
            "exec 1<\"$1\" 2<\"$1\"; shift; exec \"$@\"",
            "unwritable-output",
        ])
        .arg(&readonly)
        .arg(&f.runner)
        .output()?;
    // この検査では JSON 自体を書けないため、report の代わりに生の Output を残す。
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert!(output.stdout.is_empty(), "{output:?}");
    assert!(output.stderr.is_empty(), "{output:?}");
    Ok(())
}
