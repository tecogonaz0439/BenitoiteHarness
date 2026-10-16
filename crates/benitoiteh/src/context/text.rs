//! 文脈の誤りと、利用者に送信先を知らせる文（設計書 01-04「送る内容の表示」）。

pub const INVALID_SKILL: &str = "Embedded Benitoite Skill metadata is invalid.";
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
