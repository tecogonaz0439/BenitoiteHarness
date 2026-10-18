//! 道具を最大八つで並べ、書き込みを境に順序を守る（設計書 01-01「道具の実行」）。

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::SyncSender;
use std::thread;

use crate::approval::{self, ApprovalAnswer, ApprovalOutcome, ApprovalRecord, Decision};
use crate::provider::{FinishReason, Item, ToolCall, ToolResult};
use crate::session::{Record, Summary};
use crate::tools::{ApprovalStatus, Invocation, ToolOutput, ValidatedCall};

use super::{Agent, AgentError, AgentEvent, text};

struct Prepared {
    call: ToolCall,
    validated: Result<ValidatedCall, String>,
    parallel: bool,
}

struct ProjectChange {
    tools: crate::tools::PreparedProjectSwitch,
    checkpoints: Option<crate::rewind::CheckpointRecorder>,
    environment: crate::context::Environment,
    environment_changes: Option<String>,
}

impl Agent {
    pub(super) fn execute_calls(
        &mut self,
        calls: &[ToolCall],
        finish: &FinishReason,
        cancelled: &AtomicBool,
        events: &SyncSender<AgentEvent>,
        summary: &mut Summary,
    ) -> Result<(), AgentError> {
        let mut prepared = Vec::new();
        for call in calls {
            let arguments = if call.arguments.is_null() {
                serde_json::from_str(&call.raw_arguments)
                    .map_err(|_| text::INVALID_ARGUMENTS.to_owned())
            } else {
                Ok(call.arguments.clone())
            };
            let validated = arguments.and_then(|arguments| {
                self.tools
                    .validate(&call.name, arguments)
                    .map_err(|error| error.to_string())
            });
            let parallel = validated.as_ref().is_ok_and(|validated| {
                self.tools
                    .definitions()
                    .iter()
                    .any(|definition| definition.id == validated.id() && definition.parallel)
            });
            self.writer.append(&Record::ToolCall {
                turn: self.turn,
                call: call.clone(),
                validation_error: validated.as_ref().err().cloned(),
            })?;
            self.emit(
                events,
                AgentEvent::ToolCall {
                    turn: self.turn,
                    call_id: call.id.clone(),
                    name: call.name.clone(),
                    arguments: call.arguments.clone(),
                },
            )?;
            if validated.is_ok() {
                summary.add_tool_call(&call.name);
            }
            prepared.push(Prepared {
                call: call.clone(),
                validated,
                parallel,
            });
        }
        let mut remaining = prepared.as_slice();
        while let Some(first) = remaining.first() {
            let suppressed = if self.stop_run.is_some() {
                Some(text::LEFTOVER_PROCESSES)
            } else if cancelled.load(Ordering::Acquire) {
                Some(text::CANCELLED)
            } else if *finish == FinishReason::OutputLimit {
                Some(text::OUTPUT_LIMIT)
            } else if self.token_limit_reached(summary) {
                Some(text::TOKEN_LIMIT)
            } else if matches!(finish, FinishReason::Refusal(_)) {
                Some(text::CANCELLED)
            } else {
                None
            };
            if let Some(message) = suppressed {
                self.result(&first.call, ToolOutput::error(message), events, summary)?;
                remaining = remaining.get(1..).ok_or(AgentError::Worker)?;
                continue;
            }
            // 確認しうる write/edit と将来の bash は定義で並行不可にする。
            // 並行の経路は確認を扱わない（設計書 03-03「確認の共通の規則」）。
            if first.parallel {
                let count = remaining
                    .iter()
                    .take_while(|call| call.parallel)
                    .take(8)
                    .count();
                let (batch, rest) = remaining.split_at(count);
                let tools = &self.tools;
                let context = &self.tool_context;
                let session_domains = &self.session_domains;
                let confirmed_effects = &self.confirmed_effects;
                let outputs = thread::scope(|scope| {
                    let mut workers = Vec::new();
                    let mut failed = false;
                    for prepared in batch {
                        let Ok(validated) = prepared.validated.as_ref() else {
                            failed = true;
                            break;
                        };
                        match thread::Builder::new().spawn_scoped(scope, move || {
                            if cancelled.load(Ordering::Acquire) {
                                ToolOutput::error(text::CANCELLED)
                            } else {
                                tools.execute_validated(
                                    context,
                                    validated,
                                    ApprovalStatus::Unapproved,
                                    Invocation {
                                        call_id: &prepared.call.id,
                                        cancelled,
                                        session_domains,
                                        confirmed_effects,
                                    },
                                )
                            }
                        }) {
                            Ok(worker) => workers.push(worker),
                            Err(_) => {
                                failed = true;
                                break;
                            }
                        }
                    }
                    // 一つが失敗しても全員を join する。scope の暗黙の join に
                    // パニックを伝えない（設計書 06-01「lint と実装の規約」）。
                    let joined = workers
                        .into_iter()
                        .map(|worker| worker.join().map_err(|_| AgentError::Worker))
                        .collect::<Vec<_>>();
                    if failed {
                        return Err(AgentError::Worker);
                    }
                    joined.into_iter().collect::<Result<Vec<_>, AgentError>>()
                })?;
                // join の順序と会話の順序を応答に揃える。完了時刻は順序に使わない。
                for (prepared, output) in batch.iter().zip(outputs) {
                    self.result(&prepared.call, output, events, summary)?;
                }
                remaining = rest;
            } else {
                let output = match &first.validated {
                    Ok(validated) => {
                        // 書く道具は呼び出しの実行前に flush する。
                        // （設計書 01-05「セッションの記録」）。
                        self.writer.append(&Record::ToolStart {
                            call: first.call.clone(),
                        })?;
                        let output = self.tools.execute_validated(
                            &self.tool_context,
                            validated,
                            ApprovalStatus::Unapproved,
                            Invocation {
                                call_id: &first.call.id,
                                cancelled,
                                session_domains: &self.session_domains,
                                confirmed_effects: &self.confirmed_effects,
                            },
                        );
                        self.approve(&first.call, validated, output, cancelled, events, summary)?
                    }
                    Err(message) => ToolOutput::error(message.clone()),
                };
                self.result(&first.call, output, events, summary)?;
                remaining = remaining.get(1..).ok_or(AgentError::Worker)?;
            }
        }
        Ok(())
    }

