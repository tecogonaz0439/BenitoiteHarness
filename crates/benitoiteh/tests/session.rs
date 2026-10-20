//! 記録の保存と復元、一時領域の寿命を実ファイルで確かめる（設計書 01-05）。

#[path = "support/session.rs"]
mod session_support;
#[path = "support/temporary_cleanup.rs"]
mod temporary_cleanup;

use std::error::Error;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::os::unix::fs::{DirBuilderExt, PermissionsExt, symlink};
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

// 関門: 記録順の平坦な一覧では、兄弟の前順・根での分岐・途中の現在地を表せない。
// 記録を受ける公開の表示行の境界で、依頼と先端の重複も検出する。
#[test]
fn tree_rows_preserve_preorder_branch_depth_and_combined_marks() -> TestResult {
    use benitoiteh::session::{SessionTree, tree_rows};
    let request = |node, text: &str| Record::User {
        node,
        kind: UserKind::Request,
        message: UserMessage {
            parts: vec![UserPart {
                kind: UserPartKind::Request,
                text: text.into(),
            }],
        },
    };
    let assistant = |node| Record::Assistant {
        node,
        turn: 1,
        response: response(&[]),
    };
    let mut records = vec![
        request(1, "shared"),
        assistant(2),
        request(3, "left"),
        assistant(4),
        Record::Branch {
            to: Some(2),
            from: Some(4),
            checkpoint: None,
        },
        request(5, "right without answer"),
        Record::Branch {
            to: None,
            from: Some(5),
            checkpoint: None,
        },
        request(6, "other root"),
        Record::Branch {
            to: Some(2),
            from: Some(6),
            checkpoint: None,
        },
    ];
    let rows = tree_rows(&records, &SessionTree::from_records(&records)?);
    assert_eq!(
        rows.iter()
            .map(|row| (
                row.node,
                row.depth,
                row.request.as_deref(),
                row.is_tip,
                row.is_current
            ))
            .collect::<Vec<_>>(),
        vec![
            (None, 0, None, false, false),
            (Some(1), 1, Some("shared"), false, false),
            (Some(2), 1, None, false, true),
            (Some(3), 2, Some("left"), false, false),
            (Some(4), 2, None, true, false),
            (Some(5), 2, Some("right without answer"), true, false),
            (Some(6), 1, Some("other root"), true, false),
        ]
    );
    for tip in [None, Some(3), Some(4), Some(5)] {
        records.push(Record::Branch {
            to: tip,
            from: Some(2),
            checkpoint: None,
        });
        let rows = tree_rows(&records, &SessionTree::from_records(&records)?);
        let current = rows.iter().filter(|row| row.is_current).collect::<Vec<_>>();
        assert_eq!(current.len(), 1);
        assert_eq!(current.first().map(|row| row.node), Some(tip));
        assert_eq!(rows.len(), 6); // 依頼でも先端でもない 2 の行は現在地のときだけ現れる。
    }
    assert!(tree_rows(&[], &SessionTree::from_records(&[])?).is_empty());
    Ok(())
}

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
        forked_from: None,
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
        plugins: Vec::new(),
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
        node: 1,
        kind: UserKind::Request,
        message: message.clone(),
    })?;
    writer.append(&Record::Assistant {
        node: 2,
        turn: 1,
        response: response.clone(),
    })?;
    for (index, result) in [&second, &first].into_iter().enumerate() {
        writer.append(&Record::ToolResult {
            node: 3 + u64::try_from(index)?,
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
        node: 1,
        turn: 1,
        response: response(&[started.clone(), cancelled.clone(), done.clone()]),
    })?;
    writer.append(&Record::ToolStart {
        call: started.clone(),
    })?;
    writer.append(&Record::ToolResult {
        node: 2,
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
        node: 3,
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
        node: 1,
        turn: 1,
        response: response(&[call("discarded")]),
    })?;
    writer.append(&Record::ToolStart {
        call: call("retained"),
    })?;
    writer.append(&Record::Compaction {
        node: 2,
        conversation: Conversation::default(),
        reason: Default::default(),
        summarized_items: 0,
        kept_items: 0,
    })?;
    let base = Conversation {
        items: vec![Item::AssistantTurn(response(&[call("turn-7-call-0")]))],
    };
    // 節点は必須とし、圧縮の補助情報は省略しても読める。
    writeln!(
        OpenOptions::new()
            .append(true)
            .open(session.directory().join("session.jsonl"))?,
        "{}",
        json!({"type":"compaction", "node":3, "conversation":base})
    )?;
    assert!(read_records(session.directory())?.iter().any(|record| matches!(record,
        Record::Compaction { reason: benitoiteh::session::CompactionReason::Threshold, summarized_items: 0, kept_items: 0, conversation, .. }
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
    for version in [1, 3] {
        let mut unsupported = header(&session);
        if let Record::Header(header) = &mut unsupported {
            header.format = version;
        }
        fs::write(&path, format!("{}\n", serde_json::to_string(&unsupported)?))?;
        assert!(matches!(
            read_records(session.directory()),
            Err(SessionError::Invalid(_))
        ));
        assert_eq!(
            benitoiteh::session::list_sessions(&fixture.paths)?
                .first()
                .and_then(|entry| entry.error.as_deref()),
            Some("Unsupported session format version.")
        );
    }
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
        node: 1,
        turn: 1,
        response: response.clone(),
    })?;
    let body = fs::read(session.directory().join("session.jsonl"))?;
    assert!(body.len() < MAX_BYTES);
    assert!(String::from_utf8(body)?.contains("\"type\":\"spilled\""));
    assert_eq!(
        replay(session.directory())?.items,
        vec![Item::AssistantTurn(response.clone())]
    );
    // 写した側でも分割保存を読み直せ、元の分割行をそのまま参照しない。
    let copied = session
        .fork(
            &fixture.paths,
            std::path::Path::new("/project"),
            benitoiteh::session::ForkTarget::Current,
        )?
        .session;
    assert_eq!(
        replay(copied.directory())?.items,
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

// 作成時の関門: ロック中の一時領域・記録を消す、記録のない一時領域を消す、リンクの先を
// 消す、ID の時刻を使って更新済みの記録も消す退行を、同じ掃除の公開 API で捕まえる。
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
    let active_askpass = fixture
        .paths
        .temporary_root
        .join(format!("bha-{}", rustix::process::geteuid().as_raw()))
        .join(if cfg!(target_os = "macos") {
            "0000000000000001"
        } else {
            active.id().as_str()
        });
    let stale_askpass = fixture
        .paths
        .temporary_root
        .join(format!("bha-{}", rustix::process::geteuid().as_raw()))
        .join(if cfg!(target_os = "macos") {
            "0000000000000002"
        } else {
            "20200101T000000Z-0000000000000002"
        });
    // 補助の子は `id` なしで作る。前の版が作った子を名前の対応表で扱う後始末の試しである
    // （設計書 03-04「補助」）。
    let parent = active_askpass.parent().ok_or("missing parent")?;
    fs::create_dir(parent)?;
    fs::set_permissions(parent, fs::Permissions::from_mode(0o700))?;
    for path in [&active_askpass, &stale_askpass] {
        fs::create_dir(path)?;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
        fs::write(path.join("sock"), b"stale socket entry")?;
    }
    struct FallbackDirectory(PathBuf);
    impl Drop for FallbackDirectory {
        fn drop(&mut self) {
            let _removed = fs::remove_dir_all(&self.0);
        }
    }
    let fallback_session = fixture.session()?;
    let fallback_id = fallback_session.id().clone();
    drop(fallback_session);
    let foreign_id = SessionId::generate(SystemTime::now())?;
    let fallback = std::path::Path::new("/tmp")
        .canonicalize()?
        .join(format!("bha-{}", rustix::process::geteuid().as_raw()));
    match fs::DirBuilder::new().mode(0o700).create(&fallback) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(error.into()),
    }
    let known = FallbackDirectory(fallback.join(fallback_id.as_str()));
    let unknown = FallbackDirectory(fallback.join(foreign_id.as_str()));
    for path in [&known.0, &unknown.0] {
        fs::create_dir(path)?;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
        let _listener = std::os::unix::net::UnixListener::bind(path.join("sock"))?;
    }
    let report = cleanup(&fixture.paths, DEFAULT_RETENTION_DAYS, SystemTime::now())?;
    // 記録のない orphan は、ほかのデータの置き場所のものでありうるので残す
    // （設計書 01-05「一時ディレクトリ」）。
    assert_eq!(report.temporary_removed, 3);
    assert!(!known.0.exists());
    assert!(unknown.0.join("sock").exists());
    assert!(active_askpass.join("sock").exists());
    assert!(!stale_askpass.exists());
    assert_eq!(report.sessions_removed, 1);
    assert!(report.failures.is_empty());
    assert!(!stale_temp.exists());
    assert!(!stale_dir.exists());
    assert!(orphan.exists());
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
    // 消し損ねた項目は墓標の名前に替わるので、根の下のすべての項目の blocked を戻す。
    struct Restore(Vec<PathBuf>);
    impl Drop for Restore {
        fn drop(&mut self) {
            for root in &self.0 {
                for entry in fs::read_dir(root).into_iter().flatten().flatten() {
                    let _restored = fs::set_permissions(
                        entry.path().join("blocked"),
                        fs::Permissions::from_mode(0o700),
                    );
                }
            }
        }
    }
    let fixture = Fixture::new()?;
    let blocked = fixture.session()?;
    let id = blocked.id().clone();
    let directory = blocked.directory().to_owned();
    let temporary = blocked.temporary_dir().to_owned();
    fs::File::open(directory.join("session.jsonl"))?.set_modified(UNIX_EPOCH)?;
    let restore = Restore(vec![
        fixture.paths.sessions_dir.clone(),
        fixture.paths.temporary_root.clone(),
    ]);
    for path in [directory.join("blocked"), temporary.join("blocked")] {
        fs::create_dir(&path)?;
        fs::write(path.join("keep"), b"keep")?;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o000))?;
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
    // 消し損ねたセッションは、錠を持ったまま墓標の名前で残り、ID では開けない
    // （設計書 01-05「記録を残す期間」）。中身は消せるものだけ消えるので、記録の有無は
    // 列挙の順に依る。記録を消すことは期間切れの一時ディレクトリのテストで確かめる。
    assert!(!directory.exists());
    assert!(session_support::open_created_session(&fixture.paths, &id).is_err());
    let tombstones = fs::read_dir(&fixture.paths.sessions_dir)?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<Result<Vec<_>, _>>()?;
    let [tombstone] = tombstones.as_slice() else {
        return Err(format!("unexpected session entries: {tombstones:?}").into());
    };
    assert!(
        tombstone
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.starts_with(&format!(".removing-{id}-")))
    );
    assert!(tombstone.join("lock").is_file());
    // 見えるまま残ったファイルは、後始末の失敗後も保存量に含める。
    fs::write(tombstone.join("remaining"), b"count me")?;
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
    assert!(!String::from_utf8(output.stdout)?.contains(id.as_str()));
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
    // 墓標は既に一度数えたセッションなので sessions_removed に数えない。
    assert_eq!((retry.sessions_removed, retry.temporary_removed), (0, 1));
    assert_eq!(fs::read_dir(&fixture.paths.sessions_dir)?.count(), 0);
    assert_eq!(fs::read_dir(&fixture.paths.temporary_root)?.count(), 0);
    Ok(())
}

