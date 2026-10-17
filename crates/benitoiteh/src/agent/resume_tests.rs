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
