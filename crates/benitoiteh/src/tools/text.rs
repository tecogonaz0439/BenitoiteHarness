//! 道具の結果と診断の英文を集める（設計書 01-03「read」「grep」「出力の切り詰め」）。

use crate::provider::ArgumentType;
use serde_json::Value;

pub const READ: (&str, &str) = prompt_parts(include_str!("../../prompts/tools/read.md"));
pub const GLOB: (&str, &str) = prompt_parts(include_str!("../../prompts/tools/glob.md"));
pub const GREP: (&str, &str) = prompt_parts(include_str!("../../prompts/tools/grep.md"));
pub const WRITE: (&str, &str) = prompt_parts(include_str!("../../prompts/tools/write.md"));
pub const BASH: (&str, &str) = prompt_parts(include_str!("../../prompts/tools/bash.md"));
pub const EDIT: (&str, &str) = prompt_parts(include_str!("../../prompts/tools/edit.md"));

// 最初の一行を説明、残りを規則として同じファイルから取り出す。
// 定数の参照を保ち、ToolDefinition の共有の型を変えない（設計書 01-03「道具の定義の共通の規則」）。
const fn prompt_parts(prompt: &str) -> (&str, &str) {
    let mut position = 0_usize;
    let mut bytes = prompt.as_bytes();
    while let Some((byte, rest)) = bytes.split_first() {
        if *byte == b'\n' {
            let (summary, rest) = prompt.split_at(position);
            let (_, rules) = rest.split_at(1);
            return (summary, rules.trim_ascii());
        }
        bytes = rest;
        position = position.saturating_add(1);
    }
    (prompt, "")
}
pub const OBJECT_REQUIRED: &str = "Arguments must be a JSON object.";
pub const MODE_MISMATCH: &str =
    "Tool set and execution context modes differ. Use the session's tool set.";
pub const SAFE_FS: &str = "Cannot open this path safely. Use a path within the allowed scope.";
pub const IO: &str = "File operation failed. Check the path and permissions, then retry.";
pub const OUTSIDE_SCOPE: &str = "Path is outside the allowed scope. Use a project-relative path; read also allows this session's temporary and spill directories.";
pub const NOT_TEXT: &str = "Cannot read a binary or image file. Choose a text file.";
pub const STATE: &str = "Read-version state is unavailable. Restart the session before retrying.";
pub const CHANGED: &str = "File changed while it was being read. Read it again, then retry.";
pub const NOT_READ: &str = "File has not been read in this session. Read it with read, then retry.";
pub const STALE_READ: &str = "File changed since it was last read. Read it again, then retry.";
pub const RESTRICTED_EXTENSION: &str =
    "Restricted mode can write only .bnt files. Choose a .bnt path.";
pub const PROTECTED_WRITE: &str =
    "Restricted mode cannot write inside .git or .benitoiteh. Choose an unprotected path.";
pub const EMPTY_OLD: &str = "old_string must not be empty. Use write to create a file.";
pub const REGULAR_FILE: &str = "Expected a regular file. Use glob to list directories.";
pub const NO_FILES: &str = "No matching files.";
pub const NO_MATCHES: &str = "No matches.";
pub const SEARCH_TRUNCATED: &str =
    "[Results truncated. Narrow path, glob, or pattern to continue.]";
pub const DEFAULT_SECRET: &str = "default secret-file rule";
pub const CONFIG_DIRECTORY: &str = "benitoiteh configuration directory";
pub const DATA_DIRECTORY: &str = "benitoiteh data directory (except this session's spill/)";