// 関門: 再帰によるスタックの枯渇と、段ごとに記述子を開いたままにすることによる
// EMFILE は浅い掃除のテストで捕まらない。子の小さいスタックと記述子の上限 64 から、
// 公開の cleanup と最後の Session の破棄を通す。
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
            // 開いたままの記述子を 32 段までにするので、上限 64 でも深さ 750 を
            // 消し切る（設計書 01-05「一時ディレクトリ」）。
            setrlimit(Resource::Nofile, limited).map_err(|error| error.to_string())?;
            let report = cleanup(&paths, DEFAULT_RETENTION_DAYS, SystemTime::now())
                .map_err(|error| error.to_string())?;
            drop(live);
            setrlimit(Resource::Nofile, limit).map_err(|error| error.to_string())?;
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

// 関門: 子プロセスが権限を変えた一時ディレクトリで、再開が UNSAFE_PATH で止まる退行、
// 掃除が 0700 でないものを黙って残し続ける退行、中を消せない残りで再開が止まる退行を、
// 本物の権限と公開の Session::open・cleanup で捕まえる（設計書 01-05「一時ディレクトリ」）。
#[test]
fn changed_temporary_directories_do_not_stop_resume_or_cleanup() -> TestResult {
    let fixture = Fixture::new()?;
    let session = fixture.session()?;
    session.writer()?.append(&header(&session))?;
    let id = session.id().clone();
    let temporary = session.temporary_dir().to_owned();
    drop(session);
    let leftover = |mode| -> io::Result<()> {
        fs::create_dir(&temporary)?;
        fs::write(temporary.join("left"), b"left")?;
        fs::set_permissions(&temporary, fs::Permissions::from_mode(mode))
    };

    // 掃除: 止まったセッションの 0755 の一時ディレクトリを消し、そのことを示す。
    leftover(0o755)?;
    let report = cleanup(&fixture.paths, DEFAULT_RETENTION_DAYS, SystemTime::now())?;
    assert!(report.failures.is_empty(), "{:?}", report.failures);
    assert_eq!(report.temporary_removed, 1);
    assert_eq!(report.temporary_unsafe_removed, vec![temporary.clone()]);
    assert_eq!(
        report.diagnostics(),
        vec![format!(
            "Removed temporary directory {} whose permissions had been changed.",
            temporary.display()
        )]
    );
    assert!(!temporary.exists());

    // 再開: 0755 の残りを消し、新しい一時ディレクトリを 0700 で作る。
    leftover(0o755)?;
    let resumed = session_support::open_created_session(&fixture.paths, &id)?;
    assert_eq!(resumed.temporary_dir(), temporary);
    assert_eq!(
        fs::symlink_metadata(&temporary)?.permissions().mode() & 0o7777,
        0o700
    );
    assert_eq!(fs::read_dir(&temporary)?.count(), 0);
    assert!(resumed.startup_notices().is_empty());
    drop(resumed);

    if rustix::process::geteuid().is_root() {
        // root は権限 0000 のディレクトリも消せるので、消せない残りを作れない。
        eprintln!("Skipped unremovable temporary check: root can remove mode 000 directories.");
        return Ok(());
    }
    // 再開: 中に消せないディレクトリが残っても、墓標の名前に替えて続け、知らせる。
    struct Restore(PathBuf);
    impl Drop for Restore {
        fn drop(&mut self) {
            for entry in fs::read_dir(&self.0).into_iter().flatten().flatten() {
                let _restored = fs::set_permissions(
                    entry.path().join("blocked"),
                    fs::Permissions::from_mode(0o700),
                );
            }
        }
    }
    let restore = Restore(fixture.paths.temporary_root.clone());
    leftover(0o700)?;
    fs::create_dir(temporary.join("blocked"))?;
    fs::write(temporary.join("blocked/keep"), b"keep")?;
    fs::set_permissions(temporary.join("blocked"), fs::Permissions::from_mode(0o000))?;
    let resumed = session_support::open_created_session(&fixture.paths, &id)?;
    assert_eq!(resumed.temporary_dir(), temporary);
    assert_eq!(fs::read_dir(&temporary)?.count(), 0);
    let [notice] = resumed.startup_notices() else {
        return Err(format!("unexpected notices: {:?}", resumed.startup_notices()).into());
    };
    let prefix = format!(
        "Could not remove the previous temporary directory {}.removing-",
        temporary.display()
    );
    assert!(notice.starts_with(&prefix), "{notice}");
    let permission_error = format!("(os error {})", rustix::io::Errno::ACCESS.raw_os_error());
    assert!(notice.contains(&permission_error), "{notice}");
    drop(resumed);
    // 錠を取れる間は、権限を戻した後の掃除が墓標を消す。
    drop(restore);
    let report = cleanup(&fixture.paths, DEFAULT_RETENTION_DAYS, SystemTime::now())?;
    assert!(report.failures.is_empty(), "{:?}", report.failures);
    assert_eq!(report.temporary_removed, 1);
    temporary_cleanup::check_temporary_root(&fixture.paths.temporary_root)?;
    Ok(())
}

