//! 道具の結果と診断の英文を集める（設計書 01-03「read」「grep」「出力の切り詰め」）。

use crate::provider::ArgumentType;
use serde_json::Value;

pub const READ: (&str, &str) = prompt_parts(include_str!("../../prompts/tools/read.md"));
pub const READ_PLAIN: (&str, &str) =
    prompt_parts(include_str!("../../prompts/tools/read-plain.md"));
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
pub const OUTSIDE_SCOPE: &str = "Path is outside the allowed scope. Use a project-relative path; read also allows this session's temporary and spill directories, and read, glob, grep, and the benitoite tools allow the loaded skill directories.";
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
pub const READ_TOO_LARGE: &str =
    "File exceeds the 64 MiB read limit. Use grep to find text in this file.";
pub const STOP_ENTRY: &str = "entry limit";
pub const STOP_TIME: &str = "time limit";
pub const STOP_CANCELLED: &str = "cancelled";
pub const SKIP_LARGE: &str = "too large";
pub const SKIP_SPECIAL: &str = "non-regular";
pub const SKIP_UNREADABLE: &str = "unreadable";
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
        "url" => "Absolute HTTPS URL to fetch.",
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
pub fn glob_partial(total: usize) -> String {
    format!("[Showing 500 of at least {total} files. Narrow path or pattern to continue.]")
}
pub fn skipped_ignores(count: usize, paths: &str) -> String {
    format!(
        "[Skipped .gitignore files: {count}; first 5 paths: {paths}. Only readable UTF-8 regular files up to 1 MiB are used.]"
    )
}
pub fn skipped_directories(count: usize, paths: &str) -> String {
    format!("[Skipped unreadable directories: {count}; first 5 paths: {paths}.]")
}
pub fn skipped_files(count: usize, paths: &str) -> String {
    format!("[Skipped files: {count}; first 5 paths: {paths}.]")
}
pub fn stopped_search(reasons: &str, count: usize, paths: &str) -> String {
    format!(
        "[Search stopped ({reasons}): {count} path(s); first 5 paths: {paths}. Narrow path, glob, or pattern to continue.]"
    )
}

