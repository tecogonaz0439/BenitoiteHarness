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

pub const POLICY_WEB_DENIED: &str = "Tool call was denied by the never approval policy: this web domain requires approval. Use a domain in web_fetch.allowed_domains or stop and report.";
pub const USER_WEB_DENIED: &str =
    "The user denied this web fetch. No DNS lookup or HTTP request was made by this call.";

pub const POLICY_EFFECTS_DENIED: &str = "Tool call was denied by the never approval policy: running a Benitoite script requires confirming its effects. Stop and report.";
fn target(facts: &super::ApprovalFacts, project: &std::path::Path) -> String {
    facts
        .targets
        .first()
        .map(|p| {
            p.strip_prefix(project)
                .unwrap_or(p)
                .to_string_lossy()
                .into_owned()
        })
        .unwrap_or_default()
}
fn effects(facts: &super::ApprovalFacts) -> String {
    if facts.effects.is_empty() {
        "none".into()
    } else {
        facts.effects.join(", ")
    }
}
pub fn effects_summary(facts: &super::ApprovalFacts, project: &std::path::Path) -> String {
    format!(
        "Run {} with effects: {}",
        target(facts, project),
        effects(facts)
    )
}
pub fn effects_denied(facts: &super::ApprovalFacts, project: &std::path::Path) -> String {
    format!(
        "The user declined to run {} with effects: {}. Ask the user how to proceed, or change the script.",
        target(facts, project),
        effects(facts)
    )
}
