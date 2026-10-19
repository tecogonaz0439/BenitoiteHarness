//! 認証の診断と端末に示す英文（設計書 05-01「login と logout」）。

pub const VALIDATION: &str = "ChatGPT authentication response could not be verified.";
pub const STORAGE: &str = "ChatGPT authentication storage must be private (directories 0700, files 0600), owned by the current user, and contain valid data.";
pub const COMMUNICATION: &str =
    "ChatGPT authentication request failed; credentials were not printed.";
pub const CANCELLED: &str = "ChatGPT sign-in cancelled.";
pub const TIMEOUT: &str = "Timed out waiting for ChatGPT sign-in.";
pub const LOGIN_REQUIRED: &str = "Please run benitoiteh login chatgpt again.";
pub const INVALID_CLIENT: &str =
    "ChatGPT rejected the registered client; automatic retry is unavailable.";
pub const SCOPE: &str = "Allow ChatGPT plan usage, then run benitoiteh login chatgpt again.";
pub const ACCOUNT: &str = "A different ChatGPT account is already registered. Run benitoiteh logout chatgpt, then benitoiteh login chatgpt to use another account.";
pub const RANDOM: &str = "Could not obtain randomness for ChatGPT sign-in.";
pub const WAITING: &str = "Continue with ChatGPT";
pub const CANCEL: &str = "Press Ctrl-C to cancel.";
pub const SIGNED_IN: &str = "Signed in to ChatGPT.";
pub const SIGNED_OUT: &str = "Signed out of ChatGPT.";
pub const NOT_SIGNED_IN: &str = "Not signed in to ChatGPT.";
pub const REVOCATION_UNCONFIRMED: &str = "Local ChatGPT credentials were removed, but revocation could not be confirmed. Disconnect benitoiteh in ChatGPT settings.";
pub const CALLBACK: &str = "Return to the terminal to continue.\n";
pub const REDACTED: &str = "[REDACTED]";
/// アカウントの宛先をログインの通知に添える。
pub fn signed_in(email: &str) -> String {
    format!("Signed in to ChatGPT as {email}.")
}
/// 安全な文字に制限済みの OAuth の誤りの符号を示す。
pub fn rejected(code: &str) -> String {
    format!("ChatGPT sign-in was rejected ({code}).")
}
