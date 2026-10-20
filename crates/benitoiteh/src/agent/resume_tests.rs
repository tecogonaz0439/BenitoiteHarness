//! exec では保持できないセッション承認を、再開の入口で確かめる（設計書 01-05「再開」）。

use super::*;
use std::io::Write;
#[allow(dead_code, clippy::duplicate_mod)]
#[path = "../../tests/support/mock_provider.rs"]
mod mock_provider;
use crate::config::{ReadConfig, paths::DataPaths};
use crate::safe_fs::ScopeRoot;
use crate::tools::exclude::ExcludeRules;

struct Fixture(std::path::PathBuf);

impl Drop for Fixture {
    fn drop(&mut self) {
        let _removed = std::fs::remove_dir_all(&self.0);
    }
}

fn tool_context(
    paths: &DataPaths,
    project: &std::path::Path,
    session: &Session,
) -> Result<ToolContext, Box<dyn std::error::Error>> {
    let spill = session.directory().join("spill");
    Ok(ToolContext::new(
        Mode::Normal,
        ScopeRoot::open(project)?,
        ScopeRoot::open(session.temporary_dir())?,
        ScopeRoot::open(&spill)?,
        ExcludeRules::new(
            project,
            paths,
            &spill,
            &ReadConfig::default(),
            &ReadConfig::default(),
        )?,
    )
    .with_session(session.clone()))
}

// 作成時の関門: 記録の ApprovedForSession を、新しい起動のコマンド許可へ戻す
// 退行を防ぐ。exec は never だけなので、Agent::resume の境界で守る。
#[test]
fn resume_starts_with_empty_session_command_approvals() -> Result<(), Box<dyn std::error::Error>> {
    let id = session::SessionId::generate(std::time::SystemTime::now())?;
    let directory = std::env::temp_dir().join(format!("benitoiteh-resume-approvals-{id}"));
    std::fs::create_dir(&directory)?;
    let fixture = Fixture(std::fs::canonicalize(directory)?);
    for name in ["home", "project", "tmp"] {
        std::fs::create_dir(fixture.0.join(name))?;
    }
    let paths = DataPaths::from_values(
        Some(&fixture.0.join("home")),
        Some(&fixture.0.join("config")),
        Some(&fixture.0.join("state")),
        Some(&fixture.0.join("tmp")),
    )?;
    let project = fixture.0.join("project");
    let session = Session::create(&paths, id.clone())?;
    let response = || {
        mock_provider::ResponseScript::sse(format!(
            "data: {}\n\ndata: [DONE]\n\n",
            serde_json::json!({"choices":[{"delta":{"content":"answer"},"finish_reason":"stop"}]})
        ))
    };
    let mut server = mock_provider::MockProvider::start(vec![response(), response()])?;
    let config = AgentConfig {
        compaction: Default::default(),
        agent: Default::default(),
        project_parent: None,
        allow_effects: None,
        mode: Mode::Normal,
        destination: Destination {
            provider: "test".into(),
            base_url: server.base_url(),
            model: "model".into(),
        },
        kind: AdapterKind::OpenAiChat,
        capabilities: Capabilities::default(),
        effort: "high".into(),
        limits: Limits::default(),
        http_timeouts: HttpTimeouts::default(),
        retry_initial: Duration::from_millis(1),
    };
    let environment = Environment::capture(
        &project,
        Mode::Normal,
        &[],
        false,
        std::time::SystemTime::now(),
    )?;
    let mut agent = Agent::new(
        config.clone(),
        String::new(),
        tool_context(&paths, &project, &session)?,
        environment.clone(),
        session.clone(),
    )?;
    agent.allowed_commands.insert("cat private".into());
    agent.session_domains.insert("mock.invalid".into());
    agent.writer.append(&Record::Approval {
        phase: session::ApprovalPhase::Result,
        id: "approval".into(),
        tool_id: "call".into(),
        request: Some("cat private".into()),
        outcome: Some(session::ApprovalOutcome::ApprovedForSession),
        kind: Some(crate::approval::ApprovalKind::Command),
        facts: crate::approval::ApprovalFacts {
            command: Some("cat private".into()),
            ..Default::default()
        },
        justification: None,
        user_message: None,
    })?;
    let (sender, receiver) = std::sync::mpsc::sync_channel(32);
    agent.run(
        "First request".into(),
        Arc::new(AtomicBool::new(false)),
        sender,
    )?;
    let _events = receiver.into_iter().collect::<Vec<_>>();
    let previous = agent.conversation().clone();
    drop(agent);
    let log = session.directory().join("session.jsonl");
    drop(session);
    std::fs::OpenOptions::new()
        .append(true)
        .open(&log)?
        .write_all(b"{\"type\":")?;
    let before = std::fs::read(&log)?;
    // 並行するテストの fork が継承した錠は exec まで残る。Locked だけを短時間待ち、
    // ほかの誤りや錠を放さない退行は隠さない（設計書 07-01）。
    let started = std::time::Instant::now();
    let session = loop {
        match Session::open(&paths, &id) {
            Err(session::SessionError::Locked) if started.elapsed() < Duration::from_secs(2) => {
                std::thread::sleep(Duration::from_millis(20));
            }
            result => break result?,
        }
    };
    let state = session::ResumeState::read(session.directory())?;
    let mut invalid = state.clone();
    invalid.reasoning.clear();
    let failed = Agent::resume(
        config.clone(),
        String::new(),
        tool_context(&paths, &project, &session)?,
        environment.clone(),
        session.clone(),
        invalid,
    );
    assert!(matches!(failed, Err(AgentError::InvalidConfiguration)));
    assert_eq!(std::fs::read(&log)?, before);
    let mut agent = Agent::resume(
        config,
        String::new(),
        tool_context(&paths, &project, &session)?,
        environment,
        session.clone(),
        state,
    )?;
    assert!(agent.allowed_commands.is_empty());
    assert!(agent.session_domains.is_empty());
    assert_eq!(agent.conversation(), &previous);
    let (sender, receiver) = std::sync::mpsc::sync_channel(32);
    agent.run(
        "Second request".into(),
        Arc::new(AtomicBool::new(false)),
        sender,
    )?;
    let _events = receiver.into_iter().collect::<Vec<_>>();
    assert_eq!(session::replay(session.directory())?, *agent.conversation());
    assert_eq!(server.requests()?.len(), 2);
    server.shutdown()?;
    Ok(())
}

