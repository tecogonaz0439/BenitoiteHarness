//! サンドボックスの誤りの英文をまとめる（設計書 03-02）。

pub const INVALID_PATH: &str = "Sandbox paths must be canonical, absolute UTF-8 paths.";
pub const EMPTY_WRITABLE: &str = "A sandbox needs at least one writable directory.";
pub const INVALID_DIRECTORY: &str = "Sandbox roots must be directories.";
pub const INVALID_PROTECTED: &str = "Protected names must be single components using only ASCII letters, digits, '.', '-' or '_', and include .git and .benitoiteh.";
pub const UNAVAILABLE: &str = "The OS sandbox is unavailable.";
pub const IO: &str = "Could not prepare the OS sandbox:";
