//! 巻き戻しの契約と、主プロセスの操作が外や守る場所を変えない性質（設計書 01-05、07-01）。

#[path = "support/rewind.rs"]
mod fixture;
#[allow(dead_code)]
mod support;
use benitoiteh::agent::{Agent, AgentEvent};
use benitoiteh::config::Mode;
use benitoiteh::rewind::{
    CheckpointLimits, CheckpointReason, CheckpointRecorder, ManifestKind, RewindScope,
};
use benitoiteh::session::{self, Record};
use benitoiteh::tools::{ApprovalStatus, Invocation, ToolSet};
use fixture::{Fixture, TestResult, capture, response, rewind, run};
use serde_json::json;
use std::fs;
use std::os::unix::fs::{MetadataExt, PermissionsExt, symlink};
use std::path::Path;
use std::sync::{Arc, atomic::AtomicBool, mpsc};
use std::thread;
use std::time::Duration;
use support::mock_provider::MockProvider;

// 作成時の関門: 時点の持ち主の境界で、秘密をリンク越しに保存する退行、除外した
// ディレクトリへ降りる退行、上限を無視する退行を捕まえる。既存の道具の探索とは別の走査である。
#[test]
fn capture_respects_exclusions_and_never_follows_links() -> TestResult {
    let f = Fixture::new()?;
    for path in [".git", "sub/.benitoiteh", "target", "nested"] {
        fs::create_dir_all(f.project.join(path))?;
    }
    fs::write(f.project.join(".git/config"), "protected")?;
    fs::write(f.project.join(".env"), "secret")?;
    fs::write(f.project.join(".ignore"), "ordinary\n")?;
    fs::write(f.project.join("sub/.GIT"), "protected")?;
    fs::write(f.project.join(".gitignore"), "/target/\n*.log\n!keep.log\n")?;
    fs::write(f.project.join("nested/.gitignore"), "local.txt\n")?;
    for name in [
        "target/hidden",
        "nested/local.txt",
        "ignored.log",
        "keep.log",
        "ordinary",
    ] {
        fs::write(f.project.join(name), name)?;
    }
    let big = fs::File::create(f.project.join("large"))?;
    big.set_len(8 * 1024 * 1024 + 1)?;
    fs::create_dir(f.root.join("outside"))?;
    fs::write(f.root.join("outside/private"), "outside-content")?;
    for (target, name) in [
        (".env", "notes.txt"),
        ("../outside", "dir"),
        ("missing", "dangling"),
    ] {
        symlink(target, f.project.join(name))?;
    }
    let recorder = f.recorder()?;
    let point = capture(&recorder)?;
    assert!(point.incomplete.is_empty());
    let manifest = recorder.manifest(&point.id)?;
    let entry = |path| {
        manifest
            .entries
            .iter()
            .find(|e| e.path.as_deref() == Some(path))
    };
    for path in [".git", "sub/.GIT", "sub/.benitoiteh"] {
        assert_eq!(
            entry(path).and_then(|e| e.excluded.as_deref()),
            Some("protected")
        );
    }
    for path in ["target", "nested/local.txt", "ignored.log"] {
        assert_eq!(
            entry(path).and_then(|e| e.excluded.as_deref()),
            Some("gitignore")
        );
    }
    assert!(entry("target/hidden").is_none());
    assert!(entry("keep.log").is_some_and(|e| e.stored));
    assert_eq!(
        entry("large").and_then(|e| e.excluded.as_deref()),
        Some("file_size")
    );
    for name in ["notes.txt", "dir", "dangling"] {
        assert_eq!(entry(name).map(|e| e.kind), Some(ManifestKind::Symlink));
    }
    for object in fs::read_dir(f.session.directory().join("objects"))? {
        let bytes = fs::read(object?.path())?;
        assert_ne!(bytes, b"secret");
        assert_ne!(bytes, b"outside-content");
        assert_ne!(bytes, b"protected");
    }
    assert_eq!(
        fs::metadata(f.session.directory().join("objects"))?
            .permissions()
            .mode()
            & 0o777,
        0o700
    );
    Ok(())
}

// 作成時の関門: 待つ open や WalkBuilder に戻ると、FIFO と /dev/zero のリンクで
// 主プロセスが止まる。見張りはスレッドの完了を時間で区切り、本物の OS の要素を使う。
#[test]
fn malicious_gitignore_cannot_block_capture() -> TestResult {
    for fifo in [false, true] {
        let f = Fixture::new()?;
        if fifo {
            assert!(
                std::process::Command::new("mkfifo")
                    .arg(f.project.join(".gitignore"))
                    .status()?
                    .success()
            );
        } else {
            symlink("/dev/zero", f.project.join(".gitignore"))?;
        }
        fs::write(f.project.join("normal"), "normal")?;
        let recorder = f.recorder()?;
        let (sender, receiver) = mpsc::sync_channel(1);
        let worker = thread::spawn(move || {
            let result = capture(&recorder)
                .map(|point| point.incomplete.is_empty())
                .map_err(|e| e.to_string());
            let _sent = sender.send(result);
            drop(f);
        });
        assert!(receiver.recv_timeout(Duration::from_secs(2))??);
        worker.join().map_err(|_| "capture worker failed")?;
    }
    Ok(())
}

// 作成時の関門: 変化のないファイルを読み直す退行と、同じ秒の変更を見落とす退行は
// 公開の統計で区別する。上限で保存しなかった値を使い回す退行も同じ境界で捕まえる。
#[test]
fn content_reuse_requires_old_timestamps_and_saved_objects() -> TestResult {
    let f = Fixture::new()?;
    fs::write(f.project.join("file"), "first")?;
    let recorder = f.recorder()?;
    let mut first = capture(&recorder)?;
    assert_eq!(recorder.stats()?.files_hashed, 1);
    // 秒の境を偶然またいだ場合は書き直し、同じ秒というテストの前提を作る。
    for _attempt in 0..3 {
        if fs::metadata(f.project.join("file"))?.ctime() == first.created_at.seconds {
            break;
        }
        fs::write(f.project.join("file"), "first")?;
        first = capture(&recorder)?;
    }
    assert_eq!(
        fs::metadata(f.project.join("file"))?.ctime(),
        first.created_at.seconds
    );
    capture(&recorder)?;
    assert_eq!(recorder.stats()?.files_hashed, 1);
    // ctime を書き換えず、前の時点 T より古い秒になるまで待つ。
    thread::sleep(Duration::from_millis(1100));
    capture(&recorder)?;
    capture(&recorder)?;
    assert_eq!(recorder.stats()?.files_hashed, 0);
    fs::write(f.project.join("file"), "other")?;
    capture(&recorder)?;
    assert_eq!(recorder.stats()?.files_hashed, 1);
    let objects = fs::read_dir(f.session.directory().join("objects"))?.count();
    let bytes = f.session.read_object(
        first.manifest.as_deref().ok_or("missing hash")?,
        1024 * 1024,
    )?;
    assert_eq!(
        f.session.store_object(&bytes)?.map(|(_, written)| written),
        Some(false)
    );
    assert_eq!(
        fs::read_dir(f.session.directory().join("objects"))?.count(),
        objects
    );
    recorder.set_limits(CheckpointLimits {
        max_content_bytes: 1,
        ..Default::default()
    })?;
    let limited = capture(&recorder)?;
    assert!(limited.incomplete.is_empty());
    assert!(recorder.manifest(&limited.id)?.complete);
    let missing = recorder.restore_files(&limited.id, false)?;
    assert!(
        missing
            .failures
            .iter()
            .any(|failure| failure.path == Path::new("file"))
    );
    assert!(
        !recorder
            .manifest(&limited.id)?
            .entries
            .first()
            .is_some_and(|e| e.stored)
    );
    recorder.set_limits(CheckpointLimits::default())?;
    capture(&recorder)?;
    assert_eq!(recorder.stats()?.files_hashed, 1);
    recorder.set_limits(CheckpointLimits {
        max_entries: 0,
        ..Default::default()
    })?;
    assert!(!capture(&recorder)?.incomplete.is_empty());
    recorder.set_limits(CheckpointLimits {
        max_duration: Duration::ZERO,
        ..Default::default()
    })?;
    assert!(!capture(&recorder)?.incomplete.is_empty());
    assert!(f.session.reserve(1024 * 1024 * 1024 - 1024 * 1024));
    // configure_budget は起動の残量を任意に小さくできる既存の口である。
    f.session.configure_budget(&session::StorageUsage {
        current_outputs: 1024 * 1024 * 1024,
        ..Default::default()
    });
    recorder.set_limits(CheckpointLimits::default())?;
    fs::write(f.project.join("file"), "new-content")?;
    let budget = capture(&recorder)?;
    assert!(!budget.incomplete.is_empty());
    assert!(budget.manifest.is_none());
    Ok(())
}

