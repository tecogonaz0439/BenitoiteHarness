//! bash の道具が本物の OS のサンドボックスの中で動かすコマンドの境界を確かめる（設計書 03-02、07-01）。
//!
//! OS のサンドボックスを要するので、実装プラン 00-common「検査」の決まりにより `sandbox_*.rs` に置く。

// 準備には、このファイルのテストが使わない部分がある。
#[allow(dead_code)]
#[path = "support/bash.rs"]
mod bash_support;
use bash_support::Fixture;
use benitoiteh::config::Sandbox;
use serde_json::json;
use std::error::Error;
use std::fs;
use std::sync::atomic::AtomicBool;
#[cfg(any(target_os = "linux", target_os = "macos"))]
use std::time::Duration;

type TestResult = Result<(), Box<dyn Error>>;

// 関門: 外で動かす一回の PATH から、中から書ける場所に届く項目（直に、リンクで、リンクの途中の段で、
// まだない場所で、相対で）を除き、サンドボックスの中の PATH は変えない（設計書 03-02「通常モードの方針」）。
#[test]
fn escalated_bash_path_skips_entries_that_reach_writable_roots() -> TestResult {
    let original = std::cell::RefCell::new(String::new());
    let f = Fixture::with_path(Sandbox::WorkspaceWrite, |root| {
        // macOS の既定の APFS は大小文字を区別しないので、綴りの違う名前でも書ける場所に入る。
        // 名前の比べでは捕まらず、デバイスと inode の比べだけが捕まえる。
        let case_variant = if cfg!(target_os = "macos") {
            format!("{}/PROJECT/bin:", root.display())
        } else {
            String::new()
        };
        let value = format!(
            "{case_variant}{p}/bin:{r}/link-bin:{r}/hop/bin:{p}/missing:bin::{r}/trusted:/bin:/usr/bin",
            p = root.join("project").display(),
            r = root.display()
        );
        original.replace(value.clone());
        value
    })?;
    let original = original.into_inner();
    use std::os::unix::fs::{PermissionsExt, symlink};
    for (directory, text) in [
        (f.project.join("bin"), "PLANTED"),
        (f.root.join("trusted"), "TRUSTED"),
    ] {
        fs::create_dir_all(&directory)?;
        let tool = directory.join("probe-tool");
        fs::write(&tool, format!("#!/bin/sh\necho {text}\n"))?;
        fs::set_permissions(&tool, fs::Permissions::from_mode(0o755))?;
    }
    symlink(f.project.join("bin"), f.root.join("link-bin"))?;
    symlink(&f.project, f.root.join("hop"))?;
    let command = r#"printf 'PATH=%s\n' "$PATH"; probe-tool"#;
    let escalated = f.execute(
        "escalated",
        json!({"command": command, "escalate": true}),
        &AtomicBool::new(false),
    )?;
    assert!(!escalated.is_error, "{}", escalated.text);
    let expected = format!("PATH={}/trusted:/bin:/usr/bin\n", f.root.display());
    assert!(escalated.text.contains(&expected), "{}", escalated.text);
    assert!(escalated.text.contains("TRUSTED"), "{}", escalated.text);
    assert!(!escalated.text.contains("PLANTED"), "{}", escalated.text);
    let sandboxed = f.execute(
        "sandboxed",
        json!({"command": command}),
        &AtomicBool::new(false),
    )?;
    assert!(
        sandboxed.text.contains(&format!("PATH={original}\n")),
        "{}",
        sandboxed.text
    );
    Ok(())
}

