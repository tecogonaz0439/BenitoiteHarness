//! 制限モードで起動と再開の安全の性質を確かめる（設計書 01-05、03-02）。
#[allow(dead_code, unused_imports)]
#[path = "support/exec_fixture.rs"]
mod fixture;
#[allow(dead_code)]
mod support;
use fixture::*;
#[path = "support/session.rs"]
mod session_support;
use benitoiteh::config::paths::DataPaths;
use benitoiteh::provider::Item;
use benitoiteh::session::{ResumeState, Session};
use std::os::unix::fs::symlink;
#[allow(dead_code)]
#[path = "support/exec_resume.rs"]
mod shared;
use shared::*;

// 関門: 形の検査だけでは、exec が誤りを無視して依頼の記録や API への要求へ進む退行を捕まえない。
// 本物の処理系と同じ場所の条件を用意し、拒否の理由まで入口で確かめる（設計書 03-02）。
#[test]
fn invalid_processor_shapes_stop_before_conversation_and_provider_requests() -> TestResult {
    for shape in ["shebang", "project", "dynamic-elf"] {
        let mut server = MockProvider::start(vec![])?;
        let f = Fixture::restricted(Protocol::Chat, &server)?;
        let path = if shape == "project" {
            let path = f.root.join("project/benitoite");
            fs::copy(support::benitoite()?, &path)?;
            path
        } else {
            let path = f.root.join("invalid-benitoite");
            let bytes = if shape == "shebang" {
                b"#!/bin/sh\nexit 0\n".to_vec()
            } else {
                // ELF64 little-endian、対象の CPU、ET_DYN、PT_INTERP の一つだけを持つ。
                let mut elf = vec![0u8; 120];
                for (offset, bytes) in [
                    (0usize, b"\x7fELF\x02\x01".as_slice()),
                    (16, &[3]),
                    (
                        18,
                        &u16::to_le_bytes(if cfg!(target_arch = "aarch64") {
                            183
                        } else {
                            62
                        }),
                    ),
                    (32, &[64]),
                    (54, &[56]),
                    (56, &[1]),
                    (64, &[3]),
                ] {
                    elf.get_mut(offset..offset.saturating_add(bytes.len()))
                        .ok_or("ELF fixture range")?
                        .copy_from_slice(bytes);
                }
                elf
            };
            fs::write(&path, bytes)?;
            fs::set_permissions(&path, fs::Permissions::from_mode(0o700))?;
            path
        };
        let config = f.root.join("config/benitoiteh/config.toml");
        fs::write(
            &config,
            fs::read_to_string(&config)?.replace(
                &serde_json::to_string(&support::benitoite()?)?,
                &serde_json::to_string(&path)?,
            ),
        )?;
        let request = "request: invalid processor must stop before recording this input";
        let output = Running::start(f.command(request).args(["--mode", "restricted", "--json"]))?
            .finish()?;
        assert_eq!(output.status.code(), Some(2), "{shape}");
        assert!(output.stdout.is_empty(), "{shape}");
        let stderr = String::from_utf8(output.stderr)?;
        assert!(stderr.contains("The program must be a native executable outside writable locations (statically linked on Linux)."), "{shape}: {stderr}");
        // 処理系の問い合わせにはセッションの一時領域が要る。依頼と会話がまだ記録されないことを守る。
        let (_, records) = f.records()?;
        assert!(
            !serde_json::to_string(&records)?.contains(request),
            "{shape}: request recorded: {records:?}"
        );
        assert!(
            records.iter().all(|record| !matches!(
                record.get("type").and_then(Value::as_str),
                Some("user" | "assistant" | "tool_call" | "tool_result" | "compaction")
            )),
            "{shape}: conversation recorded: {records:?}"
        );
        assert!(server.requests()?.is_empty(), "{shape}");
        server.shutdown()?;
    }
    Ok(())
}

