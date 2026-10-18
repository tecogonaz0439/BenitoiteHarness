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
        ApprovalKind::Effects => "╭─ Run Benitoite script?",
        ApprovalKind::TestEffects => "╭─ Run Benitoite tests?",
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
        Some(ApprovalKind::Effects | ApprovalKind::TestEffects) => {
            "[y] run  [n] decline  [m] decline with a message"
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

pub const UNKNOWN_COMMAND: &str = "Unknown command. Type /help for available commands.";
pub const COMMAND_ARGUMENTS: &str = "This command does not accept arguments. Type /help for usage.";
pub const COMMAND_RUNNING: &str = "This command is unavailable while processing.";
pub const STARTING_NEW: &str = "Starting a new session...";
pub const NEW_SESSION_REASON: &str = "New session started.";
pub const COMPACTING: &str = "Compacting...";
pub const REWINDING: &str = "Rewinding...";
pub const WAIT_REWIND: &str = "Waiting for rewind to finish; rewind cannot be cancelled.";
pub const NO_CHECKPOINTS: &str = "No checkpoints available.";
pub const REWIND_CANCELLED: &str = "Rewind cancelled.";
pub const SELECT_CHECKPOINT: &str = "Select a checkpoint (UTC)";
pub const SELECT_SCOPE: &str = "Select rewind scope";
pub const SELECT_KEYS: &str = "[up/down] select  [Enter] continue  [ESC] cancel";
pub const FILES_AND_CONVERSATION: &str = "files and conversation";
pub const FILES_ONLY: &str = "files only";
pub const CONVERSATION_ONLY: &str = "conversation only";
pub const NOT_AVAILABLE: &str = "(not available)";
pub const REWIND_TITLE: &str = "╭─ Rewind to checkpoint?";
pub const INCOMPLETE_TITLE: &str = "╭─ Rewind with an incomplete backup?";
pub const CHECKPOINT: &str = "Checkpoint";
pub const SCOPE: &str = "Scope";
pub const BACKUP: &str = "Before rewind";
pub const BACKUP_NOTICE: &str =
    "The current files will be recorded as a checkpoint before restoring files.";
pub const INCOMPLETE_TARGET: &str =
    "The selected checkpoint is incomplete; added files will not be deleted.";
pub const INCOMPLETE_BACKUP: &str =
    "The backup before rewind is incomplete. The current state may not be recoverable.";
pub const REWIND_KEYS: &str = "[y] rewind  [n] cancel";
pub const INCOMPLETE_KEYS: &str = "[f] rewind anyway  [n] cancel";
pub const REWIND_DECLINE: &str = "[n] cancel";
pub const IRREVERSIBLE: &str = "Operations that cannot be undone (including external effects):";
pub const INCOMPLETE_REASONS: &str = "Reasons the backup is incomplete:";
pub const HELP: &str = "/help                  Show commands and input controls.
/status                Show session status and token usage.
/compact [instructions] Compact the conversation.
/rewind                Select a checkpoint and restore files or conversation.
/new                   Start a new session.
/quit                  Quit.
Enter: send a request, or steer while processing.
Alt+Enter: queue a follow-up while processing.
Shift+Enter or Ctrl+J: insert a newline.
ESC: cancel the current step; twice: cancel the run.
Ctrl+C: clear input; twice on empty input: quit.
Up/Down: browse input history when input is one line.
//: send a literal leading slash.";

pub fn scope(scope: crate::rewind::RewindScope) -> &'static str {
    use crate::rewind::RewindScope;
    match scope {
        RewindScope::FilesAndConversation => FILES_AND_CONVERSATION,
        RewindScope::Files => FILES_ONLY,
        RewindScope::Conversation => CONVERSATION_ONLY,
    }
}

