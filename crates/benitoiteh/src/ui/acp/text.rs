//! ACP（エディタとの連携）で利用者・クライアントに示す英文をまとめる
//! （設計書 05-03、06-01「lint と実装の規約」）。

use crate::approval::{ApprovalKind, ApprovalRecord};

/// 既定の JSON-RPC エラー文言。
pub const PARSE_ERROR: &str = "Parse error.";
pub const INVALID_REQUEST: &str = "Invalid JSON-RPC 2.0 request.";
pub const METHOD_NOT_FOUND: &str = "Method not found.";
pub const INVALID_PARAMS: &str = "Invalid params.";
pub const INTERNAL_ERROR: &str = "Internal error.";
pub const LINE_TOO_LONG: &str = "JSON-RPC message exceeds the 16 MiB limit.";

/// 状態の誤りの文言。
pub const PROMPT_IN_PROGRESS: &str = "A prompt is already running for the active session.";
pub const UNKNOWN_SESSION: &str = "The requested sessionId is not active.";
pub const SESSION_SWITCH_REASON: &str = "Switched to another ACP session.";
pub const SESSION_CLOSED_REASON: &str = "ACP connection closed.";

/// パラメータ検証の文言。
pub const INVALID_PROTOCOL_VERSION: &str = "protocolVersion must be a positive integer.";
pub const INVALID_METHOD_ID: &str = "methodId must be a non-empty string.";
pub const UNKNOWN_AUTH_METHOD: &str = "Unknown authentication methodId.";
pub const INVALID_CWD: &str = "cwd must be an absolute path string.";
pub const INVALID_MCP_SERVERS: &str = "mcpServers must be an array.";
pub const INVALID_SESSION_ID: &str = "sessionId must be a non-empty string.";
pub const INVALID_CONFIG_OPTION: &str = "configId must be model or effort with a valid value.";
pub const EFFORT_UNSUPPORTED: &str = "The selected provider does not support reasoning effort.";
pub const INVALID_PROMPT: &str =
    "prompt must be a non-empty array of supported ContentBlock values.";
pub const UNSUPPORTED_CONTENT_BLOCK: &str =
    "Unsupported ContentBlock type or binary resource blob.";
pub const IMAGE_UNSUPPORTED: &str = "The selected provider does not support image input.";
pub const IMAGE_LIMIT_EXCEEDED: &str = "At most 20 images are allowed per prompt.";
pub const IMAGE_INVALID: &str = "Image block must contain valid base64 data and a supported image MIME type within the 64 MiB limit.";

/// 認証方法の表示。
pub const AUTH_API_KEY_ID: &str = "api-key";
pub const AUTH_API_KEY_NAME: &str = "API key";
pub const AUTH_API_KEY_DESCRIPTION: &str =
    "Use the provider API key configured in benitoiteh config.toml.";
pub const AUTH_CHATGPT_ID: &str = "chatgpt";
pub const AUTH_CHATGPT_NAME: &str = "ChatGPT";
pub const AUTH_CHATGPT_DESCRIPTION: &str =
    "Use the saved ChatGPT login from benitoiteh login chatgpt.";

/// `configOptions` の表示。
pub const CONFIG_MODEL_NAME: &str = "Model";
pub const CONFIG_EFFORT_NAME: &str = "Reasoning effort";

/// 確認の選択肢のラベル。
pub const OPTION_ALLOW_ONCE: &str = "Allow once";
pub const OPTION_ALLOW_COMMAND: &str = "Allow for this command";
pub const OPTION_ALLOW_SESSION: &str = "Allow for this session";
pub const OPTION_ALLOW_DOMAIN_SESSION: &str = "Allow domain for this session";
pub const OPTION_DECLINE: &str = "Decline";

/// 確認の補足事実。
pub const SCOPE_LABEL: &str = "Scope";
pub const ESCALATE_SCOPE: &str =
    "This command will run outside the OS sandbox. This approval covers this call only.";

/// MCP サーバ設定の件数だけを示す診断。資格情報・URL・コマンドは絶対に含めない（設計書 05-03）。
pub fn ignored_mcp_servers(count: usize) -> String {
    format!(
        "Ignoring {count} MCP server(s) from ACP client; benitoiteh does not connect to MCP servers."
    )
}

/// 通信の中継の確認に添える有効範囲の説明（設計書 05-03「通信の中継と秘密の入力の扱い」）。
pub fn network_scope(domain: &str) -> String {
    format!("Allowing once applies to this command only. Allowing covers every port of {domain}.")
}

/// 確認の種類と条件に応じた一行の題（設計書 05-03「実行の前の確認」）。
pub fn permission_title(request: &ApprovalRecord) -> &'static str {
    match request.kind {
        ApprovalKind::CreateProject => "Create Benitoite project?",
        ApprovalKind::Command => "Run command?",
        ApprovalKind::Escalate => "Run command outside the sandbox?",
        ApprovalKind::WriteScope => {
            if request.facts.agents_write {
                "Write under .agents/?"
            } else {
                "Write outside the allowed scope?"
            }
        }
        ApprovalKind::WebFetch => "Fetch web page?",
        ApprovalKind::Network => "Allow network access?",
        ApprovalKind::Effects => "Run Benitoite script?",
        ApprovalKind::TestEffects => "Run Benitoite tests?",
    }
}
