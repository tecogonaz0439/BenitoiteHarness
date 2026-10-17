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
        }
    }
}

struct Prepared {
    agent: Agent,
    session: Session,
    request: String,
}

fn prepare(
    args: &ExecArgs,
    paths: &DataPaths,
    check_sandbox: impl FnOnce(&Path, &Path) -> Result<(), SandboxError>,
) -> Result<Prepared, StartupError> {
    safe_fs::verify_platform_support(&paths.temporary_root).map_err(StartupError::SafeFs)?;
    let user = UserConfig::load(&paths.config_file).map_err(StartupError::Config)?;
    let cwd = std::env::current_dir().map_err(StartupError::Io)?;
    let root =
        ScopeRoot::open(args.project.as_deref().unwrap_or(&cwd)).map_err(StartupError::SafeFs)?;
    let project = ProjectConfig::load(&root);
    if project.read {
        ExecOutput::diagnostic(&text::project_loaded(root.real_path()))
            .map_err(StartupError::Io)?;
    }
    if let Some(error) = &project.error {
        ExecOutput::diagnostic(&text::project_ignored(error)).map_err(StartupError::Io)?;
    }
    let resolved = user
        .resolve_exec(args, &project.config, &cwd)
        .map_err(StartupError::Config)?;
    ExecOutput::diagnostic(&text::destination(&resolved.destination)).map_err(StartupError::Io)?;
    let key = resolved
        .api_key
        .as_ref()
        .map(|key| key.resolve())
        .transpose()
        .map_err(StartupError::Credentials)?;
    let now = SystemTime::now();
    let id = SessionId::generate(now).map_err(StartupError::Session)?;
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
    let mut locations =
        validate_writable_with_executable(paths, root.real_path(), &temporary, additional, &helper)
            .map_err(StartupError::Writable)?;
    let _cleanup = session::cleanup(paths, user.sessions.retention_days, now)
        .map_err(StartupError::Session)?;
    let session = Session::create(paths, id).map_err(StartupError::Session)?;
    // macOS の TMPDIR の /var という別名を Seatbelt に渡さない（設計書 03-02）。
    locations.temporary =
        std::fs::canonicalize(session.temporary_dir()).map_err(StartupError::Io)?;
    let mut sandbox_disabled = false;
    let shell = if resolved.mode == Mode::Normal {
        let selected = args.sandbox.unwrap_or(user.normal.sandbox);
        let scratch = private_directory(&session.directory().join("sandbox"))?;
        let placeholders = private_directory(&paths.data_dir.join("placeholders"))?;
        let mut candidates = vec![paths.config_dir.clone(), paths.data_dir.clone()];
        candidates.extend([".ssh", ".gnupg", ".aws"].map(|name| paths.home.join(name)));
        let policy = if selected == Sandbox::DangerFullAccess {
            None
        } else {
            match check_sandbox(&helper, &scratch) {
                Ok(()) => {
                    sandbox::normal_policy(selected, &locations, user.normal.network, &candidates)
                        .map_err(StartupError::Sandbox)?
                }
                Err(error) if args.no_sandbox => {
                    ExecOutput::diagnostic(&text::sandbox_disabled(&error))
                        .map_err(StartupError::Io)?;
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
            approval: Approval::Never,
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
    panic::install(Some(panic_record));
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
    );
    let approval_policy = shell.as_ref().map(|shell| ApprovalPolicy {
        approval: shell.approval,
        sandbox: shell.sandbox,
        sandboxed: shell.sandboxed,
    });
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
    let mut agent = Agent::new(
        resolved.into(),
        key.map(|key| key.expose_secret().to_owned())
            .unwrap_or_default(),
        context,
        environment,
        session.clone(),
    )
    .map_err(StartupError::Agent)?;
    if let Some(policy) = approval_policy {
        agent = agent.with_approvals(Box::new(NonInteractiveApprover), policy);
    }
    let request = args
        .request
        .read(&mut io::stdin().lock())
        .map_err(StartupError::Io)?;
    Ok(Prepared {
        agent,
        session,
        request,
    })
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
        request,
    } = prepare(
        &args,
        &DataPaths::from_env().map_err(|error| CliError::Startup(StartupError::Config(error)))?,
        sandbox::check_availability,
    )
    .map_err(CliError::Startup)?;
    let mut output = ExecOutput::new(args.json);
    let (sender, receiver) = mpsc::sync_channel(64);
    let cancelled = Arc::clone(&signals.cancelled);
    let (report, output_error) = thread::scope(|scope| {
        let worker = thread::Builder::new()
            .name("agent".into())
            .spawn_scoped(scope, || agent.run(request, cancelled, sender))
            .map_err(CliError::Output)?;
        let mut output_error = None;
        for event in receiver {
            if output_error.is_none()
                && let Err(error) = output.event(&event)
            {
                signals.cancelled.store(true, Ordering::Release);
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
    panic::install(None);
    Ok(match report.outcome {
        Outcome::Completed => 0,
        Outcome::Failed => 1,
        Outcome::Cancelled => 130,
    })
}

#[cfg(test)]
mod tests;
