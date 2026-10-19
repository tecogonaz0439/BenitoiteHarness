//! プロジェクトの切り替えをループ・確認・次の要求・記録で確かめる（設計書 01-03、01-05）。
use super::*;
use benitoiteh::rewind::{CheckpointRecorder, RewindScope};
use benitoiteh::session::{ResumeState, continue_session};

fn policy() -> ApprovalPolicy {
    ApprovalPolicy {
        approval: Approval::OnRequest,
        sandbox: Sandbox::WorkspaceWrite,
        sandboxed: true,
    }
}

// 関門: 切り替え時に旧プロジェクトを外しても、起動時に読んだ Skill の根と索引は保つ。
#[test]
fn creation_keeps_loaded_project_skills_readable_and_the_index_fixed() -> TestResult {
    let f = Fixture::new()?;
    let skill = f.project.join(".agents/skills/sample");
    fs::create_dir_all(&skill)?;
    fs::write(
        skill.join("SKILL.md"),
        "---\nname: sample\ndescription: project skill\n---\n",
    )?;
    fs::write(skill.join("reference.txt"), "skill reference")?;
    let destination = f.root.join("created-with-skill");
    let mut server = MockProvider::start(vec![
        ResponseScript::sse(calls(
            &[
                ("create_project", json!({"path":destination})),
                ("read", json!({"path":skill.join("reference.txt")})),
                ("read", json!({"path":f.project.join("a.bnt")})),
            ],
            "tool_calls",
        )),
        final_text(),
    ])?;
    let mut config = f.config(&server, AdapterKind::OpenAiChat);
    config.mode = Mode::Restricted;
    config.project_parent = Some(f.root.clone());
    config.allow_effects = Some(Default::default());
    let session = Session::create(&f.paths, SessionId::generate(SystemTime::now())?)?;
    let spill = session.directory().join("spill");
    let exclude = ExcludeRules::new(
        &f.project,
        &f.paths,
        &spill,
        &ReadConfig::default(),
        &ReadConfig::default(),
    )?;
    let (skills, roots) = benitoiteh::context::skills::collect(
        &f.project,
        &f.paths.home,
        &benitoiteh::config::UserSkillsConfig::default(),
        &exclude,
    );
    let context = ToolContext::new(
        Mode::Restricted,
        ScopeRoot::open(&f.project)?,
        ScopeRoot::open(session.temporary_dir())?,
        ScopeRoot::open(&spill)?,
        exclude,
    )
    .with_skill(support::skills()?)
    .with_skill_roots(roots)
    .with_project_creation(
        f.paths.clone(),
        fs::canonicalize(env!("CARGO_BIN_EXE_benitoiteh"))?,
    );
    let environment = Environment::capture(
        &f.project,
        Mode::Restricted,
        &[f.project.clone(), session.temporary_dir().into()],
        false,
        SystemTime::now(),
    )?
    .with_skills(skills);
    let mut agent = Agent::new(config, String::new(), context, environment, session)?;
    let (report, _) = complete(&mut agent)?;
    assert_eq!(report.outcome, Outcome::Completed);
    let outputs = results(&agent);
    assert!(
        outputs
            .get(1)
            .is_some_and(|result| !result.is_error && result.text.contains("skill reference"))
    );
    assert!(outputs.get(2).is_some_and(|result| result.is_error));
    assert_eq!(agent.status().skills.len(), 1);
    let environments = agent
        .conversation()
        .items
        .iter()
        .filter_map(|item| match item {
            Item::UserMessage(message) => Some(message.parts.iter()),
            Item::AssistantTurn(_) | Item::ToolResult(_) => None,
        })
        .flatten()
        .filter(|part| part.kind == benitoiteh::provider::UserPartKind::Environment)
        .collect::<Vec<_>>();
    assert!(
        environments
            .first()
            .is_some_and(|part| part.text.contains("<skills>"))
    );
    assert!(
        environments
            .last()
            .is_some_and(|part| !part.text.contains("<skills>")
                && part.text.contains(destination.to_str().unwrap_or_default()))
    );
    server.shutdown()?;
    Ok(())
}

