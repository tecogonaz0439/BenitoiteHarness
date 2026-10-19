//! 設定と安全の検査を終えてからセッションとループを起動する（設計書 05-01「exec の出力」「終了状態」）。

use std::fmt;
use std::fs::{DirBuilder, OpenOptions};
use std::io;
use std::os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::thread;
use std::time::SystemTime;

use crate::agent::{Agent, AgentError, Outcome};
use crate::approval::{ApprovalPolicy, NonInteractiveApprover};
use crate::benitoite::{self, Processor, QueryContext, QueryLauncher};
use crate::config::credentials::CredentialError;
use crate::config::paths::DataPaths;
use crate::config::writable::{WritableError, validate_writable_with_executable};
use crate::config::{
    Approval, ConfigError, Mode, ProjectConfig, ReadConfig, Sandbox, UserConfig, child_environment,
};
use crate::context::{ContextError, Environment, instructions, skills};
use crate::provider::{AssistantPart, Item};
use crate::safe_fs::{self, SafeFsError, ScopeRoot};
use crate::sandbox::{self, SandboxError};
use crate::session::{self, Record, Session, SessionError, SessionId};
use crate::tools::{ShellContext, ToolContext, ToolError, exclude::ExcludeRules};
use crate::ui::ExecOutput;

use super::{CliError, args::ExecArgs, panic, signals::Signals, text};

pub(super) enum StartupError {
    Benitoite(benitoite::BenitoiteError),
    Skill(benitoite::BenitoiteError),
    Config(ConfigError),
    Credentials(CredentialError),
    Writable(WritableError),
    SafeFs(SafeFsError),
    Session(SessionError),
    Context(ContextError),
    Tools(ToolError),
    Agent(AgentError),
    Io(io::Error),
    Sandbox(SandboxError),
    Invalid(&'static str),
    Declined,
}

impl fmt::Display for StartupError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Benitoite(e) => e.fmt(f),
            Self::Skill(e) => write!(
                f,
                "{}.",
                benitoite::skill::query_reason(e).trim_end_matches('.')
            ),
            Self::Config(e) => e.fmt(f),
            Self::Credentials(e) => e.fmt(f),
            Self::Writable(e) => e.fmt(f),
            Self::SafeFs(e) => e.fmt(f),
            Self::Session(e) => e.fmt(f),
            Self::Context(e) => e.fmt(f),
            Self::Tools(e) => e.fmt(f),
            Self::Agent(e) => e.fmt(f),
            Self::Io(e) => e.fmt(f),
            Self::Sandbox(e) => e.fmt(f),
            Self::Invalid(message) => f.write_str(message),
            Self::Declined => f.write_str(text::MODE_CHANGE_DECLINED),
        }
    }
}

/// 起動の誤りと、失敗する前に積んだ診断。raw の開始前にも呼び出し側で示す。
pub(super) struct PreparationError {
    pub error: StartupError,
    pub diagnostics: Vec<String>,
}

impl fmt::Display for PreparationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.error.fmt(f)
    }
}

/// 入力や端末への出力を行わない起動の結果。/new でも作り直せる。
pub(super) struct Prepared {
    pub agent: Agent,
    pub session: Session,
    pub status: crate::ui::tui::Status,
    pub diagnostics: Vec<String>,
    pub panic_record: std::fs::File,
}

/// 記録した制限モードから通常モードへの変更。
pub(super) struct ModeChange {
    pub session_id: SessionId,
}

/// /new で固定する安全の値。設定の既定値より優先する。
#[derive(Clone, Debug)]
pub(super) struct SessionOverrides {
    mode: Mode,
    project: PathBuf,
    sandbox: Sandbox,
    sandbox_disabled: bool,
    approval: Approval,
    safety: crate::agent::SessionSafety,
    benitoite_path: Option<PathBuf>,
}

impl From<&crate::agent::AgentStatus> for SessionOverrides {
    fn from(current: &crate::agent::AgentStatus) -> Self {
        Self {
            mode: current.mode,
            project: current.project.clone(),
            sandbox: current.sandbox,
            sandbox_disabled: current.sandbox_disabled,
            approval: current.approval,
            safety: current.safety.clone(),
            benitoite_path: current.benitoite_path.clone(),
        }
    }
}

