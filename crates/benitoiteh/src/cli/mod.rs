//! 引数を解釈し、出力と終了状態を決める（設計書 05-01「コマンドラインの形」「終了状態」）。

pub mod args;
mod text;

use std::io::{self, Write};
use std::process::ExitCode;

enum CliError {
    Usage,
    Arguments(args::ArgsError),
    Output(io::Error),
}

/// プロセスの引数を処理し、CLI の終了状態を返す。
pub fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            let (message, status) = match error {
                CliError::Usage => (text::INVALID_ARGUMENTS.to_owned(), ExitCode::from(2)),
                CliError::Arguments(error) => (error.to_string(), ExitCode::from(2)),
                CliError::Output(error) => (text::output_error(&error), ExitCode::FAILURE),
            };
            if writeln!(io::stderr().lock(), "{message}").is_err() {
                return ExitCode::FAILURE;
            }
            status
        }
    }
}

fn run() -> Result<(), CliError> {
    // 非 UTF-8 の引数も panic させず、使い方の誤りとして扱う（設計書 05-01「終了状態」）。
    let mut args = std::env::args_os().skip(1);
    let argument = args.next().ok_or(CliError::Usage)?;
    if argument == "exec" {
        let _options = args::ExecArgs::parse(args).map_err(CliError::Arguments)?;
        // ループと出力の接続は P1-10 が行う（設計書 05-01「exec の出力」）。
        return Err(CliError::Usage);
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
        .map_err(CliError::Output)
}