// 作成時の関門: 戻す境界で種類の変更、削除の順序、権限、リンクの保存と before_rewind
// を一つの往復で確かめる。単純なファイルの書き戻しだけでは消す順の誤りを捕まえない。
#[test]
fn restore_round_trip_handles_types_permissions_and_links() -> TestResult {
    let f = Fixture::new()?;
    fs::write(f.project.join("file"), "old")?;
    fs::set_permissions(f.project.join("file"), fs::Permissions::from_mode(0o4755))?;
    fs::create_dir(f.project.join("directory"))?;
    fs::write(f.project.join("directory/child"), "child")?;
    symlink("../outside-missing", f.project.join("link"))?;
    fs::write(f.project.join("to_directory"), "was-file")?;
    let recorder = f.recorder()?;
    let original = capture(&recorder)?;
    fs::remove_file(f.project.join("file"))?;
    fs::create_dir(f.project.join("file"))?;
    fs::write(f.project.join("file/new"), "new")?;
    fs::remove_dir_all(f.project.join("directory"))?;
    symlink("../outside", f.project.join("directory"))?;
    fs::remove_file(f.project.join("link"))?;
    fs::write(f.project.join("link"), "now-file")?;
    fs::remove_file(f.project.join("to_directory"))?;
    fs::create_dir(f.project.join("to_directory"))?;
    fs::write(f.project.join("added"), "added")?;
    let report = recorder.restore_files(&original.id, false)?;
    assert!(report.failures.is_empty(), "{:?}", report.failures);
    assert_eq!(fs::read(f.project.join("file"))?, b"old");
    assert_eq!(
        fs::metadata(f.project.join("file"))?.permissions().mode() & 0o7777,
        0o755
    );
    assert_eq!(fs::read(f.project.join("directory/child"))?, b"child");
    assert_eq!(
        fs::read_link(f.project.join("link"))?,
        Path::new("../outside-missing")
    );
    assert_eq!(fs::read(f.project.join("to_directory"))?, b"was-file");
    assert!(!f.project.join("added").exists());
    let undo = recorder.restore_files(
        report.before_rewind.as_deref().ok_or("missing before")?,
        false,
    )?;
    assert!(undo.failures.is_empty(), "{:?}", undo.failures);
    assert_eq!(fs::read(f.project.join("file/new"))?, b"new");
    assert_eq!(fs::read(f.project.join("added"))?, b"added");
    Ok(())
}

#[test]
fn complete_checkpoints_never_delete_excluded_paths() -> TestResult {
    let f = Fixture::new()?;
    fs::File::create(f.project.join("large"))?.set_len(8 * 1024 * 1024 + 1)?;
    let recorder = f.recorder()?;
    let original = capture(&recorder)?;
    assert!(original.incomplete.is_empty());
    assert!(recorder.manifest(&original.id)?.complete);
    fs::write(f.project.join("large"), "x")?;
    fs::write(f.project.join("new"), "new")?;
    fs::write(f.project.join(".gitignore"), "ignored\n")?;
    fs::write(f.project.join("ignored"), "keep")?;
    recorder.restore_files(&original.id, false)?;
    assert_eq!(fs::read(f.project.join("large"))?, b"x");
    assert!(!f.project.join("new").exists());
    assert_eq!(fs::read(f.project.join("ignored"))?, b"keep");
    Ok(())
}

// 作成時の関門: 攻撃者が有効なハッシュ名で一覧を作り直した場合にも、パスの構文と
// 守る名前の検証が働くことを確かめる。単にハッシュ不一致で拒否されるテストとは分ける。
#[test]
fn untrusted_manifests_cannot_write_outside_or_protected_paths() -> TestResult {
    for path in [
        "../outside",
        "/absolute",
        "a//b",
        "a/../b",
        ".git/config",
        ".GIT/x",
        "sub/.BeNiToItEh/x",
        "ordinary",
    ] {
        let f = Fixture::new()?;
        fs::write(f.root.join("outside"), "outside")?;
        fs::create_dir(f.project.join(".git"))?;
        fs::write(f.project.join(".git/config"), "protected")?;
        fs::write(f.project.join("ordinary"), "old")?;
        let recorder = f.recorder()?;
        let point = capture(&recorder)?;
        let mut manifest = recorder.manifest(&point.id)?;
        manifest
            .entries
            .retain(|entry| entry.path.as_deref() == Some("ordinary"));
        let entry = manifest.entries.first_mut().ok_or("missing file")?;
        entry.path = Some(path.into());
        if path == "ordinary" {
            manifest.project = f.root.clone();
        }
        let bytes = serde_json::to_vec(&manifest)?;
        let (hash, _) = f.session.store_object(&bytes)?.ok_or("budget")?;
        let mut forged = point.clone();
        forged.id = "cp-99".into();
        forged.manifest = Some(hash);
        f.session.writer()?.append(&Record::Checkpoint(forged))?;
        let records = session::read_records(f.session.directory())?;
        let context = f.context()?;
        let reopened = CheckpointRecorder::new(
            f.session.clone(),
            &context.project,
            context.exclude,
            &records,
        )?;
        assert!(reopened.restore_files("cp-99", true).is_err(), "{path}");
        assert_eq!(fs::read(f.root.join("outside"))?, b"outside");
        assert_eq!(fs::read(f.project.join(".git/config"))?, b"protected");
        assert_eq!(fs::read(f.project.join("ordinary"))?, b"old");
    }
    Ok(())
}

#[test]
fn corrupt_objects_hardlinks_and_replaced_parents_never_modify_outside() -> TestResult {
    let f = Fixture::new()?;
    fs::create_dir(f.project.join("directory"))?;
    fs::write(f.project.join("directory/file"), "old")?;
    fs::write(f.project.join("hard"), "original")?;
    fs::create_dir(f.project.join("z-limit"))?;
    fs::write(f.project.join("z-limit/cutoff"), "keep")?;
    let recorder = f.recorder()?;
    recorder.set_limits(CheckpointLimits {
        max_entries: 4,
        ..Default::default()
    })?;
    let point = capture(&recorder)?;
    recorder.set_limits(CheckpointLimits::default())?;
    assert!(!point.incomplete.is_empty());
    assert!(!recorder.manifest(&point.id)?.complete);
    let manifest = recorder.manifest(&point.id)?;
    let file_hash = manifest
        .entries
        .iter()
        .find(|e| e.path.as_deref() == Some("directory/file"))
        .and_then(|e| e.sha256.clone())
        .ok_or("hash")?;
    fs::write(f.project.join("directory/file"), "new")?;
    fs::write(
        f.session.directory().join("objects").join(file_hash),
        "corrupt",
    )?;
    fs::remove_file(f.project.join("hard"))?;
    fs::write(f.root.join("outside"), "outside")?;
    fs::hard_link(f.root.join("outside"), f.project.join("hard"))?;
    let report = recorder.restore_files(&point.id, false)?;
    assert!(
        report
            .failures
            .iter()
            .any(|e| e.path == Path::new("directory/file"))
    );
    assert!(report.failures.iter().any(|e| e.path == Path::new("hard")));
    assert_eq!(fs::read(f.root.join("outside"))?, b"outside");
    assert_eq!(fs::read(f.project.join("directory/file"))?, b"new");
    assert!(
        !fs::read_dir(f.project.join("directory"))?.any(|e| e.is_ok_and(|e| e
            .file_name()
            .to_string_lossy()
            .starts_with(".benitoiteh-tmp")))
    );
    fs::remove_dir_all(f.project.join("directory"))?;
    fs::create_dir(f.root.join("outside-dir"))?;
    fs::write(f.root.join("outside-dir/file"), "outside-dir")?;
    symlink("../outside-dir", f.project.join("directory"))?;
    let report = recorder.restore_files(&point.id, false)?;
    assert!(fs::symlink_metadata(f.project.join("directory"))?.is_symlink());
    assert!(
        report
            .failures
            .iter()
            .any(|failure| failure.path == Path::new("directory/file"))
    );
    assert_eq!(fs::read(f.root.join("outside-dir/file"))?, b"outside-dir");
    let manifest_path = f
        .session
        .directory()
        .join("objects")
        .join(point.manifest.ok_or("manifest hash")?);
    fs::write(&manifest_path, "{}")?;
    assert!(recorder.restore_files(&point.id, false).is_err());
    assert_eq!(fs::read(f.root.join("outside"))?, b"outside");
    Ok(())
}

