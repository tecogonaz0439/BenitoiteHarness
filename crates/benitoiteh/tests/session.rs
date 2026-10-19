//! 記録の保存と復元、一時領域の寿命を実ファイルで確かめる（設計書 01-05）。

#[path = "support/session.rs"]
mod session_support;

use std::error::Error;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use benitoiteh::config::Mode;
use benitoiteh::config::paths::DataPaths;
use benitoiteh::provider::{
    AdapterKind, AssistantPart, AssistantTurn, Conversation, Destination, FinishReason, Item,
    RawItems, ToolCall, ToolResult, Usage, UserMessage, UserPart, UserPartKind,
};
use benitoiteh::session::{
    ApprovalOutcome, DEFAULT_RETENTION_DAYS, FORMAT_VERSION, Header, MAX_BYTES, Record,
    RecordedProvider, Session, SessionId, Summary, UserKind, cleanup, read_records, replay,
};
use serde_json::json;

type TestResult = Result<(), Box<dyn Error>>;

struct Fixture {
    root: PathBuf,
    paths: DataPaths,
}

impl Fixture {
    fn new() -> Result<Self, Box<dyn Error>> {
        let id = SessionId::generate(SystemTime::now())?;
        let root = std::env::temp_dir().join(format!("benitoiteh-session-test-{id}"));
        fs::create_dir(&root)?;
        let root = fs::canonicalize(root)?;
        fs::create_dir(root.join("home"))?;
        fs::create_dir(root.join("tmp"))?;
        let paths = DataPaths::from_values(
            Some(&root.join("home")),
            Some(&root.join("config")),
            Some(&root.join("state")),
            Some(&root.join("tmp")),
        )?;
        Ok(Self { root, paths })
    }

    fn session(&self) -> Result<Session, Box<dyn Error>> {
        Ok(Session::create(
            &self.paths,
            SessionId::generate(SystemTime::now())?,
        )?)
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        if let Err(error) = fs::remove_dir_all(&self.root) {
            eprintln!("Could not remove session test directory: {error}");
        }
    }
}

fn header(session: &Session) -> Record {
    Record::Header(Header {
        format: FORMAT_VERSION,
        session_id: session.id().to_string(),
        started_at: session.id().started_at_utc(),
        benitoiteh_version: "0.0.1".to_owned(),
        mode: Mode::Restricted.into(),
        project_dir: PathBuf::from("/project"),
        provider: RecordedProvider {
            name: "test".to_owned(),
            base_url: "https://example.test".to_owned(),
            model: "model".to_owned(),
            adapter: AdapterKind::OpenAiResponses,
        },
        benitoite: None,
        system_prompt_sha256: "a".repeat(64),
        tools_sha256: "b".repeat(64),
        after_write_check: None,
        read_line_numbers: None,
        skills: Vec::new(),
    })
}

fn call(id: &str) -> ToolCall {
    ToolCall {
        id: id.to_owned(),
        provider_id: Some(format!("provider-{id}")),
        provider_item_id: Some(format!("item-{id}")),
        name: "write".to_owned(),
        arguments: json!({"path":"a.bnt", "text":"hello"}),
        raw_arguments: "{\"path\":\"a.bnt\",\"text\":\"hello\"}".to_owned(),
    }
}

fn response(calls: &[ToolCall]) -> AssistantTurn {
    let mut parts = vec![AssistantPart::Text("running tools".to_owned())];
    parts.extend(calls.iter().cloned().map(AssistantPart::ToolCall));
    AssistantTurn {
        parts,
        raw_items: RawItems {
            destination: Destination {
                provider: "test".to_owned(),
                base_url: "https://example.test".to_owned(),
                model: "model".to_owned(),
            },
            adapter: AdapterKind::OpenAiResponses,
            items: vec![
                json!({"id":"reason-7", "type":"reasoning", "encrypted_content":"opaque", "signature":"signed"}),
                json!({"id":"item-2", "call_id":"provider-second", "type":"function_call"}),
                json!({"id":"item-1", "call_id":"provider-first", "type":"function_call"}),
            ],
        },
        finish_reason: FinishReason::ToolCalls,
        usage: Usage {
            input_tokens: Some(100),
            cache_read_tokens: Some(20),
            cache_write_tokens: Some(3),
            output_tokens: Some(9),
            ..Usage::default()
        },
    }
}

fn result(call: &ToolCall, text: &str) -> ToolResult {
    ToolResult {
        id: call.id.clone(),
        provider_id: call.provider_id.clone(),
        text: text.to_owned(),
        is_error: false,
        images: Vec::new(),
    }
}

