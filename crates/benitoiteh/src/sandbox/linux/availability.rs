//! 実際の起動の結果から Linux の必須機能を個別に診断する（設計書 03-02「サンドボックスを掛けられないとき」）。

use std::collections::BTreeMap;
use std::fs::{self, DirBuilder};
use std::io;
use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::OnceLock;

use crate::sandbox::{
    PolicyConfig, SandboxCommand, SandboxError, SandboxPolicy, SandboxStdin, policy, text,
};

/// 実際の起動で確認した、必須機能一つの状態。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CapabilityStatus {
    /// この起動で使えた。
    Available,
    /// この起動で使えなかった。診断を保持する。
    Unavailable(String),
    /// 先の段階で止まったため、確かめていない。
    NotChecked,
}

/// `doctor` が機能ごとに表示する Linux の診断結果。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LinuxAvailability {
    /// `/usr/bin/bwrap` の起動。
    pub bubblewrap: CapabilityStatus,
    /// 特権を持たない利用者の名前空間の作成。
    pub user_namespaces: CapabilityStatus,
    /// Landlock の ABI の版 3 の適用。
    pub landlock_v3: CapabilityStatus,
    /// seccomp フィルタの適用。
    pub seccomp: CapabilityStatus,
    /// 補助のプロセスの特権の集合が空である。
    pub capabilities: CapabilityStatus,
}

/// 固定の bubblewrap の権限と版を確認し、プロセス内で結果を共有する。
pub(super) fn check_bubblewrap() -> CapabilityStatus {
    match bubblewrap_version() {
        Ok(_) => CapabilityStatus::Available,
        Err(status) => status,
    }
}

/// 固定の bubblewrap の権限と最低の版を検査し、版の表示と起動で結果を共用する。
pub fn bubblewrap_version() -> Result<String, CapabilityStatus> {
    static VERSION: OnceLock<Result<String, CapabilityStatus>> = OnceLock::new();
    VERSION
        .get_or_init(|| {
            let unavailable = |error: io::Error| CapabilityStatus::Unavailable(error.to_string());
            let metadata = fs::metadata("/usr/bin/bwrap").map_err(unavailable)?;
            let mode = check_bubblewrap_mode(metadata.permissions().mode());
            if mode != CapabilityStatus::Available {
                return Err(mode);
            }
            let output = Command::new("/usr/bin/bwrap")
                .arg("--version")
                .env_clear()
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .output()
                .map_err(unavailable)?;
            if !output.status.success() {
                return Err(CapabilityStatus::Unavailable(
                    text::bubblewrap_version_failed(output.status, &output.stderr),
                ));
            }
            match check_bubblewrap_version(&output.stdout) {
                CapabilityStatus::Available => Ok(String::from_utf8_lossy(&output.stdout)
                    .trim_end()
                    .to_owned()),
                status @ (CapabilityStatus::Unavailable(_) | CapabilityStatus::NotChecked) => {
                    Err(status)
                }
            }
        })
        .clone()
}

fn check_bubblewrap_mode(mode: u32) -> CapabilityStatus {
    if mode & 0o4000 != 0 {
        CapabilityStatus::Unavailable(text::SETUID_BUBBLEWRAP.to_owned())
    } else {
        CapabilityStatus::Available
    }
}

fn check_bubblewrap_version(stdout: &[u8]) -> CapabilityStatus {
    let version = (|| {
        let line = std::str::from_utf8(stdout)
            .ok()?
            .strip_suffix('\n')?
            .strip_prefix("bubblewrap ")?;
        let mut components = line.split('.');
        let mut component = || {
            let value = components.next()?;
            if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
                return None;
            }
            value.parse::<u32>().ok()
        };
        let version = (component()?, component()?, component()?);
        if components.next().is_some() {
            return None;
        }
        Some(version)
    })();
    if version.is_some_and(|version| version >= (0, 8, 0)) {
        CapabilityStatus::Available
    } else {
        CapabilityStatus::Unavailable(text::bubblewrap_version(stdout))
    }
}