#[test]
fn tool_checkpoints_are_written_only_at_execution_and_cancelled_capture_stops_write() -> TestResult
{
    let f = Fixture::new()?;
    fs::write(f.project.join("file"), "old")?;
    let recorder = f.recorder()?;
    let mut context = f.context()?;
    context.checkpoints = Some(recorder.clone());
    let tools = ToolSet::new(Mode::Normal);
    // 未読・置換不成立の失敗では、時点を記録しない。
    for args in [
        json!({"path":"file","old_string":"missing","new_string":"x"}),
        json!({"path":"file","old_string":"old","new_string":"x"}),
    ] {
        assert!(tools.execute(&context, "edit", args).is_error);
    }
    assert!(recorder.checkpoints()?.is_empty());
    assert!(
        !tools
            .execute(&context, "read", json!({"path":"file"}))
            .is_error
    );
    assert!(
        tools
            .execute(
                &context,
                "edit",
                json!({"path":"file","old_string":"missing","new_string":"x"})
            )
            .is_error
    );
    assert!(recorder.checkpoints()?.is_empty());
    let output = tools.execute(
        &context,
        "edit",
        json!({"path":"file","old_string":"old","new_string":"new"}),
    );
    assert!(!output.is_error, "{}", output.text);
    let points = recorder.checkpoints()?;
    assert_eq!(points.len(), 1);
    assert_eq!(
        points.first().map(|p| p.checkpoint.reason),
        Some(CheckpointReason::Edit)
    );
    let hash = recorder
        .manifest("cp-1")?
        .entries
        .into_iter()
        .find(|e| e.path.as_deref() == Some("file"))
        .and_then(|e| e.sha256)
        .ok_or("hash")?;
    assert_eq!(f.session.read_object(&hash, 100)?, b"old");
    let call = tools.validate("write", json!({"path":"added","content":"added"}))?;
    let cancelled = AtomicBool::new(true);
    let output = tools.execute_validated(
        &context,
        &call,
        ApprovalStatus::Unapproved,
        Invocation {
            confirmed_effects: &std::collections::BTreeMap::new(),
            session_domains: &Default::default(),
            call_id: "cancelled",
            cancelled: &cancelled,
        },
    );
    assert!(output.is_error);
    assert!(!f.project.join("added").exists());
    let output = tools.execute(
        &context,
        "bash",
        json!({"command":"printf forbidden > forbidden"}),
    );
    assert!(output.approval.is_some());
    assert!(!f.project.join("forbidden").exists());
    assert!(
        !recorder
            .checkpoints()?
            .iter()
            .any(|p| p.checkpoint.reason == CheckpointReason::Bash)
    );
    Ok(())
}

// 作成時の関門: Agent の実際のループで、行が実行より前に残ること、会話を切った後の
// 再開と推論の対応、番の ID の単調性、読んだ版の無効化を守る。
#[test]
fn agent_rewind_replay_resume_and_read_versions_agree() -> TestResult {
    let f = Fixture::new()?;
    fs::write(f.project.join("file"), "old")?;
    let mut server = MockProvider::start(vec![
        response(&[("read", json!({"path":"file"}))]),
        response(&[]),
        response(&[
            ("write", json!({"path":"file","content":"new"})),
            ("bash", json!({"command":"printf shell > shell"})),
        ]),
        response(&[]),
        response(&[(
            "edit",
            json!({"path":"file","old_string":"old","new_string":"edit"}),
        )]),
        response(&[]),
        response(&[]),
        response(&[]),
    ])?;
    let mut agent = f.agent(&server)?;
    run(&mut agent)?;
    let first_conversation = agent.conversation().clone();
    run(&mut agent)?;
    let points = agent.checkpoints()?;
    let requests = points
        .iter()
        .filter(|p| p.checkpoint.reason == CheckpointReason::Request)
        .collect::<Vec<_>>();
    let second = requests
        .get(1)
        .ok_or("second checkpoint")?
        .checkpoint
        .clone();
    assert!(second.node.is_some());
    let tool_point = points
        .iter()
        .find(|p| p.checkpoint.reason == CheckpointReason::Write)
        .ok_or("tool checkpoint")?;
    assert!(
        rewind(
            &mut agent,
            &tool_point.checkpoint.id,
            RewindScope::Conversation,
            false
        )
        .is_err()
    );
    let preview = agent.rewind_preview(&second.id, RewindScope::FilesAndConversation)?;
    let report = rewind(
        &mut agent,
        &second.id,
        RewindScope::FilesAndConversation,
        false,
    )?;
    assert_eq!(preview.irreversible, report.irreversible);
    assert!(report.conversation_rewound);
    assert_eq!(report.irreversible.len(), 1);
    assert!(
        report
            .irreversible
            .first()
            .is_some_and(|op| op.summary.contains("bash"))
    );
    assert_eq!(agent.conversation(), &first_conversation);
    assert_eq!(session::replay(f.session.directory())?, first_conversation);
    assert_eq!(fs::read(f.project.join("file"))?, b"old");
    assert!(!f.project.join("shell").exists());
    let events = run(&mut agent)?;
    assert!(events.iter().any(
        |event| matches!(event, AgentEvent::ToolResult {name,is_error:true,..} if name=="edit")
    ));
    let records = session::read_records(f.session.directory())?;
    let ids = records
        .iter()
        .filter_map(|record| {
            if let Record::ToolStart { call } = record {
                Some(call.id.clone())
            } else {
                None
            }
        })
        .collect::<Vec<_>>();
    assert_eq!(
        ids.len(),
        ids.iter().collect::<std::collections::BTreeSet<_>>().len()
    );
    let state = session::ResumeState::read(f.session.directory())?;
    assert_eq!(state.conversation, *agent.conversation());
    assert_eq!(
        state.reasoning.len(),
        agent
            .conversation()
            .items
            .iter()
            .filter(|item| matches!(item, benitoiteh::provider::Item::AssistantTurn(_)))
            .count()
    );
    drop(agent);
    let mut resumed = Agent::resume(
        f.config(&server),
        String::new(),
        f.context()?,
        f.environment()?,
        f.session.clone(),
        state,
    )?;
    let before = resumed.checkpoints()?.len();
    run(&mut resumed)?;
    assert!(resumed.checkpoints()?.len() > before);
    assert_eq!(
        session::replay(f.session.directory())?,
        *resumed.conversation()
    );
    // 最初の依頼は今の道の上に残っているので会話も戻せる。
    assert!(
        resumed
            .checkpoints()?
            .first()
            .is_some_and(|info| info.conversation_available)
    );
    // 会話の始めへも移れる。
    let zero = Fixture::new()?;
    let mut agent = zero.agent(&server)?;
    run(&mut agent)?;
    rewind(&mut agent, "cp-1", RewindScope::Conversation, false)?;
    assert!(agent.conversation().items.is_empty());
    // これ以後に本物の API を呼ばない。
    let (sender, _receiver) = mpsc::sync_channel(100);
    agent.run("cancelled".into(), Arc::new(AtomicBool::new(true)), sender)?;
    assert!(agent.conversation().items.first().is_some_and(|item| {
        match item {
            benitoiteh::provider::Item::UserMessage(message) => message
                .parts
                .iter()
                .any(|part| part.kind == benitoiteh::provider::UserPartKind::Environment),
            benitoiteh::provider::Item::AssistantTurn(_)
            | benitoiteh::provider::Item::ToolResult(_) => false,
        }
    }));
    server.shutdown()?;
    Ok(())
}

#[test]
fn hardlink_events_only_report_new_paths_and_compaction_preserves_conversation_rewind() -> TestResult
{
    let f = Fixture::new()?;
    fs::write(f.project.join("a"), "a")?;
    fs::hard_link(f.project.join("a"), f.root.join("outside"))?;
    let mut server = MockProvider::start(vec![
        response(&[]),
        response(&[]),
        response(&[]),
        response(&[]),
    ])?;
    let mut agent = f.agent(&server)?;
    let first = run(&mut agent)?;
    let hard = |events: &[AgentEvent]| {
        events
            .iter()
            .filter_map(|event| {
                if let AgentEvent::HardLinks { paths } = event {
                    Some(paths.len())
                } else {
                    None
                }
            })
            .sum::<usize>()
    };
    assert_eq!(hard(&first), 1);
    assert_eq!(hard(&run(&mut agent)?), 0);
    fs::hard_link(f.project.join("a"), f.project.join("b"))?;
    assert_eq!(hard(&run(&mut agent)?), 1);
    let (sender, _receiver) = mpsc::sync_channel(100);
    let compact = agent.compact(None, Arc::new(AtomicBool::new(false)), sender)?;
    assert_eq!(
        compact.outcome,
        benitoiteh::agent::CompactOutcome::Compacted
    );
    assert!(
        agent
            .checkpoints()?
            .iter()
            .all(|p| p.conversation_available)
    );
    rewind(&mut agent, "cp-1", RewindScope::Conversation, false)?;
    assert!(agent.conversation().items.is_empty());
    server.shutdown()?;
    Ok(())
}

