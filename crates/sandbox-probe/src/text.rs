//! テスト用の探査の結果を示す英文（設計書 07-01「安全の性質のテスト」）。

pub const USAGE: &str = "Invalid sandbox-probe operation or arguments.";
pub const OK: &str = "ok";
pub const CHILD_FAILED: &str = "child failed";
pub const FD_CLOSED: &str = "descriptor closed";
pub const READY: &str = "ready";

// P2-01 のプロセスの探査に使う英文。
pub const ECHO_STDERR: &str = "echo stderr";
#[cfg(target_os = "linux")]
pub const EXTRA_DESCRIPTOR: &str = "Unexpected inherited descriptor.";
#[cfg(target_os = "linux")]
pub const CONTROL_ACCESS: &str = "The helper control descriptors are accessible.";