// 作成時の関門: 設定の default_mode による昇格、会話・推論の欠落、番の再利用、
// 切れた末尾の連結は、引数や session の試験だけでは起動全体を保証できない。
#[test]
fn resume_appends_to_same_record_preserves_history_and_mode_and_repairs_tail() -> TestResult {
    for protocol in [Protocol::Chat, Protocol::Anthropic, Protocol::Responses] {
        let mut server = MockProvider::start(vec![
            ResponseScript::sse(protocol.response(
                Some(("same-id", "read", json!({"path":"a.bnt"}))),
                "Before",
                false,
            )),
            ResponseScript::sse(protocol.response(None, "First answer", false)),
            ResponseScript::sse(protocol.response(
                Some(("same-id", "read", json!({"path":"a.bnt"}))),
                "After",
                false,
            )),
            ResponseScript::sse(protocol.response(None, "Second answer", false)),
        ])?;
        let f = Fixture::restricted(protocol, &server)?;
        let output = Running::start(f.command("First request").arg("--json"))?.finish()?;
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let (directory, _) = f.records()?;
        let old = fs::read(directory.join("session.jsonl"))?;
        let mut log = fs::OpenOptions::new()
            .append(true)
            .open(directory.join("session.jsonl"))?;
        log.write_all(b"{\"type\":\"turn_start\",\"turn\":41}\n{\"type\":\"assistant\"")?;
        drop(log);
        let config = f.root.join("config/benitoiteh/config.toml");
        fs::write(
            &config,
            format!(
                "default_mode = 'normal'\n{}",
                fs::read_to_string(&config)?
                    .replace("model = \"mock-model\"", "model = \"new-default\"")
            ),
        )?;
        let output = Running::start(f.command("Second request").args([
            "--resume",
            id(&directory)?,
            "--json",
        ]))?
        .finish()?;
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let events = lines(&output.stdout)?;
        let start = events.first().ok_or("missing start")?;
        assert_eq!(start.get("resumed"), Some(&json!(true)));
        assert_eq!(start.get("mode"), Some(&json!("restricted")));
        assert_eq!(start.get("model"), Some(&json!("mock-model")));
        assert!(
            start
                .get("system_prompt_sha256")
                .is_some_and(Value::is_string)
        );
        assert!(
            events
                .iter()
                .any(|event| event.get("turn") == Some(&json!(43)))
        );
        assert!(fs::read(directory.join("session.jsonl"))?.starts_with(&old));
        let (_, records) = f.records()?;
        assert_eq!(
            records
                .iter()
                .filter(|r| r.get("type") == Some(&json!("header")))
                .count(),
            1
        );
        assert!(
            records
                .iter()
                .any(|r| r.pointer("/call/id") == Some(&json!("turn-42-call-1")))
                || records
                    .iter()
                    .any(|r| r.pointer("/call/id") == Some(&json!("turn-42-call-2")))
        );
        let requests = server.requests()?;
        let resumed =
            serde_json::to_string(&requests.get(2).ok_or("missing resumed request")?.body)?;
        assert!(
            resumed.contains("First request")
                && resumed.contains("First answer")
                && resumed.contains("Second request")
        );
        assert!(resumed.contains("Plan") || matches!(protocol, Protocol::Chat));
        let conversation = benitoiteh::session::replay(&directory)?;
        let last_user = conversation
            .items
            .iter()
            .rev()
            .find_map(|item| match item {
                Item::UserMessage(message) => Some(message),
                Item::AssistantTurn(_) | Item::ToolResult(_) => None,
            })
            .ok_or("missing user")?;
        assert_eq!(last_user.parts.len(), 1);
        // 修復した記録が、もう一度開けることまで確かめる。
        let opened = session_support::open_created_session(&paths(&f)?, &id(&directory)?.parse()?)?;
        let state = ResumeState::read(opened.directory())?;
        assert_eq!(state.max_turn, 43);
        assert!(state.files.read.contains("a.bnt"));
        drop(opened);
        server.shutdown()?;
    }
    Ok(())
}