// 関門: 作成だけのテストでは、結果の前の切り替え、残る古い根、環境の通知忘れを捕まえない。
#[test]
fn creation_switches_scope_before_later_calls_and_records_before_result() -> TestResult {
    let f = Fixture::new()?;
    let destination = f.root.join("created");
    let mut server = MockProvider::start(vec![
        ResponseScript::sse(calls(
            &[
                ("read", json!({"path":"a.bnt"})),
                ("create_project", json!({"path":destination})),
                ("read", json!({"path":"main.bnt"})),
                ("read", json!({"path":f.project.join("a.bnt")})),
                (
                    "write",
                    json!({"path":"hidden/secret.bnt","content":"hidden"}),
                ),
                ("read", json!({"path":"hidden/secret.bnt"})),
                (
                    "write",
                    json!({"path":f.project.join("bad.bnt"),"content":"bad"}),
                ),
            ],
            "tool_calls",
        )),
        final_text(),
    ])?;
    let mut config = f.config(&server, AdapterKind::OpenAiChat);
    config.mode = Mode::Restricted;
    let (mut agent, directory) = f.agent_with_read_config(
        config,
        None,
        None,
        ReadConfig {
            exclude: vec!["hidden/**".into()],
        },
    )?;
    let requests = Arc::new(Mutex::new(Vec::new()));
    agent = agent.with_approvals(
        Box::new(ScriptedApprover {
            requests: Arc::clone(&requests),
            answer: ApprovalAnswer {
                outcome: ApprovalOutcome::Approved,
                message: None,
            },
        }),
        policy(),
    );
    let (report, events) = complete(&mut agent)?;
    assert_eq!(report.outcome, Outcome::Completed);
    assert_eq!(agent.status().project, destination);
    let outputs = results(&agent);
    assert!(
        outputs
            .get(2)
            .is_some_and(|r| !r.is_error && r.text.contains("hello, world."))
    );
    assert!(outputs.get(3).is_some_and(|r| r.is_error));
    assert!(outputs.get(4).is_some_and(|r| !r.is_error));
    assert!(
        outputs
            .get(5)
            .is_some_and(|r| r.is_error && r.text.contains("hidden/**"))
    );
    assert!(outputs.get(6).is_some_and(|r| r.is_error));
    assert!(!f.project.join("bad.bnt").exists());
    assert_eq!(requests.lock().map_err(|_| "requests")?.len(), 1);
    let records = read_records(&directory)?;
    let changed = records.iter().position(|r| matches!(r, Record::ProjectDirChanged { project_dir } if project_dir == &destination)).ok_or("changed")?;
    let result = records
        .iter()
        .position(|r| matches!(r, Record::ToolResult { name, .. } if name == "create_project"))
        .ok_or("result")?;
    assert!(changed < result);
    assert!(events.iter().any(|event| matches!(event, AgentEvent::ProjectDirChanged { project_dir } if project_dir == &destination)));
    let sent = server.requests()?;
    let next = serde_json::to_string(&sent.get(1).ok_or("next request")?.body)?;
    assert!(
        next.contains(&format!(
            "<project_directory>{}</project_directory>",
            destination.display()
        )),
        "{next}"
    );
    // 新しい環境の欄が道具の結果の後の利用者の発言に入る。
    let environment = agent
        .conversation()
        .items
        .iter()
        .rev()
        .find_map(|item| match item {
            Item::UserMessage(message) => message
                .parts
                .iter()
                .find(|p| p.kind == benitoiteh::provider::UserPartKind::Environment),
            Item::AssistantTurn(_) | Item::ToolResult(_) => None,
        })
        .ok_or("environment")?;
    assert!(
        environment.text.contains("<writable_roots>")
            && environment
                .text
                .contains(destination.to_str().ok_or("destination")?)
    );
    assert!(
        !environment
            .text
            .contains(f.project.to_str().ok_or("project")?)
    );
    let old_checkpoint = records
        .iter()
        .find_map(|record| {
            if let Record::Checkpoint(c) = record {
                Some(c.id.clone())
            } else {
                None
            }
        })
        .ok_or("checkpoint")?;
    let new_checkpoints = agent.checkpoints()?;
    assert!(!new_checkpoints.is_empty());
    assert!(
        new_checkpoints
            .iter()
            .all(|c| c.checkpoint.id != old_checkpoint)
    );
    let (sender, _receiver) = mpsc::sync_channel(64);
    assert!(
        agent
            .rewind(&old_checkpoint, RewindScope::Files, false, sender)
            .is_err()
    );
    // 再開の時点一覧も、番号を使い回さず、前の根の内容を捨てる。
    let session_id = agent.status().session_id.parse()?;
    drop(agent);
    assert!(continue_session(&f.paths, &f.project).is_err());
    let session = continue_session(&f.paths, &destination)?;
    assert_eq!(session.id(), &session_id);
    let state = ResumeState::read(session.directory())?;
    assert_eq!(state.project_dir, destination);
    let root = ScopeRoot::open(&destination)?;
    let exclude = ExcludeRules::new(
        &destination,
        &f.paths,
        &directory.join("spill"),
        &ReadConfig::default(),
        &ReadConfig::default(),
    )?;
    let recorder = CheckpointRecorder::new(session.clone(), &root, exclude, &records)?;
    assert_eq!(
        recorder
            .checkpoints()?
            .iter()
            .map(|c| &c.checkpoint.id)
            .collect::<Vec<_>>(),
        new_checkpoints
            .iter()
            .map(|c| &c.checkpoint.id)
            .collect::<Vec<_>>()
    );
    let checkpoint = recorder.capture(
        benitoiteh::rewind::CheckpointReason::Request,
        None,
        Some(0),
        &AtomicBool::new(false),
    )?;
    assert_ne!(checkpoint.id, old_checkpoint);
    let context = ToolContext::new(
        Mode::Restricted,
        root,
        ScopeRoot::open(session.temporary_dir())?,
        ScopeRoot::open(directory.join("spill"))?,
        ExcludeRules::new(
            &destination,
            &f.paths,
            &directory.join("spill"),
            &ReadConfig::default(),
            &ReadConfig::default(),
        )?,
    )
    .with_skill(support::skills()?);
    let mut config = f.config(&server, AdapterKind::OpenAiChat);
    config.mode = Mode::Restricted;
    let environment = Environment::capture(
        &destination,
        Mode::Restricted,
        &[destination.clone(), session.temporary_dir().into()],
        false,
        SystemTime::now(),
    )?;
    let mut resumed = Agent::resume(config, String::new(), context, environment, session, state)?;
    assert_eq!(resumed.status().project, destination);
    let (sender, _receiver) = mpsc::sync_channel(64);
    resumed.run("resume".into(), Arc::new(AtomicBool::new(true)), sender)?;
    server.shutdown()?;
    Ok(())
}

