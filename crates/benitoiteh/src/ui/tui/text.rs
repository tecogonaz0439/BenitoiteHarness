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

pub const COMMAND: &str = "Command";
pub const DIRECTORY: &str = "Directory";
pub const PATH: &str = "Path";
pub const SANDBOX: &str = "Sandbox";
pub const URL: &str = "URL";
pub const DOMAIN: &str = "Domain";
pub const MODEL_REASON_HEADING: &str = "┆ Reason given by the model:";
pub const JUSTIFICATION_OMITTED: &str = "┆ ... remaining justification omitted.";
pub const MESSAGE_TOO_LONG: &str = "Message input is limited to 64 KiB; excess input was rejected.";
pub const WARNING: &str = "Warning";
pub const APPROVAL_BORDER: &str = "╰────────────────────────────";
pub const MODE_CHANGE: &str = "╭─ Resume in normal mode?";
pub const MODE_CHANGE_WARNING: &str = "The previous context may contain untrusted content. Normal mode allows shell commands and broader file access.";
pub const DECLINE_MESSAGE: &str = "Decline with a message: ";
pub const CANCEL_EFFECTS: &str =
    "Operations already performed by the child process are not undone.";
pub const COMPACTED_HISTORY: &str = "Earlier conversation was compacted.";

pub fn approval_title(kind: crate::approval::ApprovalKind) -> &'static str {
    use crate::approval::ApprovalKind;
    match kind {
        ApprovalKind::Command => "╭─ Run command?",
        ApprovalKind::Escalate => "╭─ Run outside the sandbox?",
        ApprovalKind::WriteScope => "╭─ Write outside the allowed scope?",
        ApprovalKind::WebFetch => "╭─ Fetch web page?",
    }
}

pub fn answer_keys(kind: Option<crate::approval::ApprovalKind>) -> &'static str {
    use crate::approval::ApprovalKind;
    match kind {
        Some(ApprovalKind::Command) => {
            "[y] run  [a] allow this exact command for this session  [n] decline  [m] decline with a message"
        }
        Some(ApprovalKind::Escalate) => {
            "[y] run outside the sandbox once  [n] decline  [m] decline with a message"
        }
        Some(ApprovalKind::WriteScope) => "[y] write  [n] decline  [m] decline with a message",
        Some(ApprovalKind::WebFetch) => {
            "[y] fetch once  [s] allow this domain for this session  [n] decline  [m] decline with a message"
        }
        None => "[y] resume in normal mode  [n] decline",
    }
}

pub fn fact_heading(label: &str, lines: usize) -> String {
    if lines > 1 {
        format!("│ {label}: ({lines} lines)")
    } else {
        format!("│ {label}:")
    }
}
pub fn queued(count: usize) -> String {
    format!(" | follow-ups {count}")
}
pub fn history_failed(error: &impl std::fmt::Display) -> String {
    format!("Input history disabled: {error}")
}

pub const HISTORY_UNSAFE: &str =
    "Input history has unsafe permissions, ownership, type, contents, or size.";

pub fn facts_do_not_fit(lines: usize, characters: usize) -> String {
    format!(
        "{lines} fact lines ({characters} characters) do not fit. Enlarge the terminal; approval is disabled."
    )
}

pub fn spaces(count: usize) -> String {
    format!("␠×{count}")
}

pub fn decline_keys(kind: Option<crate::approval::ApprovalKind>) -> &'static str {
    if kind.is_some() {
        "[n] decline  [m] decline with a message"
    } else {
        "[n] decline"
    }
}