#[cfg(target_os = "macos")]
#[test]
fn case_only_rename_is_deleted_before_restore() -> TestResult {
    let f = Fixture::new()?;
    fs::write(f.project.join("Foo.txt"), "old")?;
    let recorder = f.recorder()?;
    let point = capture(&recorder)?;
    fs::rename(f.project.join("Foo.txt"), f.project.join("foo.txt"))?;
    fs::write(f.project.join("foo.txt"), "new")?;
    let report = recorder.restore_files(&point.id, false)?;
    assert!(report.failures.is_empty(), "{:?}", report.failures);
    assert_eq!(fs::read(f.project.join("Foo.txt"))?, b"old");
    assert!(fs::read_dir(&f.project)?.any(|entry| entry.is_ok_and(|e| e.file_name() == "Foo.txt")));
    Ok(())
}

#[cfg(target_os = "linux")]
#[test]
fn non_utf8_names_and_link_targets_round_trip() -> TestResult {
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt;
    let f = Fixture::new()?;
    let name = OsStr::from_bytes(b"name-\xff");
    let target = Path::new(OsStr::from_bytes(b"target-\xfe"));
    fs::write(f.project.join(name), "old")?;
    symlink(target, f.project.join("link"))?;
    let recorder = f.recorder()?;
    let point = capture(&recorder)?;
    assert!(
        recorder
            .manifest(&point.id)?
            .entries
            .iter()
            .any(|entry| entry.path_hex.is_some())
    );
    fs::remove_file(f.project.join(name))?;
    fs::remove_file(f.project.join("link"))?;
    let report = recorder.restore_files(&point.id, false)?;
    assert!(report.failures.is_empty(), "{:?}", report.failures);
    assert_eq!(fs::read(f.project.join(name))?, b"old");
    assert_eq!(fs::read_link(f.project.join("link"))?, target);
    Ok(())
}

// 設計者が求めた実測用。通常の検査では 100 個の時点と 10 万の実ファイルを作らない。
#[test]
#[ignore = "manual macOS checkpoint size and latency measurement"]
fn measure_checkpoint_latency_and_storage() -> TestResult {
    let count = std::env::var("BENITOITEH_REWIND_MEASURE_FILES")?.parse::<usize>()?;
    let f = Fixture::new()?;
    let deep = std::env::var("BENITOITEH_REWIND_MEASURE_LAYOUT").as_deref() == Ok("deep");
    let mut files = Vec::new();
    if deep {
        assert_eq!(count, 100_000);
        for branch in 0..100 {
            let mut directory = f.project.join(format!("b-{branch:03}"));
            for depth in 0..100 {
                if depth > 0 {
                    directory.push("d");
                }
                fs::create_dir(&directory)?;
                for file in 0..10 {
                    let path = directory.join(format!("file-{file:02}"));
                    fs::write(&path, "content\n")?;
                    files.push(path);
                }
            }
        }
    } else {
        for index in 0..count {
            let path = f.project.join(format!("file-{index:06}"));
            fs::write(&path, "content\n")?;
            files.push(path);
        }
    }
    fs::create_dir(f.project.join("target"))?;
    fs::write(f.project.join(".gitignore"), "/target/\n")?;
    for index in 0..10_000 {
        fs::write(
            f.project.join("target").join(format!("ignored-{index:06}")),
            "ignored\n",
        )?;
    }
    // ファイルの作成と T の秒を分け、変化のない二回目の読み直しを避ける条件を満たす。
    thread::sleep(Duration::from_millis(1100));
    let recorder = f.recorder()?;
    let mut timings = Vec::new();
    let mut saved = 0_u64;
    let mut incomplete = 0_u64;
    for index in 0_usize..100 {
        if index == 2 {
            fs::write(files.first().ok_or("measurement file")?, "changed\n")?;
        }
        let started = std::time::Instant::now();
        let point = capture(&recorder)?;
        let elapsed = started.elapsed();
        if index < 3 {
            let manifest = recorder.manifest(&point.id)?;
            assert!(!manifest.entries.iter().any(|entry| {
                entry
                    .path
                    .as_ref()
                    .is_some_and(|path| path.starts_with("target/"))
            }));
            let size = serde_json::to_vec(&manifest)?.len();
            let stats = recorder.stats()?;
            timings.push(json!({"index":index,"milliseconds":elapsed.as_secs_f64()*1000.0,"manifest_bytes":size,"stats":stats}));
        }
        if point.manifest.is_some() {
            saved = saved.saturating_add(1);
        }
        if !point.incomplete.is_empty() {
            incomplete = incomplete.saturating_add(1);
        }
        if index.is_multiple_of(10) {
            eprintln!("measurement {count}: checkpoint {index}");
        }
        // 通知が画面に渡された状態と同じにする。
        let _notices = recorder.take_notices()?;
    }
    let mut stored_bytes = 0_u64;
    let mut objects = 0_u64;
    for object in fs::read_dir(f.session.directory().join("objects"))? {
        stored_bytes = stored_bytes.saturating_add(object?.metadata()?.len());
        objects = objects.saturating_add(1);
    }
    println!(
        "MEASUREMENT {}",
        json!({"files":count,"directories":if deep {10000} else {0},"depth":if deep {100} else {1},"layout":if deep {"deep"} else {"flat"},"ignored_files":10000,"timings":timings,"checkpoints":100,"saved_manifests":saved,"incomplete":incomplete,"cumulative_object_bytes":stored_bytes,"object_count":objects})
    );
    Ok(())
}

#[test]
fn cancellation_during_capture_prevents_writes_and_bash_and_does_not_record_execution() -> TestResult
{
    for name in ["write", "bash"] {
        let f = Fixture::new()?;
        for index in 0..10_000 {
            fs::write(f.project.join(format!("f-{index}")), "data")?;
        }
        let recorder = f.recorder()?;
        let mut context = f.context()?;
        context.checkpoints = Some(recorder.clone());
        let cancelled = Arc::new(AtomicBool::new(false));
        let worker_cancelled = Arc::clone(&cancelled);
        let worker = thread::spawn(move || {
            let tools = ToolSet::new(Mode::Normal);
            let args = if name == "write" {
                json!({"path":"must-not-exist", "content":"new"})
            } else {
                json!({"command":"printf forbidden > must-not-exist"})
            };
            let call = tools.validate(name, args).map_err(|e| e.to_string())?;
            Ok::<_, String>(
                tools
                    .execute_validated(
                        &context,
                        &call,
                        ApprovalStatus::Approved,
                        Invocation {
                            confirmed_effects: &std::collections::BTreeMap::new(),
                            session_domains: &Default::default(),
                            call_id: "cancel-mid-capture",
                            cancelled: &worker_cancelled,
                        },
                    )
                    .is_error,
            )
        });
        let deadline = std::time::Instant::now();
        while fs::read_dir(f.session.directory().join("objects"))?
            .next()
            .is_none()
            && deadline.elapsed() < Duration::from_secs(2)
        {
            thread::sleep(Duration::from_millis(1));
        }
        cancelled.store(true, std::sync::atomic::Ordering::Release);
        assert!(worker.join().map_err(|_| "worker")??);
        assert!(!f.project.join("must-not-exist").exists());
        assert!(recorder.checkpoints()?.is_empty());
        assert!(
            !session::read_records(f.session.directory())?
                .iter()
                .any(|record| matches!(record, Record::Checkpoint(_)))
        );
    }
    Ok(())
}

#[test]
fn explicit_incomplete_permission_allows_restore_without_a_saved_before_manifest() -> TestResult {
    let f = Fixture::new()?;
    fs::write(f.project.join("file"), "old")?;
    let recorder = f.recorder()?;
    let point = capture(&recorder)?;
    fs::write(f.project.join("file"), "new")?;
    f.session.configure_budget(&session::StorageUsage {
        current_outputs: 1024 * 1024 * 1024,
        ..Default::default()
    });
    assert!(recorder.restore_files(&point.id, false).is_err());
    assert_eq!(fs::read(f.project.join("file"))?, b"new");
    let report = recorder.restore_files(&point.id, true)?;
    assert!(report.failures.is_empty(), "{:?}", report.failures);
    assert_eq!(fs::read(f.project.join("file"))?, b"old");
    Ok(())
}

#[test]
fn object_links_are_rejected_even_when_the_target_has_the_right_hash() -> TestResult {
    for hard in [false, true] {
        let f = Fixture::new()?;
        fs::write(f.project.join("file"), "old")?;
        let recorder = f.recorder()?;
        let point = capture(&recorder)?;
        let hash = recorder
            .manifest(&point.id)?
            .entries
            .first()
            .and_then(|entry| entry.sha256.as_ref())
            .cloned()
            .ok_or("hash")?;
        let object = f.session.directory().join("objects").join(hash);
        let outside = f.root.join("outside");
        fs::write(&outside, "old")?;
        fs::set_permissions(&outside, fs::Permissions::from_mode(0o600))?;
        fs::remove_file(&object)?;
        if hard {
            fs::hard_link(&outside, &object)?;
        } else {
            symlink(&outside, &object)?;
        }
        fs::write(f.project.join("file"), "new")?;
        let report = recorder.restore_files(&point.id, false)?;
        assert!(
            report
                .failures
                .iter()
                .any(|failure| failure.path == Path::new("file"))
        );
        assert_eq!(fs::read(f.project.join("file"))?, b"new");
        assert_eq!(fs::read(&outside)?, b"old");
    }
    Ok(())
}