// 関門: never の直接の拒否と --project-parent の直下限定を、確認の受け手が呼ばれないことで守る。
#[test]
fn noninteractive_creation_allows_only_the_configured_immediate_parent() -> TestResult {
    for case in [
        "none",
        "direct",
        "grandchild",
        "outside",
        "existing",
        "missing-parent",
        "file-parent",
        "alias-outside",
    ] {
        let f = Fixture::new()?;
        fs::create_dir(f.root.join("parent"))?;
        fs::create_dir(f.root.join("parent/sub"))?;
        fs::write(f.root.join("file"), "keep")?;
        std::os::unix::fs::symlink(f.root.join("parent/sub"), f.root.join("alias"))?;
        let destination = match case {
            "existing" => f.root.join("file"),
            "missing-parent" => f.root.join("missing/new"),
            "file-parent" => f.root.join("file/new"),
            "alias-outside" => f.root.join("alias/new"),
            "grandchild" => f.root.join("parent/sub/new"),
            "outside" => f.root.join("outside"),
            _ => f.root.join("parent/new"),
        };
        let mut server = MockProvider::start(vec![
            ResponseScript::sse(calls(
                &[("create_project", json!({"path":destination}))],
                "tool_calls",
            )),
            final_text(),
        ])?;
        let mut config = f.config(&server, AdapterKind::OpenAiChat);
        config.mode = Mode::Restricted;
        config.allow_effects = Some(Default::default());
        config.project_parent = (case != "none").then(|| f.root.join("parent"));
        let (mut agent, directory) = f.agent(config)?;
        let requests = Arc::new(Mutex::new(Vec::new()));
        agent = agent.with_approvals(
            Box::new(ScriptedApprover {
                requests: Arc::clone(&requests),
                answer: ApprovalAnswer {
                    outcome: ApprovalOutcome::Approved,
                    message: None,
                },
            }),
            policy(),
        );
        let (report, _) = complete(&mut agent)?;
        assert_eq!(report.outcome, Outcome::Completed);
        assert!(requests.lock().map_err(|_| "requests")?.is_empty());
        let allowed = case == "direct";
        assert_eq!(destination.exists(), allowed || case == "existing");
        assert_eq!(results(&agent).first().ok_or("result")?.is_error, !allowed);
        assert_eq!(report.summary.approvals.policy_denied, u64::from(!allowed));
        assert!(
            !read_records(&directory)?
                .iter()
                .any(|r| matches!(r, Record::Approval { .. }))
        );
        if !allowed {
            assert_eq!(
                results(&agent).first().ok_or("denied")?.text,
                "Project creation was denied by policy. In exec, set --project-parent and choose a new directory directly inside it, or stop and report."
            );
        }
        server.shutdown()?;
    }
    Ok(())
}

