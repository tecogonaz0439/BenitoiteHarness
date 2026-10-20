//! 巻き戻しの境界に、本物の道具・記録と模擬 HTTP 提供者を接続する（設計書 07-01）。

use crate::support::mock_provider::{MockProvider, ResponseScript};
use benitoiteh::agent::{Agent, AgentConfig, AgentEvent};
use benitoiteh::config::{Approval, Limits, Mode, ReadConfig, Sandbox, paths::DataPaths};
use benitoiteh::context::Environment;
use benitoiteh::http::HttpTimeouts;
use benitoiteh::provider::{AdapterKind, Capabilities, Destination};
use benitoiteh::rewind::{CheckpointReason, CheckpointRecorder, RewindReport, RewindScope};
use benitoiteh::safe_fs::ScopeRoot;
use benitoiteh::session::{Header, Record, RecordedMode, RecordedProvider, Session, SessionId};
use benitoiteh::tools::{ShellContext, ToolContext, exclude::ExcludeRules};
use serde_json::{Value, json};
use std::error::Error;
use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, atomic::AtomicBool, mpsc};
use std::time::{Duration, SystemTime};

pub type TestResult = Result<(), Box<dyn Error>>;
pub struct Fixture {
    pub root: PathBuf,
    pub project: PathBuf,
    pub paths: DataPaths,
    pub session: Session,
}
impl Fixture {
    pub fn new() -> Result<Self, Box<dyn Error>> {
        let id = SessionId::generate(SystemTime::now())?;
        let root = std::env::temp_dir().join(format!("bh-rewind-{id}"));
        fs::create_dir(&root)?;
        let root = root.canonicalize()?;
        let project = root.join("project");
        for path in [&project, &root.join("home"), &root.join("tmp")] {
            fs::create_dir(path)?;
        }
        let paths = DataPaths::from_values(
            Some(&root.join("home")),
            Some(&root.join("config")),
            Some(&root.join("state")),
            Some(&root.join("tmp")),
        )?;
        let session = Session::create(&paths, id)?;
        Ok(Self {
            root,
            project,
            paths,
            session,
        })
    }
    pub fn context(&self) -> Result<ToolContext, Box<dyn Error>> {
        let spill = self.session.directory().join("spill");
        Ok(ToolContext::new(
            Mode::Normal,
            ScopeRoot::open(&self.project)?,
            ScopeRoot::open(self.session.temporary_dir())?,
            ScopeRoot::open(&spill)?,
            ExcludeRules::new(
                &self.project,
                &self.paths,
                &spill,
                &ReadConfig::default(),
                &ReadConfig::default(),
            )?,
        )
        .with_session(self.session.clone())
        .with_shell(ShellContext {
            askpass: None,
            policy: None,
            policy_name: None,
            helper: fs::canonicalize(env!("CARGO_BIN_EXE_benitoiteh"))?,
            scratch: self.session.directory().join("sandbox"),
            placeholders: self.paths.data_dir.join("placeholders"),
            env: benitoiteh::config::child_environment(
                [("PATH".into(), "/bin:/usr/bin".into())],
                &Default::default(),
                self.session.temporary_dir(),
                None,
            ),
            writable_roots: vec![self.project.clone(), self.session.temporary_dir().into()],
            sandbox: Sandbox::DangerFullAccess,
            sandboxed: false,
            approval: Approval::Never,
        }))
    }
    pub fn agent(&self, server: &MockProvider) -> Result<Agent, Box<dyn Error>> {
        Ok(Agent::new(
            self.config(server),
            String::new(),
            self.context()?,
            self.environment()?,
            self.session.clone(),
        )?)
    }
    pub fn config(&self, server: &MockProvider) -> AgentConfig {
        AgentConfig {
            agent: Default::default(),
            project_parent: None,
            allow_effects: None,
            mode: Mode::Normal,
            destination: Destination {
                provider: "mock".into(),
                base_url: server.base_url(),
                model: "model".into(),
            },
            kind: AdapterKind::OpenAiChat,
            capabilities: Capabilities::default(),
            effort: "high".into(),
            limits: Limits::default(),
            http_timeouts: HttpTimeouts::default(),
            retry_initial: Duration::from_millis(1),
        }
    }
    pub fn environment(&self) -> Result<Environment, Box<dyn Error>> {
        Ok(Environment::capture(
            &self.project,
            Mode::Normal,
            &[],
            false,
            SystemTime::now(),
        )?)
    }
    pub fn recorder(&self) -> Result<CheckpointRecorder, Box<dyn Error>> {
        self.session.writer()?.append(&Record::Header(Header {
            forked_from: None,
            format: benitoiteh::session::FORMAT_VERSION,
            session_id: self.session.id().to_string(),
            started_at: self.session.id().started_at_utc(),
            benitoiteh_version: "test".into(),
            mode: RecordedMode::Normal,
            project_dir: self.project.clone(),
            provider: RecordedProvider {
                name: "mock".into(),
                base_url: "http://localhost".into(),
                model: "model".into(),
                adapter: AdapterKind::OpenAiChat,
            },
            benitoite: None,
            system_prompt_sha256: "test".into(),
            tools_sha256: "test".into(),
            completion_check: None,
            after_write_check: None,
            read_line_numbers: None,
            plugins: Vec::new(),
            skills: Vec::new(),
        }))?;
        let context = self.context()?;
        Ok(CheckpointRecorder::new(
            self.session.clone(),
            &context.project,
            context.exclude,
            &[],
        )?)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _removed = fs::remove_dir_all(&self.root);
    }
}
pub fn capture(
    recorder: &CheckpointRecorder,
) -> Result<benitoiteh::rewind::Checkpoint, Box<dyn Error>> {
    Ok(recorder.capture(CheckpointReason::Request, None, &AtomicBool::new(false))?)
}
pub fn response(calls: &[(&str, Value)]) -> ResponseScript {
    let tools = calls.iter().enumerate().map(|(index,(name,args))| json!({"index":index,"id":format!("provider-{index}"),"type":"function","function":{"name":name,"arguments":args.to_string()}})).collect::<Vec<_>>();
    let delta = if calls.is_empty() {
        json!({"content":"Done","reasoning_content":"Reason"})
    } else {
        json!({"tool_calls":tools})
    };
    ResponseScript::sse(format!(
        "data: {}\n\ndata: [DONE]\n\n",
        json!({"choices":[{"delta":delta,"finish_reason":if calls.is_empty(){"stop"}else{"tool_calls"}}]})
    ))
}
pub fn run(agent: &mut Agent) -> Result<Vec<AgentEvent>, Box<dyn Error>> {
    let (sender, receiver) = mpsc::sync_channel(1024);
    let report = agent.run("Request".into(), Arc::new(AtomicBool::new(false)), sender)?;
    assert_eq!(
        report.outcome,
        benitoiteh::agent::Outcome::Completed,
        "{:?}",
        report
    );
    Ok(receiver.into_iter().collect())
}
pub fn rewind(
    agent: &mut Agent,
    id: &str,
    scope: RewindScope,
    allow: bool,
) -> Result<RewindReport, benitoiteh::agent::AgentError> {
    let (sender, _receiver) = mpsc::sync_channel(1024);
    agent.rewind(id, scope, allow, sender)
}