/// 一時ディレクトリだけを書ける場所として、補助の実行ファイル自身を `--version` で起動する。
/// `scratch` は一時ディレクトリを排他に作れる、実の絶対パスのディレクトリ。
pub fn probe_linux(helper: &Path, scratch: &Path) -> Result<LinuxAvailability, SandboxError> {
    let bubblewrap = check_bubblewrap();
    if bubblewrap != CapabilityStatus::Available {
        return Ok(bubblewrap_failure(bubblewrap));
    }
    policy::canonical_path(scratch)?;
    policy::directory(scratch)?;
    let mut random = [0; 16];
    getrandom::fill(&mut random).map_err(|error| SandboxError::Io(io::Error::other(error)))?;
    let root = scratch.join(format!(
        ".benitoiteh-probe-{:032x}",
        u128::from_ne_bytes(random)
    ));
    fs::create_dir(&root).map_err(SandboxError::Io)?;
    let placeholders = root.with_extension("placeholders");
    if let Err(error) = DirBuilder::new().mode(0o700).create(&placeholders) {
        let _cleanup = fs::remove_dir(&root);
        return Err(SandboxError::Io(error));
    }
    let result = (|| {
        let sandbox = SandboxCommand {
            policy: SandboxPolicy::new(PolicyConfig::new(vec![root.clone()]))?,
            program: helper.to_path_buf(),
            args: vec!["--version".into()],
            cwd: root.clone(),
            env: BTreeMap::new(),
            stdin: SandboxStdin::Null,
        }
        .prepare(helper, scratch, &placeholders)?;
        match sandbox.output() {
            Ok(output) => classify(
                output.status.code(),
                &String::from_utf8_lossy(&output.stderr),
            ),
            Err(SandboxError::Io(error)) => Ok(spawn_failure(error)),
            Err(error) => Err(error),
        }
    })();
    // 錠はこの探査専用なので消せる。実行先は --version だけであり、再帰的には消さない。
    let records_cleanup = fs::remove_dir_all(&placeholders);
    let root_cleanup = fs::remove_dir(&root);
    records_cleanup.map_err(SandboxError::Io)?;
    root_cleanup.map_err(SandboxError::Io)?;
    result
}

fn spawn_failure(error: io::Error) -> LinuxAvailability {
    bubblewrap_failure(CapabilityStatus::Unavailable(error.to_string()))
}

fn bubblewrap_failure(bubblewrap: CapabilityStatus) -> LinuxAvailability {
    LinuxAvailability {
        bubblewrap,
        user_namespaces: CapabilityStatus::NotChecked,
        landlock_v3: CapabilityStatus::NotChecked,
        seccomp: CapabilityStatus::NotChecked,
        capabilities: CapabilityStatus::NotChecked,
    }
}