pub fn session_status(
    status: &crate::agent::AgentStatus,
    tokens: &crate::session::TokenCounts,
) -> String {
    let sandbox = if status.mode == crate::config::Mode::Restricted {
        "restricted"
    } else {
        status.sandbox.name()
    };
    let disabled = if status.sandbox_disabled {
        " (disabled by --no-sandbox)"
    } else {
        ""
    };
    let approval = match status.approval {
        crate::config::Approval::Untrusted => "untrusted",
        crate::config::Approval::OnRequest => "on-request",
        crate::config::Approval::Never => "never",
    };
    let mode = match status.mode {
        crate::config::Mode::Normal => "normal",
        crate::config::Mode::Restricted => "restricted",
    };
    let limit = status
        .context_window
        .map_or_else(|| "unknown".into(), |value| value.to_string());
    let mut lines = vec![
        format!("Session: {}", status.session_id),
        format!("Mode: {mode}"),
        format!("Provider: {}", status.provider),
        format!("Model: {}", status.model),
        format!("Host: {}", status.host),
        interpreter(
            status.benitoite_path.as_deref(),
            status.benitoite_version.as_deref(),
        ),
        format!("Sandbox: {sandbox}{disabled}"),
        format!("Approval: {approval}"),
        format!(
            "Tokens (this process): input {}, cache read {}, cache write {}, output {}",
            tokens.input, tokens.cache_read, tokens.cache_write, tokens.output
        ),
        format!("Context: {} / {limit}", status.estimate),
        "Loaded project instructions:".into(),
    ];
    lines.extend(
        status
            .instruction_files
            .iter()
            .map(|path| format!("  {}", path.display())),
    );
    lines.push("Skipped project instructions:".into());
    lines.extend(
        status
            .skipped_instructions
            .iter()
            .map(|entry| format!("  {}: {}", entry.path.display(), entry.reason)),
    );
    lines
        .iter()
        .map(|line| crate::ui::terminal_text(line, false))
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn compact_result(report: &crate::agent::CompactReport) -> String {
    use crate::agent::CompactOutcome;
    let result = match report.outcome {
        CompactOutcome::Compacted => "completed",
        CompactOutcome::NothingToCompact => "nothing to compact",
        CompactOutcome::Cancelled => "cancelled",
        CompactOutcome::Failed => "failed",
    };
    format!(
        "Compaction {result}: {} items summarized, {} items kept.",
        report.summarized_items, report.kept_items
    )
}

pub fn checkpoint_row(point: &crate::rewind::CheckpointInfo) -> String {
    let checkpoint = &point.checkpoint;
    let reason = match checkpoint.reason {
        crate::rewind::CheckpointReason::Request => "request",
        crate::rewind::CheckpointReason::Bash => "bash",
        crate::rewind::CheckpointReason::BenitoiteRun => "benitoite_run",
        crate::rewind::CheckpointReason::BenitoiteTest => "benitoite_test",
        crate::rewind::CheckpointReason::BenitoiteScratch => "benitoite_scratch",
        crate::rewind::CheckpointReason::Write => "write",
        crate::rewind::CheckpointReason::Edit => "edit",
        crate::rewind::CheckpointReason::BeforeRewind => "before rewind",
    };
    let detail = point
        .tool
        .as_ref()
        .map(|call| tool_call(&call.name, &call.arguments))
        .or_else(|| point.request.clone())
        .unwrap_or_else(|| NOT_AVAILABLE.into());
    let seconds = checkpoint.created_at.seconds;
    // UTC は外部のタイムゾーンの設定に依存しない形で日付と時刻にする。
    let days = seconds.div_euclid(86400);
    let z = days.saturating_add(719468);
    let era = z.div_euclid(146097);
    let doe = z.saturating_sub(era.saturating_mul(146097));
    let yoe = doe
        .saturating_sub(doe / 1460)
        .saturating_add(doe / 36524)
        .saturating_sub(doe / 146096)
        / 365;
    let year = yoe.saturating_add(era.saturating_mul(400));
    let doy = doe.saturating_sub(
        365_i64
            .saturating_mul(yoe)
            .saturating_add(yoe / 4)
            .saturating_sub(yoe / 100),
    );
    let mp = 5_i64.saturating_mul(doy).saturating_add(2) / 153;
    let day = doy
        .saturating_sub(153_i64.saturating_mul(mp).saturating_add(2) / 5)
        .saturating_add(1);
    let month = mp.saturating_add(if mp < 10 { 3 } else { -9 });
    let year = year.saturating_add(i64::from(month <= 2));
    let time = seconds.rem_euclid(86400);
    format!(
        "{} | turn {} | {reason} | {year:04}-{month:02}-{day:02} {:02}:{:02}:{:02} UTC | {detail} | conversation: {} | incomplete: {}",
        checkpoint.id,
        checkpoint.turn,
        time / 3600,
        time / 60 % 60,
        time % 60,
        point.conversation_available,
        !checkpoint.incomplete.is_empty()
    )
}

pub fn more(count: usize) -> String {
    format!("{count} more")
}
pub fn rewind_result(report: &crate::rewind::RewindReport) -> String {
    format!(
        "Rewind completed: {} restored, {} removed, {} created.",
        report.restored, report.removed, report.created
    )
}
pub fn rewind_failure(failure: &crate::rewind::RewindFailure) -> String {
    format!("{}: {}", failure.path.display(), failure.reason)
}

pub fn selection_row(selected: bool, value: &str) -> String {
    format!("{} {value}", if selected { ">" } else { " " })
}

pub fn details_omitted(lines: usize) -> String {
    format!("... {lines} more detail lines omitted.")
}
pub fn irreversible_count(count: usize) -> String {
    format!("Cannot be undone: {count} operations")
}

pub const NEW_SESSION_ABORTED: &str = "Session ended before activation.";

/// 検査済みの処理系か、道具を外したことを示す。
pub fn interpreter(path: Option<&std::path::Path>, version: Option<&str>) -> String {
    match (path, version) {
        (Some(path), Some(version)) => {
            format!("Interpreter: {} (version {version})", path.display())
        }
        _ => "Interpreter: unavailable (Benitoite tools disabled)".into(),
    }
}

pub const TARGETS: &str = "Targets";
pub const EFFECTS: &str = "Effects";
pub const ADDED_EFFECTS: &str = "Added effects";
pub const SHELL: &str = "Shell";
pub const NO_EFFECTS: &str = "none";
pub const SHELL_REFERENCE: &str = "The script references Process.shell; the OS sandbox blocks starting a shell in restricted mode.";

pub const UNREAD_IMPORTS: &str = "Imports files that benitoiteh does not read";
