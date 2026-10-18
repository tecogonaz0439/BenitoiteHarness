//! 本物の処理系と道具をつなぐ実行テストの準備（設計書 07-01）。

use benitoiteh::benitoite::{CheckQuery, Effect, RunContext};
use benitoiteh::config::{Mode, ReadConfig, paths::DataPaths, writable::WritableLocations};
use benitoiteh::safe_fs::ScopeRoot;
use benitoiteh::session::{Session, SessionId};
use benitoiteh::tools::{
    ApprovalStatus, ConfirmKey, Invocation, ToolContext, ToolOutput, ToolSet, exclude::ExcludeRules,
};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::{Arc, atomic::AtomicBool};
use std::time::SystemTime;

pub type TestResult = Result<(), Box<dyn std::error::Error>>;
pub const PURE: &str = "function main() -> Unit\n return ()\nend function\n";
pub struct Fixture {
    pub root: PathBuf,
    pub project: PathBuf,
    pub paths: DataPaths,
    pub context: ToolContext,
    pub session: Session,
    pub tools: ToolSet,
    pub confirmed: BTreeMap<ConfirmKey, BTreeSet<Effect>>,
}
impl Fixture {
    pub fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let root = std::env::temp_dir().join(format!(
            "bh-run-{}",
            SessionId::generate(SystemTime::now())?
        ));
        std::fs::create_dir(&root)?;
        let root = root.canonicalize()?;
        let project = root.join("project");
        std::fs::create_dir(&project)?;
        let paths = DataPaths::from_values(
            Some(&root.join("home")),
            Some(&root.join("config")),
            Some(&root.join("state")),
            Some(&root.join("tmp")),
        )?;
        std::fs::create_dir_all(&paths.temporary_root)?;
        let session = Session::create(&paths, SessionId::generate(SystemTime::now())?)?;
        let spill = session.directory().join("spill");
        let context = ToolContext::new(
            Mode::Restricted,
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
        .with_check_query(crate::support::checks(project.clone())?);
        Ok(Self {
            root,
            project,
            paths,
            session,
            context,
            tools: ToolSet::new(Mode::Restricted),
            confirmed: BTreeMap::new(),
        })
    }
    pub fn sandbox(&mut self) -> TestResult {
        let scratch = self.session.directory().join("sandbox");
        let placeholders = self.paths.data_dir.join("placeholders");
        std::fs::create_dir_all(&scratch)?;
        std::fs::create_dir_all(&placeholders)?;
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&scratch, std::fs::Permissions::from_mode(0o700))?;
        std::fs::set_permissions(&placeholders, std::fs::Permissions::from_mode(0o700))?;
        let processor = crate::support::benitoite()?;
        self.context.run = Some(RunContext {
            test_timeout: std::time::Duration::from_secs(120),
            processor,
            helper: std::fs::canonicalize(env!("CARGO_BIN_EXE_benitoiteh"))?,
            scratch,
            placeholders,
            env: std::env::vars_os().collect(),
            locations: WritableLocations {
                project: self.project.clone(),
                temporary: self.session.temporary_dir().into(),
                additional: vec![],
            },
            denied_candidates: vec![],
            allowed_programs: vec![],
        });
        Ok(())
    }
    pub fn exclude(&mut self, patterns: &[&str]) -> TestResult {
        self.context.exclude = ExcludeRules::new(
            &self.project,
            &self.paths,
            &self.session.directory().join("spill"),
            &ReadConfig {
                exclude: patterns.iter().map(|s| (*s).into()).collect(),
            },
            &ReadConfig::default(),
        )?;
        Ok(())
    }
    pub fn query(&mut self, query: Arc<CheckQuery>) {
        self.context.check_query = Some(query);
    }
    pub fn call(
        &self,
        args: Value,
        approved: bool,
        id: &str,
        cancelled: &AtomicBool,
    ) -> ToolOutput {
        let call = match self.tools.validate("benitoite_run", args) {
            Ok(c) => c,
            Err(e) => return ToolOutput::error(e.to_string()),
        };
        self.tools.execute_validated(
            &self.context,
            &call,
            if approved {
                ApprovalStatus::Approved
            } else {
                ApprovalStatus::Unapproved
            },
            Invocation {
                call_id: id,
                cancelled,
                confirmed_effects: &self.confirmed,
                session_domains: &BTreeSet::new(),
            },
        )
    }
    pub fn confirm(&mut self, output: &ToolOutput) -> TestResult {
        let request = output.approval.as_ref().ok_or("no approval")?;
        let key = ConfirmKey::from_facts(request.kind, &request.facts).ok_or("no key")?;
        self.confirmed.entry(key).or_default().extend(
            request
                .facts
                .effects
                .iter()
                .filter_map(|s| Effect::from_name(s)),
        );
        Ok(())
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _removed = std::fs::remove_dir_all(&self.root);
    }
}