// 関門: uchg の印や削除を拒む ACL で名前を替えられない残りで再開が止まる退行を、本物の
// chflags と chmod +a で捕まえる。代わりの名前と、印と ACL を外すコマンドの知らせを確かめる
// （設計書 01-05「一時ディレクトリ」）。
#[cfg(target_os = "macos")]
#[test]
fn pinned_temporary_directory_uses_an_alternate_name_on_resume() -> TestResult {
    struct Undo(&'static str, &'static [&'static str], PathBuf);
    impl Drop for Undo {
        fn drop(&mut self) {
            let _status = std::process::Command::new(self.0)
                .args(self.1)
                .arg(&self.2)
                .status();
        }
    }
    let cases: [(&str, &[&str], &str, &[&str]); 2] = [
        (
            "/usr/bin/chflags",
            &["uchg"],
            "/usr/bin/chflags",
            &["-R", "nouchg"],
        ),
        (
            "/bin/chmod",
            &["+a", "everyone deny delete"],
            "/bin/chmod",
            &["-N"],
        ),
    ];
    for (program, args, undo_program, undo_args) in cases {
        let fixture = Fixture::new()?;
        let session = fixture.session()?;
        session.writer()?.append(&header(&session))?;
        let id = session.id().clone();
        let temporary = session.temporary_dir().to_owned();
        drop(session);
        fs::create_dir(&temporary)?;
        fs::set_permissions(&temporary, fs::Permissions::from_mode(0o700))?;
        let undo = Undo(undo_program, undo_args, temporary.clone());
        let status = std::process::Command::new(program)
            .args(args)
            .arg(&temporary)
            .status()?;
        assert!(status.success(), "{program} {args:?}");
        let resumed = session_support::open_created_session(&fixture.paths, &id)?;
        let alternate = PathBuf::from(format!("{}.1", temporary.display()));
        assert_eq!(resumed.temporary_dir(), alternate, "{program} {args:?}");
        assert_eq!(
            fs::symlink_metadata(&alternate)?.permissions().mode() & 0o7777,
            0o700
        );
        let [notice] = resumed.startup_notices() else {
            return Err(format!("unexpected notices: {:?}", resumed.startup_notices()).into());
        };
        let path = temporary.display();
        assert!(
            notice.starts_with(&format!(
                "Could not remove the previous temporary directory {path}: "
            )),
            "{notice}"
        );
        assert!(
            notice.ends_with(&format!(
                "Run chflags -R nouchg '{path}' and chmod -R -N '{path}', then retry."
            )),
            "{notice}"
        );
        drop(resumed);
        assert!(!alternate.exists());
        drop(undo);
        let report = cleanup(&fixture.paths, DEFAULT_RETENTION_DAYS, SystemTime::now())?;
        assert!(report.failures.is_empty(), "{:?}", report.failures);
        assert_eq!(report.temporary_removed, 1);
        temporary_cleanup::check_temporary_root(&fixture.paths.temporary_root)?;
    }
    Ok(())
}

// 関門: 期間切れのセッションの一時ディレクトリだけを消せないとき、セッションの墓標が先に
// 消えて一時ディレクトリを誰も消さなくなる退行と、記録の中身を墓標に残し続ける退行を、
// 本物の権限と公開の cleanup で捕まえる（設計書 01-05「記録を残す期間」）。
#[test]
fn expired_session_tombstone_outlives_its_unremovable_temporary_directory() -> TestResult {
    if rustix::process::geteuid().is_root() {
        // root は権限 0000 のディレクトリも消せるので、消せない一時ディレクトリを作れない。
        eprintln!("Skipped unremovable temporary check: root can remove mode 000 directories.");
        return Ok(());
    }
    struct Restore(PathBuf);
    impl Drop for Restore {
        fn drop(&mut self) {
            for entry in fs::read_dir(&self.0).into_iter().flatten().flatten() {
                let _restored = fs::set_permissions(
                    entry.path().join("blocked"),
                    fs::Permissions::from_mode(0o700),
                );
            }
        }
    }
    let fixture = Fixture::new()?;
    let session = fixture.session()?;
    session.writer()?.append(&header(&session))?;
    let id = session.id().clone();
    let directory = session.directory().to_owned();
    let temporary = session.temporary_dir().to_owned();
    let restore = Restore(fixture.paths.temporary_root.clone());
    fs::create_dir(temporary.join("blocked"))?;
    fs::write(temporary.join("blocked/keep"), b"keep")?;
    fs::set_permissions(temporary.join("blocked"), fs::Permissions::from_mode(0o000))?;
    drop(session);
    fs::File::open(directory.join("session.jsonl"))?.set_modified(UNIX_EPOCH)?;
    let permission_error = format!("(os error {})", rustix::io::Errno::ACCESS.raw_os_error());
    for run in 0..3 {
        let report = cleanup(&fixture.paths, DEFAULT_RETENTION_DAYS, SystemTime::now())?;
        assert_eq!(report.sessions_removed, 0, "run {run}");
        assert!(
            report.failures.iter().any(|failure| !failure.temporary
                && failure.id.as_ref() == Some(&id)
                && failure.reason.contains(&permission_error)
                && failure.reason.contains("lock is kept")),
            "run {run}: {:?}",
            report.failures
        );
        let entries = fs::read_dir(&fixture.paths.sessions_dir)?
            .map(|entry| entry.map(|entry| entry.path()))
            .collect::<Result<Vec<_>, _>>()?;
        let [tombstone] = entries.as_slice() else {
            return Err(format!("run {run}: unexpected session entries: {entries:?}").into());
        };
        assert!(
            tombstone
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with(&format!(".removing-{id}-")))
        );
        let inside = fs::read_dir(tombstone)?
            .map(|entry| entry.map(|entry| entry.file_name()))
            .collect::<Result<Vec<_>, _>>()?;
        assert_eq!(inside, vec![std::ffi::OsString::from("lock")], "run {run}");
        assert!(session_support::open_created_session(&fixture.paths, &id).is_err());
    }
    drop(restore);
    let report = cleanup(&fixture.paths, DEFAULT_RETENTION_DAYS, SystemTime::now())?;
    assert!(report.failures.is_empty(), "{:?}", report.failures);
    assert_eq!(fs::read_dir(&fixture.paths.sessions_dir)?.count(), 0);
    temporary_cleanup::check_temporary_root(&fixture.paths.temporary_root)?;
    Ok(())
}

// 関門: 子プロセスが一時ディレクトリへ移した入れ子のリポジトリの .git を、サンドボックスの
// 外の掃除が消す退行を、公開の cleanup で捕まえる。守る名前の下を残し、ほかは消し、残した
// パスを失敗に示す（設計書 01-05「一時ディレクトリ」）。
#[test]
fn temporary_cleanup_keeps_protected_entries_moved_into_it() -> TestResult {
    let fixture = Fixture::new()?;
    let session = fixture.session()?;
    session.writer()?.append(&header(&session))?;
    let id = session.id().clone();
    let temporary = session.temporary_dir().to_owned();
    drop(session);
    // macOS のファイルシステムは大文字と小文字を区別しないことが多いので、.GIT も守る。
    let names: &[&str] = if cfg!(target_os = "macos") {
        &[".git", ".GIT"]
    } else {
        &[".git"]
    };
    for name in names {
        fs::create_dir(&temporary)?;
        fs::set_permissions(&temporary, fs::Permissions::from_mode(0o700))?;
        fs::create_dir_all(temporary.join("sub").join(name))?;
        fs::write(temporary.join("sub").join(name).join("HEAD"), b"ref")?;
        fs::write(temporary.join("sub/other"), b"other")?;
        let report = cleanup(&fixture.paths, DEFAULT_RETENTION_DAYS, SystemTime::now())?;
        let [failure] = report.failures.as_slice() else {
            return Err(format!("{name}: unexpected failures: {:?}", report.failures).into());
        };
        assert!(failure.temporary && failure.id.as_ref() == Some(&id));
        assert!(
            failure.reason.contains(&format!("sub/{name}")),
            "{}",
            failure.reason
        );
        let prefix = format!("benitoiteh-{id}.removing-");
        let tombstones = fs::read_dir(&fixture.paths.temporary_root)?
            .map(|entry| entry.map(|entry| entry.path()))
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .filter(|path| {
                path.file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.starts_with(&prefix))
            })
            .collect::<Vec<_>>();
        let [tombstone] = tombstones.as_slice() else {
            return Err(format!("{name}: unexpected tombstones: {tombstones:?}").into());
        };
        assert_eq!(
            fs::read(tombstone.join("sub").join(name).join("HEAD"))?,
            b"ref"
        );
        assert!(!tombstone.join("sub/other").exists());
        // 守る名前の項目を移し出せば、次の掃除で墓標が消える。
        fs::remove_dir_all(tombstone.join("sub").join(name))?;
        let report = cleanup(&fixture.paths, DEFAULT_RETENTION_DAYS, SystemTime::now())?;
        assert!(report.failures.is_empty(), "{:?}", report.failures);
        temporary_cleanup::check_temporary_root(&fixture.paths.temporary_root)?;
    }
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

// 作成時の関門: 別の枝の圧縮・画像除去・未完了の呼び出しを混ぜる退行を、
// 記録の公開の復元の口で捕まえる。直線の再開のテストでは枝の混入を検出できない。
#[test]
fn replay_selects_only_the_path_and_its_reasoning_segments() -> TestResult {
    use benitoiteh::session::ResumeState;
    let fixture = Fixture::new()?;
    let session = fixture.session()?;
    let mut writer = session.writer()?;
    writer.append(&header(&session))?;
    let request = |node, text: &str| Record::User {
        node,
        kind: UserKind::Request,
        message: UserMessage {
            parts: vec![UserPart {
                kind: UserPartKind::Request,
                text: text.into(),
            }],
        },
    };
    writer.append(&request(1, "shared"))?;
    let image_call = call("image");
    writer.append(&Record::Assistant {
        node: 2,
        turn: 1,
        response: response(std::slice::from_ref(&image_call)),
    })?;
    let mut image = result(&image_call, "saved image");
    image.images.push(benitoiteh::provider::ToolImage {
        media_type: "image/gif".into(),
        sha256: "c".repeat(64),
        width: 1,
        height: 1,
        bytes: 10,
        data: None,
    });
    writer.append(&Record::ToolResult {
        node: 3,
        turn: 1,
        name: "read".into(),
        result: image.clone(),
        spill: None,
        stderr_spill: None,
    })?;
    writer.append(&Record::Assistant {
        node: 4,
        turn: 2,
        response: response(&[call("abandoned")]),
    })?;
    writer.append(&Record::ImagesRemoved {
        node: 5,
        results: vec!["image".into()],
    })?;
    let compacted = Conversation {
        items: vec![Item::AssistantTurn(response(&[]))],
    };
    writer.append(&Record::Compaction {
        node: 6,
        conversation: compacted.clone(),
        reason: Default::default(),
        summarized_items: 5,
        kept_items: 0,
    })?;
    writer.append(&Record::SessionStart {
        session_id: session.id().to_string(),
        resumed: true,
        base_url: None,
        system_prompt_sha256: Some("d".repeat(64)),
        tools_sha256: None,
        mode: Mode::Restricted.into(),
        provider: "test".into(),
        model: "new-model".into(),
        host: "example.test".into(),
        benitoiteh_version: "test".into(),
        notice: None,
        instruction_files: vec![],
        skipped_instructions: vec![],
        plugins: Vec::new(),
        skills: vec![],
        skipped_plugins: Vec::new(),
        plugin_notices: Vec::new(),
        skipped_skills: vec![],
    })?;
    writer.append(&Record::Branch {
        to: Some(3),
        from: Some(6),
        checkpoint: None,
    })?;
    writer.append(&request(7, "active"))?;
    writer.append(&Record::Assistant {
        node: 8,
        turn: 3,
        response: response(&[call("pending")]),
    })?;
    let state = ResumeState::read(session.directory())?;
    assert_eq!(state.tree.tip, Some(8));
    assert_eq!(state.tree.next_node, 9);
    assert_eq!(state.tree.leaves(), vec![6, 8]);
    assert_eq!(state.conversation.items.len(), 6);
    assert_eq!(
        state.conversation.items.get(2),
        Some(&Item::ToolResult(image))
    );
    assert_eq!(state.missing_results.len(), 1);
    assert!(
        matches!(state.missing_results.first(), Some(Record::ToolResult { result, .. }) if result.id == "pending")
    );
    assert_eq!(state.reasoning.len(), 2);
    assert_eq!(
        state
            .reasoning
            .first()
            .ok_or("first reasoning")?
            .system_prompt_sha256,
        "a".repeat(64)
    );
    assert_eq!(
        state
            .reasoning
            .last()
            .ok_or("last reasoning")?
            .system_prompt_sha256,
        "d".repeat(64)
    );
    assert_eq!(replay(session.directory())?, state.conversation);
    writer.append(&Record::Branch {
        to: Some(6),
        from: Some(8),
        checkpoint: None,
    })?;
    let state = ResumeState::read(session.directory())?;
    assert_eq!(state.conversation, compacted);
    assert_eq!(state.reasoning.len(), 1);
    assert_eq!(
        state
            .reasoning
            .first()
            .ok_or("compaction reasoning")?
            .system_prompt_sha256,
        "a".repeat(64)
    );
    writer.append(&Record::Branch {
        to: Some(5),
        from: Some(6),
        checkpoint: None,
    })?;
    let state = ResumeState::read(session.directory())?;
    assert!(
        matches!(state.conversation.items.get(2), Some(Item::ToolResult(result)) if result.images.is_empty() && result.text.contains("image was removed"))
    );
    assert!(
        matches!(state.missing_results.first(), Some(Record::ToolResult { result, .. }) if result.id == "abandoned")
    );
    // 別の枝に結果があっても、移った道にない結果は未記録として補う。
    writer.append(&Record::Branch {
        to: Some(2),
        from: Some(5),
        checkpoint: None,
    })?;
    let state = ResumeState::read(session.directory())?;
    assert!(
        matches!(state.missing_results.first(), Some(Record::ToolResult { result, .. }) if result.id == "image")
    );
    Ok(())
}

// 作成時の関門: 木の誤りを受け入れる退行と、検査の前に再開の一時領域を
// 消してしまう退行を、Session::open の境界で同時に捕まえる。
#[test]
fn invalid_trees_are_rejected_before_resume_touches_temporary_files() -> TestResult {
    let user = |node| Record::User {
        node,
        kind: UserKind::Request,
        message: UserMessage { parts: vec![] },
    };
    let cases = vec![
        vec![
            user(1),
            Record::Branch {
                to: Some(99),
                from: Some(1),
                checkpoint: None,
            },
        ],
        vec![
            user(1),
            Record::Branch {
                to: Some(2),
                from: Some(1),
                checkpoint: None,
            },
            user(2),
        ],
        vec![
            user(1),
            Record::Branch {
                to: Some(1),
                from: Some(99),
                checkpoint: None,
            },
        ],
        vec![
            user(1),
            Record::Branch {
                to: Some(1),
                from: Some(2),
                checkpoint: None,
            },
            user(2),
        ],
        vec![user(2), user(1)],
        vec![user(1), user(1)],
        vec![user(0)],
        vec![user(u64::MAX)],
    ];
    for records in cases {
        let fixture = Fixture::new()?;
        let session = fixture.session()?;
        let mut writer = session.writer()?;
        writer.append(&header(&session))?;
        for record in records {
            writer.append(&record)?;
        }
        let id = session.id().clone();
        let temporary = session.temporary_dir().to_owned();
        drop(writer);
        drop(session);
        fs::create_dir(&temporary)?;
        fs::set_permissions(&temporary, fs::Permissions::from_mode(0o700))?;
        let sentinel = temporary.join("keep");
        fs::write(&sentinel, b"untouched")?;
        assert!(matches!(
            session_support::open_created_session(&fixture.paths, &id),
            Err(benitoiteh::session::SessionError::Invalid(_))
        ));
        assert_eq!(fs::read(sentinel)?, b"untouched");
    }
    // 版 2 では会話の行の節点を省略できない。
    assert!(
        serde_json::from_value::<Record>(json!({"type":"compaction", "conversation":{"items":[]}}))
            .is_err()
    );
    Ok(())
}

// 関門: 写す道・推論の区間・独立した参照先を、実ファイルの再開の境界で守る。
// 直線の再開では、別の枝や圧縮前の記録の混入・欠落を見つけられない。
#[test]
fn fork_copies_the_whole_selected_path_and_only_its_assets() -> TestResult {
    use benitoiteh::session::{ForkTarget, ForkedFrom, ResumeState};
    let f = Fixture::new()?;
    let source = f.session()?;
    let mut writer = source.writer()?;
    writer.append(&header(&source))?;
    let request = |node, value: &str| Record::User {
        node,
        kind: UserKind::Request,
        message: UserMessage {
            parts: vec![UserPart {
                kind: UserPartKind::Request,
                text: value.into(),
            }],
        },
    };
    let start = |hash: &str| Record::SessionStart {
        session_id: source.id().to_string(),
        resumed: true,
        base_url: None,
        system_prompt_sha256: Some(hash.repeat(64)),
        tools_sha256: None,
        mode: Mode::Restricted.into(),
        provider: "test".into(),
        model: "model".into(),
        host: "example.test".into(),
        benitoiteh_version: "old".into(),
        notice: None,
        instruction_files: vec![],
        skipped_instructions: vec![],
        plugins: Vec::new(),
        skills: vec![],
        skipped_plugins: Vec::new(),
        plugin_notices: Vec::new(),
        skipped_skills: vec![],
    };
    writer.append(&start("a"))?;
    writer.append(&request(1, "root request"))?;
    let image_bytes = b"GIF89a\x01\x00\x01\x00\x80\x00\x00\x00\x00\x00\xff\xff\xff\x2c\x00\x00\x00\x00\x01\x00\x01\x00\x00\x02\x02\x44\x01\x00\x3b";
    let (hash, _) = source.store_object(image_bytes)?.ok_or("image storage")?;
    let mut image_call = call("image");
    image_call.name = "read".into();
    writer.append(&Record::Assistant {
        node: 2,
        turn: 1,
        response: response(std::slice::from_ref(&image_call)),
    })?;
    writer.append(&Record::ToolCall {
        turn: 1,
        call: image_call.clone(),
        validation_error: None,
    })?;
    let mut spill = source.spill_writer("image")?;
    spill.write_all(b"full stdout")?;
    let spill = spill.finish()?;
    let mut stderr = source.spill_writer("stderr")?;
    stderr.write_all(b"full stderr")?;
    let stderr = stderr.finish()?;
    let mut image = result(
        &image_call,
        &format!(
            "Full output: {}/{}",
            source.directory().display(),
            spill.path.display()
        ),
    );
    image.images.push(benitoiteh::provider::ToolImage {
        sha256: hash.clone(),
        media_type: "image/gif".into(),
        width: 1,
        height: 1,
        bytes: u64::try_from(image_bytes.len())?,
        data: None,
    });
    writer.append(&Record::ToolResult {
        node: 3,
        turn: 1,
        name: "read".into(),
        result: image.clone(),
        spill: Some(spill.clone()),
        stderr_spill: Some(stderr.clone()),
    })?;
    writer.append(&request(4, "other branch"))?;
    let other_call = call("other");
    writer.append(&Record::Assistant {
        node: 5,
        turn: 2,
        response: response(std::slice::from_ref(&other_call)),
    })?;
    let mut compact_spill = source.spill_writer("only-in-compaction")?;
    compact_spill.write_all(b"compacted output")?;
    let compact_spill = compact_spill.finish()?;
    let other_result = result(
        &other_call,
        &format!(
            "Full output: {}/{}",
            source.directory().display(),
            compact_spill.path.display()
        ),
    );
    writer.append(&Record::ToolResult {
        node: 6,
        turn: 2,
        name: "write".into(),
        result: other_result.clone(),
        spill: Some(compact_spill.clone()),
        stderr_spill: None,
    })?;
    writer.append(&start("d"))?;
    writer.append(&Record::Branch {
        to: Some(3),
        from: Some(6),
        checkpoint: None,
    })?;
    writer.append(&request(7, "/literal request"))?;
    let compacted = Conversation {
        items: vec![
            Item::UserMessage(UserMessage {
                parts: vec![UserPart {
                    kind: UserPartKind::Request,
                    text: "summary".into(),
                }],
            }),
            Item::AssistantTurn(response(std::slice::from_ref(&image_call))),
            Item::ToolResult(image),
            Item::AssistantTurn(response(std::slice::from_ref(&other_call))),
            Item::ToolResult(other_result),
        ],
    };
    writer.append(&Record::Compaction {
        node: 8,
        conversation: compacted,
        reason: Default::default(),
        summarized_items: 4,
        kept_items: 2,
    })?;
    writer.append(&Record::ImagesRemoved {
        node: 9,
        results: vec![],
    })?;
    let pending = call("turn-3-call-1");
    writer.append(&Record::Assistant {
        node: 10,
        turn: 3,
        response: response(std::slice::from_ref(&pending)),
    })?;
    // 最後の節点の後にも、未完の呼び出しに必要な制御の記録がある。
    writer.append(&Record::ToolCall {
        turn: 3,
        call: pending.clone(),
        validation_error: None,
    })?;
    writer.append(&Record::ToolStart { call: pending })?;
    writer.append(&Record::Checkpoint(benitoiteh::rewind::Checkpoint {
        id: "point".into(),
        reason: benitoiteh::rewind::CheckpointReason::Request,
        turn: 3,
        call_id: None,
        created_at: Default::default(),
        node: Some(10),
        manifest: None,
        incomplete: vec![],
    }))?;
    writer.append(&Record::Approval {
        phase: benitoiteh::session::ApprovalPhase::Result,
        id: "approval".into(),
        tool_id: "pending".into(),
        request: None,
        outcome: Some(ApprovalOutcome::ApprovedForSession),
        kind: None,
        facts: Default::default(),
        justification: None,
        user_message: None,
    })?;
    writer.append(&Record::WriteTarget {
        call_id: "pending".into(),
        real_path: "/unrelated".into(),
    })?;
    writer.append(&Record::TurnEnd {
        turn: 3,
        finish_reason: FinishReason::Cancelled,
        usage: Default::default(),
    })?;
    let unused_object = source
        .store_object(b"checkpoint-only")?
        .ok_or("unused object")?
        .0;
    let mut unused_spill = source.spill_writer("unrelated")?;
    unused_spill.write_all(b"not copied")?;
    let unused_spill = unused_spill.finish()?;
    let original = fs::read(source.directory().join("session.jsonl"))?;
    let source_state = ResumeState::read(source.directory())?;
    let copied = source
        .fork(
            &f.paths,
            std::path::Path::new("/current-project"),
            ForkTarget::Current,
        )?
        .session;
    let records = read_records(copied.directory())?;
    let Some(Record::Header(h)) = records.first() else {
        return Err("missing header".into());
    };
    assert_eq!(
        h.forked_from,
        Some(ForkedFrom {
            session: source.id().to_string(),
            node: 10
        })
    );
    assert_eq!(h.project_dir, PathBuf::from("/current-project"));
    assert_eq!(h.system_prompt_sha256, "a".repeat(64));
    assert_eq!(h.session_id, copied.id().to_string());
    assert_eq!(
        records.iter().filter_map(Record::node).collect::<Vec<_>>(),
        (1..=7).collect::<Vec<_>>()
    );
    assert!(records.iter().all(|record| matches!(
        record,
        Record::Header(_)
            | Record::SessionStart { .. }
            | Record::User { .. }
            | Record::Assistant { .. }
            | Record::ToolCall { .. }
            | Record::ToolStart { .. }
            | Record::ToolResult { .. }
            | Record::Compaction { .. }
            | Record::ImagesRemoved { .. }
    )));
    assert_eq!(records.iter().filter(|record| matches!(record, Record::SessionStart { session_id, .. } if session_id == copied.id().as_str())).count(), 2);
    assert!(
        !copied
            .directory()
            .join("objects")
            .join(unused_object)
            .exists()
    );
    assert!(!copied.directory().join(unused_spill.path).exists());
    let copied_state = ResumeState::read(copied.directory())?;
    let mut expected = source_state.conversation.clone();
    for item in &mut expected.items {
        if let Item::ToolResult(result) = item {
            result.text = result.text.replace(
                &source.directory().display().to_string(),
                &copied.directory().display().to_string(),
            );
        }
    }
    assert_eq!(copied_state.conversation, expected);
    assert_eq!(copied_state.max_turn, source_state.max_turn);
    assert_eq!(
        copied_state
            .reasoning
            .iter()
            .map(|r| (&r.system_prompt_sha256, &r.tools_sha256, &r.destination))
            .collect::<Vec<_>>(),
        source_state
            .reasoning
            .iter()
            .map(|r| (&r.system_prompt_sha256, &r.tools_sha256, &r.destination))
            .collect::<Vec<_>>()
    );
    assert_eq!(copied_state.files.read, source_state.files.read);
    assert!(copied_state.missing_results.iter().any(|record| matches!(record, Record::ToolResult { result, .. } if result.text.contains("interrupted"))));
    for node in [4, 7] {
        let before = source
            .fork(
                &f.paths,
                std::path::Path::new("/project"),
                ForkTarget::BeforeRequest(node),
            )?
            .session;
        let records = read_records(before.directory())?;
        assert_eq!(
            records.iter().filter_map(Record::node).collect::<Vec<_>>(),
            vec![1, 2, 3]
        );
        assert_eq!(
            records
                .iter()
                .filter(|record| matches!(record, Record::SessionStart { .. }))
                .count(),
            1
        );
        assert_eq!(ResumeState::read(before.directory())?.max_turn, 1);
    }
    assert!(
        source
            .fork(
                &f.paths,
                std::path::Path::new("/project"),
                ForkTarget::BeforeRequest(1)
            )
            .is_err()
    );
    assert_eq!(
        fs::read(source.directory().join("session.jsonl"))?,
        original
    );
    let directory = source.directory().to_owned();
    drop(writer);
    drop(source);
    fs::remove_dir_all(directory)?;
    assert_eq!(copied.read_object(&hash, 4096)?, image_bytes);
    for (info, bytes) in [
        (spill, b"full stdout".as_slice()),
        (stderr, b"full stderr".as_slice()),
        (compact_spill, b"compacted output".as_slice()),
    ] {
        assert_eq!(fs::read(copied.directory().join(info.path))?, bytes);
    }
    let copied_id = copied.id().clone();
    drop(copied);
    let reopened = session_support::open_created_session(&f.paths, &copied_id)?;
    assert_eq!(reopened.take_resume_state()?.conversation, expected);
    Ok(())
}

// 関門: 上限超過・不正な参照先の失敗が元の記録を変えたり、作りかけを残す退行を守る。
#[test]
fn fork_failures_leave_no_destination_and_preserve_the_source() -> TestResult {
    use benitoiteh::session::{ForkTarget, SessionError, TOTAL_LIMIT};
    for failure in [
        "session quota",
        "total quota",
        "spill link",
        "spill mode",
        "object hash",
        "missing",
    ] {
        let f = Fixture::new()?;
        let source = f.session()?;
        let mut writer = source.writer()?;
        writer.append(&header(&source))?;
        let tool = call("output");
        let mut spill = source.spill_writer("output")?;
        spill.write_all(b"output")?;
        let spill = spill.finish()?;
        let mut output = result(&tool, "result");
        let (hash, _) = source.store_object(b"image bytes")?.ok_or("object")?;
        output.images.push(benitoiteh::provider::ToolImage {
            media_type: "image/gif".into(),
            sha256: hash.clone(),
            width: 1,
            height: 1,
            bytes: 11,
            data: None,
        });
        writer.append(&Record::ToolResult {
            node: 1,
            turn: 1,
            name: "read".into(),
            result: output,
            spill: Some(spill.clone()),
            stderr_spill: None,
        })?;
        let spill_path = source.directory().join(&spill.path);
        match failure {
            "session quota" => fs::File::options()
                .write(true)
                .open(&spill_path)?
                .set_len(1024 * 1024 * 1024)?,
            "total quota" => {
                let filler = source.spill_writer("filler")?.finish()?;
                fs::File::options()
                    .write(true)
                    .open(source.directory().join(filler.path))?
                    .set_len(TOTAL_LIMIT)?;
            }
            "spill link" => {
                fs::remove_file(&spill_path)?;
                symlink(source.directory().join("session.jsonl"), &spill_path)?;
            }
            "spill mode" => fs::set_permissions(&spill_path, fs::Permissions::from_mode(0o644))?,
            "object hash" => fs::write(source.directory().join("objects").join(&hash), b"corrupt")?,
            "missing" => {
                fs::remove_file(&spill_path)?;
                fs::remove_file(source.directory().join("objects").join(&hash))?;
            }
            _ => return Err("case".into()),
        }
        let original = fs::read(source.directory().join("session.jsonl"))?;
        let result = source.fork(
            &f.paths,
            std::path::Path::new("/project"),
            ForkTarget::Current,
        );
        if failure == "missing" {
            assert!(result.is_ok());
        } else {
            let error = result.err().ok_or("fork should fail")?;
            if failure.contains("quota") {
                assert!(
                    matches!(error, SessionError::Invalid(message) if message.contains("storage limit"))
                );
            }
            assert_eq!(fs::read_dir(&f.paths.sessions_dir)?.count(), 1, "{failure}");
            assert_eq!(
                fs::read_dir(&f.paths.temporary_root)?.count(),
                1,
                "{failure}"
            );
        }
        assert_eq!(
            fs::read(source.directory().join("session.jsonl"))?,
            original
        );
    }
    Ok(())
}

// 関門: 参照数に比例して記述子を保持する退行を、上限を下げた独立した子プロセスで守る。
#[test]
fn fork_copies_more_references_than_the_descriptor_limit() -> TestResult {
    const CHILD: &str = "BENITOITEH_FORK_FD_CHILD";
    let f = Fixture::new()?;
    if std::env::var_os(CHILD).is_none() {
        let output = std::process::Command::new(std::env::current_exe()?)
            .args([
                "--exact",
                "fork_copies_more_references_than_the_descriptor_limit",
                "--nocapture",
            ])
            .env(CHILD, "1")
            .env("XDG_CONFIG_HOME", f.root.join("config"))
            .env("XDG_STATE_HOME", f.root.join("state"))
            .env("TMPDIR", &f.paths.temporary_root)
            .output()?;
        assert!(
            output.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        return Ok(());
    }
    let source = f.session()?;
    let mut writer = source.writer()?;
    writer.append(&header(&source))?;
    for node in 1..=300_u64 {
        let mut spill = source.spill_writer(&format!("reference-{node}"))?;
        spill.write_all(&node.to_le_bytes())?;
        writer.append(&Record::ToolResult {
            node,
            turn: node,
            name: "read".into(),
            result: result(&call(&format!("call-{node}")), "output"),
            spill: Some(spill.finish()?),
            stderr_spill: None,
        })?;
    }
    let mut limit = rustix::process::getrlimit(rustix::process::Resource::Nofile);
    limit.current = Some(64);
    rustix::process::setrlimit(rustix::process::Resource::Nofile, limit)?;
    let copied = source
        .fork(
            &f.paths,
            std::path::Path::new("/project"),
            benitoiteh::session::ForkTarget::Current,
        )?
        .session;
    assert_eq!(fs::read_dir(copied.directory().join("spill"))?.count(), 300);
    assert_eq!(
        read_records(copied.directory())?
            .iter()
            .filter_map(Record::node)
            .count(),
        300
    );
    Ok(())
}

// 関門: 同じセッションの別のファイルや、実在するパスの前方一致まで書き換える退行を守る。
#[test]
fn fork_rewrites_only_complete_spill_paths_owned_by_each_result() -> TestResult {
    let f = Fixture::new()?;
    let source = f.session()?;
    let mut writer = source.writer()?;
    writer.append(&header(&source))?;
    let mut spill = source.spill_writer("stdout")?;
    spill.write_all(b"stdout")?;
    let spill = spill.finish()?;
    let mut other = source.spill_writer("other")?;
    other.write_all(b"other")?;
    let other = other.finish()?;
    let own = source.directory().join(&spill.path).display().to_string();
    let foreign = source.directory().join(&other.path).display().to_string();
    let fake = source
        .directory()
        .join("spill/not-a-real-spill")
        .display()
        .to_string();
    let body = format!("Full output: {own}]\n{fake}\n{own}.suffix\n{own}/child\n{foreign}\n{own} ");
    let first = result(&call("first"), &body);
    writer.append(&Record::ToolResult {
        node: 1,
        turn: 1,
        name: "bash".into(),
        result: first.clone(),
        spill: Some(spill.clone()),
        stderr_spill: None,
    })?;
    writer.append(&Record::ToolResult {
        node: 2,
        turn: 1,
        name: "bash".into(),
        result: result(&call("second"), &foreign),
        spill: Some(other.clone()),
        stderr_spill: None,
    })?;
    writer.append(&Record::Compaction {
        node: 3,
        conversation: Conversation {
            items: vec![Item::ToolResult(first)],
        },
        reason: Default::default(),
        summarized_items: 2,
        kept_items: 1,
    })?;
    let copied = source
        .fork(
            &f.paths,
            std::path::Path::new("/project"),
            benitoiteh::session::ForkTarget::Current,
        )?
        .session;
    let replacement = copied.directory().join(&spill.path).display().to_string();
    let expected = format!(
        "Full output: {replacement}]\n{fake}\n{own}.suffix\n{own}/child\n{foreign}\n{replacement} "
    );
    let records = read_records(copied.directory())?;
    assert!(records.iter().any(|record| matches!(record, Record::ToolResult { result, .. } if result.id == "first" && result.text == expected)));
    assert!(records.iter().any(|record| matches!(record, Record::Compaction { conversation, .. } if matches!(conversation.items.first(), Some(Item::ToolResult(result)) if result.text == expected))));
    Ok(())
}

// 関門: 一覧では断る写した元の不正な ID を、開く処理が受け入れる退行を守る。
#[test]
fn fork_origin_ids_are_checked_by_both_listing_and_opening() -> TestResult {
    for origin in [
        "invalid",
        "../../escape",
        "20261019T120000Z-0123456789abcdef",
    ] {
        let f = Fixture::new()?;
        let source = f.session()?;
        let mut value = serde_json::to_value(header(&source))?;
        value
            .as_object_mut()
            .ok_or("header")?
            .insert("forked_from".into(), json!({"session":origin,"node":1}));
        fs::write(
            source.directory().join("session.jsonl"),
            format!("{value}\n"),
        )?;
        let id = source.id().clone();
        drop(source);
        let entries = benitoiteh::session::list_sessions(&f.paths)?;
        let entry = entries
            .iter()
            .find(|entry| entry.id == id.as_str())
            .ok_or("entry")?;
        let opened = session_support::open_created_session(&f.paths, &id);
        if origin == "20261019T120000Z-0123456789abcdef" {
            assert!(entry.error.is_none());
            assert!(opened.is_ok());
        } else {
            let error = opened.err().ok_or("invalid origin was accepted")?;
            assert_eq!(entry.error.as_deref(), Some(error.to_string().as_str()));
            assert!(error.to_string().contains("Invalid session record format"));
        }
    }
    Ok(())
}

// 関門: 一時の根が共有されても、別のデータの根の生きている補助と一時ディレクトリや、
// 記録のない補助を消さない。補助の子は `id` なしで作り、前の版が作った子を名前の対応表で
// 扱う後始末を確かめる（設計書 03-04「補助」）。
#[test]
fn shared_temporary_cleanup_preserves_foreign_live_entries_and_unrecorded_helpers() -> TestResult {
    let first = Fixture::new()?;
    let second = Fixture::new()?;
    let mut second_paths = second.paths.clone();
    second_paths.temporary_root = first.paths.temporary_root.clone();
    let foreign = Session::create(&second_paths, SessionId::generate(SystemTime::now())?)?;
    let known = first.session()?;
    let unknown_id = SessionId::generate(SystemTime::now())?;
    let paths = [
        &foreign.id().to_string(),
        &known.id().to_string(),
        &unknown_id.to_string(),
    ]
    .map(|id| {
        first
            .paths
            .temporary_root
            .join(format!("bha-{}", rustix::process::geteuid().as_raw()))
            .join(id)
    });
    let parent = paths
        .first()
        .and_then(|path| path.parent())
        .ok_or("missing parent")?;
    fs::create_dir(parent)?;
    fs::set_permissions(parent, fs::Permissions::from_mode(0o700))?;
    for path in &paths {
        fs::create_dir(path)?;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
        fs::write(path.join("keep"), b"keep")?;
    }
    let missing_record = known.directory().join("session.jsonl");
    drop(known);
    fs::remove_file(missing_record)?;
    let report = cleanup(&first.paths, DEFAULT_RETENTION_DAYS, SystemTime::now())?;
    assert!(report.failures.is_empty(), "{:?}", report.failures);
    for path in &paths {
        assert_eq!(fs::read(path.join("keep"))?, b"keep");
    }
    assert!(foreign.directory().exists());
    // ほかのデータの置き場所で動くセッションの一時ディレクトリは、この置き場所に記録が
    // ないので消さない（設計書 01-05「一時ディレクトリ」）。
    assert!(foreign.temporary_dir().exists());
    Ok(())
}

// 関門: macOS の子の名前は ID の乱数の部分だけなので、名前の対応表では乱数の重なる
// 二つのセッションの子をどちらも決められない。`id` で錠を確かめ、錠のない方だけを消す。
// 同じ名前の子は一つの親に一つしか置けないので、一時の根と /tmp の親に分けて置く。
#[cfg(target_os = "macos")]
#[test]
fn helper_ids_resolve_children_whose_random_parts_collide() -> TestResult {
    use std::os::unix::fs::OpenOptionsExt;
    let f = Fixture::new()?;
    let random = SessionId::generate(SystemTime::now())?.to_string();
    let random = random.rsplit_once('-').ok_or("missing random part")?.1;
    let live = Session::create(&f.paths, format!("20200101T000000Z-{random}").parse()?)?;
    let stale = Session::create(&f.paths, format!("20200102T000000Z-{random}").parse()?)?;
    let stale_id = stale.id().clone();
    drop(stale);
    let parent_name = format!("bha-{}", rustix::process::geteuid().as_raw());
    struct Remove(PathBuf);
    impl Drop for Remove {
        fn drop(&mut self) {
            let _removed = fs::remove_dir_all(&self.0);
        }
    }
    let shared = std::path::Path::new("/tmp")
        .canonicalize()?
        .join(&parent_name);
    match fs::DirBuilder::new().mode(0o700).create(&shared) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(error.into()),
    }
    let private = f.paths.temporary_root.join(&parent_name);
    fs::DirBuilder::new().mode(0o700).create(&private)?;
    let live_child = private.join(random);
    let stale_child = Remove(shared.join(random));
    for (child, id) in [(&live_child, live.id()), (&stale_child.0, &stale_id)] {
        fs::DirBuilder::new().mode(0o700).create(child)?;
        fs::set_permissions(child, fs::Permissions::from_mode(0o700))?;
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(child.join("id"))?;
        file.set_permissions(fs::Permissions::from_mode(0o600))?;
        writeln!(file, "{id}")?;
    }
    let report = cleanup(&f.paths, DEFAULT_RETENTION_DAYS, SystemTime::now())?;
    assert!(report.failures.is_empty(), "{:?}", report.failures);
    assert!(live_child.join("id").exists());
    assert!(!stale_child.0.exists());
    drop(live);
    Ok(())
}

// 一時の根だけが読めなくても、記録の保持期間の掃除まで中断しない。
#[test]
fn unavailable_temporary_root_is_diagnosed_without_skipping_session_cleanup() -> TestResult {
    for inaccessible in [false, true] {
        if inaccessible && rustix::process::geteuid().is_root() {
            continue;
        }
        let f = Fixture::new()?;
        let stale = Session::create(&f.paths, "20200101T000000Z-0000000000000009".parse()?)?;
        let directory = stale.directory().to_path_buf();
        fs::File::open(directory.join("session.jsonl"))?.set_modified(UNIX_EPOCH)?;
        drop(stale);
        if inaccessible {
            fs::set_permissions(&f.paths.temporary_root, fs::Permissions::from_mode(0o000))?;
        } else {
            fs::remove_dir_all(&f.paths.temporary_root)?;
            fs::write(&f.paths.temporary_root, b"not a directory")?;
        }
        let result = cleanup(&f.paths, DEFAULT_RETENTION_DAYS, SystemTime::now());
        if inaccessible {
            fs::set_permissions(&f.paths.temporary_root, fs::Permissions::from_mode(0o700))?;
        }
        let report = result?;
        assert!(!directory.exists());
        assert_eq!(report.sessions_removed, 1);
        let diagnostic = report
            .failures
            .iter()
            .find(|failure| failure.temporary && failure.id.is_none())
            .ok_or("missing cleanup failure")?
            .diagnostic();
        // ID の分からない失敗は、`unknown` と二重の前置きを付けず、パスを一度だけ示す。
        assert!(diagnostic.starts_with("Skipped cleanup of"), "{diagnostic}");
        assert!(!diagnostic.contains("unknown"), "{diagnostic}");
        assert_eq!(
            diagnostic
                .matches(&f.paths.temporary_root.display().to_string())
                .count(),
            1,
            "{diagnostic}"
        );
        assert!(
            !diagnostic.contains("mediation was disabled"),
            "{diagnostic}"
        );
    }
    Ok(())
}

// 関門: 掃除を飛ばした診断が、進行中の仲介を止めたと誤って告げない。
#[test]
fn cleanup_uses_cleanup_specific_diagnostic_for_nonprivate_parent() -> TestResult {
    let f = Fixture::new()?;
    let parent = f
        .paths
        .temporary_root
        .join(format!("bha-{}", rustix::process::geteuid().as_raw()));
    fs::DirBuilder::new().mode(0o755).create(&parent)?;
    // コンテナの umask によって 0700 へ変わらないよう、検査する属性を確定する。
    fs::set_permissions(&parent, fs::Permissions::from_mode(0o755))?;
    fs::write(parent.join("keep"), b"keep")?;
    let report = cleanup(&f.paths, DEFAULT_RETENTION_DAYS, SystemTime::now())?;
    let message = report
        .failures
        .iter()
        .find(|failure| failure.temporary)
        .ok_or("missing cleanup diagnostic")?
        .diagnostic();
    assert!(message.starts_with("Skipped cleanup of"), "{message}");
    assert!(!message.contains("unknown"), "{message}");
    assert_eq!(
        message.matches(&parent.display().to_string()).count(),
        1,
        "{message}"
    );
    assert!(
        message.contains("not a private directory owned by you"),
        "{message}"
    );
    assert!(!message.contains("mediation was disabled"), "{message}");
    assert_eq!(fs::read(parent.join("keep"))?, b"keep");
    Ok(())
}
