//! bash の道具の境界を本物の子プロセスで確かめる準備（設計書 07-01）。

use std::error::Error;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::time::SystemTime;

use benitoiteh::config::{
    Approval, Mode, ReadConfig, Sandbox, child_environment, paths::DataPaths,
};
use benitoiteh::safe_fs::ScopeRoot;
use benitoiteh::sandbox::{PolicyConfig, SandboxPolicy};
use benitoiteh::session::{Session, SessionId};
use benitoiteh::tools::{
    ApprovalStatus, Invocation, ShellContext, ToolContext, ToolOutput, ToolSet,
    exclude::ExcludeRules,
};
use serde_json::Value;

pub struct Fixture {
    pub root: PathBuf,
    pub project: PathBuf,
    pub session: Session,
    pub context: ToolContext,
    pub tools: ToolSet,
}
impl Fixture {
    pub fn new(sandbox: Sandbox) -> Result<Self, Box<dyn Error>> {
        let mut random = [0; 16];
        getrandom::fill(&mut random)?;
        let root =
            std::env::temp_dir().join(format!("bh-bash-{:032x}", u128::from_ne_bytes(random)));
        fs::create_dir(&root)?;
        let root = root.canonicalize()?;
        let project = root.join("project");
        fs::create_dir(&project)?;
        let paths = DataPaths::from_values(Some(&root), None, None, Some(&root))?;
        let session = Session::create(&paths, SessionId::generate(SystemTime::now())?)?;
        let spill = session.directory().join("spill");
        let writable_roots = if sandbox == Sandbox::ReadOnly {
            vec![session.temporary_dir().to_owned()]
        } else {
            vec![project.clone(), session.temporary_dir().to_owned()]
        };
        let policy = if sandbox == Sandbox::DangerFullAccess {
            None
        } else {
            let mut config = PolicyConfig::new(writable_roots.clone());
            config.deny_exec_in_writable = false;
            Some(SandboxPolicy::new(config)?)
        };
        let env = child_environment(
            [
                ("PATH", "/bin:/usr/bin"),
                ("OPENAI_API_KEY", "synthetic-openai"),
                ("ANTHROPIC_API_KEY", "synthetic-anthropic"),
                ("CUSTOM_PROVIDER_KEY", "synthetic-custom"),
            ]
            .into_iter()
            .map(|(k, v)| (k.into(), v.into())),
            &["CUSTOM_PROVIDER_KEY".into()].into_iter().collect(),
            session.temporary_dir(),
            policy.as_ref().map(|_| (sandbox.name(), false)),
        );
        let context = ToolContext::new(
            Mode::Normal,
            ScopeRoot::open(&project)?,
            ScopeRoot::open(session.temporary_dir())?,
            ScopeRoot::open(&spill)?,
            ExcludeRules::new(
                &project,
                &paths,
                &spill,
                &ReadConfig::default(),
                &ReadConfig::default(),
            )?,
        )
        .with_session(session.clone())
        .with_shell(ShellContext {
            askpass: None,
            policy,
            policy_name: (sandbox != Sandbox::DangerFullAccess).then_some(sandbox.name()),
            helper: fs::canonicalize(env!("CARGO_BIN_EXE_benitoiteh"))?,
            scratch: session.directory().join("sandbox"),
            placeholders: paths.data_dir.join("placeholders"),
            env,
            writable_roots,
            sandbox,
            sandboxed: sandbox != Sandbox::DangerFullAccess,
            approval: Approval::Never,
        });
        Ok(Self {
            root,
            project,
            session,
            context,
            tools: ToolSet::new(Mode::Normal),
        })
    }
    pub fn execute(
        &self,
        id: &str,
        args: Value,
        cancelled: &AtomicBool,
    ) -> Result<ToolOutput, Box<dyn Error>> {
        let call = self.tools.validate("bash", args)?;
        Ok(self.tools.execute_validated(
            &self.context,
            &call,
            ApprovalStatus::Approved,
            Invocation {
                confirmed_effects: &std::collections::BTreeMap::new(),
                session_domains: &Default::default(),
                call_id: id,
                cancelled,
            },
        ))
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _removed = fs::remove_dir_all(&self.root);
    }
}
