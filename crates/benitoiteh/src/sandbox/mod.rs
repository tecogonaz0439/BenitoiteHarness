//! 子プロセスに OS のサンドボックスを掛ける起動の形を作る（設計書 03-02）。

#[cfg(target_os = "linux")]
mod linux;
mod macos;
mod normal;
mod policy;
mod restricted;
pub use normal::{check_availability, normal_policy};
pub use restricted::{RestrictedAccess, restricted_policy, scratch_policy};
pub mod run;
pub mod session;
mod text;

#[cfg(test)]
mod tests;

use std::collections::BTreeMap;
use std::error::Error;
use std::ffi::OsString;
use std::fmt;
use std::io;
#[cfg(target_os = "linux")]
use std::os::unix::process::CommandExt;
#[cfg(not(target_os = "linux"))]
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;
use std::process::Stdio;

#[cfg(target_os = "linux")]
pub use linux::{
    CapabilityStatus, LinuxAvailability, PreparedSandbox, SandboxChild, bubblewrap_version,
    probe_linux, run_helper,
};
pub use macos::SeatbeltProfile;
pub use policy::{PolicyConfig, ProgramAccess, ReadAccess, SandboxPolicy};

/// 補助の準備に失敗したときの共通の終了状態。目的のプログラムも同じ値を返せる。
pub(crate) const SETUP_FAILURE: u8 = 125;

/// 方針の検証、利用可能性、OS の準備の誤り。
#[derive(Debug)]
pub enum SandboxError {
    /// 方針か起動のパスが不正である。
    InvalidPolicy(&'static str),
    /// この環境ではサンドボックスを掛けられない。
    Unavailable,
    /// 準備中の OS の誤り。
    Io(io::Error),
    /// Linux の補助のプロセスの段階ごとの失敗。
    Setup { stage: &'static str, detail: String },
}

impl fmt::Display for SandboxError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidPolicy(reason) => f.write_str(reason),
            Self::Unavailable => f.write_str(text::UNAVAILABLE),
            Self::Io(error) => write!(f, "{} {error}", text::IO),
            Self::Setup { stage, detail } => write!(f, "{} {stage}: {detail}", text::SETUP),
        }
    }
}

impl Error for SandboxError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::InvalidPolicy(_) | Self::Unavailable | Self::Setup { .. } => None,
        }
    }
}

/// 子プロセスの標準入力の扱い。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SandboxStdin {
    /// EOF を返す。
    Null,
    /// 親の標準入力を引き継ぐ。
    Inherit,
    /// 呼び出し元が書き込むパイプを作る。
    Piped,
}

/// OS に依存しない子プロセスの起動の入力。
#[derive(Clone, Debug)]
pub struct SandboxCommand {
    /// 検証済みの方針。
    pub policy: SandboxPolicy,
    /// 起動する実行ファイルの実の絶対パス。
    pub program: PathBuf,
    /// 実行ファイルに渡す引数。
    pub args: Vec<OsString>,
    /// 実の絶対パスの作業ディレクトリ。
    pub cwd: PathBuf,
    /// 子に渡す環境変数の全体。
    pub env: BTreeMap<OsString, OsString>,
    /// 標準入力の扱い。
    pub stdin: SandboxStdin,
}

impl SandboxCommand {
    /// 補助を経て macOS の `sandbox-exec` を新しいセッションで起動する。
    #[cfg(not(target_os = "linux"))]
    pub fn to_command(&self, helper: &Path) -> Result<Command, SandboxError> {
        self.policy.validate()?;
        policy::canonical_path(&self.program)?;
        policy::canonical_path(&self.cwd)?;
        policy::directory(&self.cwd)?;
        #[cfg(target_os = "macos")]
        {
            if !Path::new("/usr/bin/sandbox-exec").is_file() {
                return Err(SandboxError::Unavailable);
            }
            let profile = self.policy.seatbelt_profile();
            let mut command = session::command(helper, Path::new("/usr/bin/sandbox-exec"));
            command.arg("-p").arg(profile.source);
            for (name, value) in profile.parameters {
                let mut argument = OsString::from(format!("-D{name}="));
                argument.push(value);
                command.arg(argument);
            }
            command.arg("--").arg(&self.program).args(&self.args);
            self.configure(command)
        }
        #[cfg(not(any(target_os = "macos", target_os = "linux")))]
        {
            Err(SandboxError::Unavailable)
        }
    }

    fn configure(&self, mut command: Command) -> Result<Command, SandboxError> {
        command.current_dir(&self.cwd).env_clear().envs(&self.env);
        #[cfg(target_os = "linux")]
        command.process_group(0);
        command.stdin(match self.stdin {
            SandboxStdin::Null => Stdio::null(),
            SandboxStdin::Inherit => Stdio::inherit(),
            SandboxStdin::Piped => Stdio::piped(),
        });
        Ok(command)
    }
}
