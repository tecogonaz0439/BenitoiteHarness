//! 制御端末を持たない補助へ exec する（設計書 03-02「端末からの切り離し」）。

use std::ffi::{OsStr, OsString};
use std::fmt;
use std::io::{self, Write};
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Command, ExitCode};

use super::{SETUP_FAILURE, text};

enum SessionError {
    Usage,
    RelativeProgram,
    Setsid(io::Error),
    Exec(io::Error),
}

impl fmt::Display for SessionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Usage => formatter.write_str(text::SESSION_USAGE),
            Self::RelativeProgram => formatter.write_str(text::SESSION_ABSOLUTE_PROGRAM),
            Self::Setsid(error) => write!(formatter, "{} {error}", text::SESSION_SETSID),
            Self::Exec(error) => write!(formatter, "{} {error}", text::SESSION_EXEC),
        }
    }
}

/// 環境と標準入出力を設定する前に、目的のプログラムを補助で包む。
/// この Command に process_group(0) を付けてはならない。
pub fn command(helper: &Path, program: &Path) -> Command {
    let mut command = Command::new(helper);
    command.arg("__session").arg("--").arg(program);
    command
}

/// 設定、環境変数、標準入出力を読まず、失敗は補助共通の終了状態 125 で返す。
pub fn run_helper(args: impl Iterator<Item = OsString>) -> ExitCode {
    let Err(error) = exec(args);
    let diagnostic = error.to_string().replace(['\n', '\r'], " ");
    let _result = writeln!(io::stderr().lock(), "{diagnostic}");
    ExitCode::from(SETUP_FAILURE)
}

fn exec(
    mut args: impl Iterator<Item = OsString>,
) -> Result<std::convert::Infallible, SessionError> {
    if args.next().as_deref() != Some(OsStr::new("--")) {
        return Err(SessionError::Usage);
    }
    let program = args.next().ok_or(SessionError::Usage)?;
    if !Path::new(&program).is_absolute() {
        return Err(SessionError::RelativeProgram);
    }
    rustix::process::setsid().map_err(|error| SessionError::Setsid(error.into()))?;
    Err(SessionError::Exec(Command::new(program).args(args).exec()))
}