#[test]
fn irreversible_operations_include_excluded_and_outside_writes_but_not_denied_bash() -> TestResult {
    use benitoiteh::approval::{ApprovalAnswer, ApprovalPolicy, ApprovalRecord, Approver};
    use benitoiteh::config::{Approval, Sandbox};
    use benitoiteh::session::ApprovalOutcome;
    struct Deny;
    impl Approver for Deny {
        fn approve(&mut self, _: &ApprovalRecord) -> ApprovalAnswer {
            ApprovalAnswer {
                outcome: ApprovalOutcome::Denied,
                message: None,
            }
        }
    }
    let f = Fixture::new()?;
    fs::write(f.project.join(".gitignore"), "ignored\n")?;
    let outside = f.root.join("outside");
    let mut server = MockProvider::start(vec![
        response(&[
            ("bash", json!({"command":"printf denied > denied"})),
            ("write", json!({"path":"ignored","content":"ignored"})),
            ("write", json!({"path":outside,"content":"outside"})),
        ]),
        response(&[]),
    ])?;
    let mut agent = f.agent(&server)?.with_approvals(
        Box::new(Deny),
        ApprovalPolicy {
            approval: Approval::Untrusted,
            sandbox: Sandbox::WorkspaceWrite,
            sandboxed: false,
        },
    );
    run(&mut agent)?;
    // 範囲外 write の確認も拒むため、この時点では excluded write だけが実行される。
    assert!(!f.project.join("denied").exists());
    let report = rewind(&mut agent, "cp-1", RewindScope::Files, false)?;
    assert!(
        report
            .irreversible
            .iter()
            .any(|op| op.summary.contains("ignored"))
    );
    assert!(
        !report
            .irreversible
            .iter()
            .any(|op| op.summary.starts_with("bash"))
    );
    server.shutdown()?;
    // 範囲外を実際に許す別のセッションでは、時点を増やさずに報告する。
    let f = Fixture::new()?;
    let outside = f.root.join("outside-dir");
    fs::create_dir(&outside)?;
    fs::create_dir(f.project.join("inside-dir"))?;
    symlink(&outside, f.project.join("link"))?;
    let mut server = MockProvider::start(vec![
        response(&[("write", json!({"path":"link/outside","content":"outside"}))]),
        response(&[]),
    ])?;
    let mut agent = f.agent(&server)?;
    run(&mut agent)?;
    assert_eq!(agent.checkpoints()?.len(), 1);
    assert!(
        session::read_records(f.session.directory())?
            .iter()
            .any(|record| matches!(record,
        Record::WriteTarget {real_path, ..} if real_path == &outside.join("outside")))
    );
    fs::remove_file(f.project.join("link"))?;
    symlink("inside-dir", f.project.join("link"))?;
    let report = rewind(&mut agent, "cp-1", RewindScope::Files, false)?;
    assert_eq!(report.irreversible.len(), 1);
    assert_eq!(fs::read(outside.join("outside"))?, b"outside");
    server.shutdown()?;
    Ok(())
}

// 作成時の関門: 再開が補った未完の結果を、巻き戻した後の会話に追記する退行を防ぐ。
// API を呼ぶ前に rewind する順は、通常の run→rewind→resume のテストでは届かない。
#[test]
fn rewinding_immediately_after_resume_does_not_reappend_discarded_results() -> TestResult {
    use benitoiteh::provider::{AssistantPart, Item, ToolCall};
    let f = Fixture::new()?;
    let mut server = MockProvider::start(vec![response(&[]), response(&[])])?;
    let mut agent = f.agent(&server)?;
    run(&mut agent)?;
    let mut unfinished = agent
        .conversation()
        .items
        .iter()
        .find_map(|item| {
            if let Item::AssistantTurn(turn) = item {
                Some(turn.clone())
            } else {
                None
            }
        })
        .ok_or("response")?;
    let call = ToolCall {
        id: "turn-10-call-0".into(),
        provider_id: Some("call".into()),
        provider_item_id: None,
        name: "write".into(),
        arguments: json!({"path":"file","content":"unknown"}),
        raw_arguments: String::new(),
    };
    unfinished.parts = vec![AssistantPart::ToolCall(call.clone())];
    f.session.writer()?.append(&Record::Assistant {
        node: 100,
        turn: 10,
        response: unfinished,
    })?;
    f.session.writer()?.append(&Record::ToolStart { call })?;
    f.session.writer()?.append(&Record::Branch {
        to: Some(2),
        from: Some(100),
        checkpoint: None,
    })?;
    drop(agent);
    let state = session::ResumeState::read(f.session.directory())?;
    assert!(state.missing_results.is_empty());
    let mut resumed = Agent::resume(
        f.config(&server),
        String::new(),
        f.context()?,
        f.environment()?,
        f.session.clone(),
        state,
    )?;
    let (sender, _receiver) = mpsc::sync_channel(1024);
    resumed.switch_branch(
        benitoiteh::agent::BranchTarget::Tip(100),
        resumed
            .branch_preview(
                benitoiteh::agent::BranchTarget::Tip(100),
                RewindScope::Conversation,
            )?
            .selected
            .checkpoint
            .as_deref(),
        RewindScope::Conversation,
        false,
        sender,
    )?;
    assert_eq!(
        session::ResumeState::read(f.session.directory())?
            .missing_results
            .len(),
        1
    );
    let rewound = rewind(
        &mut resumed,
        "cp-1",
        RewindScope::FilesAndConversation,
        false,
    )?;
    let before_id = rewound.before_rewind.ok_or("before rewind")?;
    assert_eq!(
        resumed
            .checkpoints()?
            .into_iter()
            .find(|info| info.checkpoint.id == before_id)
            .map(|info| info.checkpoint.turn),
        Some(10)
    );
    run(&mut resumed)?;
    assert_eq!(
        session::replay(f.session.directory())?,
        *resumed.conversation()
    );
    let records = session::read_records(f.session.directory())?;
    assert_eq!(
        records
            .iter()
            .filter(|record| matches!(record,
        Record::ToolResult { node: 101, result, .. } if result.id == "turn-10-call-0"))
            .count(),
        1
    );
    assert!(records.iter().any(|record| matches!(
        record,
        Record::Branch {
            from: Some(101),
            to: None,
            ..
        }
    )));
    server.shutdown()?;
    Ok(())
}

// 作成時の関門: Manifest が生の名前を保持していても、HardLinks を JSON にすると
// セッション全体の出力が失敗する退行を防ぐ。JSON の出力の型の境界の独立した契約である。
#[test]
fn hardlink_event_json_preserves_non_utf8_paths() -> TestResult {
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt;
    let event = AgentEvent::HardLinks {
        paths: vec![
            "ordinary".into(),
            std::path::PathBuf::from(OsString::from_vec(b"name-\xff".to_vec())),
        ],
    };
    let encoded = serde_json::to_value(&event)?;
    assert_eq!(
        encoded,
        json!({"type":"hard_links","paths":["ordinary",{"path_hex":"6e616d652dff"}]})
    );
    let decoded: AgentEvent = serde_json::from_value(encoded)?;
    assert_eq!(decoded, event);
    Ok(())
}

// 作成時の関門: モデルを変えた再開の最初の依頼より前に rewind しても、残った
// 別のモデルの推論を要約する契約を飛ばさない。通常のモデルのままの再開では届かない。
#[test]
fn rewind_before_first_resumed_request_keeps_required_reasoning_compaction() -> TestResult {
    use benitoiteh::provider::ReasoningReplay;
    let f = Fixture::new()?;
    let mut server = MockProvider::start(vec![
        response(&[]),
        response(&[]),
        response(&[]),
        response(&[]),
    ])?;
    let mut config = f.config(&server);
    config.capabilities.reasoning_replay = ReasoningReplay::AllTurns;
    let mut agent = Agent::new(
        config.clone(),
        String::new(),
        f.context()?,
        f.environment()?,
        f.session.clone(),
    )?;
    run(&mut agent)?;
    run(&mut agent)?;
    let state = session::ResumeState::read(f.session.directory())?;
    drop(agent);
    config.destination.model = "changed-model".into();
    let mut resumed = Agent::resume(
        config,
        String::new(),
        f.context()?,
        f.environment()?,
        f.session.clone(),
        state,
    )?;
    rewind(&mut resumed, "cp-2", RewindScope::Conversation, false)?;
    let events = run(&mut resumed)?;
    assert!(events.iter().any(|event| matches!(
        event,
        AgentEvent::Compaction {
            reason: session::CompactionReason::Resume,
            ..
        }
    )));
    assert_eq!(
        session::replay(f.session.directory())?,
        *resumed.conversation()
    );
    server.shutdown()?;
    Ok(())
}