// 作成時の関門: 保存した会話の順序・署名・ID が変わると、再開後の要求を壊す。
// exec は未実装なので、記録の持ち主の公開 API と本物のファイルで契約を守る（P1-05）。
#[test]
fn records_replay_conversation_losslessly_and_finish_with_summary() -> TestResult {
    let fixture = Fixture::new()?;
    let session = fixture.session()?;
    // 新しい欄を省略した従来の形式を、そのまま読める（P5-06）。
    let legacy = serde_json::to_value(header(&session))?;
    assert!(legacy.get("after_write_check").is_none());
    assert!(legacy.get("read_line_numbers").is_none());
    assert!(legacy.get("skills").is_none());
    let Record::Header(loaded) = serde_json::from_value(legacy)? else {
        return Err("header required".into());
    };
    assert_eq!(loaded.after_write_check, None);
    assert_eq!(loaded.read_line_numbers, None);
    assert!(loaded.skills.is_empty());
    let mut writer = session.writer()?;
    let mut summary_writer = session.writer()?;
    writer.append(&header(&session))?;
    assert!(summary_writer.append(&header(&session)).is_err());
    let message = UserMessage {
        parts: vec![
            UserPart {
                kind: UserPartKind::Environment,
                text: "environment".to_owned(),
            },
            UserPart {
                kind: UserPartKind::Request,
                text: "request".to_owned(),
            },
        ],
    };
    let calls = [call("first"), call("second")];
    let response = response(&calls);
    // 保存順を再読込で書き換えないことも独立に確かめる。
    let second = result(
        calls
            .get(1)
            .ok_or_else(|| io::Error::from(io::ErrorKind::NotFound))?,
        "second result",
    );
    let first = result(
        calls
            .first()
            .ok_or_else(|| io::Error::from(io::ErrorKind::NotFound))?,
        "first result",
    );
    writer.append(&Record::User {
        kind: UserKind::Request,
        message: message.clone(),
    })?;
    writer.append(&Record::Assistant {
        turn: 1,
        response: response.clone(),
    })?;
    for result in [&second, &first] {
        writer.append(&Record::ToolResult {
            turn: 1,
            name: "write".to_owned(),
            result: result.clone(),
            spill: None,
            stderr_spill: None,
        })?;
    }
    let mut summary = Summary::default();
    summary.add_turn(&response.usage);
    summary.add_tool_call("write");
    summary.add_tool_call("write");
    summary.add_approval(ApprovalOutcome::Approved);
    summary.add_approval(ApprovalOutcome::ApprovedForSession);
    summary.add_approval(ApprovalOutcome::Denied);
    summary.add_approval(ApprovalOutcome::DeniedWithMessage);
    summary.add_approval(ApprovalOutcome::Cancelled);
    summary.checks = 2;
    summary.fixes = 1;
    summary.elapsed_ms = 123;
    summary.reason = "completed".to_owned();
    writer.append(&Record::SessionEnd {
        outcome: "completed".to_owned(),
        reason: "completed".to_owned(),
    })?;
    summary_writer.append(&Record::Summary { summary })?;
    let expected = Conversation {
        items: vec![
            Item::UserMessage(message),
            Item::AssistantTurn(response),
            Item::ToolResult(second),
            Item::ToolResult(first),
        ],
    };
    assert_eq!(replay(session.directory())?, expected);
    // 書き手を閉じる前でも flush 済みで、集計の JSON を別の読み手が読める。
    let body = fs::read_to_string(session.directory().join("session.jsonl"))?;
    let last: serde_json::Value = serde_json::from_str(
        body.lines()
            .last()
            .ok_or_else(|| io::Error::from(io::ErrorKind::NotFound))?,
    )?;
    assert_eq!(
        last,
        json!({"type":"summary", "turns":1, "compactions":0, "tool_calls":{"write":2}, "checks":2, "after_write_checks":0, "fixes":1, "approvals":{"approved":2,"denied":2,"cancelled":1,"policy_denied":0}, "tokens":{"input":100,"cache_read":20,"cache_write":3,"output":9}, "elapsed_ms":123,"reason":"completed"})
    );
    Ok(())
}

