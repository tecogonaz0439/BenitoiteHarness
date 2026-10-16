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
        "benitoiteh doctor",
        "benitoiteh sessions",
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
        &[],
        &["--unknown"],
        &["exec"],
        &["doctor"],
        &["sessions"],
        &["--licenses"],
        &["--help", "--unknown"],
        &["--version", "--help"],
        &["exec", "--no-sandbox", "request"],
    ];
    for args in cases {
        let output = run("usage", args)?;
        assert_eq!(output.status.code(), Some(2), "{args:?}");
        assert!(output.stdout.is_empty(), "{args:?}");
        let error = String::from_utf8(output.stderr)?;
        assert_eq!(error.lines().count(), 1, "{args:?}: {error}");
        assert!(error.starts_with("benitoiteh: "), "{error}");
        assert!(error.ends_with('\n'), "{error}");
        if args.contains(&"--no-sandbox") {
            assert!(
                error.contains("--no-sandbox is not available yet"),
                "{error}"
            );
        }
    }
    Ok(())
}