// 作成時の関門: 使い方の誤りで保存済み記録を消す退行と、リンクの先を消す退行を
// 実行ファイルで守る。エラーの理由も確認して別の拒否による成功を防ぐ。
#[test]
fn resume_errors_preserve_records_and_never_escalate_or_follow_links() -> TestResult {
    let mut server = MockProvider::start(vec![ResponseScript::sse(
        Protocol::Chat.response(None, "Saved", false),
    )])?;
    let f = Fixture::restricted(Protocol::Chat, &server)?;
    assert!(
        Running::start(&mut f.command("Save"))?
            .finish()?
            .status
            .success()
    );
    let (directory, original) = f.records()?;
    fs::create_dir(f.root.join("other-project"))?;
    let other = f.root.join("other-project");
    for (options, diagnostic) in [
        (
            vec!["--resume", id(&directory)?, "--mode", "normal"],
            "cannot resume a restricted session in normal mode",
        ),
        (
            vec![
                "--resume",
                id(&directory)?,
                "--project",
                other.to_str().ok_or("path")?,
            ],
            "does not match",
        ),
        (
            vec!["--resume", id(&directory)?, "--provider", "missing"],
            "missing",
        ),
        (vec!["--resume", "../x"], "Invalid session ID"),
        (
            vec!["--resume", "20200101T000000Z-0000000000000000"],
            "Session file operation failed",
        ),
        (
            vec!["--resume", id(&directory)?, "--continue"],
            "invalid arguments",
        ),
    ] {
        rewrite(&directory, &original)?;
        fs::OpenOptions::new()
            .append(true)
            .open(directory.join("session.jsonl"))?
            .write_all(b"{\"type\":")?;
        let before = fs::read(directory.join("session.jsonl"))?;
        let output = Running::start(f.command("Next").args(options))?.finish()?;
        assert_eq!(output.status.code(), Some(2));
        assert!(
            String::from_utf8_lossy(&output.stderr).contains(diagnostic),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            fs::read(directory.join("session.jsonl"))?,
            before,
            "{diagnostic}"
        );
    }
    rewrite(&directory, &original)?;
    let mut unsafe_project = original.clone();
    *unsafe_project
        .first_mut()
        .ok_or("header")?
        .get_mut("project_dir")
        .ok_or("project")? = json!(f.root.join("home"));
    rewrite(&directory, &unsafe_project)?;
    fs::OpenOptions::new()
        .append(true)
        .open(directory.join("session.jsonl"))?
        .write_all(b"{\"type\":")?;
    let before = fs::read(directory.join("session.jsonl"))?;
    let output = Running::start(f.command("Next").args(["--resume", id(&directory)?]))?.finish()?;
    assert_eq!(output.status.code(), Some(2));
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("home"),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(fs::read(directory.join("session.jsonl"))?, before);
    rewrite(&directory, &original)?;
    let temp = paths(&f)?.temporary_dir(id(&directory)?)?;
    fs::write(f.root.join("home/keep"), "keep")?;
    symlink(f.root.join("home"), &temp)?;
    let output = Running::start(f.command("Next").args(["--resume", id(&directory)?]))?.finish()?;
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("unsafe type"));
    assert!(fs::symlink_metadata(&temp)?.is_symlink());
    assert_eq!(fs::read_to_string(f.root.join("home/keep"))?, "keep");
    fs::remove_file(&temp)?;
    let lock = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(directory.join("lock"))?;
    rustix::fs::flock(&lock, rustix::fs::FlockOperation::NonBlockingLockExclusive)?;
    let output = Running::start(f.command("Next").args(["--resume", id(&directory)?]))?.finish()?;
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("Session is in use"));
    drop(lock);
    let mut invalid = original.clone();
    *invalid
        .first_mut()
        .ok_or("header")?
        .get_mut("format")
        .ok_or("format")? = json!(99);
    rewrite(&directory, &invalid)?;
    let output = Running::start(f.command("Next").args(["--resume", id(&directory)?]))?.finish()?;
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("Unsupported session format"));
    assert_eq!(lines(&fs::read(directory.join("session.jsonl"))?)?, invalid);
    let link_id = "20990101T000000Z-0000000000000000";
    symlink(
        &directory,
        directory.parent().ok_or("parent")?.join(link_id),
    )?;
    assert_eq!(
        Running::start(f.command("Next").args(["--resume", link_id]))?
            .finish()?
            .status
            .code(),
        Some(2)
    );
    assert_eq!(lines(&fs::read(directory.join("session.jsonl"))?)?, invalid);
    assert_eq!(server.requests()?.len(), 1);
    server.shutdown()?;
    Ok(())
}

// 作成時の関門: 走査中の錠と、最後のプロジェクトの照合は、明示 ID の再開では守れない。
#[test]
fn continue_selects_latest_matching_readable_inactive_session() -> TestResult {
    let mut server = MockProvider::start(vec![
        ResponseScript::sse(Protocol::Chat.response(None, "initial", false)),
        ResponseScript::sse(Protocol::Chat.response(None, "continued", false)),
    ])?;
    let f = Fixture::restricted(Protocol::Chat, &server)?;
    assert!(
        Running::start(&mut f.command("Initial"))?
            .finish()?
            .status
            .success()
    );
    let (_, records) = f.records()?;
    let template = records.first().ok_or("header")?;
    let other = f.root.join("other");
    fs::create_dir(&other)?;
    let selected_id = "20990101T000000Z-0000000000000001";
    let selected = saved(&f, template, selected_id, &other)?;
    let mut log = fs::OpenOptions::new()
        .append(true)
        .open(selected.join("session.jsonl"))?;
    writeln!(
        log,
        "{}",
        json!({"type":"project_dir_changed", "project_dir":f.root.join("project")})
    )?;
    drop(log);
    let unreadable = saved(
        &f,
        template,
        "20990101T000000Z-0000000000000002",
        &f.root.join("project"),
    )?;
    fs::write(unreadable.join("session.jsonl"), "unreadable\n")?;
    let unsupported = saved(
        &f,
        template,
        "20990101T000000Z-0000000000000003",
        &f.root.join("project"),
    )?;
    fs::write(unsupported.join("session.jsonl"), "{\"format\":99}\n")?;
    let _other = saved(&f, template, "20990101T000000Z-0000000000000004", &other)?;
    let busy_id = "20990101T000000Z-0000000000000005";
    let _busy = saved(&f, template, busy_id, &f.root.join("project"))?;
    let busy = session_support::open_created_session(&paths(&f)?, &busy_id.parse()?)?;
    let output = Running::start(f.command("Continue").args(["--continue", "--json"]))?.finish()?;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        lines(&output.stdout)?
            .first()
            .and_then(|event| event.get("session_id")),
        Some(&json!(selected_id))
    );
    assert_eq!(server.requests()?.len(), 2);
    assert!(unreadable.exists() && unsupported.exists() && busy.directory().exists());
    let missing = Running::start(
        f.command("Continue")
            .arg("--continue")
            .arg("--project")
            .arg(&other)
            .arg("--mode=normal"),
    )?
    .finish()?;
    assert_eq!(missing.status.code(), Some(2));
    // 別プロジェクトは見つかるが、制限モードからの昇格は候補選択の後でも拒む。
    assert!(
        String::from_utf8_lossy(&missing.stderr).contains("cannot resume a restricted session")
    );
    server.shutdown()?;
    Ok(())
}

