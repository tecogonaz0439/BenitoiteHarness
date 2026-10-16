//! セッションの診断と補う結果の英文（設計書 01-05「再開」、01-03「出力の切り詰め」）。

pub const INVALID_ID: &str = "Invalid session ID.";
pub const INVALID_TIME: &str = "Session time must be between 1970 and 9999 UTC.";
pub const IO_ERROR: &str = "Session file operation failed.";
pub const JSON_ERROR: &str = "Invalid session record.";
pub const RANDOM_ERROR: &str = "Could not generate a session ID.";
pub const UNSAFE_PATH: &str =
    "Session entry has an unsafe type, owner, link count, or permissions.";
pub const LOCKED: &str = "Session is in use.";
pub const INVALID_FORMAT: &str = "Unsupported session format.";
pub const RECORD_TOO_LARGE: &str = "Session record exceeds the line size limit.";
pub const INVALID_SPILL: &str = "Invalid session spill reference.";
pub const COUNTER_OVERFLOW: &str = "Spill byte count overflowed.";
pub const WRITER_FAILED: &str = "Session writer cannot continue after a failed append.";
pub const INTERRUPTED: &str = "Tool call was interrupted before its result was recorded. Its effects are unknown; check the files before retrying.";
pub const CANCELLED: &str = "Tool call was cancelled by the user.";

pub fn spill_footer(written: u64, discarded: u64) -> String {
    format!("\n[Partial output: wrote {written} bytes; discarded {discarded} bytes.]\n")
}
