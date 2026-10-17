//! 処理系の探索と問い合わせの英文（設計書 04-01）。

pub const NOT_FOUND: &str = "Could not find benitoite in the configured path or PATH.";
pub const ABSOLUTE: &str = "benitoite.path must be an absolute path.";
pub const INVALID_PROGRAM: &str = "The program must be a native executable outside writable locations (statically linked on Linux).";
pub const BANNED_PROGRAM: &str = "The program name is not allowed in restricted mode.";
pub const INVALID_JSON: &str =
    "benitoite did not return one complete JSON object with the expected schemaVersion and kind.";
pub const UNSUPPORTED: &str = "benitoite does not support the required version 1 formats.";
pub const OUTPUT_LIMIT: &str = "The inspection report from benitoite exceeded 16 MiB and was not read. Pass fewer or narrower paths.";
pub const TIMED_OUT: &str = "The benitoite query timed out.";
pub const CANCELLED: &str = "The benitoite query was cancelled.";
pub const FAILED: &str = "The benitoite query failed.";
pub const IO: &str = "Could not inspect benitoite:";
pub const DISABLED: &str = "Benitoite tools are disabled:";

/// 対応を確かめた版との差を利用者へ示す。
pub fn version_warning(version: &str) -> String {
    format!(
        "Warning: benitoite {version} differs from the prompt-verified version {}.",
        super::PROMPT_VERIFIED_VERSION
    )
}

/// 前回と今回の処理系の版を添えてハッシュの変化を示す。
pub fn changed(previous: &str, current: &str) -> String {
    format!(
        "The benitoite executable changed (SHA-256 differs; previous version {previous}, current version {current})."
    )
}

pub const LEFTOVERS: &str = "The benitoite query may have left child processes running.";
pub const PREVIOUS_UNREADABLE: &str = "Could not read the previous session's benitoite identity; the executable change comparison was skipped.";
