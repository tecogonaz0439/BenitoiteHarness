//! 再開・一覧の契約を、実行ファイルと実ファイルの境界で守る（設計書 07-01）。

#[path = "../support/session.rs"]
mod session_support;

use super::*;
use benitoiteh::config::paths::DataPaths;
use benitoiteh::provider::Item;
use benitoiteh::session::{Record, ResumeState, Session};
use std::os::unix::fs::symlink;

fn paths(f: &Fixture) -> Result<DataPaths, Box<dyn Error>> {
    Ok(DataPaths::from_values(
        Some(&f.root.join("home")),
        Some(&f.root.join("config")),
        Some(&f.root.join("state")),
        Some(&f.root.join("tmp")),
    )?)
}

fn rewrite(directory: &std::path::Path, records: &[Value]) -> TestResult {
    fs::write(
        directory.join("session.jsonl"),
        records
            .iter()
            .map(serde_json::to_string)
            .collect::<Result<Vec<_>, _>>()?
            .join("\n")
            + "\n",
    )?;
    Ok(())
}

fn id(directory: &std::path::Path) -> Result<&str, Box<dyn Error>> {
    directory
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| "missing ID".into())
}

fn saved(
    f: &Fixture,
    template: &Value,
    session_id: &str,
    project: &std::path::Path,
) -> Result<PathBuf, Box<dyn Error>> {
    let session = Session::create(&paths(f)?, session_id.parse()?)?;
    let mut header = template.clone();
    *header.get_mut("session_id").ok_or("session_id")? = json!(session_id);
    *header.get_mut("project_dir").ok_or("project_dir")? = json!(project);
    *header.get_mut("started_at").ok_or("started_at")? = json!(session.id().started_at_utc());
    session.writer()?.append(&serde_json::from_value(header)?)?;
    Ok(session.directory().to_owned())
}

fn sessions_command(f: &Fixture) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_benitoiteh"));
    command
        .arg("sessions")
        .current_dir(f.root.join("project"))
        .env("HOME", f.root.join("home"))
        .env("XDG_CONFIG_HOME", f.root.join("config"))
        .env("XDG_STATE_HOME", f.root.join("state"))
        .env("TMPDIR", f.root.join("tmp"))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    command
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
        let f = Fixture::new(protocol, &server)?;
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
    let f = Fixture::new(Protocol::Chat, &server)?;
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

// 作成時の関門: 中断した副作用を自動で繰り返す退行を、実際に変更済みのファイルで守る。
#[test]
fn resume_records_missing_results_without_reexecuting_tools() -> TestResult {
    for started in [true, false] {
        let protocol = Protocol::Chat;
        let mut server = MockProvider::start(vec![
            ResponseScript::sse(protocol.response(
                Some((
                    "write",
                    "write",
                    json!({"path":"a.bnt", "content":"written"}),
                )),
                "",
                false,
            )),
            ResponseScript::sse(protocol.response(None, "done", false)),
            ResponseScript::sse(protocol.response(None, "resumed", false)),
        ])?;
        let f = Fixture::new(protocol, &server)?;
        assert!(
            Running::start(&mut f.command("Write"))?
                .finish()?
                .status
                .success()
        );
        let (directory, records) = f.records()?;
        let mut interrupted = Vec::new();
        for record in records {
            if record.get("type") == Some(&json!("tool_result")) {
                break;
            }
            if started || record.get("type") != Some(&json!("tool_start")) {
                interrupted.push(record);
            }
        }
        rewrite(&directory, &interrupted)?;
        fs::write(f.root.join("project/a.bnt"), "changed after interruption")?;
        let output = Running::start(
            f.command("Continue safely")
                .args(["--resume", id(&directory)?]),
        )?
        .finish()?;
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            fs::read_to_string(f.root.join("project/a.bnt"))?,
            "changed after interruption"
        );
        let records = benitoiteh::session::read_records(&directory)?;
        let filled = records
            .iter()
            .find_map(|record| {
                if let Record::ToolResult { result, .. } = record {
                    Some(result)
                } else {
                    None
                }
            })
            .ok_or("missing result")?;
        assert!(filled.is_error);
        assert_eq!(
            filled.text,
            if started {
                "Tool call was interrupted before its result was recorded. Its effects are unknown; check the files before retrying."
            } else {
                "Tool call was cancelled by the user."
            }
        );
        assert_eq!(ResumeState::read(&directory)?.missing_results.len(), 0);
        let persisted = lines(&fs::read(directory.join("session.jsonl"))?)?;
        let filled_at = persisted
            .iter()
            .position(|record| record.get("type") == Some(&json!("tool_result")))
            .ok_or("filled result")?;
        let resume_at = persisted
            .iter()
            .position(|record| record.get("resumed") == Some(&json!(true)))
            .ok_or("resumed")?;
        assert!(filled_at < resume_at);
        assert_eq!(server.requests()?.len(), 3);
        server.shutdown()?;
    }
    Ok(())
}

