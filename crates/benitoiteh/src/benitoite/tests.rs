//! 探索の安全条件と、本物の処理系との問い合わせの契約を守る（実装プラン P2-05）。

use super::*;
use crate::config::writable::WritableLocations;
use serde_json::json;
use std::error::Error;
use std::fs;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, SystemTime};

#[allow(dead_code)]
#[path = "../../tests/support/processor.rs"]
mod support;

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Result<Self, Box<dyn Error>> {
        let path = std::env::temp_dir().join(format!(
            "bh-processor-{}",
            crate::session::SessionId::generate(SystemTime::now())?
        ));
        fs::create_dir(&path)?;
        let root = Self(fs::canonicalize(path)?);
        for name in ["project", "temporary", "bin", "scratch", "placeholders"] {
            fs::create_dir(root.0.join(name))?;
        }
        Ok(root)
    }
    fn locations(&self) -> WritableLocations {
        WritableLocations {
            project: self.0.join("project"),
            temporary: self.0.join("temporary"),
            additional: vec![],
        }
    }
    fn context(&self) -> QueryContext {
        QueryContext {
            helper: PathBuf::new(),
            scratch: self.0.join("scratch"),
            placeholders: self.0.join("placeholders"),
            env: crate::config::child_environment(
                std::env::vars_os(),
                &Default::default(),
                &self.0.join("temporary"),
                Some(("restricted", false)),
            ),
            locations: self.locations(),
            denied_candidates: vec![],
            launcher: QueryLauncher::TestDirect,
        }
    }
    fn probe(&self) -> Result<Processor, Box<dyn Error>> {
        let artifact = std::env::current_exe()?
            .parent()
            .and_then(Path::parent)
            .ok_or("no target directory")?
            .join("sandbox-probe");
        let path = self.0.join("probe");
        fs::copy(artifact, &path)?;
        let (path, sha256) = validate_executable(&path, &self.locations(), false)?;
        Ok(Processor {
            path,
            sha256,
            version: String::new(),
            build: Value::Null,
            context: self.context().into(),
        })
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _removed = fs::remove_dir_all(&self.0);
    }
}

// 関門: PATH と実のパスの判定を誤るとプロジェクトが選んだ処理系を起動する。
// 純粋な探索と形の検査の境界で表にし、CLI 側では失敗時の扱いだけを確かめる。
#[test]
fn discovery_prioritizes_config_and_excludes_untrusted_path_entries() -> Result<(), Box<dyn Error>>
{
    let f = Fixture::new()?;
    let real = support::benitoite()?;
    fs::create_dir(f.0.join("project/bin"))?;
    symlink(f.0.join("project/bin"), f.0.join("alias"))?;
    symlink(&real, f.0.join("project/bin/benitoite"))?;
    symlink(&real, f.0.join("bin/benitoite"))?;
    let path = std::env::join_paths([
        Path::new("bin"),
        &f.0.join("project/bin"),
        &f.0.join("alias"),
        &f.0.join("bin"),
    ])?;
    assert_eq!(discover(None, Some(&path), &f.locations())?, real);
    assert_eq!(
        discover(Some(&real), Some("".as_ref()), &f.locations())?,
        real
    );
    assert!(discover(Some(Path::new("relative")), Some(&path), &f.locations()).is_err());
    let path = std::env::join_paths([Path::new("bin"), &f.0.join("alias")])?;
    assert!(discover(None, Some(&path), &f.locations()).is_err());
    fs::copy(&real, f.0.join("temporary/benitoite"))?;
    let path = std::env::join_paths([f.0.join("temporary")])?;
    let found = discover(None, Some(&path), &f.locations())?;
    assert!(validate_executable(&found, &f.locations(), false).is_err());
    Ok(())
}

