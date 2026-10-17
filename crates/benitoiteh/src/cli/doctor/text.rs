//! doctor の診断に使う英文（設計書 05-01「doctor」）。

use std::path::Path;

pub const BENITOITE: &str = "Not checked until Benitoite integration is implemented.";
pub const DEFAULTS: &str = "No configuration file; using defaults.";
pub const LOADED: &str = "Configuration loaded.";
pub const CONFIG_REQUIRED: &str = "Requires readable user configuration and data paths.";
pub const NO_PROVIDERS: &str = "No providers configured; exec cannot start.";
pub const INVALID_DEFAULT: &str =
    "The default provider is missing or does not name a configured provider.";
pub const NO_KEY: &str = "No API key reference configured.";
pub const KEY_OK: &str = "API key reference resolved.";
pub const KEY_INVALID: &str = "Invalid API key reference.";
pub const KEY_OUTPUT: &str = "API key command has no usable first output line.";
pub const NO_CONTEXT: &str = "No context_window; compaction before a turn is disabled.";
pub const INVALID_URL: &str = "Invalid base URL (value omitted).";
pub const WRITABLE_OK: &str = "Writable roots passed validation.";
pub const WRITABLE_REQUIRED: &str = "Requires validated writable roots.";
pub const DISABLED: &str = "Sandbox disabled by configuration (danger-full-access).";
pub const NORMAL_OK: &str =
    "Policy built; sandbox application verified by the restricted rule check.";
pub const NO_SANDBOX: &str = "Sandbox unavailable; normal mode requires an explicit --no-sandbox option to continue without it.";
pub const RULE_OK: &str = "benitoiteh --version succeeded and /bin/sh was denied.";
pub const RULE_REQUIRED: &str = "Requires all sandbox features and empty capability sets.";
pub const SHELL_MISSING: &str = "Cannot check denial: /bin/sh does not exist.";
pub const RULE_FAILED: &str = "The shell denial check did not confirm an execution refusal.";
pub const POSITIVE_FAILED: &str = "The allowed benitoiteh --version check failed.";
#[cfg(target_os = "linux")]
pub const DYNAMIC: &str = "The allowed benitoiteh --version check failed; a dynamically linked executable needs a loader that the allowlist denies. Use a statically linked benitoiteh.";
#[cfg(target_os = "linux")]
pub const LINUX_SCOPE: &str = "Linux protects .git and .benitoiteh at writable roots and existing nested directories (by default, three levels); newly created nested repositories are outside that protection, unlike macOS.";
#[cfg(target_os = "linux")]
pub const USER_NAMESPACES: &str = "Unprivileged user namespaces created.";
#[cfg(target_os = "linux")]
pub const LANDLOCK: &str = "Landlock ABI V3 or newer rules applied (ABI number not queried).";
#[cfg(target_os = "linux")]
pub const SECCOMP: &str = "seccomp filter applied.";
#[cfg(target_os = "linux")]
pub const CAPABILITIES: &str =
    "Effective, permitted, inheritable and ambient capability sets are empty.";
#[cfg(target_os = "linux")]
pub const REQUIRES_BWRAP: &str = "Requires bubblewrap.";
#[cfg(target_os = "linux")]
pub const REQUIRES_NAMESPACES: &str = "Requires user namespaces.";
#[cfg(target_os = "linux")]
pub const REQUIRES_CAPABILITIES: &str = "Requires empty capability sets.";
#[cfg(target_os = "linux")]
pub const REQUIRES_LANDLOCK: &str = "Requires Landlock.";
pub const DIRECTORY_UNSAFE: &str =
    "Expected a real directory owned by the effective user with mode 0700.";
pub const DIRECTORY_OK: &str = "Directory is owned by the effective user with mode 0700.";
pub const DIRECTORY_MISSING: &str = "Not created yet.";
pub const STORAGE_REQUIRED: &str = "Requires a safe sessions directory.";
pub const TEMPORARY_LOCATION: &str =
    "The doctor temporary directory must be outside the project and data directories.";
pub const RESTRICTED_OK: &str = "Restricted mode sandbox: available (Benitoite not checked).";
pub const RESTRICTED_UNAVAILABLE: &str =
    "Restricted mode sandbox: unavailable (Benitoite not checked).";
pub const NORMAL_ENABLED: &str = "Normal mode sandbox: enabled.";
pub const NORMAL_DISABLED: &str = "Normal mode sandbox: disabled by configuration.";
pub const NORMAL_UNAVAILABLE: &str = "Normal mode sandbox: unavailable.";

/// プロジェクトの設定を一部も採用しないことを示す。
pub fn ignored(error: &impl std::fmt::Display) -> String {
    format!("Ignoring the entire project configuration: {error}")
}

/// 診断の対象の場所を添える。
pub fn at(path: &Path, detail: &str) -> String {
    format!("{}: {detail}", path.display())
}

/// 値と長さを含めず、参照した環境変数だけを示す。
pub fn environment(name: &str) -> String {
    format!("API key environment variable {name} is missing or empty.")
}

/// OS の誤りに含まれうるコマンドの引数を除く。
pub fn spawn(kind: std::io::ErrorKind) -> String {
    format!("Could not start API key command ({kind:?}).")
}

/// コマンドの出力を含めず、終了状態だけを示す。
pub fn command(status: Option<i32>) -> String {
    match status {
        Some(status) => format!("API key command exited with status {status}."),
        None => "API key command ended without an exit code.".to_owned(),
    }
}

/// 認証の情報を除いた送り先と選択の状態を示す。
pub fn provider(kind: &str, url: &str, model: &str, default: bool) -> String {
    format!("adapter={kind}; base_url={url}; model={model}; default={default}")
}

/// 論理的な保存量と上限に対する割合を示す。
pub fn storage(bytes: u64, limit: u64) -> String {
    let percent = u128::from(bytes)
        .saturating_mul(10_000)
        .checked_div(u128::from(limit))
        .unwrap_or(0);
    format!(
        "{bytes} / {limit} bytes ({}.{:02}% of 10 GiB).",
        percent / 100,
        percent % 100
    )
}

/// 信頼するサンドボックスの診断に、失敗した確かめの名前を添える。
pub fn probe_failed(reason: &str, stderr: &str) -> String {
    format!("{reason} {stderr}")
}

#[cfg(target_os = "linux")]
/// 起動と共用した検査の結果から版を示す。
pub fn bubblewrap(version: &str) -> String {
    format!("/usr/bin/bwrap: {version}; >= 0.8.0; no setuid bit.")
}

/// 端末用に一項目を一行にする。
pub fn item(status: &str, name: &str, detail: &str) -> String {
    format!("[{status}] {name}: {detail}")
}
