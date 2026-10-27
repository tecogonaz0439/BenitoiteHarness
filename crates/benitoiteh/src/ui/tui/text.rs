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
pub const MODEL_REASON_HEADING: &str = "| Reason given by the model:";
pub const JUSTIFICATION_OMITTED: &str = "| ... remaining justification omitted.";
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
        ApprovalKind::CreateProject => "╭─ Create Benitoite project?",
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
        Some(ApprovalKind::CreateProject) => {
            "[y] create and switch to this project  [n] decline  [m] decline with a message"
        }
        Some(ApprovalKind::Command) => {
            "[y] run  [a] allow this exact command for this session  [n] decline  [m] decline with a message"
        }
        Some(ApprovalKind::Escalate) => {
            "[y] run outside the sandbox once  [n] decline  [m] decline with a message"
        }
        Some(ApprovalKind::Effects | ApprovalKind::TestEffects) => {
            "[y] run and allow these effects for this session  [n] decline  [m] decline with a message"
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
pub const REQUEST_RESTORED: &str = "Request restored to the input. Press Ctrl+C to clear it.";
pub const REWIND_CANCELLED: &str = "Rewind cancelled.";
pub const BRANCH_CANCELLED: &str = "Branch move cancelled.";
pub const MOVING: &str = "Moving...";
pub const BRANCH_MOVED: &str = "Moved to the selected branch.";
pub const NO_BRANCHES: &str = "The session tree is empty.";
pub const SELECT_BRANCH: &str = "Select a branch or request";
pub const SELECT_BRANCH_SCOPE: &str = "Select branch move scope";
pub const BRANCH_TITLE: &str = "╭─ Move to branch?";
pub const BRANCH_KEYS: &str = "[y] move  [n] cancel";
pub const INCOMPLETE_BRANCH_TITLE: &str = "╭─ Move with an incomplete backup?";
pub const INCOMPLETE_BRANCH_KEYS: &str = "[f] move anyway  [n] cancel";
pub const BRANCH_TARGET: &str = "Branch target";
pub const CONVERSATION_START: &str = "(conversation start)";
pub const TIP_MARK: &str = " [tip]";
pub const CURRENT_MARK: &str = " [current]";
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
/tree                  Show the session tree and move to a branch or request.
/fork [before]         Copy a path into a new session.
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
    lines.extend(status.plugins.iter().map(plugin_loaded));
    lines.extend(status.skipped_plugins.iter().map(plugin_skipped));
    lines.extend(status.plugin_notices.iter().map(plugin_notice));
    lines.extend(status.skills.iter().map(skill_loaded));
    lines.extend(
        status
            .skipped_skills
            .iter()
            .map(|skill| skill_skipped(&skill.path, &skill.reason)),
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

pub const CHECKPOINT_DETAIL_CHARS: usize = 200;

fn checkpoint_detail(value: &str) -> String {
    let mut chars = value.chars();
    let mut detail: String = chars.by_ref().take(CHECKPOINT_DETAIL_CHARS).collect();
    if chars.next().is_some() {
        detail.push_str("...");
    }
    detail
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
    let detail = checkpoint_detail(&detail);
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
pub fn branch_result(
    report: &crate::rewind::RewindReport,
    scope: crate::rewind::RewindScope,
) -> String {
    if scope == crate::rewind::RewindScope::FilesAndConversation {
        format!(
            "{BRANCH_MOVED} {} restored, {} removed, {} created.",
            report.restored, report.removed, report.created
        )
    } else {
        BRANCH_MOVED.into()
    }
}

pub fn branch_target(branch: &crate::agent::BranchInfo) -> String {
    match branch.target {
        crate::agent::BranchTarget::BeforeRequest(node) => {
            let request = branch.request.as_deref().unwrap_or_default();
            let heading = request_summary(
                &crate::session::request_heading(request),
                request.lines().count().max(1),
            );
            format!("Before request {node}: {heading}")
        }
        crate::agent::BranchTarget::Tip(node) => format!("Branch tip {node}"),
    }
}

pub const TREE_MAX_INDENT: usize = 8;
pub const TREE_DEPTH_PREFIX: &str = "[d";
pub const TREE_DEPTH_SUFFIX: &str = "] ";

pub fn request_summary(heading: &str, lines: usize) -> String {
    let heading = super::control::SafeText::strict(heading, false);
    if lines > 1 {
        format!("{} ({lines} lines)", heading.as_str())
    } else {
        heading.as_str().to_owned()
    }
}

pub fn tree_row(row: &crate::session::TreeRow) -> super::control::Line {
    use super::control::{Decoration, SafeText};
    let mut line = vec![(
        SafeText::strict(&"  ".repeat(row.depth.min(TREE_MAX_INDENT)), false),
        Decoration::Dim,
    )];
    if row.depth > TREE_MAX_INDENT {
        line.push((
            SafeText::strict(
                &format!("{TREE_DEPTH_PREFIX}{}{TREE_DEPTH_SUFFIX}", row.depth),
                false,
            ),
            Decoration::Dim,
        ));
    }
    let has_body = row.node.is_none() || row.request.is_some();
    if row.node.is_none() {
        line.push((SafeText::strict(CONVERSATION_START, false), Decoration::Dim));
    } else if let Some(request) = &row.request {
        line.push((
            SafeText::strict(&request_summary(request, row.request_lines), false),
            Decoration::Plain,
        ));
    }
    if row.is_tip {
        line.push((
            SafeText::strict(
                if has_body {
                    TIP_MARK
                } else {
                    TIP_MARK.trim_start()
                },
                false,
            ),
            Decoration::Dim,
        ));
    }
    if row.is_current {
        line.push((
            SafeText::strict(
                if has_body || row.is_tip {
                    CURRENT_MARK
                } else {
                    CURRENT_MARK.trim_start()
                },
                false,
            ),
            Decoration::Dim,
        ));
    }
    line
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
pub const SHELL: &str = "Shell";
pub const NO_EFFECTS: &str = "none";
pub const SHELL_REFERENCE: &str = "The script references Process.shell; the OS sandbox blocks starting a shell in restricted mode.";

pub const UNREAD_IMPORTS: &str = "Imports files that benitoiteh does not read";

pub const PROGRAMS: &str = "Programs";
pub const OTHER_PROGRAMS_BLOCKED: &str = "The OS sandbox blocks starting any other program.";
pub const PROCESS_RUN_SCOPE: &str = "Allowing Process.Run lets the script start benitoite on another script, which can use any operation the OS sandbox allows, including effects not listed above.";
pub const MACOS_PROCESS_GROUPS: &str = "On macOS, processes that an allowed program starts in a new session or process group are not stopped when the tool ends.";

/// 書ける場所と IP の通信の範囲を、確認の一つの事実として示す。
pub fn sandbox_scope(facts: &crate::approval::RestrictedFacts) -> String {
    let project = facts
        .writable
        .split_last()
        .map(|(_, paths)| paths)
        .unwrap_or_default();
    let write = if project.is_empty() {
        "can write: only the session temporary directory".into()
    } else {
        let paths = project
            .iter()
            .map(|path| super::control::SafeText::path(path).as_str().to_owned())
            .collect::<Vec<_>>()
            .join(" and ");
        format!("can write: {paths} and the session temporary directory")
    };
    let network = if facts.network {
        "network: allowed"
    } else {
        "network: blocked"
    };
    format!("{write}\n{network}")
}

/// 追加の印、処理系の綴り、利用者向けの説明を一行にする。
pub fn effect_line(name: &str, added: bool) -> String {
    let description = crate::benitoite::Effect::from_name(name).map_or("", effect_description);
    let mark = if added { "+ " } else { "  " };
    format!("{mark}{name}  {description}")
}

// Benitoite v0.0.12 の同梱 Skill「Before running」の表から写した。
// Process.Run の末尾の (name them) はエージェント向けの指示なので含めない。
/// 同梱 Skill の確認用の説明。表明は道具の側で表示から除く。
pub fn effect_description(effect: crate::benitoite::Effect) -> &'static str {
    use crate::benitoite::Effect;
    match effect {
        Effect::ConsoleWrite => "writes to standard output and standard error",
        Effect::Log => "structured log events; independent of Console.Write and IO.All",
        Effect::ConsoleRead => "reads standard input",
        Effect::FileRead => "reads files and directories",
        Effect::FileWrite => "creates, changes, moves, or deletes files and directories",
        Effect::ProcessRun => "starts external commands",
        Effect::ProcessExit => {
            "ends the run with a chosen exit code; `or die` reports its cause and optional message to standard error and exits with status 1"
        }
        Effect::ProcessEnvironment => {
            "reads command-line arguments, environment variables, the working directory, and the script directory"
        }
        Effect::ClockTime => "reads the clock and waits for time to pass",
        Effect::RandomGenerate => "generates random numbers",
        Effect::HttpListen => "accepts network connections as an HTTP server",
        Effect::HttpConnect => "connects to HTTP servers over the network",
        Effect::State => {
            "uses mutable cells and concurrent tasks inside the program (no outside effect)"
        }
        // 表明は道具が表示用の集合から除く（設計書 04-01「実行の前の検査の報告」）。
        Effect::AssertCheck => "",
    }
}

pub use super::super::text::{plugin_loaded, plugin_notice, plugin_skipped, skill_loaded};
pub fn skill_skipped(path: &std::path::Path, reason: &str) -> String {
    super::super::text::skill_skipped(path, reason)
}

pub const IMAGES_REMOVED: &str = "Images were removed and the request was sent again successfully.";

pub const FORK_USAGE: &str = "Usage: /fork [before]";
pub const SELECT_FORK: &str = "Fork before request";
pub const FORK_CANCELLED: &str = "Fork cancelled.";
pub const STARTING_FORK: &str = "Forking session...";
pub const CONTINUATION: &str = "| ";

pub const SECRET_TITLE: &str = "╭─ Secret input requested by a command outside the sandbox";
pub const SECRET_WARNING: &str =
    "The command receiving this secret can disclose it outside benitoiteh.";
pub const SECRET_HELPER: &str = "Helper report (unverified)";
pub const SECRET_PROMPT: &str = "Untrusted prompt (up to 10 lines):";
pub const SECRET_KEYS: &str = "[Enter] send  [Tab] show/hide  [Esc] cancel";
pub const SECRET_CONFIRM_KEYS: &str = "[y] yes  [n] no  [Esc] cancel";
pub const SECRET_TOUCH_KEYS: &str = "Touch notification; no answer required. [Esc] dismiss";
pub const SECRET_DECLINE_KEYS: &str = "[Esc] cancel; enlarge the terminal to answer";
pub const SECRET_VALUE: &str = "Secret: ";
pub const SECRET_CALL: &str = "Tool call";
pub const SECRET_MODE: &str = "Mode";
pub fn secret_helper(kind: &str, pid: u32, ppid: u32, prompt: Option<&str>) -> String {
    format!(
        "kind={kind}, pid={pid}, ppid={ppid}, ssh_prompt={}",
        prompt.unwrap_or("secret")
    )
}
pub fn secret_history(call_id: &str, outcome: &str) -> String {
    format!("Secret input for {call_id}: {outcome}")
}

pub const SECRET_CLOSED: &str =
    "Secret input closed by the command; typed characters were discarded. [Esc] continue";