type InitializedProcessor = (Arc<Processor>, Arc<benitoite::skill::SkillDocuments>);
type OptionalProcessor = (
    Option<Arc<Processor>>,
    Option<Arc<benitoite::skill::SkillDocuments>>,
);

fn initialize_skill(processor: Processor) -> Result<InitializedProcessor, StartupError> {
    let processor = Arc::new(processor);
    let documents = Arc::new(benitoite::skill::SkillDocuments::from_processor(
        Arc::clone(&processor),
    ));
    let skill = documents
        .document("SKILL.md", &AtomicBool::new(false))
        .map_err(StartupError::Skill)?;
    benitoite::skill::skill_parts(skill.text()).map_err(StartupError::Skill)?;
    Ok((processor, documents))
}

fn select_processor(
    mode: Mode,
    result: Result<InitializedProcessor, StartupError>,
    diagnostics: &mut Vec<String>,
) -> Result<OptionalProcessor, StartupError> {
    match result {
        Ok((processor, skill)) => Ok((Some(processor), Some(skill))),
        Err(error) if mode == Mode::Normal => {
            diagnostics.push(format!("{} {error}", benitoite::text::DISABLED));
            Ok((None, None))
        }
        Err(error) => Err(error),
    }
}

/// 依頼を読まず、診断と panic 用の記述子を呼び出す側へ返す。
pub(super) fn prepare(
    args: &ExecArgs,
    overrides: Option<&SessionOverrides>,
    paths: &DataPaths,
    check_sandbox: impl FnOnce(&Path, &Path) -> Result<(), SandboxError>,
    query_launcher: QueryLauncher,
    confirm_mode_change: &mut dyn FnMut(&ModeChange) -> bool,
) -> Result<Prepared, PreparationError> {
    let mut diagnostics = Vec::new();
    let result = (|| {
        safe_fs::verify_platform_support(&paths.temporary_root).map_err(StartupError::SafeFs)?;
        let mut user = UserConfig::load(&paths.config_file).map_err(StartupError::Config)?;
        let mut args = args.clone();
        if let Some(fixed) = overrides {
            user.benitoite.path.clone_from(&fixed.benitoite_path);
            args.mode = Some(fixed.mode);
            args.project = Some(fixed.project.clone());
            user.normal.network = fixed.safety.network;
            user.normal
                .writable_roots
                .clone_from(&fixed.safety.writable_roots);
            user.restricted
                .allowed_programs
                .clone_from(&fixed.safety.allowed_programs);
            user.restricted
                .scratch_auto_effects
                .clone_from(&fixed.safety.scratch_auto_effects);
            if fixed.mode == Mode::Normal {
                args.sandbox = Some(fixed.sandbox);
                args.no_sandbox = fixed.sandbox_disabled;
                args.approval = Some(fixed.approval);
            } else {
                args.sandbox = None;
                args.approval = None;
                args.no_sandbox = false;
            }
        }
        let args = &args;
        let cwd = std::env::current_dir().map_err(StartupError::Io)?;
        let opened = if let Some(id) = &args.resume {
            let id = id.parse::<SessionId>().map_err(StartupError::Session)?;
            Some(Session::open(paths, &id).map_err(StartupError::Session)?)
        } else if args.continue_session {
            Some(
                session::continue_session(paths, args.project.as_deref().unwrap_or(&cwd))
                    .map_err(StartupError::Session)?,
            )
        } else {
            None
        };
        let resume = opened
            .as_ref()
            .map(Session::take_resume_state)
            .transpose()
            .map_err(StartupError::Session)?;
        if let (Some(state), Some(project)) = (&resume, &args.project) {
            let requested = std::fs::canonicalize(project).map_err(StartupError::Io)?;
            let recorded = std::fs::canonicalize(&state.project_dir).map_err(StartupError::Io)?;
            if requested != recorded {
                return Err(StartupError::Invalid(text::RESUME_PROJECT));
            }
        }
        let requested = resume
            .as_ref()
            .map(|state| state.project_dir.as_path())
            .or(args.project.as_deref())
            .unwrap_or(&cwd);
        let root = ScopeRoot::open(requested).map_err(StartupError::SafeFs)?;
        let project = ProjectConfig::load(&root);
        if project.read {
            diagnostics.push(text::project_loaded(root.real_path()));
        }
        if let Some(error) = &project.error {
            diagnostics.push(text::project_ignored(error));
        }
        let resolved = match &resume {
            Some(state) => {
                user.resolve_resume(args, &project.config, &cwd, state, args.interactive)
            }
            None => user.resolve_exec(args, &project.config, &cwd),
        }
        .map_err(StartupError::Config)?;
        if resolved.mode == Mode::Normal && args.allow.is_some() {
            return Err(StartupError::Invalid(text::ALLOW_RESTRICTED));
        }
        let project_parent = match &args.project_parent {
            Some(parent) => {
                if args.interactive || resolved.mode != Mode::Restricted {
                    return Err(StartupError::Invalid(text::PROJECT_PARENT_RESTRICTED));
                }
                Some(
                    ScopeRoot::open(parent)
                        .map_err(|_| StartupError::Invalid(text::PROJECT_PARENT_DIRECTORY))?
                        .real_path()
                        .to_path_buf(),
                )
            }
            None => None,
        };
        if let Some(state) = &resume
            && state.mode == crate::session::RecordedMode::Restricted
            && resolved.mode == Mode::Normal
            && !confirm_mode_change(&ModeChange {
                session_id: opened
                    .as_ref()
                    .ok_or(StartupError::Invalid(text::RESUME_PROJECT))?
                    .id()
                    .clone(),
            })
        {
            return Err(StartupError::Declined);
        }
        let approval = if args.interactive {
            if resolved.mode == Mode::Restricted {
                Approval::OnRequest
            } else {
                args.approval.unwrap_or(user.normal.approval)
            }
        } else {
            Approval::Never
        };
        diagnostics.push(text::destination(&resolved.destination));
        let key = resolved
            .api_key
            .as_ref()
            .map(|key| key.resolve())
            .transpose()
            .map_err(StartupError::Credentials)?;
        let now = SystemTime::now();
        let id = match &opened {
            Some(session) => session.id().clone(),
            None => SessionId::generate(now).map_err(StartupError::Session)?,
        };
        let temporary = paths
            .temporary_dir(id.as_str())
            .map_err(StartupError::Config)?;
        let additional = if resolved.mode == Mode::Normal {
            user.normal.writable_roots.as_slice()
        } else {
            &[]
        };
        let helper = std::env::current_exe()
            .and_then(std::fs::canonicalize)
            .map_err(StartupError::Io)?;
        let mut locations = validate_writable_with_executable(
            paths,
            root.real_path(),
            &temporary,
            additional,
            &helper,
        )
        .map_err(|error| match &error {
            WritableError::Rejected { path, .. } if path == root.real_path() => {
                StartupError::Io(io::Error::other(text::project_startup_error(
                    root.real_path(),
                    args.project.is_some(),
                    &error,
                )))
            }
            WritableError::Resolve { .. } | WritableError::Rejected { .. } => {
                StartupError::Writable(error)
            }
        })?;
        if let Some(fixed) = overrides
            && (locations.project != fixed.project
                || locations.additional != fixed.safety.writable_roots)
        {
            return Err(StartupError::Invalid(text::SESSION_PATH_CHANGED));
        }
        let allowed_programs = if resolved.mode == Mode::Restricted {
            user.restricted
                .allowed_programs
                .iter()
                .map(|path| {
                    benitoite::validate_executable(path, &locations, true).map(|(path, _)| path)
                })
                .collect::<Result<Vec<_>, _>>()
                .map_err(StartupError::Benitoite)?
        } else {
            Vec::new()
        };
        if let Some(fixed) = overrides
            && allowed_programs != fixed.safety.allowed_programs
        {
            return Err(StartupError::Invalid(text::SESSION_PATH_CHANGED));
        }
        let _cleanup = session::cleanup(paths, user.sessions.retention_days, now)
            .map_err(StartupError::Session)?;
        let session = match opened {
            Some(session) => {
                diagnostics.push(text::resuming(session.id(), root.real_path()));
                session
            }
            None => Session::create(paths, id).map_err(StartupError::Session)?,
        };
        let usage =
            session::storage_usage(paths, Some(session.id())).map_err(StartupError::Session)?;
        session.configure_budget(&usage);
        if let Some(warning) = usage.warning() {
            diagnostics.push(warning.to_owned());
        }
        // macOS の TMPDIR の /var という別名を Seatbelt に渡さない（設計書 03-02）。
        locations.temporary =
            std::fs::canonicalize(session.temporary_dir()).map_err(StartupError::Io)?;
        let scratch = private_directory(&session.directory().join("sandbox"))?;
        let placeholders = private_directory(&paths.data_dir.join("placeholders"))?;
        let candidates = super::denied_candidates(paths);
        let fixed_disabled = overrides.is_some_and(|fixed| fixed.sandbox_disabled);
        let availability = if fixed_disabled {
            Ok(())
        } else {
            check_sandbox(&helper, &scratch)
        };
        let selected = args.sandbox.unwrap_or(user.normal.sandbox);
        if (resolved.mode == Mode::Restricted
            || (selected != Sandbox::DangerFullAccess && !args.no_sandbox))
            && let Err(error) = availability
        {
            return Err(StartupError::Sandbox(error));
        }
        let mut sandbox_disabled = fixed_disabled;
        if resolved.mode == Mode::Normal && args.no_sandbox && availability.is_err() {
            if let Err(error) = &availability {
                diagnostics.push(text::sandbox_disabled(error));
            }
            sandbox_disabled = true;
        }
        let image_helper_available = availability.is_ok() && !sandbox_disabled;
        let launcher = select_query_launcher(resolved.mode, sandbox_disabled);
        let mut processor_notices = Vec::new();
        let processor_result = if let Err(error) = availability
            && !sandbox_disabled
        {
            Err(benitoite::BenitoiteError::Sandbox(error))
        } else if overrides.is_some_and(|fixed| fixed.benitoite_path.is_none()) {
            Err(benitoite::BenitoiteError::Invalid(
                benitoite::text::NOT_FOUND,
            ))
        } else {
            Processor::inspect(
                user.benitoite.path.as_deref(),
                std::env::var_os("PATH").as_deref(),
                QueryContext {
                    helper: helper.clone(),
                    scratch: scratch.clone(),
                    placeholders: placeholders.clone(),
                    env: child_environment(
                        std::env::vars_os(),
                        &user.referenced_env_vars(),
                        &locations.temporary,
                        launcher.environment_policy(),
                    ),
                    locations: locations.clone(),
                    denied_candidates: candidates.clone(),
                    launcher: match query_launcher {
                        // 起動の形の選択を終えた後にだけ、テスト実行ファイル用の形へ替える。
                        #[cfg(test)]
                        QueryLauncher::TestDirect => QueryLauncher::TestDirect,
                        QueryLauncher::Sandboxed | QueryLauncher::Unsandboxed => launcher,
                    },
                },
            )
        };
        let initialized = processor_result
            .map_err(StartupError::Benitoite)
            .and_then(initialize_skill);
        let (processor, skill) = select_processor(resolved.mode, initialized, &mut diagnostics)?;
        if let Some(processor) = &processor {
            if let Some(notice) = processor.version_warning() {
                processor_notices.push(notice);
            }
            let previous = if resume.is_some() {
                session::recorded_benitoite(paths, session.id())
            } else {
                session::previous_benitoite(paths, &locations.project, session.id())
            };
            let previous = match previous {
                Ok(previous) => previous,
                Err(SessionError::Json(_)) => {
                    diagnostics.push(benitoite::text::PREVIOUS_UNREADABLE.into());
                    None
                }
                Err(error) => return Err(StartupError::Session(error)),
            };
            if let Some(previous) = previous
                && !previous.sha256.is_empty()
                && previous.sha256 != processor.sha256
            {
                processor_notices.push(benitoite::text::changed(
                    &previous.version,
                    &processor.version,
                ));
            }
        }
        diagnostics.extend(processor_notices.iter().cloned());
        let shell = if resolved.mode == Mode::Normal {
            let policy = if selected == Sandbox::DangerFullAccess || sandbox_disabled {
                None
            } else {
                sandbox::normal_policy(selected, &locations, user.normal.network, &candidates)
                    .map_err(StartupError::Sandbox)?
            };
            let policy_name = policy.as_ref().map(|_| selected.name());
            let env = child_environment(
                std::env::vars_os(),
                &user.referenced_env_vars(),
                &locations.temporary,
                policy
                    .as_ref()
                    .map(|policy| (selected.name(), policy.config().allow_ip)),
            );
            // OS の制限が使えなくても、書く道具の確認には選んだ方針を使う。
            // （設計書 03-03「通常モードの承認の方針」）。
            let writable_roots = match selected {
                Sandbox::ReadOnly => vec![locations.temporary.clone()],
                Sandbox::WorkspaceWrite => {
                    let mut roots = vec![locations.project.clone(), locations.temporary.clone()];
                    roots.extend(locations.additional.iter().cloned());
                    roots
                }
                Sandbox::DangerFullAccess => Vec::new(),
            };
            Some(ShellContext {
                sandbox: selected,
                sandboxed: policy.is_some(),
                writable_roots,
                policy,
                policy_name,
                helper: helper.clone(),
                scratch: scratch.clone(),
                placeholders: placeholders.clone(),
                env,
                approval,
            })
        } else {
            None
        };
        let (writable, network) = if resolved.mode == Mode::Normal {
            match shell.as_ref().and_then(|shell| shell.policy.as_ref()) {
                Some(policy) => (policy.config().writable.clone(), policy.config().allow_ip),
                None => (Vec::new(), true),
            }
        } else {
            (
                vec![locations.project.clone(), locations.temporary.clone()],
                false,
            )
        };
        let panic_record = OpenOptions::new()
            .append(true)
            .open(session.directory().join("session.jsonl"))
            .map_err(StartupError::Io)?;
        let spill = session.directory().join("spill");
        let exclude = ExcludeRules::new(
            root.real_path(),
            paths,
            &spill,
            &resolved.read,
            &ReadConfig::default(),
        )
        .map_err(StartupError::Tools)?;
        let home = std::fs::canonicalize(&paths.home);
        let instructions = if resolved.mode == Mode::Normal && home.is_ok() {
            instructions::collect(
                resolved.mode,
                root.real_path(),
                &paths.instructions_file,
                home.as_deref().unwrap_or(&paths.home),
                &exclude,
            )
        } else {
            instructions::ProjectInstructions::default()
        };
        let (skills, skill_roots) =
            skills::collect(root.real_path(), &paths.home, &resolved.skills, &exclude);
        let mut context = ToolContext::new(
            resolved.mode,
            root,
            ScopeRoot::open(session.temporary_dir()).map_err(StartupError::SafeFs)?,
            ScopeRoot::open(&spill).map_err(StartupError::SafeFs)?,
            exclude,
        )
        .with_skill_roots(skill_roots)
        .with_eval_switches(
            user.restricted.after_write_check,
            resolved.read_line_numbers,
        )
        .with_project_creation(paths.clone(), helper.clone())
        .with_benitoite(processor.clone())
        .with_session(session.clone())
        .with_web_fetch(crate::tools::WebFetchContext {
            allowed_domains: user.web_fetch.allowed_domains.iter().cloned().collect(),
            client: crate::http::HttpClient::default(),
        });
        if image_helper_available {
            context = context.with_image_helper(crate::tools::ImageHelperContext {
                helper: helper.clone(),
                scratch: scratch.clone(),
                placeholders: placeholders.clone(),
                work_parent: scratch.clone(),
                #[cfg(test)]
                test_command: None,
            });
        }
        if let Some(processor) = &processor {
            let processor = Arc::clone(processor);
            context = context.with_check_query(Arc::new(move |args, limits, cancelled| {
                processor.query(args, limits, cancelled)
            }));
        }
        if let Some(skill) = skill {
            context = context.with_skill(skill);
        }
        if resolved.mode == Mode::Restricted
            && let Some(processor) = &processor
        {
            context = context.with_run_context(benitoite::RunContext {
                scratch_auto_effects: benitoite::effects::scratch_auto_effects(
                    &user.restricted.scratch_auto_effects,
                ),
                test_timeout: std::time::Duration::from_secs(120),
                processor: processor.path.clone(),
                helper: helper.clone(),
                scratch: scratch.clone(),
                placeholders: placeholders.clone(),
                env: child_environment(
                    std::env::vars_os(),
                    &user.referenced_env_vars(),
                    &locations.temporary,
                    Some(("restricted", false)),
                ),
                locations: locations.clone(),
                denied_candidates: candidates.clone(),
                allowed_programs: allowed_programs.clone(),
            });
        }
        let approval_policy = ApprovalPolicy {
            approval,
            sandbox: shell
                .as_ref()
                .map_or(Sandbox::WorkspaceWrite, |shell| shell.sandbox),
            sandboxed: shell.as_ref().is_none_or(|shell| shell.sandboxed),
        };
        if let Some(shell) = shell {
            context = context.with_shell(shell);
        }
        let mut environment =
            Environment::capture(&locations.project, resolved.mode, &writable, network, now)
                .map_err(StartupError::Context)?
                .with_project_instructions(instructions)
                .with_skills(skills);
        if let Some(processor) = &processor {
            environment = environment.with_benitoite_version(&processor.version);
        }
        for notice in processor_notices {
            environment = environment.with_startup_notice(notice);
        }
        if sandbox_disabled {
            environment = environment.with_startup_notice(text::SANDBOX_DISABLED);
        }
        let api_key = key
            .map(|key| key.expose_secret().to_owned())
            .unwrap_or_default();
        let host = resolved
            .destination
            .base_url
            .parse::<crate::http::Uri>()
            .ok()
            .and_then(|uri| uri.host().map(str::to_owned))
            .ok_or(StartupError::Agent(AgentError::InvalidConfiguration))?;
        let status = crate::ui::tui::Status {
            mode: resolved.mode,
            provider: resolved.destination.provider.clone(),
            model: resolved.destination.model.clone(),
            host,
            sandbox: if resolved.mode == Mode::Restricted {
                "restricted".into()
            } else {
                args.sandbox.unwrap_or(user.normal.sandbox).name().into()
            },
            sandbox_disabled,
            context_window: resolved.capabilities.context_window,
            estimate: 0,
        };
        let normal_network = resolved.mode == Mode::Normal && user.normal.network;
        let mut config: crate::agent::AgentConfig = resolved.into();
        config.project_parent = project_parent;
        config.allow_effects = (!args.interactive && config.mode == Mode::Restricted)
            .then(|| args.allow.clone().unwrap_or_default());
        let mut agent = match resume {
            Some(state) => Agent::resume(
                config,
                api_key,
                context,
                environment,
                session.clone(),
                state,
            ),
            None => Agent::new(config, api_key, context, environment, session.clone()),
        }
        .map_err(StartupError::Agent)?;
        agent = agent
            .with_approvals(Box::new(NonInteractiveApprover), approval_policy)
            .with_session_safety(crate::agent::SessionSafety {
                scratch_auto_effects: if status.mode == Mode::Restricted {
                    user.restricted.scratch_auto_effects.clone()
                } else {
                    vec![]
                },
                network: normal_network,
                writable_roots: locations.additional,
                allowed_programs,
            });
        Ok(Prepared {
            agent,
            session,
            status,
            diagnostics: std::mem::take(&mut diagnostics),
            panic_record,
        })
    })();
    result.map_err(|error| PreparationError { error, diagnostics })
}