#[test]
fn native_executables_must_be_outside_writable_roots_and_allowed_names()
-> Result<(), Box<dyn Error>> {
    let f = Fixture::new()?;
    let real = support::benitoite()?;
    validate_executable(&real, &f.locations(), false)?;
    for (name, contents) in [("script", b"#!/bin/sh\n".as_slice()), ("text", b"hello")] {
        let path = f.0.join(name);
        fs::write(&path, contents)?;
        assert!(validate_executable(&path, &f.locations(), false).is_err());
    }
    assert!(validate_executable(&f.0.join("bin"), &f.locations(), false).is_err());
    for name in ["project/program", "temporary/program", "python3.12", "bash"] {
        let path = f.0.join(name);
        fs::copy(&real, &path)?;
        assert!(
            validate_executable(&path, &f.locations(), true).is_err(),
            "{name}"
        );
    }
    symlink(f.0.join("python3.12"), f.0.join("innocent"))?;
    assert!(matches!(
        validate_executable(&f.0.join("innocent"), &f.locations(), true),
        Err(BenitoiteError::Invalid(text::BANNED_PROGRAM))
    ));
    Ok(())
}

#[test]
fn version_contract_requires_every_format_and_one_complete_object() -> Result<(), Box<dyn Error>> {
    let f = Fixture::new()?;
    let processor = Processor::inspect(Some(&support::benitoite()?), None, f.context())?;
    assert_eq!(processor.version, PROMPT_VERIFIED_VERSION);
    assert_eq!(processor.sha256.len(), 64);
    assert!(processor.version_warning().is_none());
    let output = processor.query(
        &["--version=json".into()],
        QueryLimits::default(),
        &AtomicBool::new(false),
    )?;
    let original: Value = serde_json::from_slice(&output.stdout)?;
    for key in [
        "diagnostics",
        "testResults",
        "checkReport",
        "testCheckReport",
        "effectLimit",
        "skillList",
        "skillDocument",
    ] {
        for value in [Value::Null, json!([2])] {
            let mut report = original.clone();
            *report
                .get_mut("formats")
                .and_then(|v| v.get_mut(key))
                .ok_or("format")? = value;
            assert!(version_report(&serde_json::to_vec(&report)?).is_err());
        }
    }
    for suffix in ["{}", "garbage"] {
        let bytes = [output.stdout.as_slice(), suffix.as_bytes()].concat();
        assert!(version_report(&bytes).is_err());
    }
    let mut report = original;
    *report.get_mut("kind").ok_or("kind")? = json!("checkReport");
    assert!(version_report(&serde_json::to_vec(&report)?).is_err());
    *report.get_mut("kind").ok_or("kind")? = json!("version");
    *report.get_mut("version").ok_or("version")? = json!("0.0.8");
    let (version, _) = version_report(&serde_json::to_vec(&report)?)?;
    let processor = Processor {
        version,
        ..processor
    };
    assert!(
        processor
            .version_warning()
            .is_some_and(|warning| warning.contains("0.0.8"))
    );
    Ok(())
}

