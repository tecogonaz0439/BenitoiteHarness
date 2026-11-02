//! 起動の形によらず、秘密を除いた子プロセスの環境を作る（設計書 03-02「子プロセスの環境」、04-01「処理系の起動」）。

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::{OsStr, OsString};
use std::path::Path;

use super::PROVIDER_KEY_ENV_VARS;

/// 名前に含まれると秘密とみなす語。ASCII の大文字と小文字を区別せずに比べる。
/// Codex CLI の既定の除外（`*KEY*`・`*SECRET*`・`*TOKEN*`）に、パスワードと資格情報を足した。
/// （設計書 03-02「子プロセスの環境」）
const SECRET_NAME_PARTS: &[&[u8]] = &[
    b"KEY",
    b"SECRET",
    b"TOKEN",
    b"PASSWORD",
    b"PASSWD",
    b"CREDENTIAL",
];

/// 親から採った環境から、提供者のキー、秘密に見える名前（`passed` に書いたものを除く）、
/// 古い印を除き、今回の起動の値を設定する。
/// `sandbox` は方針の名前と IP 通信の可否。外で動かすときは `None` とする。
pub fn child_environment(
    inherited: impl IntoIterator<Item = (OsString, OsString)>,
    referenced_keys: &BTreeSet<String>,
    passed: &BTreeSet<String>,
    temporary: &Path,
    sandbox: Option<(&str, bool)>,
) -> BTreeMap<OsString, OsString> {
    let environment = inherited
        .into_iter()
        .filter(|(name, _)| {
            !referenced_keys.iter().any(|key| name == key.as_str())
                && (!secret_like(name) || passed.iter().any(|key| name == key.as_str()))
        })
        .collect();
    remark_child_environment(environment, temporary, sandbox)
}

/// すでに [`child_environment`] で作った環境の、今回の起動の印と値だけを付け直す。
/// 提供者のキーは、念のため改めて除く。
pub fn remark_child_environment(
    mut environment: BTreeMap<OsString, OsString>,
    temporary: &Path,
    sandbox: Option<(&str, bool)>,
) -> BTreeMap<OsString, OsString> {
    environment.retain(|name, _| !PROVIDER_KEY_ENV_VARS.iter().any(|key| name == key));
    // 外側の benitoiteh の印を引き継ぐと、今回の制限を誤って伝える。
    environment.remove(OsStr::new(crate::askpass::SOCKET_ENV));
    environment.remove(OsStr::new(crate::askpass::TOKEN_ENV));
    environment.remove(OsStr::new("BENITOITEH_SANDBOX"));
    environment.remove(OsStr::new("BENITOITEH_SANDBOX_NETWORK_DISABLED"));
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

/// 名前が秘密に見えるか。UTF-8 でない名前も、バイト列として比べる。
fn secret_like(name: &OsStr) -> bool {
    let name = name.as_encoded_bytes().to_ascii_uppercase();
    SECRET_NAME_PARTS
        .iter()
        .any(|part| name.windows(part.len()).any(|window| window == *part))
}