// 作成時の関門: 二ラベルの名前を実際に解かずに、ループが今回の答えだけをドメインの
// 許可へ記憶することを確かめる。確認直後の利用者の取り消しは本番の口を通す。
#[test]
fn web_session_answers_are_remembered_by_the_loop_before_cancelled_execution()
-> Result<(), Box<dyn std::error::Error>> {
    use crate::approval::{
        ApprovalAnswer, ApprovalKind, ApprovalOutcome, ApprovalRecord, Approver,
    };
    struct CancelAfterApproval {
        cancelled: Arc<AtomicBool>,
        outcome: ApprovalOutcome,
    }
    impl Approver for CancelAfterApproval {
        fn approve(&mut self, request: &ApprovalRecord) -> ApprovalAnswer {
            assert_eq!(request.kind, ApprovalKind::WebFetch);
            assert_eq!(request.facts.domain.as_deref(), Some("mock.invalid"));
            self.cancelled
                .store(true, std::sync::atomic::Ordering::Release);
            ApprovalAnswer {
                outcome: self.outcome,
                message: None,
            }
        }
    }
    let id = session::SessionId::generate(std::time::SystemTime::now())?;
    let directory = std::env::temp_dir().join(format!("benitoiteh-web-approvals-{id}"));
    std::fs::create_dir(&directory)?;
    let fixture = Fixture(std::fs::canonicalize(directory)?);
    for name in ["home", "project", "tmp"] {
        std::fs::create_dir(fixture.0.join(name))?;
    }
    let paths = DataPaths::from_values(
        Some(&fixture.0.join("home")),
        Some(&fixture.0.join("config")),
        Some(&fixture.0.join("state")),
        Some(&fixture.0.join("tmp")),
    )?;
    let project = fixture.0.join("project");
    for outcome in [
        ApprovalOutcome::Approved,
        ApprovalOutcome::ApprovedForSession,
    ] {
        let session = Session::create(
            &paths,
            session::SessionId::generate(std::time::SystemTime::now())?,
        )?;
        let response = serde_json::json!({"choices":[{"delta":{"tool_calls":[{"index":0,"id":"fetch","type":"function","function":{"name":"web_fetch","arguments":serde_json::json!({"url":"https://MOCK.INVALID.:443/"}).to_string()}}]},"finish_reason":"tool_calls"}]});
        let mut server =
            mock_provider::MockProvider::start(vec![mock_provider::ResponseScript::sse(format!(
                "data: {response}\n\ndata: [DONE]\n\n"
            ))])?;
        let config = AgentConfig {
            compaction: Default::default(),
            agent: Default::default(),
            project_parent: None,
            allow_effects: None,
            mode: Mode::Normal,
            destination: Destination {
                provider: "test".into(),
                base_url: server.base_url(),
                model: "model".into(),
            },
            kind: AdapterKind::OpenAiChat,
            capabilities: Capabilities::default(),
            effort: "high".into(),
            limits: Limits::default(),
            http_timeouts: HttpTimeouts::default(),
            retry_initial: Duration::from_millis(1),
        };
        let environment = Environment::capture(
            &project,
            Mode::Normal,
            &[],
            false,
            std::time::SystemTime::now(),
        )?;
        let cancelled = Arc::new(AtomicBool::new(false));
        let mut agent = Agent::new(
            config,
            String::new(),
            tool_context(&paths, &project, &session)?,
            environment,
            session.clone(),
        )?
        .with_approvals(
            Box::new(CancelAfterApproval {
                cancelled: Arc::clone(&cancelled),
                outcome,
            }),
            ApprovalPolicy {
                approval: Approval::OnRequest,
                sandbox: Sandbox::WorkspaceWrite,
                sandboxed: true,
            },
        );
        let (sender, receiver) = std::sync::mpsc::sync_channel(32);
        let report = agent.run("Fetch the page".into(), Arc::clone(&cancelled), sender)?;
        assert_eq!(report.outcome, Outcome::Cancelled);
        assert!(agent.allowed_commands.is_empty());
        let expected = if outcome == ApprovalOutcome::ApprovedForSession {
            BTreeSet::from(["mock.invalid".to_owned()])
        } else {
            BTreeSet::new()
        };
        assert_eq!(agent.session_domains, expected);
        let events = receiver.into_iter().collect::<Vec<_>>();
        assert_eq!(
            events
                .iter()
                .filter(|event| matches!(event, AgentEvent::ApprovalRequest { .. }))
                .count(),
            1
        );
        assert!(agent.conversation.items.iter().any(|item| matches!(item, Item::ToolResult(result) if result.is_error && result.text.contains("cancelled"))));
        server.shutdown()?;
    }
    Ok(())
}

