//! CLI に表示する英文をまとめる（設計書 06-01「lint と実装の規約」）。

/// 利用者向けのコマンドラインの一覧。
pub const HELP: &str = "Usage:
    benitoiteh [options] [request]
    benitoiteh exec [options] <request>
    benitoiteh doctor [--project <dir>] [--json]
    benitoiteh sessions [--project <dir>] [--json]
    benitoiteh --help | --version | --licenses

Options (interactive and exec):
    --mode restricted|normal
    --project <dir>
    --provider <name>
    --model <name>
    --effort <value>
    --max-turns <n>
    --max-check-fix <n>
    --max-run-tokens <n>
    --sandbox read-only|workspace-write|danger-full-access
    --approval untrusted|on-request|never
    --no-sandbox
    --resume <ID>
    --continue
    --json
    --output-last-message <file>
    --help

Use - as the exec request to read standard input.
--json and --output-last-message are exec-only.
exec only supports --approval never.
--licenses is not available yet.
The following options are not available yet:
    --allow, --project-parent
";

/// 使い方の誤りに共通の案内。
pub const INVALID_ARGUMENTS: &str = "benitoiteh: invalid arguments; use --help for usage";

/// 重複した指定を示す。
pub fn duplicate_option(option: &str) -> String {
    format!("benitoiteh: duplicate option {option}; use --help for usage")
}

/// 後の段階で使える指定を示す。
pub fn unavailable_option(option: &str) -> String {
    format!("benitoiteh: option {option} is not available yet")
}

/// ビルドしたパッケージの版を一行に整える。
pub fn version() -> String {
    format!("benitoiteh {}\n", env!("CARGO_PKG_VERSION"))
}

/// 出力に失敗した理由を案内に添える。
pub fn output_error(error: &std::io::Error) -> String {
    format!("benitoiteh: could not write output: {error}")
}

/// panic の内容を含めない診断。
pub const INTERNAL_ERROR: &str = "benitoiteh: internal error (panic)";
/// panic hook の中でシリアライズや共有ロックを要しない一行。
pub const PANIC_RECORD: &[u8] =
    b"{\"type\":\"error\",\"kind\":\"panic\",\"message\":\"benitoiteh: internal error (panic)\"}\n";

/// 起動の診断に実行ファイルの名前を添える。
pub fn startup_error(error: &impl std::fmt::Display) -> String {
    format!("benitoiteh: {error}")
}

/// ループの診断に実行ファイルの名前を添える。
pub fn agent_error(error: &crate::agent::AgentError) -> String {
    format!("benitoiteh: {error}")
}

/// 読んだプロジェクト設定の場所を示す。
pub fn project_loaded(root: &std::path::Path) -> String {
    format!(
        "Loaded project configuration: {}",
        root.join(".benitoiteh/config.toml").display()
    )
}

/// 設定全体を採用しなかった理由を示す。
pub fn project_ignored(error: &crate::config::ConfigError) -> String {
    format!("Ignoring project configuration: {error}")
}

/// 通信前に、選択された提供者とモデルを示す。
pub fn destination(destination: &crate::provider::Destination) -> String {
    format!(
        "Provider: {}; model: {}",
        destination.provider, destination.model
    )
}

/// 明示した許可によってサンドボックスなしで続けるときの通知。
pub const SANDBOX_DISABLED: &str =
    "Sandbox unavailable; continuing without a sandbox because --no-sandbox was specified.";

pub const PRIVATE_DIRECTORY: &str =
    "sandbox directories must be owned by the current user with mode 700";

pub fn sandbox_disabled(reason: &crate::sandbox::SandboxError) -> String {
    format!("{SANDBOX_DISABLED} Reason: {reason}")
}

pub const RESUME_PROJECT: &str = "--project does not match the recorded session project directory.";

/// 再開する ID とプロジェクトを一行で示す。
pub fn resuming(id: &crate::session::SessionId, project: &std::path::Path) -> String {
    format!("Resuming session {id}; project: {}", project.display())
}

/// 端末以外からの対話の起動を拒む。
pub const TERMINAL_REQUIRED: &str =
    "Interactive mode requires terminal input and output; use benitoiteh exec instead.";
/// モードの変更を拒否した通常終了。
pub const MODE_CHANGE_DECLINED: &str = "Resume in normal mode was declined.";

pub fn checkpoint_before_rewind(checkpoint: &str) -> String {
    format!("Checkpoint before rewind: {checkpoint}")
}

pub const SESSION_PATH_CHANGED: &str = "A fixed session path now resolves to a different location.";

pub const ALLOW_RESTRICTED: &str = "benitoiteh: --allow is available only in restricted mode.";
