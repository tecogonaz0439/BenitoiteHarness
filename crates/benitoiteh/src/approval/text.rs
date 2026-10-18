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
fn target(
    kind: super::ApprovalKind,
    facts: &super::ApprovalFacts,
    project: &std::path::Path,
) -> String {
    facts
        .targets
        .iter()
        .take(if kind == super::ApprovalKind::TestEffects {
            usize::MAX
        } else {
            1
        })
        .map(|p| {
            crate::benitoite::display::one_line(
                &p.strip_prefix(project).unwrap_or(p).to_string_lossy(),
            )
        })
        .collect::<Vec<_>>()
        .join(", ")
}
fn effects(facts: &super::ApprovalFacts) -> String {
    if facts.effects.is_empty() {
        "none".into()
    } else {
        facts.effects.join(", ")
    }
}
pub fn effects_summary(
    kind: super::ApprovalKind,
    facts: &super::ApprovalFacts,
    project: &std::path::Path,
) -> String {
    let prefix = if kind == super::ApprovalKind::TestEffects {
        "Run tests in"
    } else {
        "Run"
    };
    format!(
        "{prefix} {} with effects: {}",
        target(kind, facts, project),
        effects(facts)
    )
}
pub fn effects_denied(
    kind: super::ApprovalKind,
    facts: &super::ApprovalFacts,
    project: &std::path::Path,
) -> String {
    let (verb, object) = if kind == super::ApprovalKind::TestEffects {
        ("run tests in", "tests")
    } else {
        ("run", "script")
    };
    format!(
        "The user declined to {verb} {} with effects: {}. Ask the user how to proceed, or change the {object}.",
        target(kind, facts, project),
        effects(facts)
    )
}

pub const POLICY_TEST_EFFECTS_DENIED: &str = "Tool call was denied by the never approval policy: running Benitoite tests requires confirming their effects. Stop and report.";
pub fn allow_denied(
    kind: super::ApprovalKind,
    facts: &super::ApprovalFacts,
    project: &std::path::Path,
    allowed: &std::collections::BTreeSet<crate::benitoite::Effect>,
) -> String {
    let names = crate::benitoite::effects::names(allowed, false);
    let permitted = if names.is_empty() {
        "none".into()
    } else {
        names.join(", ")
    };
    let (uses, narrow) = if kind == super::ApprovalKind::TestEffects {
        ("use", "the tests")
    } else {
        ("uses", "main")
    };
    format!(
        "This non-interactive run allows only these effects: {permitted}. {} {uses}: {}. Narrow the uses of {narrow}, or stop and report.",
        target(kind, facts, project),
        effects(facts)
    )
}

pub fn unread_imports_denied(facts: &super::ApprovalFacts, project: &std::path::Path) -> String {
    let paths = facts
        .targets
        .iter()
        .map(|p| {
            crate::benitoite::display::one_line(
                &p.strip_prefix(project).unwrap_or(p).to_string_lossy(),
            )
        })
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "{paths} import files that benitoiteh does not read; run them interactively to confirm."
    )
}

pub const POLICY_PROJECT_DENIED: &str = "Project creation was denied by policy. In exec, set --project-parent and choose a new directory directly inside it, or stop and report.";
pub const USER_PROJECT_DENIED: &str = "The user denied creating the project. Nothing was created and the current project did not change.";
pub fn create_project_summary(path: &std::path::Path) -> String {
    format!(
        "Create a project at {} and switch this session to it?",
        path.display()
    )
}