// 作成時の関門: 道具単独の保存とアダプタ単独の要求では、ループの受け渡し、記録からの
// 中身の除外、construct の復元、破損後の省略を捕まえられない。実際に read した記録で守る。
#[test]
fn image_results_are_restored_only_when_enabled_and_metadata_and_hash_match()
-> Result<(), Box<dyn std::error::Error>> {
    let id = session::SessionId::generate(std::time::SystemTime::now())?;
    let directory = std::env::temp_dir().join(format!("benitoiteh-resume-images-{id}"));
    std::fs::create_dir(&directory)?;
    let fixture = Fixture(std::fs::canonicalize(directory)?);
    for name in ["home", "project", "tmp"] {
        std::fs::create_dir(fixture.0.join(name))?;
    }
    let paths = DataPaths::from_values(
        Some(&fixture.0.join("home")),
        Some(&fixture.0.join("config")),
        Some(&fixture.0.join("state")),
        Some(&fixture.0.join("tmp")),
    )?;
    let project = fixture.0.join("project");
    let bytes = b"GIF89a\x01\0\x01\0";
    std::fs::write(project.join("image.txt"), bytes)?;
    let session = Session::create(&paths, id)?;
    let mut server = mock_provider::MockProvider::start(vec![
        mock_provider::ResponseScript::sse(format!(
            "data: {}\n\ndata: [DONE]\n\n",
            serde_json::json!({"choices":[{"delta":{"tool_calls":(0..3).map(|index| serde_json::json!({"index":index,"id":format!("read_image_{index}"),"type":"function","function":{"name":"read","arguments":"{\"path\":\"image.txt\"}"}})).collect::<Vec<_>>()},"finish_reason":"tool_calls"}]})
        )),
        mock_provider::ResponseScript::sse(format!(
            "data: {}\n\ndata: [DONE]\n\n",
            serde_json::json!({"choices":[{"delta":{"content":"done"},"finish_reason":"stop"}]})
        )),
    ])?;
    let config = AgentConfig {
        compaction: Default::default(),
        agent: Default::default(),
        project_parent: None,
        allow_effects: None,
        mode: Mode::Normal,
        destination: Destination {
            provider: "test".into(),
            base_url: server.base_url(),
            model: "model".into(),
        },
        kind: AdapterKind::OpenAiChat,
        capabilities: Capabilities {
            image_input: true,
            ..Capabilities::default()
        },
        effort: "high".into(),
        limits: Limits::default(),
        http_timeouts: HttpTimeouts::default(),
        retry_initial: Duration::from_millis(1),
    };
    let environment = Environment::capture(
        &project,
        Mode::Normal,
        &[],
        false,
        std::time::SystemTime::now(),
    )?;
    let mut agent = Agent::new(
        config.clone(),
        String::new(),
        tool_context(&paths, &project, &session)?,
        environment.clone(),
        session.clone(),
    )?;
    let (sender, receiver) = std::sync::mpsc::sync_channel(64);
    agent.run(
        "Read the image".into(),
        Arc::new(AtomicBool::new(false)),
        sender,
    )?;
    let _events = receiver.into_iter().collect::<Vec<_>>();
    let image = agent
        .conversation()
        .items
        .iter()
        .find_map(|item| match item {
            Item::ToolResult(result) => result.images.first(),
            Item::UserMessage(_) | Item::AssistantTurn(_) => None,
        })
        .ok_or("missing image result")?;
    assert_eq!(image.data.as_deref(), Some(bytes.as_slice()));
    let hash = image.sha256.clone();
    let log = std::fs::read_to_string(session.directory().join("session.jsonl"))?;
    assert!(!log.contains("R0lGODlhAQABAA=="));
    assert!(!log.contains("GIF89a"));
    for line in log.lines() {
        let value: serde_json::Value = serde_json::from_str(line)?;
        if value.get("type").and_then(serde_json::Value::as_str) == Some("tool_result") {
            let images = value
                .pointer("/result/images")
                .and_then(serde_json::Value::as_array)
                .ok_or("missing image metadata")?;
            assert_eq!(images.len(), 1);
            assert_eq!(
                images.first(),
                Some(
                    &serde_json::json!({"media_type":"image/gif", "sha256":hash, "width":1, "height":1, "bytes":10})
                )
            );
        }
    }
    assert_eq!(session::replay(session.directory())?, *agent.conversation());
    let requests = server.requests()?;
    assert_eq!(requests.len(), 2);
    assert!(
        requests
            .get(1)
            .ok_or("missing image request")?
            .body
            .to_string()
            .contains("data:image/gif;base64,R0lGODlhAQABAA==")
    );
    drop(agent);
    let state = session::ResumeState::read(session.directory())?;
    assert!(
        state
            .conversation
            .items
            .iter()
            .filter_map(|item| match item {
                Item::ToolResult(result) => result.images.first(),
                Item::UserMessage(_) | Item::AssistantTurn(_) => None,
            })
            .all(|image| image.data.is_none())
    );
    // 同じ内容を三度参照しても写しを三つ作らない契約を、再開の入口で守る。
    // 従来の単独画像の照合では、ハッシュごとの共有の退行を検出できない。
    let restored = Agent::resume(
        config.clone(),
        String::new(),
        tool_context(&paths, &project, &session)?,
        environment.clone(),
        session.clone(),
        state.clone(),
    )?;
    let data = restored
        .conversation()
        .items
        .iter()
        .filter_map(|item| match item {
            Item::ToolResult(result) => result.images.first().and_then(|image| image.data.as_ref()),
            Item::UserMessage(_) | Item::AssistantTurn(_) => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(data.len(), 3);
    let first = data.first().ok_or("missing restored image")?;
    assert!(data.iter().all(|bytes| Arc::ptr_eq(first, bytes)));
    drop(restored);
    for variant in [
        "unsupported",
        "mime",
        "width",
        "height",
        "bytes",
        "invalid_header",
    ] {
        let mut state = state.clone();
        // 不正な参照が先でも後でも、同じハッシュを持つ正しい参照を損なわず、
        // 共有した中身に対する属性の照合も省かないことを確かめる。
        for (index, result) in state
            .conversation
            .items
            .iter_mut()
            .filter_map(|item| match item {
                Item::ToolResult(result) => Some(result),
                Item::UserMessage(_) | Item::AssistantTurn(_) => None,
            })
            .enumerate()
        {
            if index == 1 {
                continue;
            }
            let image = result.images.first_mut().ok_or("missing image metadata")?;
            match variant {
                "unsupported" => image.sha256 = "unread-object".into(),
                "mime" => image.media_type = "image/png".into(),
                "width" => image.width = 2,
                "height" => image.height = 2,
                "bytes" => image.bytes = 11,
                "invalid_header" => {
                    let invalid = b"not an image";
                    let (hash, _) = session.store_object(invalid)?.ok_or("storage limit")?;
                    image.sha256 = hash;
                    image.bytes = u64::try_from(invalid.len())?;
                }
                _ => {}
            }
        }
        let mut config = config.clone();
        config.capabilities.image_input = variant != "unsupported";
        let agent = Agent::resume(
            config,
            String::new(),
            tool_context(&paths, &project, &session)?,
            environment.clone(),
            session.clone(),
            state,
        )?;
        let result = agent
            .conversation()
            .items
            .iter()
            .find_map(|item| match item {
                Item::ToolResult(result) => Some(result),
                Item::UserMessage(_) | Item::AssistantTurn(_) => None,
            })
            .ok_or("missing result")?;
        if variant == "unsupported" {
            assert_eq!(result.images.len(), 1);
            assert!(
                result
                    .images
                    .first()
                    .is_some_and(|image| image.data.is_none())
            );
            assert!(!result.text.contains("omitted"));
        } else {
            assert!(result.images.is_empty());
            assert!(result.text.ends_with("(An image from this result could not be restored from the session and was omitted.)"));
            assert_eq!(
                agent
                    .conversation()
                    .items
                    .iter()
                    .filter_map(|item| match item {
                        Item::ToolResult(result) => result
                            .images
                            .first()
                            .and_then(|image| image.data.as_deref()),
                        Item::UserMessage(_) | Item::AssistantTurn(_) => None,
                    })
                    .filter(|data| *data == bytes.as_slice())
                    .count(),
                1
            );
        }
    }
    let resumed_config = config;
    for (corrupted, missing) in [(false, false), (true, false), (true, true)] {
        if missing {
            std::fs::remove_file(session.directory().join("objects").join(&hash))?;
        } else if corrupted {
            std::fs::write(
                session.directory().join("objects").join(&hash),
                b"corrupted!",
            )?;
        }
        let agent = Agent::resume(
            resumed_config.clone(),
            String::new(),
            tool_context(&paths, &project, &session)?.with_image_input(true),
            environment.clone(),
            session.clone(),
            state.clone(),
        )?;
        assert!(agent.tool_context.image_input);
        let result = agent
            .conversation()
            .items
            .iter()
            .find_map(|item| match item {
                Item::ToolResult(result) => Some(result),
                Item::UserMessage(_) | Item::AssistantTurn(_) => None,
            })
            .ok_or("missing resumed result")?;
        if corrupted {
            assert!(result.images.is_empty());
            assert!(result.text.ends_with("(An image from this result could not be restored from the session and was omitted.)"));
        } else {
            assert_eq!(
                result
                    .images
                    .first()
                    .and_then(|image| image.data.as_deref()),
                Some(bytes.as_slice())
            );
        }
    }
    server.shutdown()?;
    Ok(())
}

// 作成時の関門: 正常な記録からは作れない推論の個数の不整合を準備の境界で与える。
// 準備の失敗で会話・キャッシュの状態を変えず、成功後も追記まで反映しない契約を守る。
#[test]
fn branch_preparation_failure_preserves_log_and_memory_until_commit()
-> Result<(), Box<dyn std::error::Error>> {
    let id = session::SessionId::generate(std::time::SystemTime::now())?;
    let directory = std::env::temp_dir().join(format!("benitoiteh-resume-approvals-{id}"));
    std::fs::create_dir(&directory)?;
    let fixture = Fixture(std::fs::canonicalize(directory)?);
    for name in ["home", "project", "tmp"] {
        std::fs::create_dir(fixture.0.join(name))?;
    }
    let paths = DataPaths::from_values(
        Some(&fixture.0.join("home")),
        Some(&fixture.0.join("config")),
        Some(&fixture.0.join("state")),
        Some(&fixture.0.join("tmp")),
    )?;
    let project = fixture.0.join("project");
    let session = Session::create(&paths, id.clone())?;
    let response = || {
        mock_provider::ResponseScript::sse(format!(
            "data: {}\n\ndata: [DONE]\n\n",
            serde_json::json!({"choices":[{"delta":{"content":"answer"},"finish_reason":"stop"}]})
        ))
    };
    let mut server = mock_provider::MockProvider::start(vec![response()])?;
    let config = AgentConfig {
        compaction: Default::default(),
        agent: Default::default(),
        project_parent: None,
        allow_effects: None,
        mode: Mode::Normal,
        destination: Destination {
            provider: "test".into(),
            base_url: server.base_url(),
            model: "model".into(),
        },
        kind: AdapterKind::OpenAiChat,
        capabilities: Capabilities::default(),
        effort: "high".into(),
        limits: Limits::default(),
        http_timeouts: HttpTimeouts::default(),
        retry_initial: Duration::from_millis(1),
    };
    let environment = Environment::capture(
        &project,
        Mode::Normal,
        &[],
        false,
        std::time::SystemTime::now(),
    )?;
    let mut agent = Agent::new(
        config.clone(),
        String::new(),
        tool_context(&paths, &project, &session)?,
        environment.clone(),
        session.clone(),
    )?;

    let (sender, _receiver) = std::sync::mpsc::sync_channel(1024);
    agent.run("request".into(), Arc::new(AtomicBool::new(false)), sender)?;
    let log = session.directory().join("session.jsonl");
    let before = std::fs::read(&log)?;
    let conversation = agent.conversation.clone();
    let current_node = agent.current_node;
    let next_node = agent.next_node;
    agent.resume_compaction = true;
    agent.resume_has_reasoning = true;
    agent.environment_changes = Some("preserved environment changes".into());
    agent.previous_last_item = Some(0);
    let mut invalid = session::ResumeState::read(session.directory())?;
    invalid.reasoning.clear();
    assert!(matches!(
        agent.prepare_path_state(invalid),
        Err(AgentError::InvalidConfiguration)
    ));
    assert_eq!(std::fs::read(&log)?, before);
    assert_eq!(agent.conversation, conversation);
    assert_eq!(agent.current_node, current_node);
    assert_eq!(agent.next_node, next_node);
    assert!(agent.resume_compaction);
    assert!(agent.resume_has_reasoning);
    assert_eq!(
        agent.environment_changes.as_deref(),
        Some("preserved environment changes")
    );
    assert_eq!(agent.previous_last_item, Some(0));
    let branch = Record::Branch {
        to: None,
        from: current_node,
        checkpoint: None,
    };
    let mut records = session::read_records(session.directory())?;
    let mut tree = session::SessionTree::from_records(&records)?;
    tree.push_record(&branch, records.len())?;
    records.push(branch.clone());
    let state = session::ResumeState::from_records_with_tree(records, tree)?;
    let prepared = agent.prepare_path_state(state)?;
    assert_eq!(std::fs::read(&log)?, before);
    assert_eq!(agent.conversation, conversation);
    agent.commit_branch(&branch, prepared)?;
    assert!(agent.conversation.items.is_empty());
    assert!(agent.environment_changes.is_none());
    assert!(!agent.resume_compaction);
    assert!(!agent.resume_has_reasoning);
    assert_eq!(
        session::read_records(session.directory())?.last(),
        Some(&branch)
    );
    assert_eq!(session::replay(session.directory())?, agent.conversation);
    server.shutdown()?;
    Ok(())
}