// 作成時の関門: 復元の口で、不要な置換による権限・inode の変更と、ハードリンクの誤警告を捕まえる。
#[test]
fn unchanged_files_keep_permissions_and_hardlinks_and_changed_files_keep_access_bits() -> TestResult
{
    let f = Fixture::new()?;
    for name in ["private", "hard", "changed"] {
        fs::write(f.project.join(name), "old")?;
        fs::set_permissions(f.project.join(name), fs::Permissions::from_mode(0o600))?;
    }
    fs::hard_link(f.project.join("hard"), f.root.join("outside-hard"))?;
    fs::set_permissions(f.project.join("changed"), fs::Permissions::from_mode(0o710))?;
    let original_inode = fs::metadata(f.project.join("private"))?.ino();
    let recorder = f.recorder()?;
    let point = capture(&recorder)?;
    fs::write(f.project.join("changed"), "new")?;
    fs::set_permissions(f.project.join("changed"), fs::Permissions::from_mode(0o640))?;
    let report = recorder.restore_files(&point.id, false)?;
    assert!(report.failures.is_empty(), "{:?}", report.failures);
    assert_eq!(
        fs::metadata(f.project.join("private"))?
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
    assert_eq!(
        fs::metadata(f.project.join("private"))?.ino(),
        original_inode
    );
    assert_eq!(
        fs::metadata(f.project.join("changed"))?
            .permissions()
            .mode()
            & 0o777,
        0o750
    );
    assert_eq!(fs::read(f.project.join("changed"))?, b"old");
    assert_eq!(
        report.changed_paths,
        vec![std::path::PathBuf::from("changed")]
    );
    assert_eq!(fs::read(f.root.join("outside-hard"))?, b"old");
    Ok(())
}

// 作成時の関門: 内容が不変の時点の保存量と、除外領域の活動が一覧を変えない契約を確かめる。
#[test]
fn unchanged_manifests_are_byte_identical_and_excluded_entries_have_no_attributes() -> TestResult {
    let f = Fixture::new()?;
    fs::create_dir(f.project.join(".git"))?;
    fs::create_dir(f.project.join("a"))?;
    fs::write(f.project.join("a/file"), "content")?;
    fs::write(f.project.join("a-"), "content")?;
    let recorder = f.recorder()?;
    let first = capture(&recorder)?;
    let objects = fs::read_dir(f.session.directory().join("objects"))?.count();
    fs::write(f.project.join(".git/index"), "changed")?;
    let second = capture(&recorder)?;
    assert_eq!(first.manifest, second.manifest);
    assert_eq!(
        objects,
        fs::read_dir(f.session.directory().join("objects"))?.count()
    );
    let manifest = serde_json::to_value(recorder.manifest(&first.id)?)?;
    assert!(manifest.get("started_at").is_none());
    let entries = manifest
        .get("entries")
        .and_then(serde_json::Value::as_array)
        .ok_or("entries")?;
    let excluded = entries
        .iter()
        .find(|e| e.get("path") == Some(&json!(".git")))
        .ok_or("excluded")?;
    assert_eq!(
        excluded,
        &json!({"path":".git", "kind":"directory", "excluded":"protected"})
    );
    assert_eq!(
        entries
            .iter()
            .filter_map(|e| e.get("path").and_then(serde_json::Value::as_str))
            .collect::<Vec<_>>(),
        vec![".git", "a", "a-", "a/file"]
    );
    assert!(!serde_json::to_string(&manifest)?.contains(":null"));
    Ok(())
}

// 作成時の関門: ウェブの通信をファイルと一緒に戻せると誤認する退行を、巻き戻しの境界で捕まえる。
#[test]
fn web_fetch_calls_are_reported_as_irreversible_after_the_selected_checkpoint() -> TestResult {
    let f = Fixture::new()?;
    let mut server = MockProvider::start(vec![
        response(&[("web_fetch", json!({"url":"https://mock.invalid/path"}))]),
        response(&[]),
    ])?;
    let mut agent = f.agent(&server)?;
    run(&mut agent)?;
    let points = agent.checkpoints()?;
    let first = points.first().ok_or("missing checkpoint")?;
    let preview = agent.rewind_preview(&first.checkpoint.id, RewindScope::Files)?;
    let report = rewind(&mut agent, &first.checkpoint.id, RewindScope::Files, false)?;
    assert_eq!(preview.irreversible, report.irreversible);
    assert_eq!(report.irreversible.len(), 1);
    assert!(
        report
            .irreversible
            .first()
            .is_some_and(|operation| operation.summary.contains("web_fetch")
                && operation.summary.contains("https://mock.invalid/path"))
    );
    server.shutdown()?;
    Ok(())
}

// 作成時の関門: 圧縮をまたぐ枝の移動で、捨てた依頼を送る退行、同じ親の時点を
// 取り違える退行、ファイルと読んだ版を取り違える退行をエージェントの境界で捕まえる。
#[test]
fn switching_branches_restores_requests_files_and_resumes_the_selected_path() -> TestResult {
    use benitoiteh::agent::BranchTarget;
    let f = Fixture::new()?;
    fs::write(f.project.join("file"), "old")?;
    let mut server = MockProvider::start(vec![
        response(&[("read", json!({"path":"file"}))]),
        response(&[]),
        response(&[("write", json!({"path":"file", "content":"left"}))]),
        response(&[]),
        response(&[]), // 圧縮の要約。
        response(&[(
            "edit",
            json!({"path":"file", "old_string":"old", "new_string":"unexpected"}),
        )]),
        response(&[]),
        response(&[]),
    ])?;
    let mut agent = f.agent(&server)?;
    let run_request = |agent: &mut Agent, request: &str| -> TestResult {
        let (sender, _receiver) = mpsc::sync_channel(1024);
        let report = agent.run(request.into(), Arc::new(AtomicBool::new(false)), sender)?;
        assert_eq!(report.outcome, benitoiteh::agent::Outcome::Completed);
        Ok(())
    };
    run_request(&mut agent, "root request")?;
    let root_conversation = agent.conversation().clone();
    run_request(&mut agent, "left request\nsecond line")?;
    let records = session::read_records(f.session.directory())?;
    let left_request = records
        .iter()
        .find_map(|record| {
            (session::request_text(record).as_deref() == Some("left request\nsecond line"))
                .then(|| record.node())
                .flatten()
        })
        .ok_or("left request node")?;
    let left_checkpoint = agent
        .branch_targets()?
        .into_iter()
        .find(|info| info.target == BranchTarget::BeforeRequest(left_request))
        .and_then(|info| info.checkpoint)
        .ok_or("left checkpoint")?;
    let (sender, _receiver) = mpsc::sync_channel(1024);
    assert_eq!(
        agent
            .compact(None, Arc::new(AtomicBool::new(false)), sender)?
            .outcome,
        benitoiteh::agent::CompactOutcome::Compacted
    );
    let compacted = agent.conversation().clone();
    let left_tip = session::ResumeState::read(f.session.directory())?
        .tree
        .tip
        .ok_or("left tip")?;
    let before = agent.checkpoints()?.len();
    let (sender, _receiver) = mpsc::sync_channel(1024);
    let report = agent.switch_branch(
        BranchTarget::BeforeRequest(left_request),
        agent
            .branch_preview(
                BranchTarget::BeforeRequest(left_request),
                RewindScope::Conversation,
            )?
            .selected
            .checkpoint
            .as_deref(),
        RewindScope::FilesAndConversation,
        false,
        sender,
    )?;
    assert_eq!(agent.checkpoints()?.len(), before.saturating_add(1));
    assert_eq!(report.request.as_deref(), Some("left request\nsecond line"));
    assert!(report.before_rewind.is_some());
    assert_eq!(agent.conversation(), &root_conversation);
    assert_eq!(fs::read(f.project.join("file"))?, b"old");
    run_request(&mut agent, "right request")?;
    let right_conversation = agent.conversation().clone();
    assert!(right_conversation.items.iter().any(|item| matches!(item,
        benitoiteh::provider::Item::ToolResult(result) if result.is_error && result.text.contains("read"))));
    let requests = server.requests()?;
    let right_body = requests
        .get(5)
        .ok_or("right HTTP request")?
        .body
        .to_string();
    assert!(right_body.contains("root request"));
    assert!(right_body.contains("right request"));
    assert!(!right_body.contains("left request"));
    assert!(!right_body.contains("compacted_summary"));
    let points = agent.checkpoints()?;
    assert!(
        !points
            .iter()
            .find(|point| point.checkpoint.id == left_checkpoint)
            .ok_or("left checkpoint")?
            .conversation_available
    );
    assert!(
        points
            .iter()
            .rev()
            .find(|point| point.checkpoint.reason == CheckpointReason::Request)
            .ok_or("right checkpoint")?
            .conversation_available
    );
    fs::write(f.project.join("file"), "right")?;
    let right_tip = session::ResumeState::read(f.session.directory())?
        .tree
        .tip
        .ok_or("right tip")?;
    let (sender, _receiver) = mpsc::sync_channel(1024);
    agent.switch_branch(
        BranchTarget::Tip(left_tip),
        agent
            .branch_preview(BranchTarget::Tip(left_tip), RewindScope::Conversation)?
            .selected
            .checkpoint
            .as_deref(),
        RewindScope::FilesAndConversation,
        false,
        sender,
    )?;
    assert_eq!(agent.conversation(), &compacted);
    assert_eq!(fs::read(f.project.join("file"))?, b"left");
    let (sender, _receiver) = mpsc::sync_channel(1024);
    agent.switch_branch(
        BranchTarget::Tip(right_tip),
        agent
            .branch_preview(BranchTarget::Tip(right_tip), RewindScope::Conversation)?
            .selected
            .checkpoint
            .as_deref(),
        RewindScope::FilesAndConversation,
        false,
        sender,
    )?;
    assert_eq!(agent.conversation(), &right_conversation);
    assert_eq!(fs::read(f.project.join("file"))?, b"right");
    let state = session::ResumeState::read(f.session.directory())?;
    let next = state.tree.next_node;
    drop(agent);
    let mut agent = Agent::resume(
        f.config(&server),
        String::new(),
        f.context()?,
        f.environment()?,
        f.session.clone(),
        state,
    )?;
    run_request(&mut agent, "after resume")?;
    let body = server
        .requests()?
        .last()
        .ok_or("resumed request")?
        .body
        .to_string();
    assert!(body.contains("right request"));
    assert!(!body.contains("left request"));
    let records = session::read_records(f.session.directory())?;
    let nodes = records.iter().filter_map(Record::node).collect::<Vec<_>>();
    assert!(
        nodes
            .windows(2)
            .all(|pair| matches!(pair, [first, second] if first < second))
    );
    assert!(records.iter().any(|record| record.node() == Some(next)
        && session::request_text(record).as_deref() == Some("after resume")));
    assert_eq!(
        session::replay(f.session.directory())?,
        *agent.conversation()
    );
    server.shutdown()?;
    Ok(())
}

// 作成時の関門: 会話の始めへ移るときだけ起きる、環境の二重送信を HTTP 本文で捕まえる。
#[test]
fn rewinding_to_start_sends_environment_once() -> TestResult {
    let f = Fixture::new()?;
    let mut server = MockProvider::start(vec![response(&[]), response(&[])])?;
    let mut agent = f.agent(&server)?;
    run(&mut agent)?;
    rewind(&mut agent, "cp-1", RewindScope::Conversation, false)?;
    run(&mut agent)?;
    let requests = server.requests()?;
    let body = requests.last().ok_or("request")?.body.to_string();
    assert_eq!(body.matches("<environment>").count(), 1, "{body}");
    server.shutdown()?;
    Ok(())
}

// 作成時の関門: 補う結果を持つ今の先端への移動が、兄弟の結果を増やす退行を捕まえる。
#[test]
fn switching_to_current_tip_keeps_pending_results_until_run() -> TestResult {
    use benitoiteh::agent::BranchTarget;
    let f = Fixture::new()?;
    fs::write(f.project.join("file"), "old")?;
    let mut server = MockProvider::start(vec![
        response(&[("read", json!({"path":"file"}))]),
        response(&[]),
        response(&[]),
    ])?;
    let mut agent = f.agent(&server)?;
    run(&mut agent)?;
    let mut records = session::read_records(f.session.directory())?;
    let cut = records
        .iter()
        .position(|record| matches!(record, Record::ToolResult { .. }))
        .ok_or("result")?;
    records.truncate(cut);
    let log = f.session.directory().join("session.jsonl");
    fs::write(
        &log,
        records
            .iter()
            .map(serde_json::to_string)
            .collect::<Result<Vec<_>, _>>()?
            .join("\n")
            + "\n",
    )?;
    drop(agent);
    let state = session::ResumeState::read(f.session.directory())?;
    assert_eq!(state.missing_results.len(), 1);
    let tip = state.tree.tip.ok_or("tip")?;
    let call_id = state
        .missing_results
        .first()
        .and_then(|record| {
            if let Record::ToolResult { result, .. } = record {
                Some(result.id.clone())
            } else {
                None
            }
        })
        .ok_or("pending result")?;
    let mut agent = Agent::resume(
        f.config(&server),
        String::new(),
        f.context()?,
        f.environment()?,
        f.session.clone(),
        state,
    )?;
    let before = fs::read(&log)?;
    for scope in [RewindScope::Conversation, RewindScope::FilesAndConversation] {
        let (sender, _receiver) = mpsc::sync_channel(1024);
        assert!(
            agent
                .switch_branch(
                    BranchTarget::Tip(tip),
                    Some("changed"),
                    scope,
                    false,
                    sender
                )
                .is_err()
        );
        assert_eq!(fs::read(&log)?, before);
        let (sender, _receiver) = mpsc::sync_channel(1024);
        let report = agent.switch_branch(
            BranchTarget::Tip(tip),
            agent
                .branch_preview(BranchTarget::Tip(tip), RewindScope::Conversation)?
                .selected
                .checkpoint
                .as_deref(),
            scope,
            false,
            sender,
        )?;
        assert!(!report.conversation_rewound);
        assert!(report.before_rewind.is_none());
        assert_eq!(fs::read(&log)?, before);
    }
    run(&mut agent)?;
    let records = session::read_records(f.session.directory())?;
    assert_eq!(
        records
            .iter()
            .filter(
                |record| matches!(record, Record::ToolResult { result, .. } if result.id == call_id)
            )
            .count(),
        1
    );
    assert_eq!(
        session::replay(f.session.directory())?,
        *agent.conversation()
    );
    server.shutdown()?;
    Ok(())
}

// 作成時の関門: 中断した依頼の時点を、次の依頼に取り違えて結ぶ退行を捕まえる。
#[test]
fn only_latest_request_checkpoint_is_associated_with_the_next_request() -> TestResult {
    let f = Fixture::new()?;
    let recorder = f.recorder()?;
    let first = capture(&recorder)?;
    let second = capture(&recorder)?;
    f.session.writer()?.append(&Record::User {
        node: 1,
        kind: session::UserKind::Request,
        message: benitoiteh::provider::UserMessage {
            parts: vec![benitoiteh::provider::UserPart {
                kind: benitoiteh::provider::UserPartKind::Request,
                text: "accepted request".into(),
            }],
        },
    })?;
    let points = recorder.checkpoints()?;
    assert_eq!(
        points
            .iter()
            .map(|point| (point.checkpoint.id.as_str(), point.conversation_available))
            .collect::<Vec<_>>(),
        vec![(first.id.as_str(), false), (second.id.as_str(), true)]
    );
    Ok(())
}

// 作成時の関門: 索引を共有する一覧が、道の判定と最後に離れた時点を取り違えない。
// 依頼と移動を同期して並べ、件数や所要時間には依存しない。
#[test]
fn branch_indexes_preserve_request_checkpoints_and_latest_tip_backups() -> TestResult {
    use benitoiteh::agent::BranchTarget::{BeforeRequest, Tip};
    let f = Fixture::new()?;
    let mut server = MockProvider::start(vec![])?;
    let mut agent = f.agent(&server)?;
    let request = |agent: &mut Agent| -> TestResult {
        let (sender, _receiver) = mpsc::sync_channel(1024);
        agent.run("request".into(), Arc::new(AtomicBool::new(true)), sender)?;
        Ok(())
    };
    let switch = |agent: &mut Agent, target| -> TestResult {
        let (sender, _receiver) = mpsc::sync_channel(1024);
        agent.switch_branch(
            target,
            agent
                .branch_preview(target, RewindScope::Conversation)?
                .selected
                .checkpoint
                .as_deref(),
            RewindScope::Conversation,
            false,
            sender,
        )?;
        Ok(())
    };
    request(&mut agent)?;
    request(&mut agent)?;
    switch(&mut agent, BeforeRequest(2))?;
    let rows = agent.branch_tree()?;
    let current = rows
        .iter()
        .find(|row| row.row.is_current)
        .ok_or("current request")?;
    assert_eq!(current.row.node, Some(1));
    assert_eq!(
        current.branch.as_ref().map(|info| info.target),
        Some(BeforeRequest(1))
    );
    request(&mut agent)?;
    switch(&mut agent, Tip(2))?;
    let rows = agent.branch_tree()?;
    assert_eq!(
        rows.iter()
            .map(|row| (
                row.row.node,
                row.row.depth,
                row.row.is_tip,
                row.row.is_current,
                row.branch.as_ref().map(|info| info.target),
            ))
            .collect::<Vec<_>>(),
        vec![
            (None, 0, false, false, None),
            (Some(1), 0, false, false, Some(BeforeRequest(1))),
            (Some(2), 1, true, true, None),
            (Some(3), 1, true, false, Some(BeforeRequest(3))),
        ]
    );
    // 今の道の巻き戻しには使えない時点も、枝の移動には使える。
    let records_before = session::read_records(f.session.directory())?;
    for target in [BeforeRequest(3), Tip(3)] {
        let preview = agent.branch_preview(target, RewindScope::FilesAndConversation)?;
        assert_eq!(preview.selected.target, target);
        assert!(
            preview
                .checkpoint
                .as_ref()
                .is_some_and(|point| !point.conversation_available)
        );
        assert!(agent.branch_preview(target, RewindScope::Files).is_err());
    }
    assert_eq!(
        session::read_records(f.session.directory())?,
        records_before
    );
    let targets = agent.branch_targets()?;
    assert_eq!(
        targets
            .iter()
            .map(|info| (info.target, info.checkpoint.as_deref()))
            .collect::<Vec<_>>(),
        vec![
            (BeforeRequest(1), Some("cp-1")),
            (BeforeRequest(2), Some("cp-2")),
            (BeforeRequest(3), Some("cp-4")),
            (Tip(2), Some("cp-3")),
            (Tip(3), Some("cp-5"))
        ]
    );
    assert_eq!(
        agent
            .checkpoints()?
            .iter()
            .map(|point| (point.checkpoint.id.clone(), point.conversation_available))
            .collect::<Vec<_>>(),
        vec![
            ("cp-1".into(), true),
            ("cp-2".into(), true),
            ("cp-3".into(), false),
            ("cp-4".into(), false),
            ("cp-5".into(), false)
        ]
    );
    switch(&mut agent, Tip(3))?;
    assert_eq!(
        agent
            .branch_targets()?
            .iter()
            .find(|info| info.target == Tip(2))
            .and_then(|info| info.checkpoint.as_deref()),
        Some("cp-6")
    );
    assert!(server.requests()?.is_empty());
    server.shutdown()?;
    Ok(())
}

// 関門: 時点を持たない古い枝も、会話だけなら確認・移動できる。
// /rewind は時点を必須にするので、この契約を代わりには守らない。
#[test]
fn branches_without_checkpoints_can_preview_and_move_conversation_only() -> TestResult {
    use benitoiteh::agent::BranchTarget::{BeforeRequest, Tip};
    use benitoiteh::provider::{UserMessage, UserPart, UserPartKind};
    let f = Fixture::new()?;
    let mut server = MockProvider::start(vec![])?;
    drop(f.recorder()?);
    let user = |node, text: &str| Record::User {
        node,
        kind: session::UserKind::Request,
        message: UserMessage {
            parts: vec![UserPart {
                kind: UserPartKind::Request,
                text: text.into(),
            }],
        },
    };
    let mut writer = f.session.writer()?;
    writer.append(&user(1, "old request"))?;
    writer.append(&Record::Branch {
        to: None,
        from: Some(1),
        checkpoint: None,
    })?;
    writer.append(&user(2, "current request"))?;
    let state = session::ResumeState::read(f.session.directory())?;
    let mut agent = Agent::resume(
        f.config(&server),
        String::new(),
        f.context()?,
        f.environment()?,
        f.session.clone(),
        state,
    )?;
    let rows = agent.branch_tree()?;
    assert!(
        rows.first()
            .is_some_and(|row| row.row.node.is_none() && row.branch.is_none())
    );
    let old = rows
        .iter()
        .find(|row| row.row.node == Some(1))
        .and_then(|row| row.branch.as_ref())
        .ok_or("old branch")?;
    assert_eq!(old.target, BeforeRequest(1));
    assert!(old.checkpoint.is_none());
    assert!(
        rows.iter()
            .find(|row| row.row.node == Some(2))
            .is_some_and(|row| row.row.is_current && row.branch.is_none())
    );
    for target in [BeforeRequest(1), Tip(1)] {
        let preview = agent.branch_preview(target, RewindScope::Conversation)?;
        assert!(preview.checkpoint.is_none());
        assert!(preview.irreversible.is_empty());
        assert!(
            agent
                .branch_preview(target, RewindScope::FilesAndConversation)
                .is_err()
        );
    }
    let (events, _received) = mpsc::sync_channel(1024);
    let report = agent.switch_branch(
        BeforeRequest(1),
        agent
            .branch_preview(BeforeRequest(1), RewindScope::Conversation)?
            .selected
            .checkpoint
            .as_deref(),
        RewindScope::Conversation,
        false,
        events,
    )?;
    assert_eq!(report.request.as_deref(), Some("old request"));
    assert!(report.conversation_rewound);
    assert_eq!(
        session::ResumeState::read(f.session.directory())?.tree.tip,
        None
    );
    assert!(server.requests()?.is_empty());
    server.shutdown()?;
    Ok(())
}

// 関門: 一覧の表示行が全文を二つ持つ退行を、公開の行のデータで検出する。
// 移動先と全文の復元は保持し、時間や割り当て回数には依存しない。
#[test]
fn branch_tree_rows_keep_only_short_request_headings_and_the_target() -> TestResult {
    let f = Fixture::new()?;
    let mut server = MockProvider::start(vec![])?;
    let mut agent = f.agent(&server)?;
    let request = "日".repeat(5000);
    for text in [&request, &"current".to_owned()] {
        let (tx, _rx) = mpsc::sync_channel(1024);
        agent.run(text.clone(), Arc::new(AtomicBool::new(true)), tx)?;
    }
    let rows = agent.branch_tree()?;
    let row = rows
        .iter()
        .find(|row| row.row.node == Some(1))
        .ok_or("old request")?;
    assert_eq!(
        row.row.request.as_deref(),
        Some(format!("{}...", "日".repeat(60)).as_str())
    );
    let branch = row.branch.as_ref().ok_or("branch target")?;
    assert_eq!(
        branch.target,
        benitoiteh::agent::BranchTarget::BeforeRequest(1)
    );
    assert_eq!(branch.request.as_deref(), Some(request.as_str()));
    assert!(server.requests()?.is_empty());
    server.shutdown()?;
    Ok(())
}

// 関門: 確認した時点と実行時の時点が違う場合に、会話・記録・ファイルを変えない。
// None と異なる ID の両方を公開の移動の口で確かめる。
#[test]
fn branch_checkpoint_mismatch_changes_neither_records_nor_files() -> TestResult {
    use benitoiteh::agent::BranchTarget;
    let f = Fixture::new()?;
    fs::write(f.project.join("file"), "saved")?;
    let mut server = MockProvider::start(vec![])?;
    let mut agent = f.agent(&server)?;
    for text in ["old request", "current request"] {
        let (tx, _rx) = mpsc::sync_channel(1024);
        agent.run(text.into(), Arc::new(AtomicBool::new(true)), tx)?;
    }
    fs::write(f.project.join("file"), "current")?;
    let log = f.session.directory().join("session.jsonl");
    let records = fs::read(&log)?;
    let conversation = agent.conversation().clone();
    for target in [BranchTarget::BeforeRequest(2), BranchTarget::Tip(2)] {
        for scope in [RewindScope::Conversation, RewindScope::FilesAndConversation] {
            for checkpoint in [Some("cp-stale"), Some("cp-1")] {
                let (tx, _rx) = mpsc::sync_channel(1024);
                let error = agent
                    .switch_branch(target, checkpoint, scope, false, tx)
                    .err()
                    .ok_or("stale checkpoint was accepted")?;
                assert_eq!(error.to_string(), "The branch changed; open /tree again.");
                assert_eq!(fs::read(&log)?, records);
                assert_eq!(fs::read(f.project.join("file"))?, b"current");
                assert_eq!(agent.conversation(), &conversation);
            }
        }
    }
    let (tx, _rx) = mpsc::sync_channel(1024);
    assert!(
        agent
            .switch_branch(
                BranchTarget::BeforeRequest(2),
                None,
                RewindScope::Conversation,
                false,
                tx
            )
            .is_err()
    );
    assert_eq!(fs::read(&log)?, records);
    assert_eq!(fs::read(f.project.join("file"))?, b"current");
    server.shutdown()?;
    Ok(())
}
