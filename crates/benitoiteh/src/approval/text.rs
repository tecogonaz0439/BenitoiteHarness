//! 方針による拒否と利用者による拒否の英文（設計書 03-03「通常モードの承認の方針」）。

pub const POLICY_WRITE_DENIED: &str = "Tool call was denied by the never approval policy: writing outside the writable roots or to a protected location requires approval.";
pub const POLICY_COMMAND_DENIED: &str =
    "Tool call was denied by the never approval policy: this command requires approval.";
pub const POLICY_ESCALATE_DENIED: &str = "Tool call was denied by the never approval policy: running a command outside the sandbox requires approval.";
pub const USER_WRITE_DENIED: &str = "The user denied this write. No file was changed by this call.";
pub const USER_COMMAND_DENIED: &str = "The user denied this command. It was not run.";
pub const USER_ESCALATE_DENIED: &str =
    "The user denied running this command outside the sandbox. It was not run.";

pub fn with_user_message(denial: &str, message: &str) -> String {
    format!("{denial}\nMessage from the user: {message}")
}
