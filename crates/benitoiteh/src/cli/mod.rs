//! 引数を解釈し、出力と終了状態を決める（設計書 05-01「コマンドラインの形」「終了状態」）。

pub mod args;
mod exec;
mod panic;
mod sessions;
mod signals;
mod text;

use std::io::{self, Write};
use std::process::ExitCode;

enum CliError {
    Usage,
    Arguments(args::ArgsError),
    Output(io::Error),
    Startup(exec::StartupError),
    Agent(crate::agent::AgentError),
}

/// プロセスの引数を処理し、CLI の終了状態を返す。
pub fn main() -> ExitCode {
    let mut args = std::env::args_os().skip(1);
    let argument = args.next();
    // panic hook と通常の引数の解釈より先に補助の入口を分ける（設計書 03-02「サンドボックスの掛け方」）。
    #[cfg(target_os = "linux")]
    if argument.as_deref() == Some(std::ffi::OsStr::new("__sandbox")) {
        return crate::sandbox::run_helper(args);
    }
    panic::install(None);
    match run(argument, args) {
        Ok(status) => ExitCode::from(status),
        Err(error) => {
            let (message, status) = match error {
                CliError::Usage => (text::INVALID_ARGUMENTS.to_owned(), ExitCode::from(2)),
                CliError::Arguments(error) => (error.to_string(), ExitCode::from(2)),
                CliError::Output(error) => (text::output_error(&error), ExitCode::FAILURE),
                CliError::Startup(error) => (text::startup_error(&error), ExitCode::from(2)),
                CliError::Agent(error) => (text::agent_error(&error), ExitCode::FAILURE),
            };
            let message = crate::ui::terminal_text(&message, false);
            if writeln!(io::stderr().lock(), "{message}").is_err() {
                return ExitCode::FAILURE;
            }
            status
        }
    }
}

fn run(
    argument: Option<std::ffi::OsString>,
    mut args: impl Iterator<Item = std::ffi::OsString>,
) -> Result<u8, CliError> {
    // 非 UTF-8 の引数も panic させず、使い方の誤りとして扱う（設計書 05-01「終了状態」）。
    let argument = argument.ok_or(CliError::Usage)?;
    if argument == "sessions" {
        return sessions::run(args);
    }
    if argument == "exec" {
        let arguments = args.collect::<Vec<_>>();
        if arguments.len() == 1 && arguments.first().is_some_and(|arg| arg == "--help") {
            io::stdout()
                .lock()
                .write_all(text::HELP.as_bytes())
                .map_err(CliError::Output)?;
            return Ok(0);
        }
        let options = args::ExecArgs::parse(arguments).map_err(CliError::Arguments)?;
        return exec::run(options);
    }
    if args.next().is_some() {
        return Err(CliError::Usage);
    }
    let output = match argument.to_str() {
        Some("--version") => text::version(),
        Some("--help") => text::HELP.to_owned(),
        _ => return Err(CliError::Usage),
    };
    io::stdout()
        .lock()
        .write_all(output.as_bytes())
        .map_err(CliError::Output)?;
    Ok(0)
}