// 解除を許すオプションと、実際に解除した状態を区別する
// （設計書 04-01「処理系の起動」）。テスト用の直接起動への差し替えより先に選ぶ。
fn select_query_launcher(mode: Mode, sandbox_disabled: bool) -> QueryLauncher {
    if mode == Mode::Normal && sandbox_disabled {
        QueryLauncher::Unsandboxed
    } else {
        QueryLauncher::Sandboxed
    }
}

// 新しい領域は 700 で作り、既存のものの権限や所有者を黙って変えない。
// 錠の差し替えを子から許さない（設計書 03-02「Linux のファイルシステムの見え方」）。
fn private_directory(path: &Path) -> Result<PathBuf, StartupError> {
    match DirBuilder::new().mode(0o700).create(path) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(StartupError::Io(error)),
    }
    let metadata = std::fs::symlink_metadata(path).map_err(StartupError::Io)?;
    if !metadata.is_dir()
        || metadata.permissions().mode() & 0o7777 != 0o700
        || metadata.uid() != rustix::process::geteuid().as_raw()
    {
        return Err(StartupError::Io(io::Error::new(
            io::ErrorKind::PermissionDenied,
            text::PRIVATE_DIRECTORY,
        )));
    }
    std::fs::canonicalize(path).map_err(StartupError::Io)
}