    fn approve(
        &mut self,
        call: &ToolCall,
        validated: &ValidatedCall,
        output: ToolOutput,
        cancelled: &AtomicBool,
        events: &SyncSender<AgentEvent>,
        summary: &mut Summary,
    ) -> Result<ToolOutput, AgentError> {
        let Some(request) = output.approval else {
            if matches!(
                validated.id(),
                crate::tools::ToolId::BenitoiteRun
                    | crate::tools::ToolId::BenitoiteTest
                    | crate::tools::ToolId::BenitoiteScratch
            ) && output.processor_launches >= 2
            {
                self.effects_denied = 0;
            }
            return Ok(output);
        };
        summary.checks = summary.checks.saturating_add(output.processor_launches);
        if request.kind == approval::ApprovalKind::CreateProject && self.tool_context.noninteractive
        {
            let permitted = self.config.project_parent.as_deref().is_some_and(|parent| {
                request
                    .facts
                    .path
                    .as_deref()
                    .and_then(std::path::Path::parent)
                    == Some(parent)
            });
            if permitted {
                return Ok(self.execute_approved(call, validated, cancelled));
            }
            summary.approvals.policy_denied = summary.approvals.policy_denied.saturating_add(1);
            return Ok(ToolOutput::error(approval::policy_denial_message(
                request.kind,
            )));
        }
        if matches!(
            request.kind,
            approval::ApprovalKind::Effects | approval::ApprovalKind::TestEffects
        ) && let Some(allowed) = &self.config.allow_effects
        {
            if request.facts.unread_imports > 0 {
                summary.approvals.policy_denied = summary.approvals.policy_denied.saturating_add(1);
                return Ok(ToolOutput::error(approval::unread_imports_denied(
                    &request.facts,
                    self.tool_context.project.real_path(),
                )));
            }
            let explicitly_allowed = allowed;
            let mut allowed = allowed.clone();
            if validated.id() == crate::tools::ToolId::BenitoiteScratch {
                allowed.extend(
                    self.tool_context
                        .run
                        .as_ref()
                        .map(|run| run.scratch_auto_effects.clone())
                        .unwrap_or_else(|| crate::benitoite::effects::scratch_auto_effects(&[])),
                );
            }
            let effects = request
                .facts
                .effects
                .iter()
                .map(|s| crate::benitoite::Effect::from_name(s))
                .collect::<Option<std::collections::BTreeSet<_>>>();
            if let Some(effects) = effects.filter(|set| {
                set.iter().all(|effect| {
                    *effect == crate::benitoite::Effect::AssertCheck || allowed.contains(effect)
                })
            }) {
                let key = crate::tools::ConfirmKey::from_facts(request.kind, &request.facts)
                    .ok_or(AgentError::Worker)?;
                let confirmed = if validated.id() == crate::tools::ToolId::BenitoiteScratch {
                    self.tool_context
                        .remember_scratch_automatic(call.id.as_str(), &effects, explicitly_allowed)
                        .map_err(|_| AgentError::Worker)?;
                    effects.intersection(explicitly_allowed).copied().collect()
                } else {
                    allowed
                };
                self.confirmed_effects
                    .entry(key)
                    .or_default()
                    .extend(confirmed);
                self.effects_denied = 0;
                return Ok(self.execute_approved(call, validated, cancelled));
            }
            self.effects_denied = self.effects_denied.saturating_add(1);
            summary.approvals.policy_denied = summary.approvals.policy_denied.saturating_add(1);
            return Ok(ToolOutput::error(approval::allow_denied(
                request.kind,
                &request.facts,
                self.tool_context.project.real_path(),
                &allowed,
            )));
        }
        match approval::decide_in_mode(
            self.config.mode,
            &self.approval_policy,
            request.kind,
            &request.facts,
            &self.allowed_commands,
            &self.session_domains,
        ) {
            Decision::Allow => return Ok(self.execute_approved(call, validated, cancelled)),
            Decision::Deny => {
                summary.approvals.policy_denied = summary.approvals.policy_denied.saturating_add(1);
                return Ok(ToolOutput::error(approval::policy_denial_message(
                    request.kind,
                )));
            }
            Decision::Ask => {}
        }
        let record = ApprovalRecord {
            call_id: call.id.clone(),
            kind: request.kind,
            facts: request.facts,
            justification: request.justification,
            summary: request.summary,
            decision: None,
            user_message: None,
        };
        self.emit(
            events,
            AgentEvent::ApprovalRequest {
                call_id: record.call_id.clone(),
                kind: record.kind,
                facts: record.facts.clone(),
                justification: record.justification.clone(),
                summary: record.summary.clone(),
                user_message: None,
            },
        )?;
        let answer = if cancelled.load(Ordering::Acquire) {
            ApprovalAnswer {
                outcome: ApprovalOutcome::Cancelled,
                message: None,
            }
        } else {
            self.approver.approve(&record)
        };
        answer.remember_command(&record, &mut self.allowed_commands);
        answer.remember_domain(&record, &mut self.session_domains);
        answer.remember_effects(&record, &mut self.confirmed_effects);
        summary.add_approval(answer.outcome);
        self.emit(
            events,
            AgentEvent::ApprovalResult {
                call_id: record.call_id,
                kind: record.kind,
                facts: record.facts.clone(),
                justification: record.justification,
                summary: record.summary,
                decision: answer.outcome,
                user_message: answer.message.clone(),
            },
        )?;
        Ok(match answer.outcome {
            ApprovalOutcome::Approved | ApprovalOutcome::ApprovedForSession => {
                self.execute_approved(call, validated, cancelled)
            }
            ApprovalOutcome::Denied => ToolOutput::error(approval::user_denial_message(
                record.kind,
                &record.facts,
                self.tool_context.project.real_path(),
                None,
            )),
            ApprovalOutcome::DeniedWithMessage => ToolOutput::error(approval::user_denial_message(
                record.kind,
                &record.facts,
                self.tool_context.project.real_path(),
                answer.message.as_deref(),
            )),
            ApprovalOutcome::Cancelled => ToolOutput::error(text::CANCELLED),
        })
    }