// 関門: 中継の方式の bash は、プロキシの変数を受け取り、許したドメインへは中継を通して届く。
// 許していないドメインは、尋ねて許せば届き、断れば届かず、その宛先と理由が道具の結果に載る
// （設計書 03-02「通信の中継」）。
#[cfg(target_os = "linux")]
#[test]
fn proxied_bash_reaches_allowed_and_approved_domains_and_reports_denials() -> TestResult {
    use benitoiteh::approval::{ApprovalAnswer, ApprovalOutcome, ApprovalPrompt};
    use benitoiteh::sandbox::proxy::NetworkGate;
    use std::io::{Read, Write};
    use std::sync::{Arc, mpsc};
    let upstream = std::net::TcpListener::bind("127.0.0.1:0")?;
    let port = upstream.local_addr()?.port();
    std::thread::spawn(move || {
        for _ in 0..2 {
            let Ok((mut stream, _)) = upstream.accept() else {
                return;
            };
            let mut chunk = [0_u8; 1024];
            let _read = stream.read(&mut chunk);
            let _written = stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok");
        }
    });
    let gate = Arc::new(
        NetworkGate::new(["allowed.example".to_owned()].into_iter().collect(), true).with_upstream(
            Arc::new(move |_, _| Ok(vec![std::net::SocketAddr::from(([127, 0, 0, 1], port))])),
            |_| true,
        ),
    );
    let (prompts, requests) = mpsc::channel::<ApprovalPrompt>();
    gate.connect_prompts(prompts);
    let answers = std::thread::spawn(move || {
        let mut asked = Vec::new();
        for outcome in [ApprovalOutcome::Approved, ApprovalOutcome::Denied] {
            let Ok(prompt) = requests.recv_timeout(Duration::from_secs(30)) else {
                break;
            };
            asked.push(prompt.request.facts.domain.clone().unwrap_or_default());
            let _sent = prompt.reply.send(ApprovalAnswer {
                outcome,
                message: None,
            });
        }
        asked
    });
    let f = Fixture::with_network(
        Sandbox::WorkspaceWrite,
        |_| "/bin:/usr/bin".into(),
        Some(gate),
    )?;
    let command = r#"printf 'proxy=%s\n' "$HTTPS_PROXY"
for host in allowed.example asked.example declined.example; do
  exec 3<>/dev/tcp/127.0.0.1/3128
  printf 'GET http://%s/ HTTP/1.1\r\n\r\n' "$host" >&3
  printf '%s: ' "$host"; head -c 12 <&3; echo
  exec 3>&-
done"#;
    let output = f.execute(
        "proxy",
        json!({"command": command}),
        &AtomicBool::new(false),
    )?;
    for expected in [
        "proxy=http://127.0.0.1:3128",
        "allowed.example: HTTP/1.1 200",
        "asked.example: HTTP/1.1 200",
        "declined.example: HTTP/1.1 403",
        "benitoiteh blocked network access to declined.example:80: the user declined.",
    ] {
        assert!(
            output.text.contains(expected),
            "{expected}: {}",
            output.text
        );
    }
    assert_eq!(
        answers.join().map_err(|_| "answers")?,
        ["asked.example", "declined.example"]
    );
    Ok(())
}

