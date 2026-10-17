//! 問い合わせの方針を本物の OS の拒否と処理系で確かめる（設計書 03-02、04-01、07-01）。

#[allow(dead_code)]
mod support;

use benitoiteh::config::{child_environment, writable::WritableLocations};
use benitoiteh::sandbox::run::{self, RunCommand, RunLimits};
use benitoiteh::sandbox::{self, ReadAccess, RestrictedAccess, SandboxCommand, SandboxStdin};
use benitoiteh::session::SessionId;
use std::error::Error;
use std::ffi::OsString;
use std::fs;
use std::net::TcpListener;
use std::os::unix::fs::DirBuilderExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

type TestResult = Result<(), Box<dyn Error>>;
struct Fixture(PathBuf);
impl Drop for Fixture {
    fn drop(&mut self) {
        let _removed = fs::remove_dir_all(&self.0);
    }
}
impl Fixture {
    fn new() -> Result<Self, Box<dyn Error>> {
        let path = std::env::temp_dir().join(format!(
            "bh-query-policy-{}",
            SessionId::generate(SystemTime::now())?
        ));
        fs::create_dir(&path)?;
        let fixture = Self(fs::canonicalize(path)?);
        for name in ["project", "temporary", "scratch", "placeholders"] {
            fs::DirBuilder::new()
                .mode(0o700)
                .create(fixture.0.join(name))?;
        }
        Ok(fixture)
    }
    fn command(&self, program: &Path, args: Vec<OsString>) -> Result<RunCommand, Box<dyn Error>> {
        let locations = WritableLocations {
            project: self.0.join("project"),
            temporary: self.0.join("temporary"),
            additional: vec![],
        };
        let policy = sandbox::restricted_policy(
            &RestrictedAccess {
                write_project: false,
                allow_ip: false,
                programs: vec![program.into()],
                readable: ReadAccess::All,
            },
            &locations,
            &[],
        )?;
        let env = child_environment(
            [
                ("OPENAI_API_KEY".into(), "secret".into()),
                ("CUSTOM_PROVIDER_KEY".into(), "secret-custom".into()),
                ("QUERY_SENTINEL".into(), "kept".into()),
            ],
            &["CUSTOM_PROVIDER_KEY".into()].into_iter().collect(),
            &locations.temporary,
            Some(("restricted", false)),
        );
        let command = SandboxCommand {
            policy,
            program: program.into(),
            args,
            cwd: locations.project,
            env,
            stdin: SandboxStdin::Null,
        };
        let helper = Path::new(env!("CARGO_BIN_EXE_benitoiteh"));
        #[cfg(target_os = "linux")]
        {
            Ok(command
                .prepare(
                    helper,
                    &self.0.join("scratch"),
                    &self.0.join("placeholders"),
                )?
                .into())
        }
        #[cfg(not(target_os = "linux"))]
        {
            Ok(command.to_command(helper)?.into())
        }
    }
    fn run(&self, program: &Path, args: Vec<OsString>) -> Result<Output, Box<dyn Error>> {
        let output = Arc::new(Mutex::new(Vec::new()));
        let errors = Arc::new(Mutex::new(Vec::new()));
        let result = run::run(
            self.command(program, args)?,
            vec![],
            Arc::clone(&output),
            Arc::clone(&errors),
            RunLimits::default(),
            &AtomicBool::new(false),
        )?;
        Ok(Output {
            status: result.status,
            stdout: output.lock().map_err(|_| "poisoned")?.clone(),
            stderr: errors.lock().map_err(|_| "poisoned")?.clone(),
        })
    }
}