    fn execute_approved(
        &self,
        call: &ToolCall,
        validated: &ValidatedCall,
        cancelled: &AtomicBool,
    ) -> ToolOutput {
        if cancelled.load(Ordering::Acquire) {
            ToolOutput::error(text::CANCELLED)
        } else {
            self.tools.execute_validated(
                &self.tool_context,
                validated,
                ApprovalStatus::Approved,
                Invocation {
                    call_id: &call.id,
                    cancelled,
                    session_domains: &self.session_domains,
                    confirmed_effects: &self.confirmed_effects,
                },
            )
        }
    }

    fn prepare_project_change(
        &self,
        root: crate::safe_fs::ScopeRoot,
    ) -> Result<ProjectChange, AgentError> {
        let writable = vec![
            root.real_path().into(),
            self.tool_context.temporary.real_path().into(),
        ];
        let environment = self
            .environment
            .clone()
            .with_project(root.real_path(), &writable)?;
        let environment_changes = environment.changes_since(&self.environment);
        let tools = self
            .tool_context
            .prepare_project_switch(root)
            .map_err(|_| AgentError::Worker)?;
        let checkpoints = self
            .tool_context
            .checkpoints
            .as_ref()
            .map(|recorder| recorder.prepare_restart(&tools.project, tools.exclude.clone()))
            .transpose()?;
        Ok(ProjectChange {
            tools,
            checkpoints,
            environment,
            environment_changes,
        })
    }

