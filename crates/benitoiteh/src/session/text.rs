//! セッションの診断と補う結果の英文（設計書 01-05「再開」、01-03「出力の切り詰め」）。

pub const INVALID_ID: &str = "Invalid session ID.";
pub const INVALID_TIME: &str = "Session time must be between 1970 and 9999 UTC.";
pub const IO_ERROR: &str = "Session file operation failed.";
pub const JSON_ERROR: &str = "Invalid session record.";
pub const RANDOM_ERROR: &str = "Could not generate a session ID.";
pub const UNSAFE_PATH: &str =
    "Session entry has an unsafe type, owner, link count, or permissions.";
pub const LOCKED: &str = "Session is in use.";
pub const INVALID_FORMAT: &str = "Invalid session record format.";
pub const UNSUPPORTED_FORMAT: &str = "Unsupported session format version.";
pub const RECORD_TOO_LARGE: &str = "Session record exceeds the line size limit.";
pub const INVALID_SPILL: &str = "Invalid session spill reference.";
pub const COUNTER_OVERFLOW: &str = "Spill byte count overflowed.";
pub const WRITER_FAILED: &str = "Session writer cannot continue after a failed append.";
pub const INTERRUPTED: &str = "Tool call was interrupted before its result was recorded. Its effects are unknown; check the files before retrying.";
pub const CANCELLED: &str = "Tool call was cancelled by the user.";

pub fn cleanup_failed(failure: &super::CleanupFailure) -> String {
    let id = failure
        .id
        .as_ref()
        .map_or("unknown", super::SessionId::as_str);
    let kind = if failure.temporary {
        "temporary directory"
    } else {
        "session"
    };
    format!("Could not clean up {kind} {id}: {}", failure.reason)
}

pub fn spill_footer(written: u64, discarded: u64) -> String {
    format!("\n[Partial output: wrote {written} bytes; discarded {discarded} bytes.]\n")
}

/// 一覧で分割された最初の依頼を読まない場合の表示。
pub const NOT_AVAILABLE: &str = "(not available)";
pub const NO_SESSION: &str = "No resumable session was found for this project.";
pub const STORAGE_LIMIT: &str = "Session storage limit reached; output could not be saved.";
pub const TOTAL_LIMIT: &str =
    "Session storage exceeds 10 GiB; reduce [sessions] retention_days to remove older sessions.";

pub const INVALID_OBJECT: &str = "Invalid checkpoint object or content hash.";

pub const IMAGE_RESTORE_OMITTED: &str =
    "(An image from this result could not be restored from the session and was omitted.)";

pub const REQUEST_ELLIPSIS: &str = "...";

pub const EMPTY_FORK: &str = "The selected path is empty; no session was forked.";

pub const FORK_ASSET_CHANGED: &str = "A referenced file changed while forking the session.";
pub fn fork_cleanup(
    error: &super::SessionError,
    cleanup: &std::io::Error,
    id: &super::SessionId,
) -> String {
    format!("{error} Could not remove incomplete fork {id}: {cleanup}")
}

/// 後始末の省略は、進行中の秘密の仲介を止めた診断と区別する。
pub const NOT_PRIVATE_DIRECTORY: &str = "not a private directory owned by you.";
pub fn cleanup_skipped(path: &std::path::Path, reason: &str) -> String {
    let path = crate::benitoite::display::one_line(&path.to_string_lossy());
    let reason = crate::benitoite::display::one_line(reason);
    format!("Skipped cleanup of {path}: {reason}")
}