// 関門: macOS の中継の方式では、bash は合言葉を含むプロキシの URL を受け取り、curl はそれに従って
// 許したドメインと尋ねて許したドメインへ届き、断ったドメインは 403 と記録になる。合言葉のない要求は
// 中継が断り、中継を飛ばす手元の宛先への直接の接続は Seatbelt が拒む。Seatbelt が許す `[::1]` の同じ番号も
// 中継が持つ（設計書 03-02「通信の中継」、BH-119）。
#[cfg(target_os = "macos")]
#[test]
fn macos_proxied_bash_needs_the_token_and_cannot_bypass_the_relay() -> TestResult {
    use benitoiteh::approval::{ApprovalAnswer, ApprovalOutcome, ApprovalPrompt};
    use benitoiteh::sandbox::proxy::NetworkGate;
    use std::io::{Read, Write};
    use std::sync::{Arc, mpsc};
    let upstream = std::net::TcpListener::bind("127.0.0.1:0")?;
    let port = upstream.local_addr()?.port();
    std::thread::spawn(move || {
        for stream in upstream.incoming() {
            let Ok(mut stream) = stream else {
                return;
            };
            let mut chunk = [0_u8; 1024];
            let _read = stream.read(&mut chunk);
            let _written = stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok");
        }
    });
    let gate = Arc::new(
        NetworkGate::new(["allowed.example".to_owned()].into_iter().collect(), true).with_upstream(
            Arc::new(move |_, _| Ok(vec![std::net::SocketAddr::from(([127, 0, 0, 1], port))])),
            |_| true,
        ),
    );
    let (prompts, requests) = mpsc::channel::<ApprovalPrompt>();
    gate.connect_prompts(prompts);
    let answers = std::thread::spawn(move || {
        let mut asked = Vec::new();
        for outcome in [ApprovalOutcome::Approved, ApprovalOutcome::Denied] {
            let Ok(prompt) = requests.recv_timeout(Duration::from_secs(30)) else {
                break;
            };
            asked.push(prompt.request.facts.domain.clone().unwrap_or_default());
            let _sent = prompt.reply.send(ApprovalAnswer {
                outcome,
                message: None,
            });
        }
        asked
    });
    let f = Fixture::with_network(
        Sandbox::WorkspaceWrite,
        |_| "/bin:/usr/bin".into(),
        Some(gate),
    )?;
    let command = format!(
        r#"case "$HTTPS_PROXY" in http://benitoiteh:*@127.0.0.1:*) echo token-url;; *) echo "bad=$HTTPS_PROXY";; esac
[ "$HTTP_PROXY" = "$HTTPS_PROXY" ] && echo same-url
for host in allowed.example asked.example declined.example; do
  printf '%s: %s\n' "$host" "$(curl -s -o /dev/null -w '%{{http_code}}' http://$host/)"
done
bare=${{HTTP_PROXY#http://*@}}
printf 'bare: %s\n' "$(curl -s -o /dev/null -w '%{{http_code}}' --noproxy '' -x "http://$bare" http://allowed.example/)"
v6=$(printf '%s' "$HTTP_PROXY" | sed 's/@127.0.0.1:/@[::1]:/')
printf 'v6: %s\n' "$(curl -s -o /dev/null -w '%{{http_code}}' --noproxy '' -x "$v6" http://allowed.example/)"
if curl -s --noproxy '*' -m 5 http://127.0.0.1:{port}/ >/dev/null; then echo direct-open; else echo direct-blocked; fi"#
    );
    let output = f.execute(
        "proxy",
        json!({"command": command}),
        &AtomicBool::new(false),
    )?;
    for expected in [
        "token-url",
        "same-url",
        "allowed.example: 200",
        "asked.example: 200",
        "declined.example: 403",
        "bare: 407",
        "v6: 200",
        "direct-blocked",
        "benitoiteh blocked network access to declined.example:80: the user declined.",
    ] {
        assert!(
            output.text.contains(expected),
            "{expected}: {}",
            output.text
        );
    }
    assert!(!output.text.contains("direct-open"), "{}", output.text);
    assert_eq!(
        answers.join().map_err(|_| "answers")?,
        ["asked.example", "declined.example"]
    );
    Ok(())
}

// 作成時の関門: macOS で拒否と見なしたときは、Seatbelt が記録した拒否の操作と対象を道具の結果に添える。
// 既定の拒否と個別の拒む規則の両方に印が付き、前の起動の拒否は混ぜず、拒否と見なさない失敗には添えない
// （設計書 03-02「拒否の検出」）。
// 推定の文（BASH_SANDBOX）だけを確かめる既存のテストは、記録を読まない退行を捕まえない。
#[cfg(target_os = "macos")]
#[test]
fn macos_denied_bash_reports_the_recorded_operation_and_path() -> TestResult {
    let f = Fixture::new(Sandbox::WorkspaceWrite)?;
    let stop = AtomicBool::new(false);
    let denied = |name: &str| {
        let target = f.root.join(name);
        f.execute(
            name,
            json!({"command": format!("printf x > '{}'", target.display())}),
            &stop,
        )
        .map(|output| (target, output))
    };
    let reported = |text: &str| -> Vec<String> {
        text.lines()
            .filter(|line| line.starts_with("[sandbox denied: "))
            .map(str::to_owned)
            .collect()
    };
    let (first, output) = denied("first-outside.txt")?;
    assert!(output.is_error && !first.exists(), "{}", output.text);
    let lines = reported(&output.text);
    assert!(
        lines.iter().any(|line| line.contains("file-write")
            && line.contains(&first.display().to_string())
            && line.ends_with("(bash)]")),
        "{}",
        output.text
    );
    assert!(!output.text.contains("dtracehelper"), "{}", output.text);
    let (second, output) = denied("second-outside.txt")?;
    let lines = reported(&output.text);
    assert!(
        lines
            .iter()
            .any(|line| line.contains(&second.display().to_string())),
        "{}",
        output.text
    );
    assert!(
        !lines
            .iter()
            .any(|line| line.contains(&first.display().to_string())),
        "{}",
        output.text
    );
    // 個別の拒む規則（守る名前）の拒否にも印が付く。
    let protected = f.project.join(".git");
    let output = f.execute("protected", json!({"command": "mkdir .git"}), &stop)?;
    assert!(
        reported(&output.text)
            .iter()
            .any(|line| line.contains(&protected.display().to_string())),
        "{}",
        output.text
    );
    let plain = f.execute("plain", json!({"command": "exit 1"}), &stop)?;
    assert!(
        plain.is_error && reported(&plain.text).is_empty(),
        "{}",
        plain.text
    );
    Ok(())
}