// 作成時の関門: 解釈だけでは分からない既定モードと利用者設定を含め、API を呼ぶ前の拒否を守る。
#[test]
fn exec_rejects_normal_options_in_restricted_mode_and_interactive_approval() -> TestResult {
    for (normal_default, options, diagnostic) in [
        (false, vec!["--sandbox", "read-only"], "require normal mode"),
        (false, vec!["--approval", "never"], "require normal mode"),
        (false, vec!["--no-sandbox"], "require normal mode"),
        (
            true,
            vec!["--mode", "restricted", "--sandbox", "workspace-write"],
            "require normal mode",
        ),
        (
            true,
            vec!["--mode", "restricted", "--approval", "never"],
            "require normal mode",
        ),
        (
            true,
            vec!["--mode", "restricted", "--no-sandbox"],
            "require normal mode",
        ),
        (
            true,
            vec!["--approval", "untrusted"],
            "exec only supports --approval never",
        ),
        (
            true,
            vec!["--approval", "on-request"],
            "exec only supports --approval never",
        ),
    ] {
        let mut server = MockProvider::start(vec![])?;
        let f = Fixture::restricted(Protocol::Chat, &server)?;
        if normal_default {
            let file = f.root.join("config/benitoiteh/config.toml");
            fs::write(
                &file,
                format!("default_mode = 'normal'\n{}", fs::read_to_string(&file)?),
            )?;
        }
        let output = Running::start(f.command("request").args(options).arg("--json"))?.finish()?;
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stdout.is_empty());
        assert!(
            String::from_utf8_lossy(&output.stderr).contains(diagnostic),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(server.requests()?.is_empty());
        server.shutdown()?;
    }
    Ok(())
}

// 作成時の関門: 一覧の順序、JSON Lines、最初の依頼の可視化と切り詰めを CLI で守る。
#[test]
fn sessions_lists_last_mode_filters_projects_and_survives_unreadable_records() -> TestResult {
    let mut server = MockProvider::start(vec![ResponseScript::sse(
        Protocol::Chat.response(None, "saved", false),
    )])?;
    let f = Fixture::restricted(Protocol::Chat, &server)?;
    let request = format!("first\x1b[2J\n{}", "日".repeat(65));
    assert!(
        Running::start(&mut f.command(&request))?
            .finish()?
            .status
            .success()
    );
    let (directory, records) = f.records()?;
    let mut updated = records.clone();
    let mut start = records
        .iter()
        .find(|r| r.get("type") == Some(&json!("session_start")))
        .ok_or("start")?
        .clone();
    *start.get_mut("mode").ok_or("mode")? = json!("normal");
    updated.push(start);
    rewrite(&directory, &updated)?;
    let template = records.first().ok_or("header")?;
    let other = f.root.join("other\x1b[2J");
    fs::create_dir(&other)?;
    let other_id = "20990101T000000Z-0000000000000001";
    let _other = saved(&f, template, other_id, &other)?;
    let bad_id = "20990101T000000Z-0000000000000002";
    let bad = saved(&f, template, bad_id, &other)?;
    fs::write(bad.join("session.jsonl"), "bad\n")?;
    let busy = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(directory.join("lock"))?;
    rustix::fs::flock(&busy, rustix::fs::FlockOperation::NonBlockingLockExclusive)?;
    let output = Running::start(sessions_command(&f).arg("--json"))?.finish()?;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let entries = lines(&output.stdout)?;
    assert_eq!(entries.len(), 3);
    assert_eq!(
        entries.first().and_then(|row| row.get("id")),
        Some(&json!(bad_id))
    );
    assert!(entries.first().and_then(|row| row.get("error")).is_some());
    let row = entries.last().ok_or("last")?;
    assert_eq!(row.get("mode"), Some(&json!("normal")));
    assert_eq!(row.get("active"), Some(&json!(true)));
    assert_eq!(
        row.get("first_request"),
        Some(&json!(benitoiteh::ui::terminal_text(
            &request.chars().take(60).collect::<String>(),
            false
        )))
    );
    let output = Running::start(
        sessions_command(&f)
            .arg("--project")
            .arg(f.root.join("project")),
    )?
    .finish()?;
    let display = String::from_utf8(output.stdout)?;
    assert!(
        display.contains(id(&directory)?)
            && display.contains(bad_id)
            && display.contains("[active]")
    );
    assert!(!display.contains(other_id) && !display.contains('\x1b'));
    let output = Running::start(&mut sessions_command(&f))?.finish()?;
    assert!(String::from_utf8(output.stdout)?.contains("other\\x1b[2J"));
    assert_eq!(server.requests()?.len(), 1);
    server.shutdown()?;
    Ok(())
}