struct ChangingApprover {
    outcome: ApprovalOutcome,
    message: Option<String>,
    swap: Option<(PathBuf, PathBuf)>,
    cancel: Option<Arc<AtomicBool>>,
}
impl Approver for ChangingApprover {
    fn approve(&mut self, _: &ApprovalRecord) -> ApprovalAnswer {
        if let Some((link, target)) = &self.swap {
            // テストの準備に失敗したら承認を返さず、期待する結果で失敗する。
            if fs::remove_file(link)
                .and_then(|()| std::os::unix::fs::symlink(target, link))
                .is_err()
            {
                return ApprovalAnswer {
                    outcome: ApprovalOutcome::DeniedWithMessage,
                    message: Some("fixture swap failed".into()),
                };
            }
        }
        if let Some(cancel) = &self.cancel {
            cancel.store(true, Ordering::Release);
        }
        ApprovalAnswer {
            outcome: self.outcome,
            message: self.message.clone(),
        }
    }
}

// 関門: 承認の後の呼び直しを省く退行と、拒否・取消しの結果を落とす退行を守る。
#[test]
fn declined_cancelled_and_retargeted_requests_never_create_or_switch() -> TestResult {
    for case in ["denied", "message", "cancelled", "swap"] {
        let f = Fixture::new()?;
        fs::create_dir(f.root.join("parent"))?;
        fs::create_dir(f.root.join("other"))?;
        std::os::unix::fs::symlink(f.root.join("parent"), f.root.join("alias"))?;
        let destination = f.root.join("alias/new");
        let mut server = MockProvider::start(vec![
            ResponseScript::sse(calls(
                &[("create_project", json!({"path":destination}))],
                "tool_calls",
            )),
            final_text(),
        ])?;
        let mut config = f.config(&server, AdapterKind::OpenAiChat);
        config.mode = Mode::Restricted;
        let (mut agent, _) = f.agent(config)?;
        let cancel = Arc::new(AtomicBool::new(false));
        agent = agent.with_approvals(
            Box::new(ChangingApprover {
                outcome: if case == "denied" {
                    ApprovalOutcome::Denied
                } else if case == "message" {
                    ApprovalOutcome::DeniedWithMessage
                } else {
                    ApprovalOutcome::Approved
                },
                message: (case == "message").then(|| "Use another location".into()),
                swap: (case == "swap").then(|| (f.root.join("alias"), f.root.join("other"))),
                cancel: (case == "cancelled").then(|| Arc::clone(&cancel)),
            }),
            policy(),
        );
        let (_, events) = run(&mut agent, cancel, |_| {})?;
        assert!(!f.root.join("parent/new").exists() && !f.root.join("other/new").exists());
        assert_eq!(agent.status().project, f.project);
        let result = results(&agent).first().copied().ok_or("result")?;
        assert!(result.is_error);
        if case == "message" {
            assert!(
                result
                    .text
                    .contains("Message from the user: Use another location")
            );
        }
        assert!(
            !events
                .iter()
                .any(|e| matches!(e, AgentEvent::ProjectDirChanged { .. }))
        );
        server.shutdown()?;
    }
    Ok(())
}

