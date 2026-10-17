//! 指示の読み取りで除外・切り詰めを示す英文（設計書 01-04「プロジェクトの指示のファイル」）。

pub const NOT_REGULAR: &str = "Not a regular file.";
pub const HARD_LINKS: &str = "Multiple hard links are not allowed.";
pub const DUPLICATE: &str = "This file has already been included.";
pub const TOTAL_LIMIT: &str = "The total instruction limit (64 KiB) has been reached.";
pub const NOT_UTF8: &str = "The instruction file is not valid UTF-8.";

pub fn truncated(bytes: u64) -> String {
    format!("[truncated: {bytes} bytes omitted]")
}
