//! ファイル操作の英語の診断をまとめる（設計書 01-03「道具の定義の共通の規則」）。

pub const INVALID_PATH: &str =
    "Use a nonempty relative path without empty, dot, parent, or NUL components.";
pub const OUTSIDE_SCOPE: &str = "Path resolves outside the scope. Choose a path inside the scope.";
pub const INTERMEDIATE_SYMLINK: &str =
    "Path contains a symbolic link that cannot be followed safely.";
pub const FINAL_SYMLINK: &str = "Cannot write to a symbolic link. Choose a regular file.";
pub const MULTIPLE_HARD_LINKS: &str = "Cannot replace a file with multiple hard links.";
pub const NOT_REGULAR_FILE: &str = "Cannot replace this entry. Choose a regular file.";
pub const ALREADY_EXISTS: &str = "Entry already exists.";
pub const NOT_FOUND: &str = "Entry does not exist.";
pub const IO_ERROR: &str = "Filesystem operation failed.";
pub const TEMPORARY_COLLISIONS: &str =
    "Could not create an exclusive temporary file after repeated name collisions.";
