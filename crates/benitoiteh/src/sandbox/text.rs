//! サンドボックスの誤りの英文をまとめる（設計書 03-02）。

pub const INVALID_PATH: &str = "Sandbox paths must be canonical, absolute UTF-8 paths.";
pub const EMPTY_WRITABLE: &str = "A sandbox needs at least one writable directory.";
pub const INVALID_DIRECTORY: &str = "Sandbox roots must be directories.";
pub const INVALID_PROTECTED: &str = "Protected names must be single components using only ASCII letters, digits, '.', '-' or '_', and include .git and .benitoiteh.";
pub const DENIED_WRITABLE: &str = "A denied path must not contain a writable root.";
#[cfg(target_os = "linux")]
pub const WRITABLE_HELPER: &str = "The sandbox helper must be outside every writable root.";
#[cfg(target_os = "linux")]
pub const WRITABLE_SYMLINK: &str =
    "A protected or denied path must not pass through a symlink in a writable root.";
#[cfg(target_os = "linux")]
pub const INVALID_GITDIR: &str = "A root .git file must contain a valid gitdir: path.";
#[cfg(target_os = "linux")]
pub const INVALID_DENIED_TYPE: &str = "A denied path must be a directory or a regular file.";
#[cfg(target_os = "linux")]
pub const WRITABLE_COVER: &str = "The cover file must be outside every writable root.";
#[cfg(target_os = "linux")]
pub const INVALID_COVER: &str = "The cover must be an empty regular file with mode 000.";
#[cfg(target_os = "linux")]
pub const AVAILABILITY: &str = "availability";
#[cfg(target_os = "linux")]
pub const PROBE_FAILED: &str = "Unexpected sandbox probe exit status";
pub const UNAVAILABLE: &str = "The OS sandbox is unavailable.";
pub const IO: &str = "Could not prepare the OS sandbox:";
pub const SETUP: &str = "Sandbox setup failed at";
#[cfg(target_os = "linux")]
pub const POLICY: &str = "policy";
#[cfg(target_os = "linux")]
pub const HELPER_USAGE: &str =
    "Expected __sandbox <policy JSON> -- <absolute program> [arguments].";
#[cfg(target_os = "linux")]
pub const DESCRIPTORS: &str = "descriptors";
#[cfg(target_os = "linux")]
pub const EXTRA_FD: &str =
    "An inherited descriptor other than standard input/output/error is open.";
#[cfg(target_os = "linux")]
pub const NO_NEW_PRIVS: &str = "no_new_privs";
#[cfg(target_os = "linux")]
pub const LANDLOCK: &str = "Landlock";
#[cfg(target_os = "linux")]
pub const SECCOMP: &str = "seccomp";
#[cfg(target_os = "linux")]
pub const EXEC: &str = "exec";
#[cfg(target_os = "linux")]
pub const ABI_REQUIRED: &str = "The required Landlock ABI is unavailable (minimum V3).";
#[cfg(target_os = "linux")]
pub const NOT_ENFORCED: &str = "The mandatory Landlock ruleset was not fully enforced.";
#[cfg(target_os = "linux")]
pub const INVALID_ABI: &str = "required_landlock_abi must be an unsigned integer of at least 3.";

#[cfg(target_os = "linux")]
pub const INVALID_PROGRAM: &str = "An executable allowlist entry must be a regular file.";

#[cfg(target_os = "linux")]
pub const CONTROL: &str =
    "Expected a Unix stream control socket and exactly one standard input descriptor.";
#[cfg(target_os = "linux")]
pub const DUMPABLE: &str = "dumpable";
