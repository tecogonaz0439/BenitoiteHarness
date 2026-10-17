//! 巻き戻しの英語の診断（設計書 01-05「巻き戻し」）。
pub const CANCELLED: &str = "Checkpoint capture was cancelled.";
pub const INVALID_CHECKPOINT: &str = "Invalid or unknown checkpoint ID.";
pub const INVALID_MANIFEST: &str = "Invalid checkpoint manifest.";
pub const NO_MANIFEST: &str = "The checkpoint manifest was not saved.";
pub const PROJECT_MISMATCH: &str = "The checkpoint belongs to a different project or format.";
pub const STATE: &str = "Checkpoint state is unavailable.";
pub const ENTRY_LIMIT: &str = "Checkpoint entry limit reached.";
pub const TIME_LIMIT: &str = "Checkpoint time limit reached.";
pub const FILE_LIMIT: &str = "Checkpoint file size limit reached.";
pub const MANIFEST_LIMIT: &str = "Checkpoint manifest size limit reached.";
pub const STORAGE_LIMIT: &str = "Checkpoint storage budget exhausted.";
pub const IGNORE_INVALID: &str = "The .gitignore is not a regular file of an allowed size.";
pub const CHANGED: &str = "The entry changed while it was being recorded or restored.";
pub const INCOMPLETE_BEFORE: &str =
    "The before_rewind checkpoint is incomplete; explicit permission is required.";
pub const NOT_DELETABLE: &str = "The checkpoint does not prove this path can be deleted.";
pub const HARD_LINK: &str = "The current file has multiple hard links.";
pub const MISSING_CONTENT: &str = "The checkpoint has no saved content for this path.";
pub const HASH_MISMATCH: &str = "Checkpoint object content does not match its hash.";

pub const CURRENT_EXCLUSION: &str = "The path is excluded or unreadable under the current rules.";

pub const TIMESTAMP_EXPECTED: &str = "an integer number of nanoseconds";