    fn result(
        &mut self,
        call: &ToolCall,
        mut output: ToolOutput,
        events: &SyncSender<AgentEvent>,
        summary: &mut Summary,
    ) -> Result<(), AgentError> {
        if let Some(root) = self
            .tool_context
            .take_created_project(&call.id)
            .map_err(|_| AgentError::Worker)?
        {
            self.confirmed_effects.clear();
            self.check_failures.clear();
            self.check_limit.clear();
            let project_dir = root.real_path().to_path_buf();
            match self.prepare_project_change(root) {
                Ok(prepared) => {
                    let event = AgentEvent::ProjectDirChanged { project_dir };
                    if let Some(record) = event.record() {
                        self.writer.append(&record)?;
                    }
                    // 準備の失敗では、元の根・時点・環境を保つ。出来事を記録した後は
                    // Result を返さない状態の置き換えだけを行う（設計書 01-03「create_project（制限モード）」）。
                    self.tool_context.switch_project(prepared.tools);
                    self.tool_context.checkpoints = prepared.checkpoints;
                    self.environment = prepared.environment;
                    self.environment_changes = prepared.environment_changes;
                    // 受信側が閉じても、記録と内部のプロジェクトは一致したまま終える。
                    events.send(event).map_err(|_| AgentError::ReceiverClosed)?;
                }
                Err(error) => {
                    output = ToolOutput::error(crate::tools::text::project_switch_failed(
                        &project_dir,
                        &error,
                    ));
                }
            }
        }
        if output.policy_denied {
            summary.approvals.policy_denied = summary.approvals.policy_denied.saturating_add(1);
        }
        self.checkpoint_notices(events)?;
        summary.checks = summary.checks.saturating_add(output.processor_launches);
        if call.name == "benitoite_scratch" {
            self.tool_context.scratch_result(&call.id, &mut output);
        }
        if let Some(message) = output.stop_run.take() {
            self.stop_run = Some(message);
        }
        if let Some(launch) = output.check_launch {
            summary.checks = summary.checks.saturating_add(1);
            if launch == crate::tools::CheckLaunch::AfterWrite {
                summary.after_write_checks = summary.after_write_checks.saturating_add(1);
            }
        }
        if let Some(check) = &output.check {
            if check.failed {
                summary.fixes = summary.fixes.saturating_add(1);
            }
            if self.config.mode == crate::config::Mode::Restricted {
                let count = self.check_failures.entry(check.key.clone()).or_default();
                *count = if check.failed {
                    count.saturating_add(1)
                } else {
                    0
                };
                if *count >= self.config.limits.max_check_fix {
                    self.check_limit
                        .insert(check.key.clone(), check.diagnostics.clone());
                } else {
                    self.check_limit.remove(&check.key);
                }
            }
        }
        let arguments = self
            .tools
            .validate(&call.name, call.arguments.clone())
            .map(|validated| validated.arguments().clone())
            .unwrap_or_else(|_| call.arguments.clone());
        let count = match &self.repeated {
            Some((name, previous, count)) if *name == call.name && *previous == arguments => {
                count.saturating_add(1)
            }
            Some(_) | None => 1,
        };
        self.repeated = Some((call.name.clone(), arguments, count));
        if [3, 5, 8].contains(&count) {
            output.text.push('\n');
            output.text.push_str(&text::repeated(count));
        }
        output.text = crate::tools::truncate::cap_middle(&output.text);
        let result = ToolResult {
            id: call.id.clone(),
            provider_id: call.provider_id.clone(),
            text: output.text,
            is_error: output.is_error,
        };
        self.writer.append(&Record::ToolResult {
            turn: self.turn,
            name: call.name.clone(),
            result: result.clone(),
            spill: output.spill,
            stderr_spill: output.stderr_spill,
        })?;
        self.emit(
            events,
            AgentEvent::ToolResult {
                turn: self.turn,
                call_id: call.id.clone(),
                name: call.name.clone(),
                screen: output.screen,
                display: output.display,
                is_error: result.is_error,
                summary: result.text.lines().next().unwrap_or_default().to_owned(),
            },
        )?;
        if !result.is_error {
            self.files.add_call(call);
        }
        self.push_item(Item::ToolResult(result));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Limits, Mode, ReadConfig, paths::DataPaths};
    use crate::context::Environment;
    use crate::http::HttpTimeouts;
    use crate::provider::{AdapterKind, Capabilities, Destination};
    use crate::safe_fs::ScopeRoot;
    use crate::session::{Session, SessionId, read_records};
    use crate::tools::{FileVersion, ToolContext, exclude::ExcludeRules};
    use std::error::Error;
    use std::fs;
    use std::path::PathBuf;
    use std::sync::mpsc;
    use std::time::{Duration, SystemTime};

