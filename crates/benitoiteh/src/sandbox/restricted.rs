//! エフェクトから独立した制限モードの方針の入力（設計書 03-02「制限モードの方針」）。

use super::{PolicyConfig, ProgramAccess, ReadAccess, SandboxError, SandboxPolicy};
use crate::config::writable::WritableLocations;
use std::path::PathBuf;

/// 呼び出し元が確定した、今回の起動に限る権限。
#[derive(Clone, Debug)]
pub struct RestrictedAccess {
    /// プロジェクトへの書き込みを許すか。
    pub write_project: bool,
    /// IP の通信を許すか。
    pub allow_ip: bool,
    /// 起動できるプログラムの実のパス。
    pub programs: Vec<PathBuf>,
    /// 読める範囲。
    pub readable: ReadAccess,
}

/// 検査済みの場所から、問い合わせと実行に共通の制限モードの方針を作る。
pub fn restricted_policy(
    access: &RestrictedAccess,
    locations: &WritableLocations,
    denied_candidates: &[PathBuf],
) -> Result<SandboxPolicy, SandboxError> {
    let mut writable = vec![locations.temporary.clone()];
    if access.write_project {
        writable.insert(0, locations.project.clone());
    }
    let mut config = PolicyConfig::new(writable);
    config.readable = access.readable.clone();
    config.allow_ip = access.allow_ip;
    config.programs = ProgramAccess::Only(access.programs.clone());
    for candidate in denied_candidates {
        match std::fs::canonicalize(candidate) {
            Ok(path) => config.denied_read.push(path),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(SandboxError::Io(error)),
        }
    }
    SandboxPolicy::new(config)
}

/// 確認なしの一時スクリプトで読む範囲を狭める（設計書 03-03「一時的なスクリプト」）。
pub fn scratch_policy(
    policy: &SandboxPolicy,
    locations: &WritableLocations,
    processor: &std::path::Path,
) -> Result<SandboxPolicy, SandboxError> {
    let mut config = policy.config().clone();
    config.readable =
        ReadAccess::Roots(vec![locations.project.clone(), locations.temporary.clone()]);
    config.readable_files = vec![processor.into()];
    config.readable_data_roots.clear();
    config.readable_metadata.clear();
    #[cfg(target_os = "macos")]
    {
        // オーケストレータの v0.0.7・macOS 27.0.1 の実測の組を使う（実装プラン P2-09）。
        // 全体のメタデータを許すと、範囲外のファイルの有無や大きさを調べられる。
        if let ReadAccess::Roots(roots) = &mut config.readable {
            for path in [
                "/usr/lib",
                "/System/Library",
                "/System/Volumes/Preboot/Cryptexes",
            ] {
                if let Some(real) = existing_path(path)? {
                    roots.push(real);
                }
            }
        }
        if let Some(real) = existing_path("/private/var/db/timezone")? {
            config.readable_data_roots.push(real);
        }
        config.readable_files.push(PathBuf::from("/"));
        if let Some(real) = existing_path("/private/etc/localtime")? {
            config.readable_files.push(real);
            // 実体だけでは localOffsetMinutes が 0 になる。リンク自身のメタデータも
            // 必要である（設計書 03-03「一時的なスクリプト」、2026-10-18 の点検）。
            config
                .readable_metadata
                .push(PathBuf::from("/private/etc/localtime"));
        }
        let mut metadata = std::collections::BTreeSet::new();
        if let ReadAccess::Roots(roots) = &config.readable {
            for path in roots
                .iter()
                .chain(config.readable_files.iter())
                .chain(config.readable_data_roots.iter())
            {
                metadata.extend(path.ancestors().skip(1).map(std::path::Path::to_path_buf));
            }
        }
        // 時間帯の情報を開く実装が使うリンクの綴りも含める。
        for path in [
            "/etc",
            "/var",
            "/private/etc",
            "/private/var",
            "/private/var/db",
        ] {
            if std::path::Path::new(path).is_dir() {
                metadata.insert(PathBuf::from(path));
            }
        }
        config.readable_metadata.extend(metadata);
    }
    #[cfg(target_os = "linux")]
    {
        // 静的リンクの処理系にはシステムのライブラリは要らない。
        // localOffsetMinutes の値を確認の有無で変えないため、時間帯の実ファイルだけを許す。
        // 必要な組は本物の処理系を起動する sandbox_benitoite_scratch で確かめる。
        if let Some(real) = existing_path("/etc/localtime")? {
            config.readable_files.push(real);
        }
    }
    SandboxPolicy::new(config)
}

fn existing_path(path: &str) -> Result<Option<PathBuf>, SandboxError> {
    match std::fs::canonicalize(path) {
        Ok(real) => Ok(Some(real)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(SandboxError::Io(error)),
    }
}

/// 画像を解く補助を、利用者のファイルと通信から隔離する（設計書 01-03「画像のファイルの読み取り」）。
pub fn image_policy(
    exe: &std::path::Path,
    workdir: &std::path::Path,
) -> Result<SandboxPolicy, SandboxError> {
    let mut config = PolicyConfig::new(vec![workdir.into()]);
    config.readable = ReadAccess::Roots(Vec::new());
    config.readable_files = vec![exe.into()];
    #[cfg(target_os = "macos")]
    config.readable_files.push(PathBuf::from("/"));
    config.programs = ProgramAccess::Only(vec![exe.into()]);
    SandboxPolicy::new(config)
}
