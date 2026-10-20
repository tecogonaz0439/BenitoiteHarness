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
    Limits(io::Error),
    Exec(io::Error),
}

impl fmt::Display for SessionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Usage => formatter.write_str(text::SESSION_USAGE),
            Self::RelativeProgram => formatter.write_str(text::SESSION_ABSOLUTE_PROGRAM),
            Self::Setsid(error) => write!(formatter, "{} {error}", text::SESSION_SETSID),
            Self::Limits(error) => write!(formatter, "{} {error}", text::SESSION_LIMITS),
            Self::Exec(error) => write!(formatter, "{} {error}", text::SESSION_EXEC),
        }
    }
}

/// 環境と標準入出力を設定する前に、目的のプログラムを補助で包む。
/// この Command に process_group(0) を付けてはならない。
pub fn command(helper: &Path, program: &Path) -> Command {
    command_with_limits(helper, program, ResourceLimits::default())
}

/// 補助だけに掛け、exec 先とその子孫へ継がせる資源の上限。既存の上限は広げない。
#[derive(Clone, Copy, Debug, Default)]
pub struct ResourceLimits {
    /// プロセス当たりの CPU 時間（秒）。
    pub cpu_seconds: Option<u64>,
    /// 一つの通常のファイルへ書けるバイト数。
    pub file_size_bytes: Option<u64>,
}

/// 呼び出し元の資源の上限を変えず、setsid の後に補助で上限を掛ける。
pub fn command_with_limits(helper: &Path, program: &Path, limits: ResourceLimits) -> Command {
    let mut command = Command::new(helper);
    command.arg("__session");
    for (flag, value) in [
        ("--rlimit-cpu", limits.cpu_seconds),
        ("--rlimit-fsize", limits.file_size_bytes),
    ] {
        if let Some(value) = value {
            command.arg(flag).arg(value.to_string());
        }
    }
    command.arg("--").arg(program);
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
    let mut limits = ResourceLimits::default();
    loop {
        let flag = args.next().ok_or(SessionError::Usage)?;
        if flag == OsStr::new("--") {
            break;
        }
        let slot = match flag.to_str() {
            Some("--rlimit-cpu") => &mut limits.cpu_seconds,
            Some("--rlimit-fsize") => &mut limits.file_size_bytes,
            _ => return Err(SessionError::Usage),
        };
        if slot.is_some() {
            return Err(SessionError::Usage);
        }
        *slot = Some(
            args.next()
                .and_then(|value| {
                    value
                        .to_str()
                        .filter(|value| {
                            !value.is_empty() && value.bytes().all(|b| b.is_ascii_digit())
                        })
                        .and_then(|value| value.parse::<u64>().ok())
                        .filter(|value| *value > 0)
                })
                .ok_or(SessionError::Usage)?,
        );
    }
    let program = args.next().ok_or(SessionError::Usage)?;
    if !Path::new(&program).is_absolute() {
        return Err(SessionError::RelativeProgram);
    }
    rustix::process::setsid().map_err(|error| SessionError::Setsid(error.into()))?;
    apply_limits(limits)?;
    Err(SessionError::Exec(Command::new(program).args(args).exec()))
}

fn apply_limits(limits: ResourceLimits) -> Result<(), SessionError> {
    use rustix::process::{Resource, Rlimit, getrlimit, setrlimit};
    for (resource, requested) in [
        (Resource::Cpu, limits.cpu_seconds),
        (Resource::Fsize, limits.file_size_bytes),
    ] {
        let Some(requested) = requested else { continue };
        let previous = getrlimit(resource);
        let limit = previous
            .current
            .into_iter()
            .chain(previous.maximum)
            .fold(requested, u64::min);
        setrlimit(
            resource,
            Rlimit {
                current: Some(limit),
                maximum: Some(limit),
            },
        )
        .map_err(|error| SessionError::Limits(error.into()))?;
    }
    Ok(())
}
