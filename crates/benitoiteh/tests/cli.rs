//! 実行ファイルの出力と終了状態を確かめる（設計書 05-01「コマンドラインの形」「終了状態」）。

use std::error::Error;
use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};

// 子プロセスごとに独立した作業場所を作る（実装プラン 00-common「実装の規約の補足」）。
struct TestDirectory(PathBuf);

impl TestDirectory {
    fn new(name: &str) -> Result<Self, Box<dyn Error>> {
        let parent = std::env::temp_dir().join("benitoiteh-cli-tests");
        fs::create_dir_all(&parent)?;
        let nonce = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
        let path = parent.join(format!("{name}-{}-{nonce}", std::process::id()));
        fs::create_dir(&path)?;
        Ok(Self(path))
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        if let Err(error) = fs::remove_dir_all(&self.0) {
            eprintln!(
                "Could not remove test directory {}: {error}",
                self.0.display()
            );
        }
    }
}

fn run(name: &str, args: &[&str]) -> Result<Output, Box<dyn Error>> {
    let directory = TestDirectory::new(name)?;
    Ok(Command::new(env!("CARGO_BIN_EXE_benitoiteh"))
        .args(args)
        .current_dir(&directory.0)
        .output()?)
}

#[test]
fn version_reports_package_version() -> Result<(), Box<dyn Error>> {
    let output = run("version", &["--version"])?;
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(
        String::from_utf8(output.stdout)?,
        format!("benitoiteh {}\n", env!("CARGO_PKG_VERSION"))
    );
    assert!(output.stderr.is_empty());
    Ok(())
}

#[test]
fn help_lists_command_forms() -> Result<(), Box<dyn Error>> {
    let output = run("help", &["--help"])?;
    assert_eq!(output.status.code(), Some(0));
    assert!(output.stderr.is_empty());
    let help = String::from_utf8(output.stdout)?;
    assert!(help.starts_with("Usage:\n"));
    for command in [
        "benitoiteh [options] [request]",
        "benitoiteh exec [options] <request>",
        "benitoiteh doctor [--project <dir>] [--json]",
        "benitoiteh sessions [--project <dir>] [--json]",
        "benitoiteh --help | --version | --licenses",
    ] {
        assert!(help.lines().any(|line| line.trim() == command), "{help}");
    }
    for option in [
        "--mode restricted|normal",
        "--project <dir>",
        "--provider <name>",
        "--model <name>",
        "--effort <value>",
        "--max-turns <n>",
        "--max-check-fix <n>",
        "--max-run-tokens <n>",
        "--sandbox read-only|workspace-write|danger-full-access",
        "--approval untrusted|on-request|never",
        "--no-sandbox",
        "--resume <ID>",
        "--continue",
        "--json",
        "--output-last-message <file>",
    ] {
        assert!(help.lines().any(|line| line.trim() == option), "{help}");
    }
    let exec_help = run("exec-help", &["exec", "--help"])?;
    assert_eq!(exec_help.status.code(), Some(0));
    assert_eq!(exec_help.stdout, help.as_bytes());
    assert!(exec_help.stderr.is_empty());
    Ok(())
}

#[test]
fn other_arguments_are_usage_errors() -> Result<(), Box<dyn Error>> {
    let cases: &[&[&str]] = &[
        &["--unknown"],
        &["exec"],
        &["sessions", "--unknown"],
        &["--licenses"],
        &["--help", "--unknown"],
        &["--version", "--help"],
    ];
    for args in cases {
        let output = run("usage", args)?;
        assert_eq!(output.status.code(), Some(2), "{args:?}");
        assert!(output.stdout.is_empty(), "{args:?}");
        let error = String::from_utf8(output.stderr)?;
        assert_eq!(error.lines().count(), 1, "{args:?}: {error}");
        assert!(error.starts_with("benitoiteh: "), "{error}");
        assert!(error.ends_with('\n'), "{error}");
    }
    Ok(())
}

// 端末でないときは設定とセッションに触る前に拒む。誤った引数だけの検査では届かない。
#[test]
fn interactive_requires_terminals_before_creating_data_and_rejects_exec_options()
-> Result<(), Box<dyn Error>> {
    for args in [
        vec![],
        vec!["request"],
        vec!["--json"],
        vec!["--output-last-message", "result"],
        vec!["-"],
        vec!["--approval", "on-request"],
        vec!["--resume", "session-id"],
        vec!["--continue"],
    ] {
        let directory = TestDirectory::new("interactive")?;
        let output = Command::new(env!("CARGO_BIN_EXE_benitoiteh"))
            .args(&args)
            .env("HOME", directory.0.join("missing-home"))
            .env("XDG_CONFIG_HOME", directory.0.join("missing-config"))
            .env("XDG_DATA_HOME", directory.0.join("missing-data"))
            .current_dir(&directory.0)
            .output()?;
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stdout.is_empty());
        let message = String::from_utf8(output.stderr)?;
        assert_eq!(message.lines().count(), 1);
        if args.is_empty()
            || args == ["request"]
            || args == ["--approval", "on-request"]
            || args == ["--resume", "session-id"]
            || args == ["--continue"]
        {
            assert!(message.contains("use benitoiteh exec"), "{message}");
        } else {
            assert!(message.contains("invalid arguments"), "{message}");
        }
        assert_eq!(fs::read_dir(&directory.0)?.count(), 0);
    }
    Ok(())
}

// 関門: 補助の失敗後の exec、設定の読み込み、環境や標準入出力の解釈を実行ファイルの口で守る。
#[test]
fn session_helper_executes_only_after_setsid_and_failures_exit_125() -> Result<(), Box<dyn Error>> {
    use std::os::unix::process::CommandExt;
    let directory = TestDirectory::new("session-helper")?;
    for arguments in [
        vec!["__session", "--", "true"],
        vec!["__session", "--", "./true"],
        vec!["__session", "--", ""],
        vec!["__session", "--"],
        vec!["__session"],
        vec!["__session", "wrong", "/usr/bin/true"],
        vec!["__session", "--", "/missing-program"],
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_benitoiteh"))
            .args(arguments)
            .current_dir(&directory.0)
            .output()?;
        assert_eq!(output.status.code(), Some(125), "{output:?}");
        assert!(output.stdout.is_empty());
        let diagnostic = String::from_utf8(output.stderr)?;
        assert_eq!(diagnostic.lines().count(), 1, "{diagnostic:?}");
        assert!(diagnostic.ends_with('\n'));
    }
    let marker = directory.0.join("must-not-exec");
    let output = benitoiteh::sandbox::session::command(
        std::path::Path::new(env!("CARGO_BIN_EXE_benitoiteh")),
        std::path::Path::new("/usr/bin/touch"),
    )
    .arg(&marker)
    .process_group(0)
    .output()?;
    assert_eq!(output.status.code(), Some(125));
    assert!(!marker.exists());
    assert!(String::from_utf8(output.stderr)?.contains("Could not create a new session:"));
    let output = benitoiteh::sandbox::session::command(
        std::path::Path::new(env!("CARGO_BIN_EXE_benitoiteh")),
        std::path::Path::new("/bin/sh"),
    )
    .args(["-c", "printf '%s' \"$SESSION_HELPER_VALUE\"; exit 17"])
    .env_clear()
    .env("SESSION_HELPER_VALUE", "passed unchanged")
    .env("HOME", "/missing-home")
    .current_dir(&directory.0)
    .output()?;
    assert_eq!(output.status.code(), Some(17));
    assert_eq!(output.stdout, b"passed unchanged");
    assert!(output.stderr.is_empty());
    Ok(())
}
