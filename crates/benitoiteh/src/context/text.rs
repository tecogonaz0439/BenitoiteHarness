//! 文脈の誤りと、利用者に送信先を知らせる文（設計書 01-04「送る内容の表示」）。

pub const INVALID_SKILL: &str = "The Benitoite Skill from the processor has an invalid format.";
pub const INVALID_PATH: &str = "Environment paths must be absolute UTF-8 paths.";
pub const INVALID_DATE: &str = "Environment date must be between 1970-01-01 and 9999-12-31 UTC.";
pub const JSON_ERROR: &str = "Cannot serialize tool definitions";
pub const NOT_CONFIGURED: &str = "not configured";

pub fn mode(mode: crate::config::Mode) -> &'static str {
    match mode {
        crate::config::Mode::Restricted => "restricted",
        crate::config::Mode::Normal => "normal",
    }
}

pub fn network(allowed: bool) -> &'static str {
    if allowed { "allowed" } else { "denied" }
}

pub fn transmission_notice(provider: &str, model: &str, host: &str) -> String {
    // JSON の引用で改行や制御文字を一行のまま見せる。
    format!(
        "Provider: {}; model: {}; host: {}. Read file contents, script and command output, and diagnostics are sent to this provider.",
        serde_json::json!(provider),
        serde_json::json!(model),
        serde_json::json!(host)
    )
}

pub const SKILL_MISMATCH: &str = "The Benitoite Skill must be given exactly when the Benitoite tools are enabled, and restricted mode requires both.";

/// 雛形を読まなかったことを示す（設計書 05-02「利用者の定義するコマンド」）。
pub fn prompt_skipped(path: &std::path::Path, reason: &str) -> String {
    format!("Skipped prompt template {}: {reason}", path.display())
}
pub const PROMPT_LINKED_DIRECTORY: &str = "the directory is reached through a symbolic link";
pub const PROMPT_NOT_REGULAR: &str = "not a regular file";
pub const PROMPT_INVALID_NAME: &str = "names must start with a lowercase letter and use only lowercase letters, digits and hyphens (up to 64)";
pub const PROMPT_RESERVED: &str = "the name is a built-in command";
pub const PROMPT_DUPLICATE: &str = "a template with the same name was loaded first";
pub const PROMPT_COUNT_LIMIT: &str = "more than 200 templates";
pub const PROMPT_HARD_LINKS: &str = "the file has more than one hard link";
pub const PROMPT_FILE_LIMIT: &str = "larger than 64 KiB";
pub const PROMPT_NOT_UTF8: &str = "not UTF-8";
