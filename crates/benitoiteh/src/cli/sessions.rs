//! 一覧の走査を起動し、JSON Lines または端末へ示す（設計書 05-01「sessions」）。

use std::collections::BTreeSet;
use std::ffi::OsString;
use std::io::{self, Write};
use std::path::PathBuf;

use super::{CliError, exec::StartupError};
use crate::config::{UserConfig, paths::DataPaths};
use crate::session;
use crate::ui::{self, ExecOutput};

pub(super) fn run(args: impl Iterator<Item = OsString>) -> Result<u8, CliError> {
    let mut args = args;
    let mut project = None;
    let mut json = false;
    let mut seen = BTreeSet::new();
    while let Some(arg) = args.next() {
        if !seen.insert(arg.clone()) {
            return Err(CliError::Usage);
        }
        if arg == "--json" {
            json = true;
        } else if arg == "--project" {
            project = Some(PathBuf::from(args.next().ok_or(CliError::Usage)?));
        } else {
            return Err(CliError::Usage);
        }
    }
    let project = project
        .map(std::fs::canonicalize)
        .transpose()
        .map_err(|error| CliError::Startup(StartupError::Io(error)))?;
    let paths =
        DataPaths::from_env().map_err(|error| CliError::Startup(StartupError::Config(error)))?;
    let config = UserConfig::load(&paths.config_file)
        .map_err(|error| CliError::Startup(StartupError::Config(error)))?;
    session::cleanup(
        &paths,
        config.sessions.retention_days,
        std::time::SystemTime::now(),
    )
    .map_err(|error| CliError::Startup(StartupError::Session(error)))?;
    let usage = session::storage_usage(&paths, None)
        .map_err(|error| CliError::Startup(StartupError::Session(error)))?;
    if let Some(warning) = usage.warning() {
        ExecOutput::diagnostic(warning).map_err(CliError::Output)?;
    }
    let entries = session::list_sessions(&paths)
        .map_err(|error| CliError::Startup(StartupError::Session(error)))?;
    let mut stdout = io::stdout().lock();
    for mut entry in entries {
        if let Some(project) = &project
            && entry.error.is_none()
            && !entry
                .project_dir
                .as_ref()
                .and_then(|path| std::fs::canonicalize(path).ok())
                .is_some_and(|path| path == *project)
        {
            continue;
        }
        entry.first_request = entry
            .first_request
            .map(|request| ui::terminal_text(&request, false));
        entry.error = entry.error.map(|error| ui::terminal_text(&error, false));
        if json {
            serde_json::to_writer(&mut stdout, &entry)
                .map_err(|error| CliError::Output(io::Error::other(error)))?;
            writeln!(stdout).map_err(CliError::Output)?;
        } else {
            writeln!(stdout, "{}", ui::session_entry(&entry)).map_err(CliError::Output)?;
        }
    }
    stdout.flush().map_err(CliError::Output)?;
    Ok(0)
}