// 関門: run の無制限の受け取り先では、壊れた処理系がメモリを使い尽くす。
// 実際の子を使い、保持の上限と中断の伝達を一つの契約で確かめる。
#[test]
fn queries_bound_output_timeout_and_propagate_cancellation() -> Result<(), Box<dyn Error>> {
    let f = Fixture::new()?;
    let probe = f.probe()?;
    assert!(matches!(
        probe.query(
            &["--version=json".into()],
            QueryLimits::default(),
            &AtomicBool::new(false)
        ),
        Err(BenitoiteError::Exit { .. })
    ));
    let limits = QueryLimits {
        timeout: Duration::from_millis(100),
        grace: Duration::from_millis(20),
        ..QueryLimits::default()
    };
    for (stdout, stderr, delay, expected) in [
        ("16777217", "0", "1000", "overflow"),
        ("0", "0", "1000", "timeout"),
        ("0", "16777217", "1000", "diagnostics"),
        ("3", "65537", "0", "ok"),
    ] {
        let args = ["query-output", stdout, stderr, delay].map(Into::into);
        let query_limits = if expected == "diagnostics" {
            QueryLimits {
                timeout: Duration::from_secs(60),
                stderr: StderrLimit::FailOver(16 * 1024 * 1024),
                ..QueryLimits::default()
            }
        } else if expected == "overflow" {
            QueryLimits {
                timeout: Duration::from_secs(60),
                ..QueryLimits::default()
            }
        } else if expected == "timeout" {
            limits
        } else {
            QueryLimits::default()
        };
        let result = probe.query(&args, query_limits, &AtomicBool::new(false));
        match expected {
            "overflow" => assert!(matches!(result, Err(BenitoiteError::OutputLimit))),
            "timeout" => assert!(matches!(result, Err(BenitoiteError::TimedOut))),
            "diagnostics" => assert!(matches!(result, Err(BenitoiteError::DiagnosticsLimit))),
            _ => {
                let output = result?;
                assert_eq!(output.stdout.len(), 3);
                assert_eq!(output.stderr.len(), 65536);
            }
        }
    }
    let cancel = AtomicBool::new(false);
    std::thread::scope(|scope| -> Result<(), Box<dyn Error>> {
        scope.spawn(|| {
            std::thread::sleep(Duration::from_millis(30));
            cancel.store(true, Ordering::Release);
        });
        assert!(matches!(
            probe.query(
                &["query-output", "0", "0", "1000"].map(Into::into),
                QueryLimits::default(),
                &cancel
            ),
            Err(BenitoiteError::Cancelled)
        ));
        Ok(())
    })?;
    Ok(())
}

// 関門: sandbox::run が返す残存の情報を問い合わせが捨てる退行は、run 自身のテストでは捕まらない。
#[test]
fn query_rejects_a_successful_child_with_possible_descendants() -> Result<(), Box<dyn Error>> {
    let f = Fixture::new()?;
    let probe = f.probe()?;
    let result = probe.query(
        &[
            "descendant".into(),
            "session".into(),
            "open".into(),
            f.0.join("child").into_os_string(),
        ],
        QueryLimits::default(),
        &AtomicBool::new(false),
    );
    assert!(
        matches!(result, Err(BenitoiteError::Invalid(message)) if message == "The benitoite query may have left child processes running.")
    );
    Ok(())
}

// 関門: 通常の File::open は FIFO の相手を待つ。失敗時も相手を開いて試験のスレッドを回収する。
#[test]
fn executable_fifo_is_rejected_without_waiting_for_a_writer() -> Result<(), Box<dyn Error>> {
    let f = Fixture::new()?;
    let path = f.0.join("fifo");
    #[cfg(target_os = "linux")]
    rustix::fs::mkfifoat(rustix::fs::CWD, &path, rustix::fs::Mode::RWXU)?;
    #[cfg(target_os = "macos")]
    assert!(
        std::process::Command::new("/usr/bin/mkfifo")
            .arg(&path)
            .status()?
            .success()
    );
    let (sender, receiver) = std::sync::mpsc::channel();
    std::thread::scope(|scope| -> Result<(), Box<dyn Error>> {
        scope.spawn(|| {
            let result = validate_executable(&path, &f.locations(), false);
            let _sent = sender.send(result);
        });
        match receiver.recv_timeout(Duration::from_millis(300)) {
            Ok(result) => assert!(matches!(
                result,
                Err(BenitoiteError::Invalid(text::INVALID_PROGRAM))
            )),
            Err(error) => {
                let _release = rustix::fs::open(
                    &path,
                    rustix::fs::OFlags::RDWR
                        | rustix::fs::OFlags::NONBLOCK
                        | rustix::fs::OFlags::CLOEXEC,
                    rustix::fs::Mode::empty(),
                )?;
                return Err(
                    format!("Executable validation waited for a FIFO writer: {error}").into(),
                );
            }
        }
        Ok(())
    })?;
    Ok(())
}
