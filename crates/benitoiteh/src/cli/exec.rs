//! 設定と安全の検査を終えてからセッションとループを起動する（設計書 05-01「exec の出力」「終了状態」）。

use std::fmt;
use std::fs::OpenOptions;
use std::io;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::sync::mpsc;
use std::thread;
use std::time::SystemTime;

use crate::agent::{Agent, AgentError, Outcome};
use crate::config::credentials::CredentialError;
use crate::config::paths::DataPaths;
use crate::config::writable::{WritableError, validate_writable};
use crate::config::{ConfigError, Mode, ProjectConfig, ReadConfig, UserConfig};
use crate::context::{ContextError, Environment, instructions};
use crate::provider::{AssistantPart, Item};
use crate::safe_fs::{self, SafeFsError, ScopeRoot};
use crate::session::{self, Record, Session, SessionError, SessionId};
use crate::tools::{ToolContext, ToolError, exclude::ExcludeRules};
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
        }
    }
}

struct Prepared {
    agent: Agent,
    session: Session,
    request: String,
}

fn prepare(args: &ExecArgs) -> Result<Prepared, StartupError> {
    let paths = DataPaths::from_env().map_err(StartupError::Config)?;
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
    let locations = validate_writable(&paths, root.real_path(), &temporary, additional)
        .map_err(StartupError::Writable)?;
    let _cleanup = session::cleanup(&paths, user.sessions.retention_days, now)
        .map_err(StartupError::Session)?;
    let session = Session::create(&paths, id).map_err(StartupError::Session)?;
    let panic_record = OpenOptions::new()
        .append(true)
        .open(session.directory().join("session.jsonl"))
        .map_err(StartupError::Io)?;
    panic::install(Some(panic_record));
    let spill = session.directory().join("spill");
    let exclude = ExcludeRules::new(
        root.real_path(),
        &paths,
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
    let context = ToolContext::new(
        resolved.mode,
        root,
        ScopeRoot::open(session.temporary_dir()).map_err(StartupError::SafeFs)?,
        ScopeRoot::open(&spill).map_err(StartupError::SafeFs)?,
        exclude,
    );
    // P1 は子プロセスを起動しない。処理系とサンドボックスの検査は P2 で加える。
    let environment = Environment::capture(
        &locations.project,
        resolved.mode,
        &[locations.project.clone(), locations.temporary],
        false,
        now,
    )
    .map_err(StartupError::Context)?
    .with_project_instructions(instructions);
    let agent = Agent::new(
        resolved.into(),
        key.map(|key| key.expose_secret().to_owned())
            .unwrap_or_default(),
        context,
        environment,
        session.clone(),
    )
    .map_err(StartupError::Agent)?;
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

pub(super) fn run(args: ExecArgs) -> Result<u8, CliError> {
    let signals = Signals::new().map_err(|error| CliError::Startup(StartupError::Io(error)))?;
    let Prepared {
        mut agent,
        session,
        request,
    } = prepare(&args).map_err(CliError::Startup)?;
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