// 関門: --no-sandbox があっても利用できるサンドボックスの中で処理系を確かめ、ヘッダへ写す。
#[test]
fn normal_query_inspects_real_processor_and_records_its_identity() -> TestResult {
    let mut server = MockProvider::start(vec![ResponseScript::sse(
        Protocol::Chat.response(None, "Done", false),
    )])?;
    let f = Fixture::new(Protocol::Chat, &server)?;
    let processor = support::benitoite()?;
    let config = f.root.join("config/benitoiteh/config.toml");
    fs::write(
        &config,
        fs::read_to_string(&config)?.replace(
            &serde_json::to_string(&f.root.join("missing-benitoite"))?,
            &serde_json::to_string(&processor)?,
        ),
    )?;
    let output = f.command("request").arg("--json").output()?;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!String::from_utf8_lossy(&output.stderr).contains("Benitoite tools are disabled"));
    let (_, records) = f.records()?;
    let header = records.first().ok_or("header")?;
    assert_eq!(header.pointer("/benitoite/path"), Some(&json!(processor)));
    assert_eq!(header.pointer("/benitoite/version"), Some(&json!("0.0.7")));
    assert_eq!(
        header
            .pointer("/benitoite/sha256")
            .and_then(Value::as_str)
            .map(str::len),
        Some(64)
    );
    assert!(
        header
            .pointer("/benitoite/build")
            .is_some_and(Value::is_object)
    );
    assert!(
        header
            .pointer("/benitoite/skill_content_hash")
            .and_then(Value::as_str)
            .is_some_and(
                |hash| hash.len() == 64 && hash.bytes().all(|byte| byte.is_ascii_hexdigit())
            )
    );
    assert!(header.get("skill_version").is_none());
    server.shutdown()?;
    Ok(())
}

// 関門: 道具の実行器のテストは処理系を直接起動する。ここでは起動時に作った
// Processor の問い合わせが、write と明示の検査の両方へ渡ることを本物の OS で守る。
#[test]
fn restricted_write_and_explicit_check_use_sandboxed_processor_queries() -> TestResult {
    let protocol = Protocol::Chat;
    let mut server = MockProvider::start(vec![
        ResponseScript::sse(protocol.response(Some(("write", "write", json!({"path":"main.bnt","content":"function main() -> Unit\n return ()\nend function\n"}))), "", false)),
        ResponseScript::sse(protocol.response(Some(("check", "benitoite_check", json!({"path":"main.bnt"}))), "", false)),
        ResponseScript::sse(protocol.response(None, "Checked", false)),
    ])?;
    let f = Fixture::restricted(protocol, &server)?;
    let output = Running::start(f.command("Write and check").arg("--json"))?.finish()?;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let (_, records) = f.records()?;
    let outputs = records
        .iter()
        .filter(|r| r.get("type") == Some(&json!("tool_result")))
        .collect::<Vec<_>>();
    assert!(
        outputs.iter().any(|r| r
            .pointer("/result/text")
            .and_then(Value::as_str)
            .is_some_and(|text| text.contains("Check of main.bnt after this change: ok"))),
        "{outputs:?}"
    );
    assert!(
        outputs
            .iter()
            .any(|r| r.pointer("/result/text") == Some(&json!("ok")))
    );
    let summary = records
        .iter()
        .find(|r| r.get("type") == Some(&json!("summary")))
        .ok_or("summary")?;
    assert_eq!(summary.get("checks"), Some(&json!(2)));
    assert_eq!(summary.get("after_write_checks"), Some(&json!(1)));
    assert_eq!(summary.get("fixes"), Some(&json!(0)));
    server.shutdown()?;
    Ok(())
}

