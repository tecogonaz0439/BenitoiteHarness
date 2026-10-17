//! 通常モードの方針を検査済みの場所から作り、起動に必要な機能を確かめる（設計書 03-02「通常モードの方針」）。

use std::path::{Path, PathBuf};

use crate::config::{Sandbox, writable::WritableLocations};

use super::{PolicyConfig, SandboxError, SandboxPolicy};

/// 書ける場所の検査に通った実のパスを使い、通常モードの子プロセスの方針を作る。
/// 読ませない候補は、存在するものだけを正規化する。ほかの I/O の失敗は無視しない。
pub fn normal_policy(
    sandbox: Sandbox,
    locations: &WritableLocations,
    network: bool,
    denied_candidates: &[PathBuf],
) -> Result<Option<SandboxPolicy>, SandboxError> {
    let writable = match sandbox {
        Sandbox::ReadOnly => vec![locations.temporary.clone()],
        Sandbox::WorkspaceWrite => {
            let mut roots = vec![locations.project.clone(), locations.temporary.clone()];
            roots.extend(locations.additional.iter().cloned());
            roots
        }
        Sandbox::DangerFullAccess => return Ok(None),
    };
    let mut config = PolicyConfig::new(writable);
    config.allow_ip = sandbox == Sandbox::WorkspaceWrite && network;
    // 通常モードでは、ビルドしたプログラムをプロジェクト内で動かす。
    config.deny_exec_in_writable = false;
    for candidate in denied_candidates {
        match std::fs::canonicalize(candidate) {
            Ok(path) => config.denied_read.push(path),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(SandboxError::Io(error)),
        }
    }
    SandboxPolicy::new(config).map(Some)
}

/// macOS では固定の実行ファイルの存在、Linux では四つの必須機能を確かめる。
pub fn check_availability(helper: &Path, scratch: &Path) -> Result<(), SandboxError> {
    #[cfg(target_os = "macos")]
    {
        let _ = (helper, scratch);
        if Path::new("/usr/bin/sandbox-exec").is_file() {
            Ok(())
        } else {
            Err(SandboxError::Unavailable)
        }
    }
    #[cfg(target_os = "linux")]
    {
        use super::CapabilityStatus;
        let availability = super::probe_linux(helper, scratch)?;
        let failed = [
            (super::text::BUBBLEWRAP, availability.bubblewrap),
            (super::text::USER_NAMESPACES, availability.user_namespaces),
            (super::text::LANDLOCK, availability.landlock_v3),
            (super::text::SECCOMP, availability.seccomp),
        ]
        .into_iter()
        .filter_map(|(name, status)| match status {
            CapabilityStatus::Available => None,
            CapabilityStatus::Unavailable(reason) => Some(format!("{name}: {reason}")),
            CapabilityStatus::NotChecked => Some(super::text::not_checked(name)),
        })
        .collect::<Vec<_>>();
        if failed.is_empty() {
            Ok(())
        } else {
            Err(SandboxError::Setup {
                stage: super::text::AVAILABILITY,
                detail: failed.join("; "),
            })
        }
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        let _ = (helper, scratch);
        Err(SandboxError::Unavailable)
    }
}
