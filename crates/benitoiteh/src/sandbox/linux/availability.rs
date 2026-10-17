//! 実際の起動の結果から Linux の必須機能を個別に診断する（設計書 03-02「サンドボックスを掛けられないとき」）。

use std::collections::BTreeMap;
use std::fs::{self, DirBuilder};
use std::io;
use std::os::unix::fs::DirBuilderExt;
use std::path::Path;

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
}

/// 一時ディレクトリだけを書ける場所として、補助の実行ファイル自身を `--version` で起動する。
/// `scratch` は一時ディレクトリを排他に作れる、実の絶対パスのディレクトリ。
pub fn probe_linux(helper: &Path, scratch: &Path) -> Result<LinuxAvailability, SandboxError> {
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
    LinuxAvailability {
        bubblewrap: CapabilityStatus::Unavailable(error.to_string()),
        user_namespaces: CapabilityStatus::NotChecked,
        landlock_v3: CapabilityStatus::NotChecked,
        seccomp: CapabilityStatus::NotChecked,
    }
}

fn classify(code: Option<i32>, stderr: &str) -> Result<LinuxAvailability, SandboxError> {
    use CapabilityStatus::{Available, NotChecked, Unavailable};
    let mut result = LinuxAvailability {
        bubblewrap: Available,
        user_namespaces: NotChecked,
        landlock_v3: NotChecked,
        seccomp: NotChecked,
    };
    if code != Some(125) && stderr.starts_with("bwrap:") {
        result.user_namespaces = Unavailable(stderr.trim().to_owned());
    } else if code == Some(0) {
        result.user_namespaces = Available;
        result.landlock_v3 = Available;
        result.seccomp = Available;
    } else if code == Some(125)
        && stderr.starts_with(&format!("{} {}:", text::SETUP, text::LANDLOCK))
    {
        result.user_namespaces = Available;
        result.landlock_v3 = Unavailable(stderr.trim().to_owned());
    } else if code == Some(125)
        && stderr.starts_with(&format!("{} {}:", text::SETUP, text::SECCOMP))
    {
        result.user_namespaces = Available;
        result.landlock_v3 = Available;
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
                missing.seccomp
            ],
            [NotChecked, NotChecked, NotChecked]
        );
        for (code, stderr, namespace, landlock, seccomp) in [
            (
                1,
                "bwrap: creating new namespace failed",
                Unavailable("bwrap: creating new namespace failed".into()),
                NotChecked,
                NotChecked,
            ),
            (
                125,
                "Sandbox setup failed at Landlock: unavailable",
                Available,
                Unavailable("Sandbox setup failed at Landlock: unavailable".into()),
                NotChecked,
            ),
            (
                125,
                "Sandbox setup failed at seccomp: unavailable",
                Available,
                Available,
                Unavailable("Sandbox setup failed at seccomp: unavailable".into()),
            ),
            (0, "", Available, Available, Available),
        ] {
            assert_eq!(
                classify(Some(code), stderr)?,
                LinuxAvailability {
                    bubblewrap: Available,
                    user_namespaces: namespace,
                    landlock_v3: landlock,
                    seccomp,
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
