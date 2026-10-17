//! 対話の画面の英文（設計書 05-02）。

use std::time::Duration;

use super::Status;

pub const PROMPT: &str = "> ";
pub const REASONING_OMITTED: &str = "...";
pub const INTERRUPTED: &str = "Interrupted.";
pub const QUIT_HINT: &str = "Press Ctrl+C again within one second to quit.";
pub const OK: &str = "ok";
pub const ERROR: &str = "error";
pub const RETRY: &str = "Retry";
pub const COMPACTION: &str = "Compaction";

pub fn more_lines(count: usize) -> String {
    format!("{count} more lines")
}

pub fn sandbox(status: &Status) -> String {
    if status.sandbox_disabled {
        format!("Sandbox: {} (disabled by --no-sandbox)", status.sandbox)
    } else {
        format!("Sandbox: {}", status.sandbox)
    }
}

pub fn status(status: &Status, elapsed: Option<Duration>) -> String {
    let mode = match status.mode {
        crate::config::Mode::Restricted => "restricted",
        crate::config::Mode::Normal => "normal",
    };
    let context = status.context_window.map_or_else(
        || status.estimate.to_string(),
        |limit| format!("{}/{limit}", status.estimate),
    );
    let elapsed = elapsed.map_or_else(String::new, |value| format!(" | {}s", value.as_secs()));
    format!(
        "{mode} | {}/{} | context {context} | {}{elapsed}",
        status.provider,
        status.model,
        sandbox(status)
    )
}

pub fn instruction_loaded(path: &std::path::Path) -> String {
    super::super::text::instruction_loaded(path)
}
pub fn instruction_skipped(path: &std::path::Path, reason: &str) -> String {
    super::super::text::instruction_skipped(path, reason)
}

pub fn event_detail(label: &str, detail: &impl std::fmt::Display) -> String {
    format!("{label}: {detail}")
}

/// 確認の要約と、操作の事実・説明・利用者の文を区別して示す。
pub fn approval(
    summary: &str,
    facts: &crate::approval::ApprovalFacts,
    justification: Option<&str>,
    user_message: Option<&str>,
) -> String {
    let cwd = facts.cwd.as_ref().map(|path| path.to_string_lossy());
    let path = facts.path.as_ref().map(|path| path.to_string_lossy());
    let mut parts = vec![summary.to_owned()];
    for (label, value) in [
        ("Command", facts.command.as_deref()),
        ("Directory", cwd.as_deref()),
        ("Path", path.as_deref()),
        ("Sandbox", facts.sandbox_policy.as_deref()),
        ("URL", facts.url.as_deref()),
        ("Domain", facts.domain.as_deref()),
        ("Reason", justification),
        ("User", user_message),
    ] {
        if let Some(value) = value {
            parts.push(event_detail(label, &value));
        }
    }
    parts.join("; ")
}

/// 道具の呼び出しを引数から一行にする。制御文字の可視化と切り詰めは描画の境界で行う。
pub fn tool_call(name: &str, arguments: &serde_json::Value) -> String {
    let arg = |key| {
        arguments
            .get(key)
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
    };
    match name {
        "read" => {
            let mut value = format!("read {}", arg("path"));
            if let (Some(offset), Some(limit)) = (
                arguments.get("offset").and_then(serde_json::Value::as_u64),
                arguments.get("limit").and_then(serde_json::Value::as_u64),
            ) {
                value.push_str(&format!(" ({offset}-{})", offset.saturating_add(limit)));
            }
            value
        }
        "glob" => format!("glob {}", arg("pattern")),
        "grep" => format!("grep {} {}", arg("pattern"), arg("path")),
        "write" | "edit" => format!("{name} {}", arg("path")),
        "bash" => format!(
            "bash: {}",
            arg("command").lines().next().unwrap_or_default()
        ),
        "web_fetch" => format!("web_fetch {}", arg("url")),
        _ => format!("{name} {arguments}"),
    }
}