// 関門: 残る診断が JSON のイベントと exec の stderr から欠けること、
// stderr に端末制御が通ることを、制限モードの本物の入口で守る。
#[test]
fn restricted_check_limit_emits_full_safe_diagnostics_and_flat_summary() -> TestResult {
    let protocol = Protocol::Chat;
    let mut server = MockProvider::start(vec![ResponseScript::sse(protocol.response(
        Some(("check", "benitoite_check", json!({"path":".","tests":true}))),
        "",
        false,
    ))])?;
    let f = Fixture::restricted(protocol, &server)?;
    fs::write(
        f.root.join("project/.env.bnt"),
        "function secret() -> Unit\n return SECRET_TOKEN_abc\nend function\n",
    )?;
    let output =
        Running::start(
            f.command("Check errors")
                .args(["--max-check-fix", "1", "--json"]),
        )?
        .finish()?;
    assert_eq!(output.status.code(), Some(1));
    let events = lines(&output.stdout)?;
    let error = events
        .iter()
        .find(|e| e.get("type") == Some(&json!("error")) && e.get("kind") == Some(&json!("limit")))
        .ok_or("limit error")?;
    let message = error
        .get("message")
        .and_then(Value::as_str)
        .ok_or("message")?;
    assert!(
        message.starts_with("The run stopped because checks of . failed 1 time without passing"),
        "{message}"
    );
    assert!(message.contains("\nRemaining diagnostics:\nerror["));
    assert!(message.contains("1 diagnostic in a file that benitoiteh does not read is not shown."));
    assert!(!message.contains("SECRET_TOKEN_abc"));
    let stderr = String::from_utf8(output.stderr)?;
    assert!(
        stderr.contains("\nRemaining diagnostics:\nerror["),
        "{stderr}"
    );
    assert!(
        !stderr.contains('\x1b') && stderr.contains("\\u{1b}"),
        "{stderr}"
    );
    let summary = events
        .iter()
        .find(|e| e.get("type") == Some(&json!("summary")))
        .ok_or("summary")?;
    assert_eq!(summary.get("checks"), Some(&json!(1)));
    assert_eq!(
        summary.get("reason").and_then(Value::as_str),
        message.lines().next()
    );
    let ended = events
        .iter()
        .position(|e| e.get("type") == Some(&json!("turn_end")))
        .ok_or("turn end")?;
    let failed = events
        .iter()
        .position(|e| std::ptr::eq(e, error))
        .ok_or("error index")?;
    assert!(ended < failed);
    server.shutdown()?;
    Ok(())
}

// 関門: exec が実行の要求を拒んでも、JSON Lines と記録に問い合わせの回数を残す。
#[test]
fn exec_denies_effects_outside_the_noninteractive_allow_set() -> TestResult {
    let mut server = MockProvider::start(vec![
        ResponseScript::sse(Protocol::Chat.response(
            Some(("run", "benitoite_run", json!({"path":"main.bnt"}))),
            "",
            false,
        )),
        ResponseScript::sse(Protocol::Chat.response(None, "declined", false)),
    ])?;
    let f = Fixture::restricted(Protocol::Chat, &server)?;
    fs::write(
        f.root.join("project/main.bnt"),
        "import Benitoite.Unofficial.IO.File\nfunction main() -> Unit uses File.Write\n bind _ <- File.writeText(path: \"MUST_NOT_EXIST\", text: \"ran\")\nend function\n",
    )?;
    let output = Running::start(f.command("run").arg("--json"))?.finish()?;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!f.root.join("project/MUST_NOT_EXIST").exists());
    let events = lines(&output.stdout)?;
    let summary = events.last().ok_or("summary")?;
    assert_eq!(summary.get("checks"), Some(&json!(1)));
    assert_eq!(summary.pointer("/approvals/policy_denied"), Some(&json!(1)));
    let (_, records) = f.records()?;
    assert!(records.iter().any(|r|r.get("type")==Some(&json!("tool_result")) && r.pointer("/result/text")==Some(&json!("This non-interactive run allows only these effects: none. main.bnt uses: File.Write. Narrow the uses of main, or stop and report."))));
    server.shutdown()?;
    Ok(())
}

