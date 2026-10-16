//! 道具の結果と診断の英文を集める（設計書 01-03「read」「grep」「出力の切り詰め」）。

use crate::provider::ArgumentType;
use serde_json::Value;

pub const READ: &str = "Read a text file with line numbers.";
pub const GLOB: &str = "Find files by glob pattern, newest first.";
pub const GREP: &str = "Search file lines using a regular expression.";
pub const WRITE: &str = "Create or replace a file.";
pub const EDIT: &str = "Replace matching text in a file.";
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
pub fn unknown_tool(name: &str) -> String {
    format!("Unknown tool {name:?}. Use read, glob, grep, write, or edit.")
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
        "Writing {} is {reason} and requires approval. Ask the user to approve this write in an interactive session or choose an unprotected path within the project or session temporary directory.",
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
