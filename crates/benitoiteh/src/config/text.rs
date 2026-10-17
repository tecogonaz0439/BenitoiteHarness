//! 設定の診断を英語でまとめる（設計書 05-01「設定ファイル」、02-01「認証の情報」）。

pub const HOME_REQUIRED: &str = "HOME must name an absolute home directory";
pub const INVALID_PATH: &str = "configuration paths must be absolute";
pub const INVALID_ID: &str = "session ID must be a single path component";
pub const INVALID_CONFIG: &str = "invalid configuration (values omitted)";
pub const NO_PROVIDER: &str = "no provider selected; set default_provider or use --provider";
pub const INVALID_KEY_REF: &str =
    "api_key must contain exactly one nonempty env or command reference";
pub const KEY_UNAVAILABLE: &str = "API key environment variable is missing, empty, or not UTF-8";
pub const KEY_COMMAND: &str = "API key command failed";
pub const KEY_OUTPUT: &str = "API key command must return a nonempty UTF-8 first line";
pub const REDACTED: &str = "[REDACTED]";
pub const INVALID_PROGRAM: &str = "restricted.allowed_programs must contain absolute paths";
pub const INVALID_EFFECT: &str =
    "restricted.scratch_auto_effects contains an effect that cannot be allowed automatically";
pub const INVALID_VALUE: &str = "configuration contains an empty name";
pub const DIRECTORY_REQUIRED: &str = "configuration and writable roots must be directories";
pub const ROOT_OR_HOME: &str = "writable root is the filesystem root, home, or an ancestor of home";
pub const HOME_PRIVATE: &str = "writable root is inside a home dot directory or macOS Library";
pub const CONFIG_OR_DATA: &str = "writable root overlaps the configuration or data directory";
pub const PROJECT_TEMP: &str = "project and temporary directory overlap";

pub fn unknown_field(field: &str) -> String {
    format!("unknown configuration field `{}`", field.escape_debug())
}

pub fn unknown_provider(name: &str) -> String {
    format!("unknown provider `{}`", name.escape_debug())
}

pub const EXEC_APPROVAL: &str = "exec only supports --approval never";
pub const NORMAL_OPTIONS: &str = "--sandbox, --approval, and --no-sandbox require normal mode";
pub const WRITABLE_EXECUTABLE: &str = "writable root contains the benitoiteh executable";

pub const RESUME_MODE: &str =
    "exec cannot resume a restricted session in normal mode; interactive confirmation is required.";