// 作成時の関門: 異常終了後、開始済みの操作を未実行と誤認する退行を捕まえる。
// 途中の不正な行を、末尾の切断と同じ扱いで黙って捨てることも防ぐ。
#[test]
fn replay_ignores_only_torn_tail_and_fills_missing_tool_results_before_next_message() -> TestResult
{
    let fixture = Fixture::new()?;
    let session = fixture.session()?;
    let mut writer = session.writer()?;
    writer.append(&header(&session))?;
    let started = call("started");
    let cancelled = call("cancelled");
    let done = call("done");
    writer.append(&Record::Assistant {
        turn: 1,
        response: response(&[started.clone(), cancelled.clone(), done.clone()]),
    })?;
    writer.append(&Record::ToolStart {
        call: started.clone(),
    })?;
    writer.append(&Record::ToolResult {
        turn: 1,
        name: "write".to_owned(),
        result: result(&done, "already done"),
        spill: None,
        stderr_spill: None,
    })?;
    let message = UserMessage {
        parts: vec![UserPart {
            kind: UserPartKind::Steering,
            text: "check files".to_owned(),
        }],
    };
    writer.append(&Record::User {
        kind: UserKind::Steer,
        message: message.clone(),
    })?;
    // P3-04 より前の確認と集計の行を、そのまま再開できることを確かめる。
    OpenOptions::new().append(true).open(session.directory().join("session.jsonl"))?.write_all(
        br#"{"type":"approval","phase":"request","id":"old","tool_id":"started","request":"outside write","outcome":null}
{"type":"approval","phase":"result","id":"old","tool_id":"started","request":"outside write","outcome":"approved"}
{"type":"summary","turns":1,"tool_calls":{},"checks":0,"fixes":0,"approvals":{"approved":1,"denied":0,"cancelled":0},"tokens":{"input":0,"cache_read":0,"cache_write":0,"output":0},"elapsed_ms":0,"reason":"completed"}
"#,
    )?;
    let records = read_records(session.directory())?;
    assert!(records.iter().any(|record| matches!(record, Record::Approval { kind: None, facts, justification: None, user_message: None, outcome: Some(ApprovalOutcome::Approved), .. } if *facts == Default::default())));
    assert!(records.iter().any(|record| matches!(record, Record::Summary { summary } if summary.approvals.policy_denied == 0)));
    let file = session.directory().join("session.jsonl");
    OpenOptions::new()
        .append(true)
        .open(&file)?
        .write_all(b"{\"type\":\"assistant\",\"response\":")?;
    let conversation = replay(session.directory())?;
    assert_eq!(conversation.items.len(), 5);
    assert_eq!(
        conversation.items.get(1),
        Some(&Item::ToolResult(result(&done, "already done")))
    );
    for (index, call, text) in [
        (
            2,
            started,
            "Tool call was interrupted before its result was recorded. Its effects are unknown; check the files before retrying.",
        ),
        (3, cancelled, "Tool call was cancelled by the user."),
    ] {
        let mut expected = result(&call, text);
        expected.is_error = true;
        assert_eq!(
            conversation.items.get(index),
            Some(&Item::ToolResult(expected))
        );
    }
    assert_eq!(conversation.items.last(), Some(&Item::UserMessage(message)));
    OpenOptions::new()
        .append(true)
        .open(file)?
        .write_all(b"\n")?;
    assert!(read_records(session.directory()).is_err());
    Ok(())
}

// 作成時の関門: 最後の圧縮より前の会話や未完了の呼び出しを戻してしまう退行を捕まえる。
#[test]
fn replay_uses_last_compaction_as_its_conversation_base() -> TestResult {
    let fixture = Fixture::new()?;
    let session = fixture.session()?;
    let mut writer = session.writer()?;
    writer.append(&header(&session))?;
    writer.append(&Record::Assistant {
        turn: 1,
        response: response(&[call("discarded")]),
    })?;
    writer.append(&Record::ToolStart {
        call: call("retained"),
    })?;
    writer.append(&Record::Compaction {
        conversation: Conversation::default(),
        reason: Default::default(),
        summarized_items: 0,
        kept_items: 0,
    })?;
    let base = Conversation {
        items: vec![Item::AssistantTurn(response(&[call("turn-7-call-0")]))],
    };
    // 欄を加える前の形式の行を、JSON Lines の読み手で確かめる。
    writeln!(
        OpenOptions::new()
            .append(true)
            .open(session.directory().join("session.jsonl"))?,
        "{}",
        json!({"type":"compaction", "conversation":base})
    )?;
    assert!(read_records(session.directory())?.iter().any(|record| matches!(record,
        Record::Compaction { reason: benitoiteh::session::CompactionReason::Threshold, summarized_items: 0, kept_items: 0, conversation }
            if *conversation == base)));

    let conversation = replay(session.directory())?;
    assert_eq!(conversation.items.first(), base.items.first());
    let mut cancelled = result(
        &call("turn-7-call-0"),
        "Tool call was cancelled by the user.",
    );
    cancelled.is_error = true;
    assert_eq!(
        conversation.items.get(1),
        Some(&Item::ToolResult(cancelled))
    );
    assert_eq!(conversation.items.len(), 2);
    let state = benitoiteh::session::ResumeState::read(session.directory())?;
    assert!(
        matches!(state.missing_results.first(), Some(Record::ToolResult { turn: 7, name, .. }) if name == "write")
    );
    Ok(())
}

// 作成時の関門: spill の参照からセッション外の内容を読める退行と、対応していない
// 形式を黙って読む退行を捕まえる。外のファイルも有効な Record にし、JSON の失敗で代用しない。
#[test]
fn reader_rejects_unsupported_formats_and_spill_references_outside_the_spill_directory()
-> TestResult {
    use benitoiteh::session::SessionError;

    let fixture = Fixture::new()?;
    let session = fixture.session()?;
    let path = session.directory().join("session.jsonl");
    let mut unsupported = header(&session);
    if let Record::Header(header) = &mut unsupported {
        header.format = 2;
    }
    fs::write(&path, format!("{}\n", serde_json::to_string(&unsupported)?))?;
    assert!(matches!(
        read_records(session.directory()),
        Err(SessionError::Invalid(_))
    ));

    let unsupported_message = read_records(session.directory())
        .err()
        .ok_or("unsupported format")?
        .to_string();
    fs::write(&path, b"")?;
    let missing_message = read_records(session.directory())
        .err()
        .ok_or("missing header")?
        .to_string();
    assert_ne!(unsupported_message, missing_message);
    let outside = fixture.root.join("outside.json");
    fs::write(
        &outside,
        b"{\"type\":\"error\",\"kind\":\"test\",\"message\":\"outside\"}",
    )?;
    fs::set_permissions(&outside, fs::Permissions::from_mode(0o600))?;
    symlink(&outside, session.directory().join("spill/link"))?;
    for reference in [
        outside,
        PathBuf::from("../outside.json"),
        PathBuf::from("session.jsonl"),
        PathBuf::from("spill/../session.jsonl"),
        PathBuf::from("spill/link"),
    ] {
        let spilled = Record::Spilled {
            parts: vec![reference],
        };
        fs::write(
            &path,
            format!(
                "{}\n{}\n",
                serde_json::to_string(&header(&session))?,
                serde_json::to_string(&spilled)?
            ),
        )?;
        assert!(matches!(
            read_records(session.directory()),
            Err(SessionError::Invalid(_) | SessionError::SafeFs(_))
        ));
    }
    Ok(())
}

