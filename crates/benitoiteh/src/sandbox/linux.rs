//! Linux の補助の入口と、Landlock・seccomp の順序を管理する（設計書 03-02「サンドボックスの掛け方」）。

mod availability;
mod control;
mod filesystem;
mod filter;
mod mounts;
mod process;

pub use availability::{CapabilityStatus, LinuxAvailability, bubblewrap_version, probe_linux};
pub use process::{PreparedSandbox, SandboxChild};

use std::ffi::{OsStr, OsString};
use std::fs::{self, File};
use std::io::{self, Write};
use std::mem::MaybeUninit;
use std::os::fd::AsRawFd;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Command, ExitCode};

use rustix::fs::RawDir;
use rustix::thread::{CapabilitySet, CapabilitySets};

use super::{PolicyConfig, SETUP_FAILURE, SandboxError, SandboxPolicy, policy, text};

fn failure(stage: &'static str, error: impl std::fmt::Display) -> SandboxError {
    SandboxError::Setup {
        stage,
        detail: error.to_string(),
    }
}

/// `__sandbox` の残りの引数を処理する。成功時は名前空間の子を回収して終わる。
pub fn run_helper(args: impl Iterator<Item = OsString>) -> ExitCode {
    let error = match prepare_and_run(args) {
        Ok(never) => match never {},
        Err(error) => error,
    };
    // 方針の値や OS の診断に改行が含まれても、親への失敗の表示は一行にする。
    let message = error.to_string().replace(['\n', '\r'], " ");
    let _result = writeln!(io::stderr().lock(), "{message}");
    ExitCode::from(SETUP_FAILURE)
}

fn prepare_and_run(
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
    let mut config: PolicyConfig =
        serde_json::from_value(value).map_err(|e| failure(text::POLICY, e))?;
    // 覆いの下の子孫は、もう存在しないか、権限 000 の祖先を辿れない。
    // 外側だけで同じ拒否になるため、冗長な子孫を落としてから実のパスを検証する
    // （設計書 03-02「Linux のファイルシステムの見え方」、実装プラン P0-07）。
    config.denied_read.sort();
    let mut outer = Vec::new();
    for path in config.denied_read {
        policy::canonical_syntax(&path).map_err(|e| failure(text::POLICY, e))?;
        if outer.iter().any(|parent| path.starts_with(parent)) {
            continue;
        }
        match fs::symlink_metadata(&path) {
            Ok(_) => outer.push(path),
            // 引数を作る時点で消えていた秘密には覆いを作らない。
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(failure(text::POLICY, error)),
        }
    }
    config.denied_read = outer;
    let policy = SandboxPolicy::new(config).map_err(|e| failure(text::POLICY, e))?;
    policy::canonical_path(Path::new(&program)).map_err(|e| failure(text::POLICY, e))?;
    let control = control::receive_stdin().map_err(|e| failure(text::DESCRIPTORS, e))?;
    check_descriptors(control.as_raw_fd()).map_err(|e| failure(text::DESCRIPTORS, e))?;
    check_capabilities()?;
    rustix::thread::set_no_new_privs(true).map_err(|e| failure(text::NO_NEW_PRIVS, e))?;
    filesystem::restrict(&policy, required)?;
    let filter =
        filter::compile(policy.config().allow_ip).map_err(|e| failure(text::SECCOMP, e))?;
    seccompiler::apply_filter(&filter).map_err(|e| failure(text::SECCOMP, e))?;
    rustix::process::set_dumpable_behavior(rustix::process::DumpableBehavior::NotDumpable)
        .map_err(|e| failure(text::DUMPABLE, e))?;
    let child = Command::new(program)
        .args(args)
        .process_group(0)
        .spawn()
        .map_err(|e| failure(text::EXEC, e))?;
    control::supervise(control, child)
}

fn check_capabilities() -> Result<(), SandboxError> {
    let sets = rustix::thread::capabilities(None).map_err(|e| failure(text::CAPABILITIES, e))?;
    let mut ambient = CapabilitySet::empty();
    // 番号を順に照会し、カーネルが知らない番号で終える。クレートがまだ名前を
    // 持たない特権も検出し、照会自体の失敗でも目的のプログラムを動かさない。
    // （設計書 03-02「サンドボックスの掛け方」）
    for bit in 0..u64::BITS {
        let capability = CapabilitySet::from_bits_retain(1_u64 << bit);
        match rustix::thread::capability_is_in_ambient_set(capability) {
            Ok(true) => ambient.insert(capability),
            Ok(false) => {}
            Err(rustix::io::Errno::INVAL)
                if bit > CapabilitySet::CHECKPOINT_RESTORE.bits().trailing_zeros() =>
            {
                break;
            }
            Err(error) => return Err(failure(text::CAPABILITIES, error)),
        }
    }
    check_capability_sets(sets, ambient)
}

fn check_capability_sets(sets: CapabilitySets, ambient: CapabilitySet) -> Result<(), SandboxError> {
    if sets.effective.is_empty()
        && sets.permitted.is_empty()
        && sets.inheritable.is_empty()
        && ambient.is_empty()
    {
        Ok(())
    } else {
        Err(failure(text::CAPABILITIES, text::REMAINING_CAPABILITIES))
    }
}

fn check_descriptors(control: i32) -> io::Result<()> {
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
        if !matches!(number, Some(0..=2)) && number != Some(own_fd) && number != Some(control) {
            return Err(io::Error::other(text::EXTRA_FD));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    //! 一般利用者では作れない特権の残存を拒む契約（設計書 03-02「サンドボックスの掛け方」）。

    use super::*;

    // 作成時の関門: root でない統合試験では、どの集合の確認を落としても空のまま通る。
    // 本番でも使う集合の判定で、それぞれ単独の残存と未知のビットを拒むことを確かめる。
    #[test]
    fn helper_rejects_each_nonempty_capability_set() -> Result<(), SandboxError> {
        let empty = CapabilitySet::empty();
        check_capability_sets(
            CapabilitySets {
                effective: empty,
                permitted: empty,
                inheritable: empty,
            },
            empty,
        )?;
        for capability in [
            CapabilitySet::DAC_OVERRIDE,
            CapabilitySet::from_bits_retain(1_u64 << 63),
        ] {
            for [effective, permitted, inheritable, ambient] in [
                [capability, empty, empty, empty],
                [empty, capability, empty, empty],
                [empty, empty, capability, empty],
                [empty, empty, empty, capability],
            ] {
                assert!(matches!(
                    check_capability_sets(
                        CapabilitySets {
                            effective,
                            permitted,
                            inheritable,
                        },
                        ambient
                    ),
                    Err(SandboxError::Setup {
                        stage: text::CAPABILITIES,
                        ..
                    })
                ));
            }
        }
        Ok(())
    }
}
