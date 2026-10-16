//! 引数を解釈し、出力と終了状態を決める（設計書 05-01「コマンドラインの形」「終了状態」）。

mod text;

use std::io::{self, Write};
use std::process::ExitCode;

enum CliError {
    Usage,
    Output(io::Error),
}

/// プロセスの引数を処理し、CLI の終了状態を返す。
pub fn main() -> ExitCode {
    let mut args = std::env::args_os().skip(1);
    let argument = args.next();
    // 設定や通常の引数の処理より先に補助の入口を分ける（設計書 03-02「サンドボックスの掛け方」）。
    #[cfg(target_os = "linux")]
    if argument.as_deref() == Some(std::ffi::OsStr::new("__sandbox")) {
        return crate::sandbox::run_helper(args);
    }
    match run(argument, args.next()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            let (message, status) = match error {
                CliError::Usage => (text::INVALID_ARGUMENTS.to_owned(), ExitCode::from(2)),
                CliError::Output(error) => (text::output_error(&error), ExitCode::FAILURE),
            };
            if writeln!(io::stderr().lock(), "{message}").is_err() {
                return ExitCode::FAILURE;
            }
            status
        }
    }
}

fn run(
    argument: Option<std::ffi::OsString>,
    extra: Option<std::ffi::OsString>,
) -> Result<(), CliError> {
    // 非 UTF-8 の引数も panic させず、使い方の誤りとして扱う（設計書 05-01「終了状態」）。
    let argument = argument.ok_or(CliError::Usage)?;
    if extra.is_some() {
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