// tools から UI の層へ依存させず、ui::terminal_text(input, false) と同じ
// 規則で名前を一行にする（設計書 01-03「出力の切り詰め」）。
pub fn visible_path(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    for ch in input.chars() {
        match ch {
            '\n' | '\t' => output.push(' '),
            '\u{00}'..='\u{1f}' | '\u{7f}'..='\u{9f}' => {
                output.push_str(&format!("\\x{:02x}", u32::from(ch)));
            }
            '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}' => {
                output.push_str(&format!("\\u{{{:x}}}", u32::from(ch)));
            }
            _ => output.push(ch),
        }
    }
    output
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

pub const WEB_FETCH: (&str, &str) = prompt_parts(include_str!("../../prompts/tools/web_fetch.md"));
pub const WEB_URL: &str = "Invalid web URL. Use an absolute https:// URL with an ASCII domain of at least two labels (punycode xn-- for international names), port 443 or no port, and no user information or IP address.";
pub const WEB_ADDRESS: &str =
    "Web fetch refused: DNS returned a private or special-use address. Choose a public HTTPS site.";
pub const WEB_DNS: &str = "Could not resolve the web domain. Check the domain and retry.";
pub const WEB_TIMEOUT: &str =
    "Web fetch exceeded its total time limit. Retry later or use another URL.";
pub const WEB_CANCELLED: &str = "Web fetch was cancelled.";
pub const WEB_WORKER: &str = "Could not complete the web fetch worker. Retry later.";
pub const WEB_REDIRECT: &str =
    "Web fetch received a missing, invalid, or ambiguous Location header. Use a valid HTTPS URL.";
pub const WEB_REDIRECT_LIMIT: &str =
    "Web fetch stopped after five redirects. Use the final HTTPS URL directly.";
pub const WEB_ENCODING: &str = "Web fetch refused an encoded response. Only an absent Content-Encoding or a single identity value is supported.";
pub const WEB_CONTENT_TYPE: &str = "Web fetch requires a Content-Type of HTML, text/*, application/json, or a +json type. Choose a text resource.";
pub const WEB_CHARSET: &str =
    "Web fetch does not support this charset. Use UTF-8, us-ascii, or iso-8859-1 (latin1).";
pub const WEB_UTF16: &str = "Web fetch refused a UTF-16 byte order mark. Use a UTF-8 resource.";

pub fn web_approval(url: &str, domain: &str) -> String {
    format!(
        "Fetching {url} requires approval for domain {domain}. No DNS lookup or HTTP request has been made. Approve once, allow this domain for this session, or deny."
    )
}
pub fn web_redirect_approval(url: &str) -> String {
    format!(
        "Web fetch stopped before following a redirect to a domain requiring approval. No DNS lookup or HTTP request was made for the target. Call web_fetch again with this URL to request approval:\nURL: {url}"
    )
}
pub fn web_http(error: &crate::http::HttpError) -> String {
    format!("Web fetch failed: {error} Check the URL and response size, or retry later.")
}
pub fn web_status(status: u16, body: &str) -> String {
    format!("Web fetch returned HTTP status {status}. Check the URL or retry later.\n{body}")
}
pub fn web_result(url: &str, kind: &str, body: &str, total: usize) -> String {
    let mut output = format!(
        "URL: {url}\nContent-Type: {kind}\n\n{}",
        super::truncate::prefix(body, 51200)
    );
    if total > 51200 {
        output.push_str(&format!(
            "\n[Showing the first 51200 of {total} bytes of the converted text.]"
        ));
    }
    output
}

/// 読んだ行数を画面に示す。
pub fn screen_lines(count: usize) -> String {
    format!("{count} lines")
}
/// コマンドの終了状態を画面に示す。
pub fn screen_exit(code: i32) -> String {
    format!("exit {code}")
}
/// 差分の追加と削除を画面に示す。
pub fn screen_diff(added: usize, deleted: usize) -> String {
    format!("+{added} -{deleted}")
}

pub const BENITOITE_REFERENCE: (&str, &str) =
    prompt_parts(include_str!("../../prompts/tools/benitoite_reference.md"));
pub const BENITOITE_REFERENCE_NORMAL: &str =
    include_str!("../../prompts/tools/benitoite_reference-normal.md").trim_ascii();
pub const BENITOITE_EXPLAIN: (&str, &str) =
    prompt_parts(include_str!("../../prompts/tools/benitoite_explain.md"));
pub const SKILL_NAME: &str =
    "Skill document name, such as idioms or stdlib/List; omit to list names.";
pub const SKILL_CODE: &str = "Diagnostic code, such as E0405.";
pub const SKILL_LIST: &str = "Available Benitoite Skill documents (pass one as `name`):";
pub const SKILL_LIST_QUERY: &str = "the Skill document list";
pub const SKILL_UNAVAILABLE: &str = "the processor is not available in this session";
pub const INVALID_CODE: &str =
    "Code must be one uppercase letter followed by four digits, such as E0405.";

pub fn unknown_skill(name: &str, names: &str) -> String {
    format!("Unknown Skill document \"{name}\".\n{names}")
}
pub fn skill_failure(name: &str, reason: &str) -> String {
    format!(
        "Could not read {name} from the Benitoite processor: {}.",
        reason.trim_end_matches('.')
    )
}
pub fn missing_code(code: &str) -> String {
    format!(
        "No section for {code} in the diagnostics reference. Check the code, or read `diagnostics` with benitoite_reference."
    )
}

pub fn reference_line_truncated(line: usize) -> String {
    format!("\n[Line {line} truncated to fit the 50 KiB output limit.]\n")
}

pub const BENITOITE_CHECK: (&str, &str) =
    prompt_parts(include_str!("../../prompts/tools/benitoite_check.md"));
pub const CHECK_TESTS: &str =
    "Check without running tests and without requiring main, like benitoite test --check.";
pub const CHECK_OK: &str = "ok";
pub fn check_failure(path: &str, reason: &str) -> String {
    format!(
        "Could not check {path} with the Benitoite processor: {}.",
        reason.trim_end_matches('.')
    )
}
pub fn after_check(path: &str, ok: bool) -> String {
    if ok {
        format!("Check of {path} after this change: ok")
    } else {
        format!("Check of {path} after this change:")
    }
}
pub fn after_check_incomplete(path: &str, reason: &str) -> String {
    format!(
        "Check of {path} after this change did not complete: {}.",
        reason.trim_end_matches('.')
    )
}
pub fn not_checked_outside(path: &str) -> String {
    format!("Not checked automatically: {path} is outside the project directory.")
}
pub fn not_checked_nested(path: &str) -> String {
    format!("Not checked automatically: {path} is in a nested package.")
}
pub fn other_diagnostics(count: usize) -> String {
    if count == 1 {
        "1 diagnostic in another file. Use benitoite_check with tests=true on the project directory to see it.".into()
    } else {
        format!(
            "{count} diagnostics in other files. Use benitoite_check with tests=true on the project directory to see them."
        )
    }
}

pub const BENITOITE_RUN: (&str, &str) =
    prompt_parts(include_str!("../../prompts/tools/benitoite_run.md"));
pub const RUN_ARGS: &str = "Arguments passed to the Benitoite script.";
pub const RUN_STDIN: &str = "Text written to the script's standard input; empty when omitted.";
pub const RUN_CONTEXT: &str =
    "The Benitoite script execution context is unavailable. Stop and report.";
pub const RUN_PATH: &str =
    "The script path must be a UTF-8 path ending in .bnt. Choose a readable Benitoite script.";
pub const RUN_STDOUT: &str = "[stdout]";
pub const RUN_STDERR: &str = "[stderr]";
pub const RUN_CANCELLED: &str =
    "[The script was stopped because the user cancelled the tool call.]";
pub const RUN_LEFTOVERS: &str = "[Processes started by the script may still be running.]";
pub fn run_timed_out(timeout: u64) -> String {
    format!("[The script timed out after {timeout} seconds and was stopped.]")
}
pub fn run_failure(error: &dyn std::fmt::Display) -> String {
    format!(
        "Could not run the Benitoite script: {error}. Check the script and the sandbox configuration, then try again."
    )
}
pub fn run_diagnostics_omitted(count: usize, error: bool) -> String {
    let noun = if count == 1 {
        "diagnostic line was"
    } else {
        "diagnostic lines were"
    };
    let errors = if error {
        " At least one omitted diagnostic was an error."
    } else {
        ""
    };
    format!("[{count} {noun} not shown (limits: first 1000 diagnostics and 16 MiB).{errors}]")
}

pub const BENITOITE_TEST: (&str, &str) =
    prompt_parts(include_str!("../../prompts/tools/benitoite_test.md"));
pub const TEST_PATHS_EMPTY: &str = "paths must contain at least one .bnt file or directory.";
pub const TEST_PATHS_LIMIT: &str = "paths must contain at most 100 entries.";
pub const TEST_PATH: &str = "Each path must be a UTF-8 path to a .bnt file or a directory. Choose readable Benitoite tests.";
pub const TEST_CONTEXT: &str =
    "The Benitoite test execution context is unavailable. Stop and report.";
pub const TEST_NO_SUMMARY: &str = "No test summary from benitoite; the run may have stopped early.";
pub const TEST_PASSED: &str = "All tests passed.";
pub const TEST_EMPTY: &str = "No tests ran.";
pub const TEST_INTERRUPTED: &str = "The test run was interrupted.";
pub const TEST_CANCELLED: &str =
    "[The tests were stopped because the user cancelled the tool call.]";
pub const TEST_LEFTOVERS: &str = "[Processes started by the tests may still be running.]";
pub fn test_timed_out(timeout: u64) -> String {
    format!("[The tests timed out after {timeout} seconds and were stopped.]")
}
pub fn test_failure(error: &dyn std::fmt::Display) -> String {
    format!(
        "Could not run the Benitoite tests: {error}. Check the tests and the sandbox configuration, then try again."
    )
}
pub fn tests_summary(passed: u64, failed: u64) -> String {
    format!("Tests: {passed} passed, {failed} failed.")
}
pub fn test_files_not_run(n: u64) -> String {
    if n == 1 {
        "1 test file was not run.".into()
    } else {
        format!("{n} test files were not run.")
    }
}
pub fn tests_unread(n: usize) -> String {
    if n == 1 {
        "1 failed test in a file that benitoiteh does not read is not shown.".into()
    } else {
        format!("{n} failed tests in files that benitoiteh does not read are not shown.")
    }
}
pub fn tests_uninspected(n: usize) -> String {
    let noun = if n == 1 {
        "failed test was"
    } else {
        "failed tests were"
    };
    format!("[{n} {noun} not inspected because the user cancelled the tool call.]")
}
pub fn tests_long(n: usize) -> String {
    let noun = if n == 1 {
        "test result line was"
    } else {
        "test result lines were"
    };
    format!("[{n} {noun} too long to read.]")
}
pub fn tests_omitted(n: usize) -> String {
    let noun = if n == 1 {
        "failed test was"
    } else {
        "failed tests were"
    };
    format!("[{n} {noun} not retained (limits: first 1000 failed tests and 16 MiB).]")
}
pub fn tests_more(n: usize) -> String {
    let noun = if n == 1 {
        "failed test"
    } else {
        "failed tests"
    };
    format!("[{n} more {noun} not shown. Fix the ones above, then run the tests again.]")
}
pub fn test_output_truncated(first: usize, total: usize) -> String {
    format!("[Showing last lines {first}-{total} of {total} of this test's output.]")
}
pub fn test_failed(
    path: &std::path::Path,
    line: u64,
    column: u64,
    name: &str,
    reason: &str,
    message: &str,
) -> String {
    format!(
        "FAILED {}:{line}:{column} {name}: {reason}: {message}",
        path.display()
    )
}
pub fn test_value(side: &str, value: &str) -> String {
    format!("  {side}: {value}")
}

pub const TEST_TRUNCATED: &str = "…[truncated]";

pub const TEST_OUTPUT_OMITTED: &str =
    "[This test's output was omitted: the combined 32 KiB output limit was reached.]";
pub const TEST_DETAILS_OMITTED: &str =
    "[Further failure details were omitted to keep all failure headings.]";
pub const TEST_RESULT_TRUNCATED: &str =
    "[Further process output and diagnostics were omitted to keep all failure headings.]";
pub fn tests_invalid(n: usize) -> String {
    let noun = if n == 1 {
        "test result line could"
    } else {
        "test result lines could"
    };
    format!("[{n} {noun} not be read or saved.]")
}

pub const BENITOITE_SCRATCH: (&str, &str) =
    prompt_parts(include_str!("../../prompts/tools/benitoite_scratch.md"));
pub const SCRATCH_CODE: &str = "code must be at most 64 MiB and contain no NUL in its first 8 KiB.";
pub const SCRATCH_NAMES: &str = "Could not create a new scratch script after 128 attempts.";
pub const SCRATCH_COUNTER: &str = "The scratch script counter is exhausted.";

pub const CREATE_PROJECT: (&str, &str) =
    prompt_parts(include_str!("../../prompts/tools/create_project.md"));
pub const CREATE_CONTEXT: &str = "Project creation context is unavailable. Stop and report.";
pub const CREATE_PATH: &str = "Invalid project path. Use a UTF-8 path with no empty, dot, parent, or control-character components. The parent directory must already exist.";
pub const CREATE_TILDE: &str = "Project paths do not expand ~. Use an absolute path or a path relative to the current project.";
pub const CREATE_EXISTS: &str = "The project destination already exists (possibly as a symbolic link). Choose a new directory name.";
pub const CREATE_CHANGED: &str = "The project destination or its parent changed after the approval request, or no matching request exists. Nothing was created. Call create_project again to request approval.";
pub const CREATE_CANCELLED: &str = "Project creation was cancelled. Nothing was created.";
pub fn project_created(path: &std::path::Path) -> String {
    format!(
        "Created project at {} and switched this session to it. The previous project is no longer readable or writable by the tools.",
        path.display()
    )
}
pub fn create_failure(error: &dyn std::fmt::Display) -> String {
    format!(
        "Could not create the project: {error}. Choose a permitted destination with an existing parent, or stop and report."
    )
}

/// 親や守る場所の実体を示さず、モデル自身が渡した要求だけを引用する。
pub fn project_unavailable(requested: &str) -> String {
    format!(
        "The project cannot be created at {}: the parent directory must exist and the location must be permitted. Choose another destination, or stop and report.",
        crate::benitoite::display::quoted(requested)
    )
}

/// 作成は承認済みなので残し、切り替えの準備の失敗を道具の結果として伝える。
pub fn project_switch_failed(path: &std::path::Path, error: &dyn std::fmt::Display) -> String {
    format!(
        "Created project at {}, but could not switch this session to it: {error}. The current project is unchanged. The created directory was kept. Stop and report before continuing with it.",
        crate::benitoite::display::quoted(&path.to_string_lossy())
    )
}

pub const IMAGE_LINES: &str = "offset and limit cannot be used when reading an image.";
pub const IMAGE_UNSUPPORTED: &str =
    "The current provider does not accept images; this file cannot be shown.";
pub const IMAGE_INVALID: &str = "The image header is invalid or truncated.";
pub const IMAGE_DECODE: &str = "The image could not be decoded.";
pub const IMAGE_HELPER_UNAVAILABLE: &str = "The image resizing helper could not be started.";
pub const IMAGE_CANCELLED: &str = "Image reading was cancelled.";
pub const IMAGE_ANIMATED_PNG: &str = "Animated PNG images cannot be shown.";
pub fn image_too_large(bytes: u64, limit: u64) -> String {
    format!("Image size is {bytes} bytes; the limit is {limit} bytes.")
}
/// 縮めた中身も送信の上限を超えることを示す。
pub fn image_too_large_after_resizing(bytes: u64, limit: u64) -> String {
    format!("Image size after resizing is {bytes} bytes; the limit is {limit} bytes.")
}
/// 解く前の画素の数と、縮小を受け付ける上限を示す。
pub fn image_pixels(width: u32, height: u32, limit: u64) -> String {
    format!("Image has {width}x{height} pixels; the limit for resizing is {limit} pixels.")
}
pub fn image_dimensions(width: u32, height: u32, limit: u32) -> String {
    format!(
        "Image dimensions are {width}x{height}; each dimension must be between 1 and {limit} pixels."
    )
}
pub fn read_image(media_type: &str, width: u32, height: u32, bytes: u64) -> String {
    format!("Read image file {media_type} ({width}x{height}, {bytes} bytes)")
}
/// 送る画像の属性と、向きを直す前の元の寸法を示す。
pub fn read_resized_image(
    media_type: &str,
    width: u32,
    height: u32,
    bytes: u64,
    original_width: u32,
    original_height: u32,
) -> String {
    format!(
        "Read image file {media_type} ({width}x{height}, {bytes} bytes; resized from {original_width}x{original_height})"
    )
}