pub(super) fn run(args: ExecArgs) -> Result<u8, CliError> {
    let signals = Signals::new().map_err(|error| CliError::Startup(StartupError::Io(error)))?;
    let Prepared {
        mut agent,
        session,
        diagnostics,
        panic_record,
        ..
    } = prepare(
        &args,
        None,
        &DataPaths::from_env().map_err(|error| CliError::Startup(StartupError::Config(error)))?,
        sandbox::check_availability,
        QueryLauncher::Sandboxed,
        &mut |_| false,
    )
    .map_err(CliError::Preparation)?;
    panic::install(Some(panic_record), None);
    for diagnostic in diagnostics {
        ExecOutput::diagnostic(&diagnostic).map_err(CliError::Output)?;
    }
    let request = args
        .request
        .read(&mut io::stdin().lock())
        .map_err(CliError::Output)?;
    let mut output = ExecOutput::new(args.json);
    let (sender, receiver) = mpsc::sync_channel(64);
    let control = crate::agent::RunControl::without_steering(
        Arc::clone(&signals.cancel_step),
        Arc::clone(&signals.cancelled),
    );
    let (report, output_error) = thread::scope(|scope| {
        let worker = thread::Builder::new()
            .name("agent".into())
            .spawn_scoped(scope, || agent.run_controlled(request, control, sender))
            .map_err(CliError::Output)?;
        let mut output_error = None;
        for event in receiver {
            if output_error.is_none()
                && let Err(error) = output.event(&event)
            {
                signals.cancelled.store(true, Ordering::Release);
                signals.cancel_step.store(true, Ordering::Release);
                output_error = Some(error);
            }
            // 出力が閉じても受信を続け、ループが session_end を記録できるようにする。
        }
        let report = worker
            .join()
            .map_err(|_| CliError::Agent(AgentError::Worker))?
            .map_err(CliError::Agent)?;
        Ok((report, output_error))
    })?;
    let last_message = agent
        .conversation()
        .items
        .iter()
        .rev()
        .find_map(|item| match item {
            Item::AssistantTurn(turn) => Some(
                turn.parts
                    .iter()
                    .filter_map(|part| match part {
                        AssistantPart::Text(text) => Some(text.as_str()),
                        AssistantPart::Reasoning(_) | AssistantPart::ToolCall(_) => None,
                    })
                    .collect::<String>(),
            ),
            Item::UserMessage(_) | Item::ToolResult(_) => None,
        })
        .unwrap_or_default();
    let summary = Record::Summary {
        summary: report.summary,
    };
    session
        .writer()
        .and_then(|mut writer| writer.append(&summary))
        .map_err(|error| CliError::Agent(AgentError::Session(error)))?;
    if let Some(error) = output_error {
        return Err(CliError::Output(error));
    }
    if let Some(path) = args.output_last_message {
        // 利用者が指定した出力先は道具の範囲と別である。ファイルには制御文字も原文で残す。
        std::fs::write(path, &last_message).map_err(CliError::Output)?;
    }
    output
        .finish(&summary, &last_message)
        .map_err(CliError::Output)?;
    // hook はセッションの寿命を延ばさないが、次の呼び出しに記述子を残さない。
    panic::install(None, None);
    Ok(match report.outcome {
        Outcome::Completed => 0,
        Outcome::Failed => 1,
        Outcome::Cancelled => 130,
    })
}

#[cfg(test)]
mod tests;

// 設定の既定値で安全の境界を変えない（設計書 03-01「モードを変える操作」、実装プラン P4-08）。
pub(super) fn prepare_new(
    original: &ExecArgs,
    current: &crate::agent::AgentStatus,
    paths: &DataPaths,
    check_sandbox: impl FnOnce(&Path, &Path) -> Result<(), SandboxError>,
    query_launcher: QueryLauncher,
) -> Result<Prepared, PreparationError> {
    let mut args = original.clone();
    args.resume = None;
    args.continue_session = false;
    args.request = super::args::Request::Text(String::new());
    prepare(
        &args,
        Some(&SessionOverrides::from(current)),
        paths,
        check_sandbox,
        query_launcher,
        &mut |_| false,
    )
}