// 作成時の関門: 行の上限を超えた署名付きの応答が欠ける退行と、保存量で
// 出力の読み取りまで止める退行を、実際の 16 MiB の境界で別々に捕まえる。
#[test]
fn oversized_records_are_spilled_losslessly_and_tool_output_counts_discarded_bytes() -> TestResult {
    let fixture = Fixture::new()?;
    let session = fixture.session()?;
    let mut writer = session.writer()?;
    writer.append(&header(&session))?;
    let mut response = response(&[]);
    response
        .raw_items
        .items
        .push(json!({"encrypted_content": "x".repeat(MAX_BYTES.saturating_add(1))}));
    writer.append(&Record::Assistant {
        turn: 1,
        response: response.clone(),
    })?;
    let body = fs::read(session.directory().join("session.jsonl"))?;
    assert!(body.len() < MAX_BYTES);
    assert!(String::from_utf8(body)?.contains("\"type\":\"spilled\""));
    assert_eq!(
        replay(session.directory())?.items,
        vec![Item::AssistantTurn(response)]
    );

    let mut spill = session.spill_writer("../../untrusted-id")?;
    let output = vec![b'a'; MAX_BYTES.saturating_sub(2)];
    spill.write_all(&output)?;
    spill.write_all(b"bcdiscard")?;
    spill.write_all(b"more")?;
    let info = spill.finish()?;
    assert_eq!(info.written_bytes, 16777216);
    assert_eq!(info.discarded_bytes, 11);
    let saved = fs::read(session.directory().join(&info.path))?;
    assert_eq!(
        saved.get(MAX_BYTES.saturating_sub(2)..MAX_BYTES),
        Some(b"bc".as_slice())
    );
    assert_eq!(
        saved.get(MAX_BYTES..),
        Some(b"\n[Partial output: wrote 16777216 bytes; discarded 11 bytes.]\n".as_slice())
    );
    assert_eq!(
        fs::metadata(session.directory().join(&info.path))?
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
    assert!(session.spill_writer("../../untrusted-id").is_err());
    let mut complete = session.spill_writer("complete")?;
    complete.write_all(b"all output")?;
    let complete = complete.finish()?;
    assert_eq!(complete.discarded_bytes, 0);
    assert_eq!(
        fs::read(session.directory().join(complete.path))?,
        b"all output"
    );
    Ok(())
}

// 作成時の関門: 共有一時領域の既存要素の再利用、広すぎる権限、書き手が
// 残っている間のロック解放を、本物の排他作成と flock で検出する。
#[test]
fn session_entries_are_private_exclusive_and_live_until_the_last_writer_drops() -> TestResult {
    let fixture = Fixture::new()?;
    let session = fixture.session()?;
    for (path, mode) in [
        (fixture.paths.sessions_dir.clone(), 0o700),
        (session.directory().to_path_buf(), 0o700),
        (session.directory().join("spill"), 0o700),
        (session.temporary_dir().to_path_buf(), 0o700),
        (session.directory().join("lock"), 0o600),
        (session.directory().join("session.jsonl"), 0o600),
    ] {
        assert_eq!(
            fs::symlink_metadata(path)?.permissions().mode() & 0o7777,
            mode
        );
    }
    assert!(Session::create(&fixture.paths, session.id().clone()).is_err());
    let temporary = session.temporary_dir().to_path_buf();
    let writer = session.writer()?;
    drop(session);
    assert_eq!(
        cleanup(&fixture.paths, 0, SystemTime::now())?.temporary_removed,
        0
    );
    assert!(temporary.exists());
    drop(writer);
    assert!(!temporary.exists());

    let id = SessionId::generate(SystemTime::now())?;
    let collision = fixture.paths.temporary_dir(id.as_str())?;
    symlink(&fixture.paths.home, &collision)?;
    fs::write(fixture.paths.home.join("keep"), "home")?;
    assert!(Session::create(&fixture.paths, id.clone()).is_err());
    assert!(fs::symlink_metadata(collision)?.is_symlink());
    assert!(!fixture.paths.sessions_dir.join(id.as_str()).exists());
    assert_eq!(fs::read_to_string(fixture.paths.home.join("keep"))?, "home");
    Ok(())
}

// 作成時の関門: ロック中の一時領域・記録を消す、リンクの先を消す、ID の時刻を
// 使って更新済みの記録も消す退行を、同じ掃除の公開 API で捕まえる。
#[test]
fn cleanup_removes_only_inactive_matching_directories_and_expired_sessions() -> TestResult {
    let fixture = Fixture::new()?;
    let active = Session::create(&fixture.paths, "20200101T000000Z-0000000000000001".parse()?)?;
    let stale = Session::create(&fixture.paths, "20200101T000000Z-0000000000000002".parse()?)?;
    let stale_dir = stale.directory().to_path_buf();
    let stale_temp = stale.temporary_dir().to_path_buf();
    fs::File::open(stale_dir.join("session.jsonl"))?.set_modified(UNIX_EPOCH)?;
    drop(stale);
    fs::DirBuilder::new().create(&stale_temp)?;
    fs::set_permissions(&stale_temp, fs::Permissions::from_mode(0o700))?;
    fs::write(fixture.paths.home.join("keep"), "keep")?;
    symlink(&fixture.paths.home, stale_temp.join("home"))?;
    let orphan = fixture
        .paths
        .temporary_root
        .join("benitoiteh-20200101T000000Z-0000000000000003");
    fs::create_dir(&orphan)?;
    fs::set_permissions(&orphan, fs::Permissions::from_mode(0o700))?;
    let unrelated = fixture.paths.temporary_root.join("benitoiteh-unrelated");
    fs::create_dir(&unrelated)?;
    let recent = fixture.session()?;
    let recent_dir = recent.directory().to_path_buf();
    drop(recent);
    let updated = Session::create(&fixture.paths, "20200101T000000Z-0000000000000004".parse()?)?;
    let updated_dir = updated.directory().to_owned();
    fs::File::open(updated_dir.join("session.jsonl"))?.set_modified(SystemTime::now())?;
    drop(updated);
    let report = cleanup(&fixture.paths, DEFAULT_RETENTION_DAYS, SystemTime::now())?;
    assert_eq!(report.temporary_removed, 2);
    assert_eq!(report.sessions_removed, 1);
    assert!(report.failures.is_empty());
    assert!(!stale_temp.exists());
    assert!(!stale_dir.exists());
    assert!(!orphan.exists());
    assert!(active.temporary_dir().exists());
    assert!(active.directory().exists());
    assert!(recent_dir.exists());
    assert!(updated_dir.exists());
    assert!(unrelated.exists());
    assert_eq!(fs::read_to_string(fixture.paths.home.join("keep"))?, "keep");
    Ok(())
}

// 関門: /tmp のような一時の根のリンクで drop・再開・掃除が失敗する退行を、
// 環境変数を変えずに本物のセッションの寿命と記録の入口から確かめる。
#[test]
fn linked_storage_parents_allow_drop_resume_and_cleanup() -> TestResult {
    let fixture = Fixture::new()?;
    let temporary_alias = fixture.root.join("tmp-alias");
    let state = fixture.root.join("state-real");
    let state_alias = fixture.root.join("state-alias");
    fs::create_dir(&state)?;
    symlink(&fixture.paths.temporary_root, &temporary_alias)?;
    symlink(&state, &state_alias)?;
    let paths = DataPaths::from_values(
        Some(&fixture.paths.home),
        Some(&fixture.root.join("config")),
        Some(&state_alias),
        Some(&temporary_alias),
    )?;
    let session = Session::create(&paths, SessionId::generate(SystemTime::now())?)?;
    session.writer()?.append(&header(&session))?;
    let id = session.id().clone();
    let directory = session.directory().to_owned();
    let temporary = session.temporary_dir().to_owned();
    assert_eq!(temporary.parent(), Some(temporary_alias.as_path()));
    fs::write(temporary.join("from-live-session"), b"temporary")?;
    drop(session);
    assert!(!temporary.exists());

    // 異常終了で残った領域も、再開時には親の別名から消して作り直せる。
    fs::create_dir(&temporary)?;
    fs::set_permissions(&temporary, fs::Permissions::from_mode(0o700))?;
    fs::write(temporary.join("from-crashed-session"), b"stale")?;
    let resumed = session_support::open_created_session(&paths, &id)?;
    assert_eq!(fs::read_dir(resumed.temporary_dir())?.count(), 0);
    fs::write(
        resumed.temporary_dir().join("from-resumed-session"),
        b"temporary",
    )?;
    drop(resumed);
    assert!(!temporary.exists());

    fs::create_dir(&temporary)?;
    fs::set_permissions(&temporary, fs::Permissions::from_mode(0o700))?;
    fs::write(temporary.join("stale"), b"stale")?;
    fs::File::open(directory.join("session.jsonl"))?.set_modified(UNIX_EPOCH)?;
    let report = cleanup(&paths, DEFAULT_RETENTION_DAYS, SystemTime::now())?;
    assert!(report.failures.is_empty(), "{:?}", report.failures);
    assert_eq!((report.sessions_removed, report.temporary_removed), (1, 1));
    assert!(!directory.exists() && !temporary.exists());
    assert!(fs::symlink_metadata(temporary_alias)?.is_symlink());
    assert!(fs::symlink_metadata(state_alias)?.is_symlink());
    Ok(())
}

// 作成時の関門: UTC、うるう年、世紀の境界、時刻順、パスに使えない ID を
// 外部の既知の時刻との対応で確かめる。実装の暦計算で期待値は作らない。
#[test]
fn ids_are_utc_chronological_and_validate_calendar_dates() -> TestResult {
    let mut previous = None;
    for (seconds, timestamp) in [
        (0, "19700101T000000Z"),
        (951782400, "20000229T000000Z"),
        (1709251199, "20240229T235959Z"),
        (1709251200, "20240301T000000Z"),
        (4107542400, "21000301T000000Z"),
    ] {
        let time = UNIX_EPOCH
            .checked_add(Duration::from_secs(seconds))
            .ok_or_else(|| io::Error::from(io::ErrorKind::InvalidInput))?;
        let id = SessionId::generate(time)?;
        assert_eq!(id.as_str().get(..16), Some(timestamp));
        assert_eq!(id.as_str().len(), 33);
        assert_eq!(id.started_at(), time);
        assert_eq!(id.as_str().parse::<SessionId>()?, id);
        if let Some(previous) = previous {
            assert!(previous < id);
        }
        previous = Some(id);
    }
    assert_eq!(
        "20000229T123456Z-0123456789abcdef"
            .parse::<SessionId>()?
            .started_at_utc(),
        "2000-02-29T12:34:56Z"
    );
    for invalid in [
        "20230229T000000Z-0000000000000000",
        "21000229T000000Z-0000000000000000",
        "20261301T000000Z-0000000000000000",
        "20260100T000000Z-0000000000000000",
        "20260101T240000Z-0000000000000000",
        "20260101T000060Z-0000000000000000",
        "20260101T000000Z-000000000000000G",
        "20260101T000000Z-0000000000000000/..",
        "../../escape",
    ] {
        assert!(invalid.parse::<SessionId>().is_err(), "{invalid}");
    }
    Ok(())
}

// 作成時の関門: spill と objects の合計の上限、discard による予約の返却を
// 記録の持ち主の公開 API で守る。リンクの先や checkpoints の扱いも実ファイルで確かめる。
#[test]
fn storage_counts_all_regular_files_and_shares_output_reservations() -> TestResult {
    use benitoiteh::session::storage_usage;
    let fixture = Fixture::new()?;
    let session = fixture.session()?;
    session.writer()?.append(&header(&session))?;
    fs::create_dir(session.directory().join("checkpoints"))?;
    fs::write(session.directory().join("checkpoints/index"), b"checkpoint")?;
    let limit = 1024_u64.saturating_mul(1024).saturating_mul(1024);
    fs::File::create(session.directory().join("objects/sparse"))?
        .set_len(limit.saturating_sub(3))?;
    fs::File::create(fixture.root.join("external"))?.set_len(limit)?;
    symlink(
        fixture.root.join("external"),
        session.directory().join("spill/external"),
    )?;
    let usage = storage_usage(&fixture.paths, Some(session.id()))?;
    assert_eq!(usage.current_outputs, limit.saturating_sub(3));
    assert_eq!(
        usage.total,
        usage
            .current_outputs
            .saturating_add(10)
            .saturating_add(fs::metadata(session.directory().join("session.jsonl"))?.len())
    );
    session.configure_budget(&usage);
    let mut first = session.spill_writer("first")?;
    first.write_all(b"abc")?;
    let clone = session.clone();
    let mut second = clone.spill_writer("second")?;
    assert!(second.write_all(b"x").is_err());
    second.discard()?;
    first.discard()?;
    let mut replacement = clone.spill_writer("replacement")?;
    replacement.write_all(b"abc")?;
    replacement.finish()?;
    assert!(!session.reserve(1));
    assert!(session.take_storage_notice().is_some());
    assert!(session.take_storage_notice().is_none());
    // 再開用の記録は出力の保存量で止めない。大きな行の分割も同じである。
    session.writer()?.append(&Record::Error {
        kind: "test".into(),
        message: "x".repeat(MAX_BYTES),
    })?;
    assert_eq!(read_records(session.directory())?.len(), 2);
    Ok(())
}

// 作成時の関門: 不正な記録や log のリンクを拒んだ後に、既存のディレクトリを
// 新規作成の失敗と同じように消してしまう退行を守る。
#[test]
fn opening_rejects_unsafe_logs_and_corruption_without_deleting_session() -> TestResult {
    let fixture = Fixture::new()?;
    let session = fixture.session()?;
    let id = session.id().clone();
    session.writer()?.append(&header(&session))?;
    let directory = session.directory().to_owned();
    let log = directory.join("session.jsonl");
    let original = fs::read(&log)?;
    drop(session);
    fs::set_permissions(&log, fs::Permissions::from_mode(0o644))?;
    assert!(session_support::open_created_session(&fixture.paths, &id).is_err());
    assert_eq!(fs::read(&log)?, original);
    fs::set_permissions(&log, fs::Permissions::from_mode(0o600))?;
    fs::hard_link(&log, fixture.root.join("hardlink"))?;
    assert!(session_support::open_created_session(&fixture.paths, &id).is_err());
    fs::remove_file(fixture.root.join("hardlink"))?;
    fs::rename(&log, fixture.root.join("outside-log"))?;
    symlink(fixture.root.join("outside-log"), &log)?;
    assert!(session_support::open_created_session(&fixture.paths, &id).is_err());
    assert_eq!(fs::read(&log)?, original);
    fs::remove_file(&log)?;
    fs::rename(fixture.root.join("outside-log"), &log)?;
    OpenOptions::new()
        .append(true)
        .open(&log)?
        .write_all(b"broken\n")?;
    let corrupt = fs::read(&log)?;
    assert!(session_support::open_created_session(&fixture.paths, &id).is_err());
    assert_eq!(fs::read(&log)?, corrupt);
    assert!(directory.join("spill").is_dir() && directory.join("lock").is_file());
    Ok(())
}

// 作成時の関門: ほかのセッションの権限不足で全起動を止める退行を、本物の権限で守る。
#[test]
fn storage_skips_unreadable_entries_in_other_sessions() -> TestResult {
    if rustix::process::geteuid().is_root() {
        eprintln!("Skipped unreadable-session storage check: root can read mode 000 directories.");
        return Ok(());
    }
    struct Restore(std::path::PathBuf);
    impl Drop for Restore {
        fn drop(&mut self) {
            let _restored = fs::set_permissions(&self.0, fs::Permissions::from_mode(0o700));
        }
    }
    let fixture = Fixture::new()?;
    let current = fixture.session()?;
    let other = fixture.session()?;
    let private = other.directory().join("unreadable");
    fs::create_dir(&private)?;
    fs::write(private.join("hidden"), b"hidden")?;
    fs::write(other.directory().join("visible"), b"visible")?;
    fs::set_permissions(&private, fs::Permissions::from_mode(0o000))?;
    let _restore = Restore(private.clone());
    let error = fs::read_dir(&private)
        .err()
        .ok_or("Mode 000 directory unexpectedly remained readable for a non-root user.")?;
    assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
    let usage = benitoiteh::session::storage_usage(&fixture.paths, Some(current.id()))?;
    assert_eq!(usage.total, 7);
    Ok(())
}

// 関門: 一項目の権限不足が以降の削除・起動を止める退行と、錠を先に消して
// 次回に掃除できなくなる退行を、実際の権限と CLI の診断で捕まえる。
#[test]
fn cleanup_reports_failures_continues_and_retries_without_losing_locks() -> TestResult {
    if rustix::process::geteuid().is_root() {
        eprintln!("Skipped cleanup permission check: root can read mode 000 directories.");
        return Ok(());
    }
    struct Restore(Vec<PathBuf>);
    impl Drop for Restore {
        fn drop(&mut self) {
            for path in &self.0 {
                let _restored = fs::set_permissions(path, fs::Permissions::from_mode(0o700));
            }
        }
    }
    let fixture = Fixture::new()?;
    let blocked = fixture.session()?;
    let id = blocked.id().clone();
    let directory = blocked.directory().to_owned();
    let temporary = blocked.temporary_dir().to_owned();
    fs::File::open(directory.join("session.jsonl"))?.set_modified(UNIX_EPOCH)?;
    let restore = Restore(vec![directory.join("blocked"), temporary.join("blocked")]);
    for path in &restore.0 {
        fs::create_dir(path)?;
        fs::write(path.join("keep"), b"keep")?;
        fs::set_permissions(path, fs::Permissions::from_mode(0o000))?;
    }
    drop(blocked);
    let other = fixture.session()?;
    let other_dir = other.directory().to_owned();
    fs::File::open(other_dir.join("session.jsonl"))?.set_modified(UNIX_EPOCH)?;
    drop(other);
    let report = cleanup(&fixture.paths, DEFAULT_RETENTION_DAYS, SystemTime::now())?;
    assert_eq!(report.sessions_removed, 1);
    assert_eq!(report.failures.len(), 2);
    let permission_error = format!("(os error {})", rustix::io::Errno::ACCESS.raw_os_error());
    for failure in &report.failures {
        assert_eq!(failure.id.as_ref(), Some(&id));
        assert!(
            failure.reason.contains(&permission_error),
            "{}",
            failure.reason
        );
        assert!(
            !failure
                .reason
                .contains(&fixture.root.to_string_lossy().to_string())
        );
    }
    assert!(report.failures.iter().any(|failure| failure.temporary));
    assert!(report.failures.iter().any(|failure| !failure.temporary));
    assert!(!other_dir.exists());
    assert!(directory.join("lock").is_file());
    assert!(directory.join("session.jsonl").is_file());
    // 見えるまま残ったファイルは、後始末の失敗後も保存量に含める。
    fs::write(directory.join("remaining"), b"count me")?;
    assert!(benitoiteh::session::storage_usage(&fixture.paths, None)?.total >= 8);
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_benitoiteh"))
        .args(["sessions", "--json"])
        .env_clear()
        .env("HOME", &fixture.paths.home)
        .env("XDG_CONFIG_HOME", fixture.root.join("config"))
        .env("XDG_STATE_HOME", fixture.root.join("state"))
        .env("TMPDIR", &fixture.paths.temporary_root)
        .output()?;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let diagnostics = String::from_utf8(output.stderr)?;
    assert_eq!(
        diagnostics
            .lines()
            .filter(|line| line.contains("Could not clean up"))
            .count(),
        2
    );
    assert!(diagnostics.contains(id.as_str()));
    assert!(!diagnostics.contains(&fixture.root.to_string_lossy().to_string()));
    drop(restore);
    let retry = cleanup(&fixture.paths, DEFAULT_RETENTION_DAYS, SystemTime::now())?;
    assert!(retry.failures.is_empty(), "{:?}", retry.failures);
    assert_eq!((retry.sessions_removed, retry.temporary_removed), (1, 1));
    assert!(!directory.exists() && !temporary.exists());
    Ok(())
}

// 関門: 再帰によるスタックの枯渇は浅い掃除のテストで捕まらない。
// 子の小さいスタックから、公開の cleanup と最後の Session の破棄を通す。
#[test]
fn deep_cleanup_and_session_drop_do_not_depend_on_thread_stack_depth() -> TestResult {
    use rustix::fs::{Mode as FsMode, OFlags, mkdirat, open, openat};
    use rustix::process::{Resource, getrlimit, setrlimit};
    const CHILD: &str = "BENITOITEH_DEEP_CLEANUP_CHILD";
    const DEPTH: usize = 750;
    let fixture = Fixture::new()?;
    if std::env::var_os(CHILD).is_none() {
        let output = std::process::Command::new(std::env::current_exe()?)
            .args([
                "--exact",
                "deep_cleanup_and_session_drop_do_not_depend_on_thread_stack_depth",
                "--nocapture",
            ])
            .env(CHILD, "1")
            .env("XDG_CONFIG_HOME", fixture.root.join("config"))
            .env("XDG_STATE_HOME", fixture.root.join("state"))
            .env("TMPDIR", &fixture.paths.temporary_root)
            .output()?;
        assert!(
            output.status.success(),
            "{:?}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        );
        return Ok(());
    }
    let mut limit = getrlimit(Resource::Nofile);
    limit.current = Some(limit.maximum.unwrap_or(4096).min(4096));
    setrlimit(Resource::Nofile, limit)?;
    let flags = OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC;
    let build = |path: &std::path::Path| -> TestResult {
        let mut directory = open(path, flags, FsMode::empty())?;
        for _ in 0..DEPTH {
            mkdirat(&directory, "d", FsMode::RWXU)?;
            directory = openat(&directory, "d", flags, FsMode::empty())?;
        }
        Ok(())
    };
    let stale = fixture.session()?;
    let directory = stale.directory().to_owned();
    let temporary = stale.temporary_dir().to_owned();
    fs::File::open(directory.join("session.jsonl"))?.set_modified(UNIX_EPOCH)?;
    drop(stale);
    fs::create_dir(&temporary)?;
    fs::set_permissions(&temporary, fs::Permissions::from_mode(0o700))?;
    build(&directory)?;
    build(&temporary)?;
    let live = fixture.session()?;
    let live_temp = live.temporary_dir().to_owned();
    build(&live_temp)?;
    let paths = fixture.paths.clone();
    let worker = std::thread::Builder::new().stack_size(256 * 1024).spawn(
        move || -> Result<_, String> {
            let mut limited = limit;
            limited.current = Some(64);
            setrlimit(Resource::Nofile, limited).map_err(|error| error.to_string())?;
            let failed = cleanup(&paths, DEFAULT_RETENTION_DAYS, SystemTime::now())
                .map_err(|error| error.to_string())?;
            setrlimit(Resource::Nofile, limit).map_err(|error| error.to_string())?;
            assert_eq!(failed.failures.len(), 2);
            let descriptor_error =
                format!("(os error {})", rustix::io::Errno::MFILE.raw_os_error());
            assert!(
                failed
                    .failures
                    .iter()
                    .all(|failure| failure.reason.contains(&descriptor_error))
            );
            let report = cleanup(&paths, DEFAULT_RETENTION_DAYS, SystemTime::now())
                .map_err(|error| error.to_string())?;
            drop(live);
            Ok(report)
        },
    )?;
    let report = worker
        .join()
        .map_err(|_| "cleanup thread panicked")?
        .map_err(io::Error::other)?;
    assert!(report.failures.is_empty(), "{:?}", report.failures);
    assert_eq!((report.sessions_removed, report.temporary_removed), (1, 1));
    assert!(!directory.exists() && !temporary.exists() && !live_temp.exists());
    Ok(())
}

// 関門: 廃止した欄を含む保存済みの記録を読めることは記録の互換性の契約である。
#[test]
fn legacy_skill_version_is_ignored_and_not_written() -> TestResult {
    let f = Fixture::new()?;
    let session = f.session()?;
    let mut value = serde_json::to_value(header(&session))?;
    value
        .as_object_mut()
        .ok_or("header object")?
        .insert("skill_version".into(), json!("0.0.6"));
    let decoded: Record = serde_json::from_value(value)?;
    session.writer()?.append(&decoded)?;
    assert_eq!(read_records(session.directory())?.len(), 1);
    assert!(
        serde_json::to_value(decoded)?
            .get("skill_version")
            .is_none()
    );
    Ok(())
}
