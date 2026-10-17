//! 設定と安全の検査を終えてからセッションとループを起動する（設計書 05-01「exec の出力」「終了状態」）。

use std::fmt;
use std::fs::{DirBuilder, OpenOptions};
use std::io;
use std::os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::sync::mpsc;
use std::thread;
use std::time::SystemTime;

use crate::agent::{Agent, AgentError, Outcome};
use crate::approval::{ApprovalPolicy, NonInteractiveApprover};
use crate::config::credentials::CredentialError;
use crate::config::paths::DataPaths;
use crate::config::writable::{WritableError, validate_writable_with_executable};
use crate::config::{
    Approval, ConfigError, Mode, ProjectConfig, ReadConfig, Sandbox, UserConfig, child_environment,
};
use crate::context::{ContextError, Environment, instructions};
use crate::provider::{AssistantPart, Item};
use crate::safe_fs::{self, SafeFsError, ScopeRoot};
use crate::sandbox::{self, SandboxError};
use crate::session::{self, Record, Session, SessionError, SessionId};
use crate::tools::{ShellContext, ToolContext, ToolError, exclude::ExcludeRules};
use crate::ui::ExecOutput;

use super::{CliError, args::ExecArgs, panic, signals::Signals, text};

pub(super) enum StartupError {
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

/// 依頼を読まず、診断と panic 用の記述子を呼び出す側へ返す。
pub(super) fn prepare(
    args: &ExecArgs,
    paths: &DataPaths,
    check_sandbox: impl FnOnce(&Path, &Path) -> Result<(), SandboxError>,
    confirm_mode_change: &mut dyn FnMut(&ModeChange) -> bool,
) -> Result<Prepared, PreparationError> {
    let mut diagnostics = Vec::new();
    let result = (|| {
        safe_fs::verify_platform_support(&paths.temporary_root).map_err(StartupError::SafeFs)?;
        let user = UserConfig::load(&paths.config_file).map_err(StartupError::Config)?;
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
        .map_err(StartupError::Writable)?;
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
        let mut sandbox_disabled = false;
        let shell = if resolved.mode == Mode::Normal {
            let selected = args.sandbox.unwrap_or(user.normal.sandbox);
            let scratch = private_directory(&session.directory().join("sandbox"))?;
            let placeholders = private_directory(&paths.data_dir.join("placeholders"))?;
            let candidates = super::denied_candidates(paths);
            let policy = if selected == Sandbox::DangerFullAccess {
                None
            } else {
                match check_sandbox(&helper, &scratch) {
                    Ok(()) => sandbox::normal_policy(
                        selected,
                        &locations,
                        user.normal.network,
                        &candidates,
                    )
                    .map_err(StartupError::Sandbox)?,
                    Err(error) if args.no_sandbox => {
                        diagnostics.push(text::sandbox_disabled(&error));
                        sandbox_disabled = true;
                        None
                    }
                    Err(error) => return Err(StartupError::Sandbox(error)),
                }
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
                helper,
                scratch,
                placeholders,
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
        let instructions = if resolved.mode == Mode::Normal {
            let home = std::fs::canonicalize(&paths.home).map_err(StartupError::Io)?;
            instructions::collect(
                resolved.mode,
                root.real_path(),
                &paths.instructions_file,
                &home,
                &exclude,
            )
        } else {
            instructions::ProjectInstructions::default()
        };
        let mut context = ToolContext::new(
            resolved.mode,
            root,
            ScopeRoot::open(session.temporary_dir()).map_err(StartupError::SafeFs)?,
            ScopeRoot::open(&spill).map_err(StartupError::SafeFs)?,
            exclude,
        )
        .with_session(session.clone())
        .with_web_fetch(crate::tools::WebFetchContext {
            allowed_domains: user.web_fetch.allowed_domains.iter().cloned().collect(),
            client: crate::http::HttpClient::default(),
        });
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
                .with_project_instructions(instructions);
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
        let mut agent = match resume {
            Some(state) => Agent::resume(
                resolved.into(),
                api_key,
                context,
                environment,
                session.clone(),
                state,
            ),
            None => Agent::new(
                resolved.into(),
                api_key,
                context,
                environment,
                session.clone(),
            ),
        }
        .map_err(StartupError::Agent)?;
        agent = agent.with_approvals(Box::new(NonInteractiveApprover), approval_policy);
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
        &DataPaths::from_env().map_err(|error| CliError::Startup(StartupError::Config(error)))?,
        sandbox::check_availability,
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
