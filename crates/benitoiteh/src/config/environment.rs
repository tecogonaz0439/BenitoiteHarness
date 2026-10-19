//! 起動の形によらず、秘密を除いた子プロセスの環境を作る（設計書 03-02「子プロセスの環境」、04-01「処理系の起動」）。

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::path::Path;

use super::PROVIDER_KEY_ENV_VARS;

/// 親から採った環境からキーと古い印を除き、今回の起動の値を設定する。
/// `sandbox` は方針の名前と IP 通信の可否。外で動かすときは `None` とする。
pub fn child_environment(
    inherited: impl IntoIterator<Item = (OsString, OsString)>,
    referenced_keys: &BTreeSet<String>,
    temporary: &Path,
    sandbox: Option<(&str, bool)>,
) -> BTreeMap<OsString, OsString> {
    let mut environment: BTreeMap<_, _> = inherited
        .into_iter()
        .filter(|(name, _)| {
            !PROVIDER_KEY_ENV_VARS.iter().any(|key| name == key)
                && !referenced_keys.iter().any(|key| name == key.as_str())
        })
        .collect();
    // 外側の benitoiteh の印を引き継ぐと、今回の制限を誤って伝える。
    environment.remove(std::ffi::OsStr::new(crate::askpass::SOCKET_ENV));
    environment.remove(std::ffi::OsStr::new(crate::askpass::TOKEN_ENV));
    environment.remove(std::ffi::OsStr::new("BENITOITEH_SANDBOX"));
    environment.remove(std::ffi::OsStr::new("BENITOITEH_SANDBOX_NETWORK_DISABLED"));
    environment.insert("TMPDIR".into(), temporary.as_os_str().to_owned());
    for (name, value) in [
        ("NO_COLOR", "1"),
        ("TERM", "dumb"),
        ("PAGER", "cat"),
        ("GIT_PAGER", "cat"),
    ] {
        environment.insert(name.into(), value.into());
    }
    if let Some((name, allow_ip)) = sandbox {
        environment.insert("BENITOITEH_SANDBOX".into(), name.into());
        if !allow_ip {
            environment.insert("BENITOITEH_SANDBOX_NETWORK_DISABLED".into(), "1".into());
        }
    }
    environment
}