// 関門: 一時領域は切り替え後も範囲内なので、同じ実パスへの版とエフェクトが残る退行を観測できる。
#[test]
fn switching_clears_read_versions_effects_and_check_failure_counters() -> TestResult {
    let f = Fixture::new()?;
    let session = Session::create(&f.paths, SessionId::generate(SystemTime::now())?)?;
    let pure = session.temporary_dir().join("pure.bnt");
    let invalid = session.temporary_dir().join("invalid.bnt");
    fs::write(&pure, "function main() -> Unit\n return ()\nend function\n")?;
    fs::write(
        &invalid,
        "function main() -> Unit\n return UNKNOWN_VALUE\nend function\n",
    )?;
    let mut server = MockProvider::start(vec![
        ResponseScript::sse(calls(
            &[
                ("read", json!({"path":pure})),
                ("benitoite_run", json!({"path":pure})),
                ("benitoite_check", json!({"path":invalid})),
                ("create_project", json!({"path":"created"})),
                (
                    "edit",
                    json!({"path":pure,"old_string":"return ()","new_string":"return () "}),
                ),
                ("benitoite_run", json!({"path":pure})),
                ("benitoite_check", json!({"path":invalid})),
            ],
            "tool_calls",
        )),
        final_text(),
    ])?;
    let mut config = f.config(&server, AdapterKind::OpenAiChat);
    config.mode = Mode::Restricted;
    config.limits.max_check_fix = 2;
    let spill = session.directory().join("spill");
    let context = ToolContext::new(
        Mode::Restricted,
        ScopeRoot::open(&f.project)?,
        ScopeRoot::open(session.temporary_dir())?,
        ScopeRoot::open(&spill)?,
        ExcludeRules::new(
            &f.project,
            &f.paths,
            &spill,
            &ReadConfig::default(),
            &ReadConfig::default(),
        )?,
    )
    .with_session(session.clone())
    .with_skill(support::skills()?)
    .with_check_query(support::checks(f.project.clone())?)
    .with_project_creation(
        f.paths.clone(),
        fs::canonicalize(env!("CARGO_BIN_EXE_benitoiteh"))?,
    );
    let environment = Environment::capture(
        &f.project,
        Mode::Restricted,
        &[f.project.clone(), session.temporary_dir().into()],
        false,
        SystemTime::now(),
    )?;
    let requests = Arc::new(Mutex::new(Vec::new()));
    let mut agent = Agent::new(config, String::new(), context, environment, session)?
        .with_approvals(
            Box::new(ScriptedApprover {
                requests: Arc::clone(&requests),
                answer: ApprovalAnswer {
                    outcome: ApprovalOutcome::Approved,
                    message: None,
                },
            }),
            policy(),
        );
    let (report, _) = complete(&mut agent)?;
    assert_eq!(
        report.outcome,
        Outcome::Completed,
        "{}",
        report.summary.reason
    );
    let requests = requests.lock().map_err(|_| "requests")?;
    assert_eq!(
        requests.iter().map(|r| r.kind).collect::<Vec<_>>(),
        [
            ApprovalKind::Effects,
            ApprovalKind::CreateProject,
            ApprovalKind::Effects
        ]
    );
    let outputs = results(&agent);
    assert!(
        outputs
            .get(4)
            .is_some_and(|r| r.is_error && r.text.contains("read")),
        "{outputs:?}"
    );
    assert!(
        outputs
            .get(2)
            .is_some_and(|r| r.is_error && r.text.contains("UNKNOWN_VALUE"))
    );
    assert!(
        outputs
            .get(6)
            .is_some_and(|r| r.is_error && r.text.contains("UNKNOWN_VALUE"))
    );
    assert!(fs::read_to_string(&pure)?.contains("return ()\n"));
    server.shutdown()?;
    Ok(())
}
