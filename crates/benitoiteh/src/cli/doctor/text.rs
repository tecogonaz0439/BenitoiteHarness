//! doctor の診断に使う英文（設計書 05-01「doctor」）。

use std::path::Path;

use crate::config::writable::WritableError;

pub const PROBE_OUTPUT_LIMIT: &str =
    "The sandbox rule check exceeded the 1 MiB stdout limit; the child was stopped.";
pub const PROCESSOR_CHANGED: &str = "The Benitoite executable changed during the doctor checks (SHA-256 mismatch); version and Skill results were rejected.";
pub const PROCESSOR_REQUIRED: &str = "Requires a discovered and validated Benitoite executable.";
pub const PROCESSOR_SANDBOX_REQUIRED: &str =
    "Requires a successful restricted mode sandbox rule check; Benitoite was not queried.";
pub const PROCESSOR_VERSION_REQUIRED: &str =
    "Requires a successful Benitoite version and formats check.";
pub const PROCESSOR_RULE_OK: &str = "benitoite --version=json succeeded and /bin/sh was denied.";
pub const PROCESSOR_POSITIVE_FAILED: &str = "The allowed benitoite --version=json check failed.";
pub const FALLBACK_RULE: &str = "This checks benitoiteh itself, not Benitoite.";
pub const UNKNOWN: &str = "unknown";
pub const DEFAULTS: &str = "No configuration file; using defaults.";
pub const LOADED: &str = "Configuration loaded.";
pub const CONFIG_REQUIRED: &str = "Requires readable user configuration and data paths.";
pub const PROJECT_REQUIRED: &str = "Requires a usable project directory; see the writable item.";
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
pub const RESTRICTED_FALLBACK_OK: &str = "Restricted mode sandbox: available (checked with benitoiteh itself; Benitoite was not found or failed validation).";
pub const RESTRICTED_UNCHECKED_OK: &str = "Restricted mode sandbox: available (checked with benitoiteh itself; Benitoite was not checked).";
pub const RESTRICTED_OK: &str = "Restricted mode sandbox: available.";
pub const RESTRICTED_UNAVAILABLE: &str = "Restricted mode sandbox: unavailable.";
pub const NORMAL_ENABLED: &str = "Normal mode sandbox: enabled.";
pub const NORMAL_DISABLED: &str = "Normal mode sandbox: disabled by configuration.";
pub const NORMAL_UNAVAILABLE: &str = "Normal mode sandbox: unavailable.";

/// プロジェクト自身が拒否されたときだけ、場所の選択元と対処を添える。
pub fn writable_error(project: &Path, from_option: bool, error: &WritableError) -> String {
    match error {
        WritableError::Rejected { path, .. } if path == project => {
            format!(
                "{} Run doctor in a project directory or pass --project <dir>.",
                super::super::text::project_unusable(project, from_option, error)
            )
        }
        WritableError::Resolve { .. } | WritableError::Rejected { .. } => error.to_string(),
    }
}

/// 探索の出どころと、設定があればそのパスも示す。
pub fn discovery(configured: Option<&Path>, result: &str) -> String {
    match configured {
        Some(path) => format!(
            "source=config.benitoite.path; configured={}; {result}",
            path.display()
        ),
        None => format!("source=PATH; {result}"),
    }
}

/// 形の検査を通った実行ファイルの実のパスとハッシュを示す。
pub fn executable(path: &Path, sha256: &str) -> String {
    #[cfg(target_os = "linux")]
    let format = "native executable; no shebang; outside writable roots; statically linked";
    #[cfg(not(target_os = "linux"))]
    let format = "native executable; no shebang; outside writable roots";
    format!("{}; {format}; sha256={sha256}", path.display())
}

/// 処理系の自己申告の情報を示す。ハッシュによる同一性の検査とは区別する。
pub fn version(version: &str, commit: &str, dirty: &str, target: &str) -> String {
    format!("version={version}; build.commit={commit}; build.dirty={dirty}; build.target={target}")
}

/// 本文を出さず、検証した Skill のハッシュを示す。
pub fn skill(hash: &str) -> String {
    format!("SKILL.md verified; contentHash=sha256:{hash}")
}

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

/// 子の診断で偽の行を作らせず、失敗した確かめの名前と長さを制限する。
pub fn probe_failed(reason: &str, stderr: &str) -> String {
    crate::benitoite::display::one_line(&format!("{reason} {stderr}"))
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

pub const CHATGPT_MISSING: &str = "Not signed in to ChatGPT; run benitoiteh login chatgpt.";
pub const CHATGPT_STORAGE: &str =
    "Saved ChatGPT authorization is unreadable or has unsafe permissions.";

/// トークンを含めず、表示用の情報と期限だけを示す。
pub fn chatgpt(email: Option<&str>, expires_at: u64, direct: bool) -> String {
    let email = crate::ui::terminal_text(email.unwrap_or("unknown"), false);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let expiry = if expires_at <= now {
        "; expired; refresh on next request"
    } else {
        ""
    };
    format!(
        "Signed in to ChatGPT; email={email}; access token expires at {}{expiry}; chatgpt.tokens.use.direct={direct}.",
        utc(expires_at)
    )
}

fn utc(seconds: u64) -> String {
    // 保存した値の上限を設け、壊れた期限で長時間ループしない。
    if seconds > 253_402_300_799 {
        return UNKNOWN.into();
    }
    let mut days = seconds / 86_400;
    let leap = |y: u64| y.is_multiple_of(4) && (!y.is_multiple_of(100) || y.is_multiple_of(400));
    let mut year = 1970u64;
    loop {
        let length = if leap(year) { 366 } else { 365 };
        if days < length {
            break;
        }
        days = days.saturating_sub(length);
        year = year.saturating_add(1);
    }
    let mut month = 1u64;
    for length in [
        31,
        if leap(year) { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ] {
        if days < length {
            break;
        }
        days = days.saturating_sub(length);
        month = month.saturating_add(1);
    }
    format!(
        "{year:04}-{month:02}-{:02}T{:02}:{:02}:{:02}Z",
        days.saturating_add(1),
        seconds / 3600 % 24,
        seconds / 60 % 60,
        seconds % 60
    )
}