// 関門: 宣言の照合ではなく、同じ操作の対照と実際の拒否で方針の漏れを検出する。
#[test]
fn queries_deny_project_writes_ip_shells_and_writable_executables() -> TestResult {
    let f = Fixture::new()?;
    let artifact = std::env::current_exe()?
        .parent()
        .and_then(Path::parent)
        .ok_or("target")?
        .join("sandbox-probe");
    let probe = f.0.join("probe");
    fs::copy(artifact, &probe)?;
    let writable_probe = f.0.join("temporary/copied-probe");
    fs::copy(&probe, &writable_probe)?;
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let project_file = f.0.join("project/written");
    let permission_denied = &[(10, "PermissionDenied")][..];
    #[cfg(target_os = "linux")]
    let write_denied = &[(11, "ReadOnlyFilesystem"), (10, "PermissionDenied")][..];
    #[cfg(target_os = "linux")]
    let network_denied = &[
        (11, "NetworkUnreachable"),
        (11, "HostUnreachable"),
        (11, "NetworkDown"),
        (10, "PermissionDenied"),
    ][..];
    #[cfg(not(target_os = "linux"))]
    let (write_denied, network_denied) = (permission_denied, permission_denied);
    let operations = [
        (
            vec![
                OsString::from("write"),
                project_file.clone().into_os_string(),
            ],
            write_denied,
        ),
        (
            vec![
                "tcp-connect".into(),
                listener.local_addr()?.to_string().into(),
            ],
            // Linux では新しいネットワークの名前空間で到達不能になりうる。
            // 終了状態 11 だけでは NotFound なども通るので、誤りの種類も限定する。
            network_denied,
        ),
        (
            vec![
                "exec".into(),
                "/bin/sh".into(),
                "-c".into(),
                "exit 0".into(),
            ],
            permission_denied,
        ),
        (
            vec![
                "exec".into(),
                writable_probe.into_os_string(),
                "write".into(),
                f.0.join("temporary/marker").into_os_string(),
            ],
            permission_denied,
        ),
    ];
    for (args, denied_outcomes) in operations {
        let operation = format!("{args:?}");
        let name = args
            .first()
            .and_then(|arg| arg.to_str())
            .ok_or("Missing probe operation")?;
        let error_prefix = format!("{name}: ");
        let control = Command::new(&probe)
            .args(&args)
            .current_dir(&f.0)
            .output()?;
        assert!(
            control.status.success(),
            "operation {operation}, control: {control:?}"
        );
        let output = f
            .run(&probe, args)
            .map_err(|error| format!("operation {operation}: {error}"))?;
        let stderr = String::from_utf8_lossy(&output.stderr);
        let error_kind = stderr.lines().find_map(|line| {
            line.strip_prefix(&error_prefix)
                .and_then(|detail| detail.split_once(": "))
                .map(|(kind, _)| kind)
        });
        assert!(
            output
                .status
                .code()
                .zip(error_kind)
                .is_some_and(|outcome| denied_outcomes.contains(&outcome)),
            "operation {operation}, expected (exit code, error kind) {denied_outcomes:?}, status: {}, error kind: {error_kind:?}, sandbox-probe stderr: {stderr}",
            output.status,
        );
    }
    // 一時領域への書き込みは許されることも確かめ、全操作の拒否で通るのを防ぐ。
    let output = f.run(
        &probe,
        vec![
            "write".into(),
            f.0.join("temporary/permitted").into_os_string(),
        ],
    )?;
    assert!(
        output.status.success(),
        "operation write temporary/permitted: {output:?}"
    );
    let output = f.run(&probe, vec!["environment".into()])?;
    assert!(output.status.success(), "operation environment: {output:?}");
    let output = String::from_utf8(output.stdout)?;
    assert!(output.contains("QUERY_SENTINEL=kept"));
    assert!(
        !output.contains("secret")
            && !output.contains("PROVIDER_KEY")
            && !output.contains("OPENAI_API_KEY")
    );
    Ok(())
}

#[test]
fn real_processor_version_runs_under_the_query_sandbox() -> TestResult {
    let f = Fixture::new()?;
    let processor = support::benitoite()?;
    let output = f.run(&processor, vec!["--version=json".into()])?;
    assert!(
        output.status.success(),
        "operation --version=json: {output:?}"
    );
    let object = benitoiteh::benitoite::json_object(&output.stdout, "version")?;
    assert_eq!(
        object.get("version").and_then(serde_json::Value::as_str),
        Some("0.0.7")
    );
    Ok(())
}
