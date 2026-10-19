//! 秘密の仲介の診断と、値を含めない記録の英文（設計書 03-04）。
pub const UNAVAILABLE: &str = "Secret input is only available for commands approved to run outside the sandbox in interactive sessions.";
pub const CLOSED: &str = "Secret input is no longer available for this command.";
pub const REFUSED: &str = "Secret input was refused.";
pub const DISABLED: &str =
    "Secret input mediation could not be prepared; inherited askpass settings will be used.";
pub const SPILL_DISCARDED: &str =
    "Raw output was not saved because secret input mediation was enabled.";
pub const REPLACEMENT: &[u8] = b"[secret]";
pub fn event(kind: &str, outcome: &str) -> String {
    format!("Secret prompt ({kind}): {outcome}.")
}

pub const NORMAL_FULL: &str = "normal (danger-full-access)";
pub const NORMAL_ESCALATED: &str = "normal (escalated)";

pub const FAILED: &str = "Could not prepare secret input:";

pub const LISTENER_DISABLED: &str =
    "Secret input mediation could not be prepared; askpass helpers will refuse requests.";
pub const CHANGED_DIRECTORY: &str = "Secret input mediation was disabled because its helper directory changed; askpass variables were removed.";

/// 他の利用者の親は使わず、利用者専用の一時領域へ切り替える案を示す。
pub fn unowned_parent(path: &std::path::Path) -> String {
    let path = crate::benitoite::display::one_line(&path.to_string_lossy());
    format!(
        "Secret input mediation could not be prepared: {path} is owned by another user. Set TMPDIR to a private directory owned by you; inherited askpass settings will be used."
    )
}