fn classify(code: Option<i32>, stderr: &str) -> Result<LinuxAvailability, SandboxError> {
    use CapabilityStatus::{Available, NotChecked, Unavailable};
    let mut result = LinuxAvailability {
        bubblewrap: Available,
        user_namespaces: NotChecked,
        landlock_v3: NotChecked,
        seccomp: NotChecked,
        capabilities: NotChecked,
    };
    if code != Some(125) && stderr.starts_with("bwrap:") {
        result.user_namespaces = Unavailable(stderr.trim().to_owned());
    } else if code == Some(0) {
        result.user_namespaces = Available;
        result.landlock_v3 = Available;
        result.seccomp = Available;
        result.capabilities = Available;
    } else if code == Some(125)
        && stderr.starts_with(&format!("{} {}:", text::SETUP, text::CAPABILITIES))
    {
        result.user_namespaces = Available;
        result.capabilities = Unavailable(stderr.trim().to_owned());
    } else if code == Some(125)
        && stderr.starts_with(&format!("{} {}:", text::SETUP, text::LANDLOCK))
    {
        result.user_namespaces = Available;
        result.capabilities = Available;
        result.landlock_v3 = Unavailable(stderr.trim().to_owned());
    } else if code == Some(125)
        && stderr.starts_with(&format!("{} {}:", text::SETUP, text::SECCOMP))
    {
        result.user_namespaces = Available;
        result.landlock_v3 = Available;
        result.capabilities = Available;
        result.seccomp = Unavailable(stderr.trim().to_owned());
    } else {
        return Err(SandboxError::Setup {
            stage: text::AVAILABILITY,
            detail: format!("{} {code:?}: {}", text::PROBE_FAILED, stderr.trim()),
        });
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    //! 起動の誤りを別の機能の失敗と誤診しない契約（実装プラン P0-07）。
    use super::*;

    // 作成時の関門: 配布物を置き換える差し込み口を作らず、版と setuid の拒否を確かめる。
    // 実機の統合試験だけでは古い版や不正な出力の受理、辞書順での比較を検出できない。
    #[test]
    fn bubblewrap_requires_a_supported_release_without_setuid() {
        use CapabilityStatus::{Available, Unavailable};
        for output in [
            b"bubblewrap 0.12.0\n".as_slice(),
            b"bubblewrap 0.8.0\n",
            b"bubblewrap 0.10.0\n",
            b"bubblewrap 1.0.0\n",
            b"bubblewrap 4294967295.4294967295.4294967295\n",
        ] {
            assert_eq!(check_bubblewrap_version(output), Available, "{output:?}");
        }
        for output in [
            b"bubblewrap 0.7.9\n".as_slice(),
            b"bubblewrap 0.11.0-rc1\n",
            b"bubblewrap 0.11\n",
            b"bubblewrap 0.11.0.1\n",
            b"bwrap 0.11.0\n",
            b"bubblewrap  0.11.0\n",
            b"bubblewrap 0.99999999999.0\n",
            b"bubblewrap 0.4294967296.0\n",
            b"bubblewrap 0.11.0\nextra\n",
            b"bubblewrap 0.11.0\n\n",
            b"bubblewrap 0.11.0",
            b"bubblewrap 0.11.0\r\n",
            b"bubblewrap 0.11.0 \n",
            b"bubblewrap +0.11.0\n",
            b"bubblewrap 0..0\n",
            b"bubblewrap 0.11.\n",
            b"bubblewrap \xff.11.0\n",
            b"",
        ] {
            let status = check_bubblewrap_version(output);
            assert!(
                matches!(status, Unavailable(ref reason) if reason.contains(&format!("{:?}", String::from_utf8_lossy(output)))),
                "{output:?}: {status:?}"
            );
        }
        for (mode, available) in [
            (0o100755, true),
            (0o100775, true),
            (0o102755, true),
            (0o104755, false),
            (0o104700, false),
            (0o106755, false),
        ] {
            assert_eq!(
                check_bubblewrap_mode(mode) == Available,
                available,
                "{mode:o}"
            );
        }
    }

    // 作成時の関門: 本物の機械では再現できない欠落と、後の項目を未確認にする契約。
    #[test]
    fn diagnostics_preserve_the_failed_stage() -> Result<(), SandboxError> {
        use CapabilityStatus::{Available, NotChecked, Unavailable};
        let missing = spawn_failure(io::Error::from(io::ErrorKind::NotFound));
        assert!(matches!(missing.bubblewrap, Unavailable(_)));
        assert_eq!(
            [
                missing.user_namespaces,
                missing.landlock_v3,
                missing.seccomp,
                missing.capabilities
            ],
            [NotChecked, NotChecked, NotChecked, NotChecked]
        );
        for (code, stderr, namespace, landlock, seccomp, capabilities) in [
            (
                1,
                "bwrap: creating new namespace failed",
                Unavailable("bwrap: creating new namespace failed".into()),
                NotChecked,
                NotChecked,
                NotChecked,
            ),
            (
                125,
                "Sandbox setup failed at Landlock: unavailable",
                Available,
                Unavailable("Sandbox setup failed at Landlock: unavailable".into()),
                NotChecked,
                Available,
            ),
            (
                125,
                "Sandbox setup failed at seccomp: unavailable",
                Available,
                Available,
                Unavailable("Sandbox setup failed at seccomp: unavailable".into()),
                Available,
            ),
            (
                125,
                "Sandbox setup failed at capabilities: remaining",
                Available,
                NotChecked,
                NotChecked,
                Unavailable("Sandbox setup failed at capabilities: remaining".into()),
            ),
            (0, "", Available, Available, Available, Available),
        ] {
            assert_eq!(
                classify(Some(code), stderr)?,
                LinuxAvailability {
                    bubblewrap: Available,
                    user_namespaces: namespace,
                    landlock_v3: landlock,
                    seccomp,
                    capabilities,
                }
            );
        }
        for (code, stderr) in [
            (125, "bwrap: unknown"),
            (1, "unexpected"),
            (125, "Sandbox setup failed at exec: denied"),
        ] {
            assert!(classify(Some(code), stderr).is_err());
        }
        Ok(())
    }
}