// 関門: --allow が CLI の解決後にも届き、確認なしの Assert.Check の実行と連続拒否の終了値を守る。
#[test]
fn exec_allows_assert_check_without_permissions_and_stops_three_effect_denials() -> TestResult {
    for (denied, allow) in [(false, None), (false, Some("Assert.Check")), (true, None)] {
        let calls = if denied { 3 } else { 1 };
        let mut scripts = (0..calls)
            .map(|n| {
                ResponseScript::sse(Protocol::Chat.response(
                    Some((
                        &format!("test-{n}"),
                        "benitoite_test",
                        json!({"paths":["test.bnt"]}),
                    )),
                    "",
                    false,
                ))
            })
            .collect::<Vec<_>>();
        if !denied {
            scripts.push(ResponseScript::sse(
                Protocol::Chat.response(None, "done", false),
            ));
        }
        let mut server = MockProvider::start(scripts)?;
        let f = Fixture::restricted(Protocol::Chat, &server)?;
        let source = if denied {
            "import Benitoite.Unofficial.IO.File\n@test\nfunction write() -> Unit uses File.Write\n bind _ <- File.writeText(path: \"MUST_NOT_EXIST\", text: \"ran\")\nend function\n"
        } else {
            "import Benitoite.Assert\n@test\nfunction passes() -> Unit uses Assert.Check\n Assert.equal(actual: 1, expected: 1)\nend function\n"
        };
        fs::write(f.root.join("project/test.bnt"), source)?;
        let mut command = f.command("test");
        command.arg("--json");
        if let Some(allow) = allow {
            command.args(["--allow", allow]);
        }
        let output = Running::start(&mut command)?.finish()?;
        assert_eq!(
            output.status.code(),
            Some(i32::from(denied)),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let events = lines(&output.stdout)?;
        let summary = events.last().ok_or("summary")?;
        assert_eq!(summary.get("checks"), Some(&json!(3)));
        assert_eq!(
            summary.pointer("/approvals/policy_denied"),
            Some(&json!(if denied { 3 } else { 0 }))
        );
        let (_, records) = f.records()?;
        assert!(
            !records
                .iter()
                .any(|r| r.get("type") == Some(&json!("approval")))
        );
        if denied {
            assert!(events.iter().any(|e| e.get("type") == Some(&json!("error"))
                && e.get("kind") == Some(&json!("limit"))));
            assert!(!f.root.join("project/MUST_NOT_EXIST").exists());
        } else {
            assert!(records.iter().any(|r| {
                r.pointer("/result/text")
                    .and_then(Value::as_str)
                    .is_some_and(|s| s.contains("All tests passed."))
            }));
            assert!(
                records
                    .iter()
                    .any(|r| r.get("type") == Some(&json!("checkpoint"))
                        && r.pointer("/reason") == Some(&json!("benitoite_test")))
            );
        }
        server.shutdown()?;
    }
    Ok(())
}

// 関門: 再開で消えた Skill の古い索引を、読める根として復元しないことを入口で守る。
#[test]
fn loaded_user_skill_runs_in_restricted_exec_and_disappears_from_scope_on_resume() -> TestResult {
    let mut server = MockProvider::start(vec![
        ResponseScript::sse(Protocol::Chat.response(
            Some(("read", "read", json!({"path":"skill-link/reference.txt"}))),
            "",
            false,
        )),
        ResponseScript::sse(Protocol::Chat.response(
            Some((
                "run",
                "benitoite_run",
                json!({"path":"skill-link/main.bnt"}),
            )),
            "",
            false,
        )),
        ResponseScript::sse(Protocol::Chat.response(None, "Done", false)),
        ResponseScript::sse(Protocol::Chat.response(
            Some((
                "old-read",
                "read",
                json!({"path":"skill-link/reference.txt"}),
            )),
            "",
            false,
        )),
        ResponseScript::sse(Protocol::Chat.response(None, "Resumed", false)),
    ])?;
    let f = Fixture::restricted(Protocol::Chat, &server)?;
    let skill = f.root.join("home/.agents/skills/sample");
    fs::create_dir_all(&skill)?;
    fs::write(
        skill.join("SKILL.md"),
        "---\nname: sample\ndescription: Test skill\n---\n",
    )?;
    fs::write(skill.join("reference.txt"), "skill reference")?;
    fs::write(
        skill.join("main.bnt"),
        "function main() -> Unit\n return ()\nend function\n",
    )?;
    std::os::unix::fs::symlink(
        "../home/.agents/skills/sample",
        f.root.join("project/skill-link"),
    )?;
    let output = Running::start(f.command("Use the skill").arg("--json"))?.finish()?;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let (directory, records) = f.records()?;
    let results = records
        .iter()
        .filter(|record| record.get("type") == Some(&json!("tool_result")))
        .collect::<Vec<_>>();
    assert_eq!(results.len(), 2);
    assert!(
        results
            .iter()
            .all(|record| record.pointer("/result/is_error") == Some(&json!(false))),
        "{results:?}"
    );
    let header = records.first().ok_or("header")?.clone();
    assert_eq!(
        header.get("skills").and_then(Value::as_array).map(Vec::len),
        Some(1)
    );
    fs::remove_file(skill.join("SKILL.md"))?;
    let output = Running::start(
        f.command("Continue").args([
            "--json",
            "--resume",
            directory
                .file_name()
                .and_then(|name| name.to_str())
                .ok_or("id")?,
        ]),
    )?
    .finish()?;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let (_, records) = f.records()?;
    assert_eq!(records.first(), Some(&header));
    let resumed = records
        .iter()
        .find(|record| record.get("resumed") == Some(&json!(true)))
        .ok_or("resumed")?;
    assert!(resumed.get("skills").is_none());
    let result = records
        .iter()
        .rev()
        .find(|record| record.get("type") == Some(&json!("tool_result")))
        .ok_or("result")?;
    assert_eq!(result.pointer("/result/is_error"), Some(&json!(true)));
    assert!(!result.to_string().contains("skill reference"));
    server.shutdown()?;
    Ok(())
}

// 関門: 記述子での切り替えだけでは、問い合わせと実行の古い OS の根・continue の誤照合を捕まえない。
#[test]
fn created_project_replaces_query_run_and_continue_roots() -> TestResult {
    let script = "import Benitoite.Unofficial.IO.File\nimport Benitoite.Unofficial.IO.Console\nfunction main() -> Unit uses File.Write, Console.Write\n  bind local <- File.writeText(path: \"local.txt\", text: \"allowed\")\n  bind old <- File.writeText(path: \"../forbidden.txt\", text: \"forbidden\")\n  Console.writeLine(\"attempted\")\nend function\n";
    let mut server = MockProvider::start(vec![
        ResponseScript::sse(Protocol::Chat.response(
            Some(("create", "create_project", json!({"path":"created"}))),
            "",
            false,
        )),
        ResponseScript::sse(Protocol::Chat.response(
            Some(("check", "benitoite_check", json!({"path":"main.bnt"}))),
            "",
            false,
        )),
        ResponseScript::sse(Protocol::Chat.response(
            Some(("run", "benitoite_run", json!({"path":"main.bnt"}))),
            "",
            false,
        )),
        ResponseScript::sse(Protocol::Chat.response(
            Some((
                "write",
                "write",
                json!({"path":"writer.bnt","content":script}),
            )),
            "",
            false,
        )),
        ResponseScript::sse(Protocol::Chat.response(
            Some(("writer", "benitoite_run", json!({"path":"writer.bnt"}))),
            "",
            false,
        )),
        ResponseScript::sse(Protocol::Chat.response(None, "Created and checked", false)),
        ResponseScript::sse(Protocol::Chat.response(None, "Continued from new project", false)),
    ])?;
    let f = Fixture::restricted(Protocol::Chat, &server)?;
    let old = f.root.join("project");
    let new = old.join("created");
    let output = Running::start(
        f.command("Create")
            .args([
                "--json",
                "--allow",
                "Console.Write,File.Write",
                "--project-parent",
            ])
            .arg(&old),
    )?
    .finish()?;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(fs::read_to_string(new.join("local.txt"))?, "allowed");
    assert!(!old.join("forbidden.txt").exists());
    let events = lines(&output.stdout)?;
    assert!(events.iter().any(
        |event| event.get("type") == Some(&json!("project_dir_changed"))
            && event.get("project_dir") == Some(&json!(new))
    ));
    let (directory, records) = f.records()?;
    let results = records
        .iter()
        .filter(|r| r.get("type") == Some(&json!("tool_result")))
        .collect::<Vec<_>>();
    assert_eq!(results.len(), 5);
    assert!(
        results
            .iter()
            .all(|r| r.pointer("/result/is_error") == Some(&json!(false))),
        "{results:?}"
    );
    assert!(results.iter().any(|r| {
        r.pointer("/result/text")
            .and_then(Value::as_str)
            .is_some_and(|s| s.contains("hello, world."))
    }));
    let previous =
        Running::start(f.command("Continue old").args(["--continue", "--json"]))?.finish()?;
    assert_eq!(previous.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&previous.stderr).contains("No resumable session"));
    let continued = Running::start(
        f.command("Continue new")
            .current_dir(&new)
            .args(["--continue", "--json"]),
    )?
    .finish()?;
    assert!(
        continued.status.success(),
        "{}",
        String::from_utf8_lossy(&continued.stderr)
    );
    assert_eq!(
        lines(&continued.stdout)?
            .first()
            .and_then(|e| e.get("session_id")),
        Some(&json!(id(&directory)?))
    );
    assert_eq!(server.requests()?.len(), 7);
    server.shutdown()?;
    Ok(())
}
