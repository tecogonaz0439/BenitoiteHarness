//! Linux の補助の入口と、Landlock・seccomp の順序を管理する（設計書 03-02「サンドボックスの掛け方」）。

mod filesystem;
mod filter;

use std::ffi::{OsStr, OsString};
use std::fs::File;
use std::io::{self, Write};
use std::mem::MaybeUninit;
use std::os::fd::AsRawFd;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Command, ExitCode};

use rustix::fs::RawDir;

use super::{SandboxError, SandboxPolicy, policy, text};

/// 補助の準備に失敗したときの終了状態。目的のプログラムも同じ値を返せる。
pub const SETUP_FAILURE: u8 = 125;

fn failure(stage: &'static str, error: impl std::fmt::Display) -> SandboxError {
    SandboxError::Setup {
        stage,
        detail: error.to_string(),
    }
}

/// `__sandbox` の残りの引数を処理する。成功時は目的のプログラムで自身を置き換える。
pub fn run_helper(args: impl Iterator<Item = OsString>) -> ExitCode {
    let error = match prepare_and_exec(args) {
        Ok(never) => match never {},
        Err(error) => error,
    };
    // 方針の値や OS の診断に改行が含まれても、親への失敗の表示は一行にする。
    let message = error.to_string().replace(['\n', '\r'], " ");
    let _result = writeln!(io::stderr().lock(), "{message}");
    ExitCode::from(SETUP_FAILURE)
}

fn prepare_and_exec(
    mut args: impl Iterator<Item = OsString>,
) -> Result<std::convert::Infallible, SandboxError> {
    let json = args
        .next()
        .ok_or_else(|| failure(text::POLICY, text::HELPER_USAGE))?;
    let separator = args.next();
    let program = args
        .next()
        .ok_or_else(|| failure(text::POLICY, text::HELPER_USAGE))?;
    if separator.as_deref() != Some(OsStr::new("--")) {
        return Err(failure(text::POLICY, text::HELPER_USAGE));
    }
    let json = json
        .to_str()
        .ok_or_else(|| failure(text::POLICY, text::HELPER_USAGE))?;
    let mut value: serde_json::Value =
        serde_json::from_str(json).map_err(|e| failure(text::POLICY, e))?;
    // 失敗を起こす指定は必須の ABI を上げる方向だけに働く。公開の方針の型は変えない。
    // 実装プラン P0-05「作るもの」4 の補助の入口での失敗の検証に用いる。
    let required = value
        .as_object_mut()
        .and_then(|object| object.remove("required_landlock_abi"));
    let required = match required {
        None => 3,
        Some(value) => value
            .as_u64()
            .filter(|abi| *abi >= 3)
            .ok_or_else(|| failure(text::POLICY, text::INVALID_ABI))?,
    };
    let policy: SandboxPolicy =
        serde_json::from_value(value).map_err(|e| failure(text::POLICY, e))?;
    policy::canonical_path(Path::new(&program)).map_err(|e| failure(text::POLICY, e))?;
    check_descriptors().map_err(|e| failure(text::DESCRIPTORS, e))?;
    rustix::thread::set_no_new_privs(true).map_err(|e| failure(text::NO_NEW_PRIVS, e))?;
    filesystem::restrict(&policy, required)?;
    let filter =
        filter::compile(policy.config().allow_ip).map_err(|e| failure(text::SECCOMP, e))?;
    seccompiler::apply_filter(&filter).map_err(|e| failure(text::SECCOMP, e))?;
    Err(failure(text::EXEC, Command::new(program).args(args).exec()))
}

fn check_descriptors() -> io::Result<()> {
    // ReadDir は自分の番号を公開しないので、番号を保持できる File と RawDir を使う。
    // 同じ /proc/self/fd を指す、親から来た別の記述子まで除外しない。
    let directory = File::open("/proc/self/fd")?;
    let own_fd = directory.as_raw_fd();
    let mut buffer = [MaybeUninit::uninit(); 8192];
    let mut entries = RawDir::new(&directory, &mut buffer);
    while let Some(entry) = entries.next() {
        let entry = entry?;
        let name = entry.file_name().to_bytes();
        if name == b"." || name == b".." {
            continue;
        }
        let number = std::str::from_utf8(name)
            .ok()
            .and_then(|name| name.parse::<i32>().ok());
        if !matches!(number, Some(0..=2)) && number != Some(own_fd) {
            return Err(io::Error::other(text::EXTRA_FD));
        }
    }
    Ok(())
}