pub fn argument(name: &str) -> &'static str {
    match name {
        "command" => "Shell command to run in the project directory.",
        "timeout" => "Time limit in seconds (1-600; default 120).",
        "escalate" => "Request execution outside the sandbox for this call.",
        "justification" => "Reason for requesting execution outside the sandbox.",
        "path" => "Project-relative or absolute path.",
        "pattern" => "Pattern to match.",
        "offset" => "First line to read (1-based).",
        "limit" => "Maximum number of lines to read (up to 2000).",
        "glob" => "Only search paths matching this glob.",
        "ignore_case" => "Match without case sensitivity.",
        "context" => "Number of surrounding lines to show.",
        "files_only" => "Return only paths of matching files.",
        "content" => "Complete file contents.",
        "old_string" => "Text to replace.",
        "new_string" => "Replacement text.",
        "replace_all" => "Replace every occurrence.",
        _ => "Argument value.",
    }
}
pub fn unknown_tool(name: &str, definitions: &[super::ToolDefinition]) -> String {
    let names = definitions
        .iter()
        .map(|tool| tool.id.name())
        .collect::<Vec<_>>()
        .join(", ");
    format!("Unknown tool {name:?}. Available tools: {names}.")
}
pub fn unknown_argument(name: &str) -> String {
    format!("Unknown argument {name:?}.")
}
pub fn missing_argument(name: &str) -> String {
    format!("Missing required argument {name:?}.")
}
pub fn wrong_type(name: &str, kind: ArgumentType) -> String {
    let expected = match kind {
        ArgumentType::String => "a string",
        ArgumentType::Integer => "an integer",
        ArgumentType::Boolean => "a boolean",
        ArgumentType::StringArray => "an array of strings",
    };
    format!("Argument {name:?} must be {expected}.")
}
pub fn invalid_arguments(detail: &str, schema: &Value) -> String {
    format!("{detail} Expected arguments: {schema}")
}
pub fn excluded(rule: &str) -> String {
    format!("File excluded by read rule: {rule}. Choose a file not covered by the exclusion rules.")
}
pub fn offset(offset: u64, total: usize) -> String {
    format!("Offset {offset} is beyond end of file ({total} lines total).")
}
pub fn positive(name: &str, zero_allowed: bool) -> String {
    let range = if zero_allowed {
        "a non-negative integer"
    } else {
        "a positive integer"
    };
    format!("Argument {name:?} must be {range}.")
}
pub fn write_approval(path: &std::path::Path, protected: bool) -> String {
    let reason = if protected {
        "inside a protected location (.git or .benitoiteh)"
    } else {
        "outside the writable roots"
    };
    format!(
        "Writing {} is {reason} and requires approval. Ask the user to approve this write in an interactive session or choose an unprotected path within the selected writable roots.",
        path.display()
    )
}
pub fn wrote(path: &str, lines: usize) -> String {
    format!("Wrote {path} ({lines} lines).")
}
pub fn edit_not_found(path: &str) -> String {
    format!(
        "Could not find old_string in {path}. It must match the file exactly, including whitespace and line breaks. Read the file again if it may have changed."
    )
}
pub fn edit_multiple(path: &str, count: usize) -> String {
    format!(
        "Found {count} occurrences of old_string in {path}. Add surrounding lines to make it unique, or set replace_all to true."
    )
}
pub fn edit_unchanged(path: &str) -> String {
    format!("old_string and new_string are identical. No change was made to {path}.")
}
pub fn pattern(error: &dyn std::fmt::Display) -> String {
    format!("Invalid pattern: {error}. Correct the pattern and retry.")
}
pub fn read_continuation(first: usize, last: usize, total: usize) -> String {
    format!(
        "[Showing lines {first}-{last} of {total}. Use offset={} to continue.]",
        last.saturating_add(1)
    )
}
pub fn long_line(line: usize, chars: usize) -> String {
    format!("[line {line} truncated: {chars} chars. Use grep to find text in this line.]")
}
pub fn glob_truncated(total: usize) -> String {
    format!("[Showing 500 of {total} files. Narrow path or pattern to continue.]")
}
pub fn middle_removed(bytes: usize) -> String {
    format!("\n[Output truncated: {bytes} bytes omitted from the middle.]\n")
}
pub fn char_range(first: usize, last: usize, total: usize) -> String {
    format!(" [chars {first}-{last} of {total}]")
}
pub fn numbered(line: usize, contents: &str) -> String {
    format!("{line:>6}\t{contents}")
}
pub fn matched(path: &str, line: usize, contents: &str) -> String {
    format!("{path}:{line}:{contents}")
}

pub const BASH_CONTEXT: &str =
    "Shell execution context or session is unavailable. Restart the session before retrying.";
pub const BASH_TIMEOUT: &str =
    "timeout must be an integer from 1 to 600 seconds. Correct the timeout and retry.";
pub const BASH_CANCELLED: &str =
    "[The command was stopped because the user cancelled the tool call.]";
pub const BASH_LEFTOVERS: &str = "[Processes started by the command may still be running.]";
pub const BASH_SANDBOX: &str = "[sandbox: the command may have been blocked by the sandbox. If it must run outside the sandbox, call bash again with escalate=true and a justification; the user will be asked.]";
pub fn bash_failure(error: &dyn std::fmt::Display) -> String {
    format!(
        "Could not execute the command safely: {error}. Check the execution environment before retrying."
    )
}
pub fn bash_approval(command: &str, escalate: bool) -> String {
    let scope = if escalate {
        "outside the sandbox"
    } else {
        "with the selected sandbox policy"
    };
    format!("Run this command {scope}: {command}")
}
pub fn bash_exit(code: i32) -> String {
    format!("[exit code: {code}]")
}
pub fn bash_timed_out(seconds: u64) -> String {
    format!("[The command timed out after {seconds} seconds and was stopped.]")
}
pub fn bash_truncated(
    first: usize,
    last: usize,
    total: usize,
    saved: Option<(&crate::session::SpillInfo, &std::path::Path)>,
) -> String {
    let location = match saved {
        Some((info, directory)) => {
            let path = directory.join(&info.path);
            if info.discarded_bytes == 0 {
                format!("Full output: {}", path.display())
            } else {
                format!(
                    "Partial output (first {} of {} bytes): {}",
                    info.written_bytes,
                    info.written_bytes.saturating_add(info.discarded_bytes),
                    path.display()
                )
            }
        }
        None => "Full output was not saved.".to_owned(),
    };
    format!("[Showing last lines {first}-{last} of {total}. {location}]")
}

pub const CHECKPOINT_CANCELLED: &str = "Tool execution was cancelled while recording a checkpoint.";
