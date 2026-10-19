//! bash から OS の拒否と承認後の実行までを確かめる（設計書 03-02、07-01）。

#[path = "bash.rs"]
mod fixture;
use benitoiteh::config::Sandbox;
use fixture::Fixture;
use serde_json::json;
use std::error::Error;
use std::fs;
use std::net::TcpListener;
use std::path::Path;
use std::sync::atomic::AtomicBool;

type TestResult = Result<(), Box<dyn Error>>;
fn quote(path: &Path) -> String {
    format!("'{}'", path.to_string_lossy().replace('\'', "'\\''"))
}
fn probe(f: &Fixture) -> TestResult {
    let executable = std::env::current_exe()?;
    let source = executable
        .parent()
        .and_then(Path::parent)
        .ok_or("missing target directory")?
        .join("sandbox-probe");
    fs::copy(source, f.project.join("probe"))?;
    Ok(())
}

// 作成時の関門: SandboxCommand 単体のテストでは捕まらない、bash での方針の渡し忘れ、
// プロジェクト内のプログラムの実行、拒否後の案内と一回の権限昇格を本物の OS で確かめる。
#[test]
fn workspace_write_enforces_shell_scope_and_allows_single_escalation() -> TestResult {
    let f = Fixture::new(Sandbox::WorkspaceWrite)?;
    fs::create_dir(f.project.join(".git"))?;
    probe(&f)?;
    let cancelled = AtomicBool::new(false);
    let output = f.execute(
        "inside",
        json!({"command":"./probe write inside; printf ok > /dev/null"}),
        &cancelled,
    )?;
    assert!(!output.is_error, "{}", output.text);
    assert_eq!(
        fs::read_to_string(f.project.join("inside"))?,
        "sandbox-probe\n"
    );
    let outside = f.root.join("outside");
    for (id, path) in [
        ("outside", outside.clone()),
        ("protected", f.project.join(".git/denied")),
    ] {
        let output = f.execute(
            id,
            json!({"command":format!("./probe write {}", quote(&path))}),
            &cancelled,
        )?;
        assert!(output.is_error, "{}", output.text);
        assert!(
            output
                .text
                .contains("[sandbox: the command may have been blocked"),
            "{}",
            output.text
        );
        assert!(!path.exists());
    }
    let listener = TcpListener::bind("127.0.0.1:0")?;
    listener.set_nonblocking(true)?;
    let output = f.execute(
        "network",
        json!({"command":format!("./probe tcp-connect {}", listener.local_addr()?)}),
        &cancelled,
    )?;
    assert!(output.is_error, "{}", output.text);
    assert!(
        matches!(listener.accept(), Err(error) if error.kind() == std::io::ErrorKind::WouldBlock)
    );
    let output = f.execute("environment", json!({"command":"env"}), &cancelled)?;
    assert!(!output.is_error, "{}", output.text);
    assert!(output.text.contains("BENITOITEH_SANDBOX=workspace-write"));
    assert!(!output.text.contains("API_KEY") && !output.text.contains("CUSTOM_PROVIDER_KEY"));
    let output = f.execute("escalated", json!({"command":format!("./probe write {}", quote(&outside)),"escalate":true,"justification":"test outside write"}), &cancelled)?;
    assert!(!output.is_error, "{}", output.text);
    assert!(!output.text.contains("[sandbox:"));
    assert_eq!(fs::read_to_string(outside)?, "sandbox-probe\n");
    Ok(())
}

#[test]
fn read_only_shell_writes_temporary_files_but_not_project_files() -> TestResult {
    let f = Fixture::new(Sandbox::ReadOnly)?;
    probe(&f)?;
    let output = f.execute(
        "readonly",
        json!({"command":"./probe write denied"}),
        &AtomicBool::new(false),
    )?;
    assert!(output.is_error, "{}", output.text);
    assert!(
        output
            .text
            .contains("[sandbox: the command may have been blocked"),
        "{}",
        output.text
    );
    assert!(!f.project.join("denied").exists());
    let output = f.execute(
        "temporary",
        json!({"command":"printf ok > \"$TMPDIR/allowed\""}),
        &AtomicBool::new(false),
    )?;
    assert!(!output.is_error, "{}", output.text);
    assert_eq!(
        fs::read_to_string(f.session.temporary_dir().join("allowed"))?,
        "ok"
    );
    Ok(())
}

// 関門: サンドボックス内から補助を起動できても、権限の環境と Unix IPC は渡らない。
#[test]
fn sandboxed_askpass_helpers_fail_without_authority_and_cannot_connect_to_broker() -> TestResult {
    let mut f = Fixture::new(Sandbox::WorkspaceWrite)?;
    probe(&f)?;
    let shell = f.context.shell.as_mut().ok_or("missing shell")?;
    let broker = std::sync::Arc::new(benitoiteh::askpass::Broker::create(
        &f.root,
        f.session.id().as_str(),
        &shell.helper,
        &[f.project.clone(), f.session.temporary_dir().into()],
        true,
    )?);
    broker.environment(&mut shell.env);
    let socket = std::path::PathBuf::from(
        shell
            .env
            .get(std::ffi::OsStr::new("GIT_ASKPASS"))
            .ok_or("missing helper")?,
    )
    .parent()
    .ok_or("missing parent")?
    .join("sock");
    shell.askpass = Some(std::sync::Arc::clone(&broker));
    let requests = broker.take_requests().ok_or("missing prompts")?;
    let output = f.execute("inside-askpass", json!({"command":"test -z \"$BENITOITEH_ASKPASS_TOKEN\" && test -z \"$BENITOITEH_ASKPASS_SOCKET\" || exit 2; \"$GIT_ASKPASS\" untrusted"}), &AtomicBool::new(false))?;
    assert!(output.is_error);
    assert!(
        output.text.contains(benitoiteh::askpass::text::UNAVAILABLE),
        "{}",
        output.text
    );
    let output = f.execute(
        "socket",
        json!({"command": format!("./probe unix-connect {}", quote(&socket))}),
        &AtomicBool::new(false),
    )?;
    assert!(output.is_error, "{}", output.text);
    assert!(requests.try_recv().is_err());
    Ok(())
}
