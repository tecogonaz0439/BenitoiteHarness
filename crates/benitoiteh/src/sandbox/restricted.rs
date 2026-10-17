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
