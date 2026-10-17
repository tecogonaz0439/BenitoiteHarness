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

pub const INVALID_SKILL: &str = "The Benitoite Skill from the processor has an invalid format.";

// Skill の取得と、検査の報告の取得では、出力の上限に達した後の案内が異なる。
pub const SKILL_OUTPUT_LIMIT: &str = "the Skill document from benitoite exceeded 16 MiB";
pub fn skill_exit(code: Option<i32>) -> String {
    match code {
        Some(code) => format!("benitoite exited with status {code}"),
        None => "benitoite exited without an exit status".into(),
    }
}

pub const DIAGNOSTICS_LIMIT: &str = "The diagnostics from benitoite exceeded 16 MiB and were not read. Check fewer or narrower paths.";
pub const CHECK_DIAGNOSTICS_LIMIT: &str = "the diagnostics from benitoite exceeded 16 MiB";
pub const CHECK_CANCELLED: &str = "the check was cancelled";

pub fn more_diagnostics(count: usize) -> String {
    if count == 1 {
        "[1 more diagnostic not shown. Fix the ones above, then check again.]".into()
    } else {
        format!("[{count} more diagnostics not shown. Fix the ones above, then check again.]")
    }
}
pub fn unread_diagnostics(count: usize) -> String {
    if count == 1 {
        "1 diagnostic in a file that benitoiteh does not read is not shown.".into()
    } else {
        format!("{count} diagnostics in files that benitoiteh does not read are not shown.")
    }
}
pub fn diagnostic_location(path: &std::path::Path, start: &super::diagnostics::Position) -> String {
    format!("{}:{}:{}", path.display(), start.line, start.column)
}
pub fn diagnostic_header(
    severity: &str,
    code: Option<&str>,
    location: Option<&str>,
    message: &str,
) -> String {
    let code = code.map(|code| format!("[{code}]")).unwrap_or_default();
    let location = location
        .map(|location| format!(" {location}:"))
        .unwrap_or_default();
    format!("{severity}{code}{location} {message}")
}
pub fn diagnostic_source(line: u64, source: &str) -> String {
    format!("  {line} | {source}")
}
pub fn diagnostic_marker(line: u64, prefix: &str, width: usize, label: &str) -> String {
    let margin = " ".repeat(line.to_string().len().saturating_add(3));
    let label = if label.is_empty() {
        String::new()
    } else {
        format!(" {label}")
    };
    format!("{margin}| {prefix}{}{label}", "^".repeat(width))
}
pub fn diagnostic_secondary(
    label: &str,
    path: &std::path::Path,
    start: &super::diagnostics::Position,
) -> String {
    format!("  note: {label} ({})", diagnostic_location(path, start))
}
pub fn diagnostic_note(note: &str) -> String {
    format!("  note: {note}")
}
pub fn diagnostic_help(message: &str) -> String {
    format!("  help: {message}")
}
pub fn diagnostic_edit(
    path: Option<&std::path::Path>,
    start: &super::diagnostics::Position,
    end: &super::diagnostics::Position,
    replacement: &str,
) -> String {
    let path = path
        .map(|path| format!("{}:", path.display()))
        .unwrap_or_default();
    format!(
        "    replace {path}{}:{}-{}:{} with {replacement}",
        start.line, start.column, end.line, end.column
    )
}

pub fn omitted_secondary(count: usize) -> String {
    if count == 1 {
        "  [1 secondary location not shown.]".into()
    } else {
        format!("  [{count} secondary locations not shown.]")
    }
}
pub fn omitted_edits(count: usize) -> String {
    if count == 1 {
        "    [1 suggested edit not shown.]".into()
    } else {
        format!("    [{count} suggested edits not shown.]")
    }
}