    struct Fixture(PathBuf);
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _removed = fs::remove_dir_all(&self.0);
        }
    }
    struct RestoreLimit(rustix::process::Rlimit);
    impl Drop for RestoreLimit {
        fn drop(&mut self) {
            let _restored = rustix::process::setrlimit(rustix::process::Resource::Nofile, self.0);
        }
    }

    // 関門: 実際の記述子不足で restart を失敗させる。記録後の半端な切り替えと、
    // 失敗を道具の結果にせず番を止める退行を、結果・根・版・時点・記録で検出する。
    #[test]
    fn project_switch_preparation_failure_keeps_the_previous_project() -> Result<(), Box<dyn Error>>
    {
        const CHILD: &str = "BENITOITEH_TEST_PROJECT_SWITCH_FAILURE";
        if std::env::var_os(CHILD).is_none() {
            let output = std::process::Command::new(std::env::current_exe()?)
                .args(["--exact", "agent::execution::tests::project_switch_preparation_failure_keeps_the_previous_project", "--nocapture"])
                .env(CHILD, "1").output()?;
            assert!(
                output.status.success(),
                "{}\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            return Ok(());
        }
        let id = SessionId::generate(SystemTime::now())?;
        let root = std::env::temp_dir().join(format!("bh-switch-failure-{id}"));
        fs::create_dir(&root)?;
        let f = Fixture(root.canonicalize()?);
        for name in ["home", "project", "tmp", "created"] {
            fs::create_dir(f.0.join(name))?;
        }
        let paths = DataPaths::from_values(
            Some(&f.0.join("home")),
            Some(&f.0.join("config")),
            Some(&f.0.join("data")),
            Some(&f.0.join("tmp")),
        )?;
        let session = Session::create(&paths, id)?;
        let project = f.0.join("project");
        let created = f.0.join("created");
        fs::write(project.join("old.bnt"), "old")?;
        fs::write(created.join("main.bnt"), "created")?;
        let spill = session.directory().join("spill");
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
        );
        let config = crate::agent::AgentConfig {
            project_parent: None,
            allow_effects: None,
            mode: Mode::Normal,
            destination: Destination {
                provider: "mock".into(),
                base_url: "https://mock.invalid".into(),
                model: "mock".into(),
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
            &[project.clone(), session.temporary_dir().into()],
            false,
            SystemTime::now(),
        )?;
        let mut agent = Agent::new(
            config,
            String::new(),
            context,
            environment.clone(),
            session.clone(),
        )?;
        let version = FileVersion::from_metadata(&fs::metadata(project.join("old.bnt"))?);
        agent
            .tool_context
            .record_read_version(project.join("old.bnt"), version.clone())?;
        let checkpoint = agent
            .tool_context
            .checkpoints
            .as_ref()
            .ok_or("recorder")?
            .capture(
                crate::rewind::CheckpointReason::Request,
                None,
                Some(0),
                &AtomicBool::new(false),
            )?;
        let call = ToolCall {
            id: "create".into(),
            provider_id: Some("create".into()),
            provider_item_id: None,
            name: "create_project".into(),
            arguments: serde_json::json!({"path":created}),
            raw_arguments: String::new(),
        };
        // 作成と承認の試験は道具の境界が守る。ここでは、作成済みの根を受けた切り替えを確かめる。
        agent
            .tool_context
            .created_projects
            .lock()
            .map_err(|_| "created projects")?
            .insert(call.id.clone(), ScopeRoot::open(&created)?);
        let (sender, receiver) = mpsc::sync_channel(64);
        let mut summary = Summary::default();
        let limit = RestoreLimit(rustix::process::getrlimit(
            rustix::process::Resource::Nofile,
        ));
        rustix::process::setrlimit(
            rustix::process::Resource::Nofile,
            rustix::process::Rlimit {
                current: Some(0),
                maximum: limit.0.maximum,
            },
        )?;
        let result = agent.result(&call, ToolOutput::success("created"), &sender, &mut summary);
        drop(limit);
        result?;
        // Linux では、UTF-8 にできない一時領域の実パスで環境の準備も失敗させる。
        // 失敗の口を本番へ加えず、準備が受け取る本物の値で確かめる。
        #[cfg(target_os = "linux")]
        {
            use std::os::unix::ffi::OsStringExt;
            let raw = f.0.join(std::ffi::OsString::from_vec(vec![b't', 0xff]));
            fs::create_dir(&raw)?;
            agent.tool_context.temporary = ScopeRoot::open(&raw)?;
            agent
                .tool_context
                .created_projects
                .lock()
                .map_err(|_| "created projects")?
                .insert(call.id.clone(), ScopeRoot::open(&created)?);
            agent.result(&call, ToolOutput::success("created"), &sender, &mut summary)?;
        }
        assert_eq!(agent.status().project, project);
        assert_eq!(agent.environment, environment);
        assert_eq!(
            agent.tool_context.read_version(&project.join("old.bnt"))?,
            Some(version)
        );
        assert!(
            agent
                .checkpoints()?
                .iter()
                .any(|c| c.checkpoint.id == checkpoint.id)
        );
        assert_eq!(fs::read_to_string(created.join("main.bnt"))?, "created");
        let records = read_records(session.directory())?;
        assert!(
            !records
                .iter()
                .any(|r| matches!(r, Record::ProjectDirChanged { .. }))
        );
        assert!(records.iter().any(|r| matches!(r, Record::ToolResult { result, .. } if result.is_error && result.text.contains("could not switch") && result.text.contains("current project is unchanged"))));
        drop(sender);
        assert!(
            !receiver
                .into_iter()
                .any(|event| matches!(event, AgentEvent::ProjectDirChanged { .. }))
        );
        Ok(())
    }
}