// 作成時の関門: 走査中の錠と、最後のプロジェクトの照合は、明示 ID の再開では守れない。
#[test]
fn continue_selects_latest_matching_readable_inactive_session() -> TestResult {
    let mut server = MockProvider::start(vec![
        ResponseScript::sse(Protocol::Chat.response(None, "initial", false)),
        ResponseScript::sse(Protocol::Chat.response(None, "continued", false)),
    ])?;
    let f = Fixture::new(Protocol::Chat, &server)?;
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

// 作成時の関門: 一覧の順序、JSON Lines、最初の依頼の可視化と切り詰めを CLI で守る。
#[test]
fn sessions_lists_last_mode_filters_projects_and_survives_unreadable_records() -> TestResult {
    let mut server = MockProvider::start(vec![ResponseScript::sse(
        Protocol::Chat.response(None, "saved", false),
    )])?;
    let f = Fixture::new(Protocol::Chat, &server)?;
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

// 作成時の関門: 二度目の再開で、最後の session_start だけを比べて古い署名を再送する
// 退行を捕まえる。ハッシュが同じ場合の送り返しは基本の再開の試験が守る。
#[test]
fn repeated_resume_filters_reasoning_per_segment() -> TestResult {
    for protocol in [Protocol::Anthropic, Protocol::Responses] {
        let first = protocol.response(
            Some(("read", "read", json!({"path":"a.bnt"}))),
            "first",
            false,
        );
        let first = String::from_utf8(first)?
            .replace("Plan", "OLD-REASONING")
            .replace("opaque+/=", "OLD-SIGNATURE")
            .replace("encrypted+/=", "OLD-ENCRYPTED")
            .into_bytes();
        let mut server = MockProvider::start(vec![
            ResponseScript::sse(first),
            ResponseScript::sse(protocol.response(None, "saved", false)),
            ResponseScript::sse(protocol.response(
                Some(("read", "read", json!({"path":"a.bnt"}))),
                "new",
                false,
            )),
            ResponseScript::sse(protocol.response(None, "resumed once", false)),
            ResponseScript::sse(protocol.response(None, "resumed twice", false)),
        ])?;
        let f = Fixture::new(protocol, &server)?;
        assert!(
            Running::start(&mut f.command("First"))?
                .finish()?
                .status
                .success()
        );
        let (directory, mut records) = f.records()?;
        for record in &mut records {
            if let Some(hash) = record.get_mut("system_prompt_sha256") {
                *hash = json!("old-hash");
            }
        }
        if matches!(protocol, Protocol::Responses) {
            for record in &mut records {
                if record.get("type") == Some(&json!("session_start")) {
                    let fields = record.as_object_mut().ok_or("session start")?;
                    fields.remove("system_prompt_sha256");
                    fields.remove("tools_sha256");
                    fields.remove("base_url");
                    fields.remove("resumed");
                }
            }
        }
        rewrite(&directory, &records)?;
        for request in ["Resume once", "Resume twice"] {
            let output =
                Running::start(f.command(request).args(["--resume", id(&directory)?]))?.finish()?;
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        let requests = server.requests()?;
        for index in [2, 3, 4] {
            let body = requests.get(index).ok_or("request")?.body.to_string();
            assert!(
                !body.contains("OLD-REASONING")
                    && !body.contains("OLD-SIGNATURE")
                    && !body.contains("OLD-ENCRYPTED"),
                "{body}"
            );
        }
        let last = requests.last().ok_or("last request")?.body.to_string();
        assert!(last.contains("Plan"));
        if matches!(protocol, Protocol::Responses) {
            let input = requests
                .get(2)
                .ok_or("request")?
                .body
                .get("input")
                .and_then(Value::as_array)
                .ok_or("input")?;
            assert!(
                input
                    .iter()
                    .filter(|item| item.get("type") == Some(&json!("function_call")))
                    .all(|item| item.get("id").is_none())
            );
        }
        server.shutdown()?;
    }
    Ok(())
}

// 作成時の関門: all-turns では、不整合な推論を除くだけでは要求が成立しない。
// 初回の要求の順序・形と、要約が失敗したときの停止を入口で守る。
#[test]
fn all_turns_resume_forces_separate_compaction_and_stops_on_failure() -> TestResult {
    for success in [true, false] {
        let protocol = Protocol::Chat;
        let summary = if success {
            ResponseScript::sse(protocol.response(None, "summary", false))
        } else {
            ResponseScript::json(400, &json!({"error":{"message":"invalid request"}}))?
        };
        let mut script = vec![
            ResponseScript::sse(protocol.response(None, "saved", false)),
            summary,
        ];
        if success {
            script.push(ResponseScript::sse(protocol.response(
                None,
                "new answer",
                false,
            )));
        }
        let mut server = MockProvider::start(script)?;
        let f = Fixture::new(protocol, &server)?;
        assert!(
            Running::start(&mut f.command("Old request"))?
                .finish()?
                .status
                .success()
        );
        let (directory, _) = f.records()?;
        let config = f.root.join("config/benitoiteh/config.toml");
        let text = fs::read_to_string(&config)?;
        fs::write(
            &config,
            format!(
                "{text}\n[providers.other]\nkind='openai-chat'\nbase_url='{}'\nmodel='other-default'\ncapabilities = {{ reasoning_replay='all-turns', tool_choice_none=true }}\n",
                server.base_url()
            ),
        )?;
        let output = Running::start(f.command("New request").args([
            "--resume",
            id(&directory)?,
            "--provider",
            "other",
            "--json",
        ]))?
        .finish()?;
        assert_eq!(
            output.status.code(),
            Some(if success { 0 } else { 1 }),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let requests = server.requests()?;
        let summary = &requests.get(1).ok_or("summary")?.body;
        assert!(summary.get("tools").is_none());
        assert!(summary.to_string().contains("<conversation>"));
        assert!(!summary.to_string().contains("New request"));
        assert_eq!(summary.get("model"), Some(&json!("other-default")));
        if success {
            let events = lines(&output.stdout)?;
            assert!(
                events
                    .iter()
                    .any(|e| e.get("reason") == Some(&json!("resume")))
            );
            let body = requests.last().ok_or("new request")?.body.to_string();
            assert!(body.contains("<compacted_summary>") && body.contains("New request"));
            assert!(!body.contains("saved"));
            assert_eq!(requests.len(), 3);
        } else {
            assert_eq!(requests.len(), 2);
            assert!(
                lines(&output.stdout)?
                    .iter()
                    .any(|e| e.get("kind") == Some(&json!("compaction")))
            );
        }
        server.shutdown()?;
    }
    Ok(())
}

// 作成時の関門: 日付と読み直した指示は、会話の先頭を書き換えず次の依頼に付ける。
// 同じ描画を追加しないことは基本の再開の試験が守る。
#[test]
fn resume_adds_changed_date_and_instructions_before_request_and_checks_provider_names() -> TestResult
{
    let protocol = Protocol::Chat;
    let mut server = MockProvider::start(vec![
        ResponseScript::sse(protocol.response(None, "saved", false)),
        ResponseScript::sse(protocol.response(None, "new answer", false)),
    ])?;
    let f = Fixture::new(protocol, &server)?;
    fs::write(f.root.join("project/AGENTS.md"), "old instructions")?;
    let output = Running::start(f.command("Old request").args([
        "--mode",
        "normal",
        "--sandbox",
        "danger-full-access",
    ]))?
    .finish()?;
    assert!(output.status.success());
    let (directory, mut records) = f.records()?;
    for record in &mut records {
        if let Some(parts) = record
            .pointer_mut("/message/parts")
            .and_then(Value::as_array_mut)
        {
            for part in parts {
                if part.get("kind") == Some(&json!("environment")) {
                    let text = part.get("text").and_then(Value::as_str).ok_or("text")?;
                    let start = text.find("<date_utc>").ok_or("date")?;
                    let end = text.find("</date_utc>").ok_or("date")?;
                    *part.get_mut("text").ok_or("text")? = json!(format!(
                        "{}<date_utc>2000-01-01{}",
                        text.get(..start).ok_or("start")?,
                        text.get(end..).ok_or("end")?
                    ));
                }
            }
        }
    }
    rewrite(&directory, &records)?;
    fs::write(f.root.join("project/AGENTS.md"), "new instructions")?;
    let output = Running::start(f.command("New request").args([
        "--resume",
        id(&directory)?,
        "--sandbox",
        "danger-full-access",
        "--model",
        "override",
    ]))?
    .finish()?;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let conversation = benitoiteh::session::replay(&directory)?;
    let Some(Item::UserMessage(message)) = conversation
        .items
        .iter()
        .rev()
        .find(|item| matches!(item, Item::UserMessage(_)))
    else {
        return Err("user".into());
    };
    assert_eq!(message.parts.len(), 2);
    let changes = &message.parts.first().ok_or("changes")?.text;
    assert!(changes.contains("<date_utc>") && changes.contains("new instructions"));
    assert!(
        !changes.contains("<mode>")
            && !changes.contains("<os>")
            && !changes.contains("old instructions")
    );
    assert_eq!(message.parts.last().ok_or("request")?.text, "New request");
    assert_eq!(
        server
            .requests()?
            .last()
            .ok_or("request")?
            .body
            .get("model"),
        Some(&json!("override"))
    );
    let config = f.root.join("config/benitoiteh/config.toml");
    fs::write(
        &config,
        fs::read_to_string(&config)?
            .replace("providers.mock", "providers.renamed")
            .replace(
                "default_provider = \"mock\"",
                "default_provider = \"renamed\"",
            ),
    )?;
    let output = Running::start(
        f.command("Cannot resume")
            .args(["--resume", id(&directory)?]),
    )?
    .finish()?;
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("mock"));
    assert_eq!(server.requests()?.len(), 2);
    server.shutdown()?;
    Ok(())
}

// 作成時の関門: cleanup を再開先の錠より前に行うと、明示した古いセッションを消す。
// 一覧の起動でも、動いているものと更新済みのものを残すことを確かめる。
#[test]
fn startup_retention_uses_log_mtime_and_preserves_explicit_resume_and_symlink_targets() -> TestResult
{
    let mut server = MockProvider::start(vec![
        ResponseScript::sse(Protocol::Chat.response(None, "saved", false)),
        ResponseScript::sse(Protocol::Chat.response(None, "resumed", false)),
    ])?;
    let f = Fixture::new(Protocol::Chat, &server)?;
    assert!(
        Running::start(&mut f.command("Initial"))?
            .finish()?
            .status
            .success()
    );
    let (directory, records) = f.records()?;
    let template = records.first().ok_or("header")?;
    let expired = saved(
        &f,
        template,
        "20000101T000000Z-0000000000000001",
        &f.root.join("project"),
    )?;
    let busy_id = "20000101T000000Z-0000000000000002";
    let _busy = saved(&f, template, busy_id, &f.root.join("project"))?;
    let busy = session_support::open_created_session(&paths(&f)?, &busy_id.parse()?)?;
    let updated = saved(
        &f,
        template,
        "20000101T000000Z-0000000000000003",
        &f.root.join("project"),
    )?;
    for directory in [&directory, &expired, &busy.directory().to_owned()] {
        fs::File::open(directory.join("session.jsonl"))?.set_modified(SystemTime::UNIX_EPOCH)?;
    }
    fs::write(f.root.join("home/keep"), "keep")?;
    symlink(f.root.join("home"), expired.join("external"))?;
    let linked = paths(&f)?
        .sessions_dir
        .join("20000101T000000Z-0000000000000004");
    symlink(f.root.join("home"), &linked)?;
    let output = Running::start(
        f.command("Explicit old session")
            .args(["--resume", id(&directory)?]),
    )?
    .finish()?;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(directory.exists() && updated.exists() && busy.directory().exists());
    assert!(!expired.exists());
    assert!(fs::symlink_metadata(&linked)?.is_symlink());
    assert_eq!(fs::read_to_string(f.root.join("home/keep"))?, "keep");
    let output = Running::start(sessions_command(&f).arg("--json"))?.finish()?;
    assert!(output.status.success());
    assert!(directory.exists());
    assert_eq!(server.requests()?.len(), 2);
    server.shutdown()?;
    Ok(())
}

// 作成時の関門: 実ファイルの起動時の走査から bash の出力までをつなぎ、保存失敗の
// 印の欠落と、超過の通知を繰り返す退行を捕まえる。疎なファイルで実容量を使わない。
#[test]
fn startup_quota_stops_bash_spill_and_warns_once_without_stopping_log() -> TestResult {
    let protocol = Protocol::Chat;
    let mut server = MockProvider::start(vec![
        ResponseScript::sse(protocol.response(
            Some(("before", "bash", json!({"command":"cat input"}))),
            "",
            false,
        )),
        ResponseScript::sse(protocol.response(None, "saved", false)),
        ResponseScript::sse(protocol.response(
            Some(("one", "bash", json!({"command":"cat input"}))),
            "",
            false,
        )),
        ResponseScript::sse(protocol.response(
            Some(("two", "bash", json!({"command":"cat input"}))),
            "",
            false,
        )),
        ResponseScript::sse(protocol.response(None, "done", false)),
    ])?;
    let f = Fixture::new(protocol, &server)?;
    let input = "line\n".repeat(2100);
    fs::write(f.root.join("project/input"), &input)?;
    assert!(
        Running::start(f.command("Initial").args([
            "--mode",
            "normal",
            "--sandbox",
            "danger-full-access"
        ]))?
        .finish()?
        .status
        .success()
    );
    let (directory, records) = f.records()?;
    let previous_spill = records
        .iter()
        .find_map(|record| record.pointer("/spill/path").and_then(Value::as_str))
        .ok_or("previous spill")?;
    assert_eq!(fs::read_to_string(directory.join(previous_spill))?, input);
    let other = saved(
        &f,
        records.first().ok_or("header")?,
        "20990101T000000Z-0000000000000001",
        &f.root.join("project"),
    )?;
    fs::File::create(other.join("sparse"))?.set_len(10_u64.saturating_mul(1024 * 1024 * 1024))?;
    fs::write(f.root.join("project/input"), "line\n".repeat(2100))?;
    let output = Running::start(f.command("Read twice").args([
        "--resume",
        id(&directory)?,
        "--sandbox",
        "danger-full-access",
        "--json",
    ]))?
    .finish()?;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stderr = String::from_utf8(output.stderr)?;
    assert_eq!(stderr.matches("Session storage limit reached;").count(), 1);
    assert!(stderr.contains("Session storage exceeds 10 GiB; reduce [sessions] retention_days"));
    let records = benitoiteh::session::read_records(&directory)?;
    let mut results = Vec::new();
    let mut resumed = false;
    for record in &records {
        if matches!(record, Record::SessionStart { resumed: true, .. }) {
            resumed = true;
        }
        if let Record::ToolResult {
            name,
            result,
            spill,
            ..
        } = record
            && name == "bash"
            && resumed
        {
            assert!(spill.is_none());
            results.push(&result.text);
        }
    }
    assert_eq!(results.len(), 2);
    assert!(
        results
            .iter()
            .all(|text| text.contains("Full output was not saved.")),
        "missing unsaved-output marker"
    );
    assert!(matches!(records.last(), Some(Record::Summary { .. })));
    assert_eq!(fs::read_dir(directory.join("spill"))?.count(), 1);
    assert_eq!(fs::read_to_string(directory.join(previous_spill))?, input);
    server.shutdown()?;
    Ok(())
}

// 作成時の関門: 絞り込み先の解決失敗を出力の失敗（1）と混同する退行を入口で守る。
#[test]
fn sessions_rejects_missing_project_as_usage_error() -> TestResult {
    let mut server = MockProvider::start(vec![])?;
    let f = Fixture::new(Protocol::Chat, &server)?;
    let output = Running::start(
        sessions_command(&f)
            .arg("--project")
            .arg(f.root.join("missing")),
    )?
    .finish()?;
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert!(server.requests()?.is_empty());
    server.shutdown()?;
    Ok(())
}

// 作成時の関門: 要約対象がない再開では、外国の推論が残る場合だけ停止する。
#[test]
fn resume_nothing_to_compact_only_stops_for_remaining_foreign_reasoning() -> TestResult {
    for reasoning in [false, true] {
        let protocol = Protocol::Chat;
        let mut server = MockProvider::start(vec![
            ResponseScript::sse(protocol.response(None, "saved", false)),
            ResponseScript::sse(protocol.response(None, "resumed", false)),
        ])?;
        let f = Fixture::new(protocol, &server)?;
        assert!(
            Running::start(&mut f.command("Initial"))?
                .finish()?
                .status
                .success()
        );
        let (directory, mut records) = f.records()?;
        records.retain(|record| record.get("type") != Some(&json!("user")));
        if reasoning {
            let response = records
                .iter_mut()
                .find_map(|record| record.get_mut("response"))
                .ok_or("response")?;
            response.get_mut("parts").and_then(Value::as_array_mut).ok_or("parts")?.push(json!({"kind":"reasoning", "content":{"text":"foreign reasoning", "opaque":null}}));
            *response.pointer_mut("/raw_items/items").ok_or("raw")? = json!([{"role":"assistant", "content":"saved", "reasoning_content":"foreign reasoning"}]);
        }
        rewrite(&directory, &records)?;
        let config = f.root.join("config/benitoiteh/config.toml");
        fs::write(
            &config,
            format!(
                "{}\n[providers.other]\nkind='openai-chat'\nbase_url='{}'\nmodel='mock-model'\ncapabilities={{ reasoning_replay='all-turns' }}\n",
                fs::read_to_string(&config)?,
                server.base_url()
            ),
        )?;
        let output = Running::start(f.command("Resume").args([
            "--resume",
            id(&directory)?,
            "--provider",
            "other",
        ]))?
        .finish()?;
        assert_eq!(
            output.status.code(),
            Some(if reasoning { 1 } else { 0 }),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(server.requests()?.len(), if reasoning { 1 } else { 2 });
        server.shutdown()?;
    }
    Ok(())
}
