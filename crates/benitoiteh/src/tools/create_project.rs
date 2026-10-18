//! 作成先を確認の前後で照合し、記述子から新しいプロジェクトを作る（設計書 01-03「create_project（制限モード）」）。

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;

use serde_json::Value;

use super::{
    ApprovalRequest, ApprovalStatus, Invocation, ToolContext, ToolError, ToolOutput, text,
};
use crate::approval::{ApprovalFacts, ApprovalKind};
use crate::safe_fs::{SafeFsError, ScopeRelPath, ScopeRoot};

const TEMPLATE: &str = "import Benitoite.Unofficial.IO.Console\n\nfunction main() -> Unit uses Console.Write\n  Console.writeLine(\"hello, world.\")\nend function\n";

pub(crate) struct Candidate {
    parent: ScopeRoot,
    name: ScopeRelPath,
    path: PathBuf,
    requested: String,
}

fn invalid(message: &str) -> ToolError {
    ToolError::InvalidArguments(message.into())
}

enum CandidateError {
    PolicyDenied,
    Invalid(ToolError),
}

fn candidate(
    context: &ToolContext,
    requested: &str,
    approval: ApprovalStatus,
) -> Result<Candidate, CandidateError> {
    let unavailable = || CandidateError::Invalid(invalid(&text::project_unavailable(requested)));
    // 非対話の許可のない要求では、親の種類や存在も調べない。
    // ループでも同じ範囲を検査する（設計書 03-03「非対話での扱い」）。
    if context.noninteractive && context.project_parent.is_none() {
        return Err(CandidateError::PolicyDenied);
    }
    if requested.starts_with('~') {
        return Err(CandidateError::Invalid(invalid(text::CREATE_TILDE)));
    }
    let relative = requested.strip_prefix('/').unwrap_or(requested);
    ScopeRelPath::new(relative).map_err(|_| CandidateError::Invalid(invalid(text::CREATE_PATH)))?;
    if requested.chars().any(char::is_control) {
        return Err(CandidateError::Invalid(invalid(text::CREATE_PATH)));
    }
    let absolute = if Path::new(requested).is_absolute() {
        PathBuf::from(requested)
    } else {
        context.project.real_path().join(requested)
    };
    let name = absolute.file_name().ok_or_else(unavailable)?;
    let parent = ScopeRoot::open(absolute.parent().ok_or_else(unavailable)?).map_err(|_| {
        if context.noninteractive {
            CandidateError::PolicyDenied
        } else {
            unavailable()
        }
    })?;
    if context.noninteractive && context.project_parent.as_deref() != Some(parent.real_path()) {
        return Err(CandidateError::PolicyDenied);
    }
    let path = parent.real_path().join(name);
    if path.to_str().is_none()
        || path
            .to_str()
            .is_some_and(|path| path.chars().any(char::is_control))
    {
        return Err(unavailable());
    }
    let name = ScopeRelPath::new(Path::new(name)).map_err(|_| unavailable())?;
    let (paths, executable) = context
        .project_creation
        .as_ref()
        .ok_or_else(|| CandidateError::Invalid(invalid(text::CREATE_CONTEXT)))?;
    // 保護された場所の有無や実のパスを、確認の前の誤りから区別させない。
    crate::config::writable::validate_project_candidate(
        paths,
        &path,
        context.temporary.real_path(),
        executable,
    )
    .map_err(|_| unavailable())?;
    if approval == ApprovalStatus::Unapproved {
        match parent.stat(&name) {
            Err(SafeFsError::NotFound(_)) => {}
            Err(_) | Ok(_) => return Err(unavailable()),
        }
    }
    Ok(Candidate {
        parent,
        name,
        path,
        requested: requested.into(),
    })
}

pub(super) fn execute(
    context: &ToolContext,
    args: &Value,
    approval: ApprovalStatus,
    invocation: Invocation<'_>,
) -> Result<ToolOutput, ToolError> {
    let requested = args
        .get("path")
        .and_then(Value::as_str)
        .ok_or_else(|| invalid(text::CREATE_PATH))?;
    let candidate = match candidate(context, requested, approval) {
        Ok(candidate) => candidate,
        Err(CandidateError::Invalid(error)) => return Err(error),
        Err(CandidateError::PolicyDenied) => {
            let mut output = ToolOutput::error(crate::approval::policy_denial_message(
                ApprovalKind::CreateProject,
            ));
            output.policy_denied = true;
            return Ok(output);
        }
    };
    if invocation.cancelled.load(Ordering::Acquire) {
        return Err(invalid(text::CREATE_CANCELLED));
    }
    if approval == ApprovalStatus::Unapproved {
        let facts = ApprovalFacts {
            path: Some(candidate.path.clone()),
            ..ApprovalFacts::default()
        };
        let summary = crate::approval::create_project_summary(&candidate.path);
        context
            .project_candidates
            .lock()
            .map_err(|_| ToolError::State)?
            .insert(invocation.call_id.into(), candidate);
        let mut output = ToolOutput::error(summary.clone());
        output.approval = Some(ApprovalRequest {
            kind: ApprovalKind::CreateProject,
            facts,
            justification: None,
            summary,
        });
        return Ok(output);
    }
    let previous = context
        .project_candidates
        .lock()
        .map_err(|_| ToolError::State)?
        .remove(invocation.call_id)
        .ok_or_else(|| invalid(text::CREATE_CHANGED))?;
    if previous.path != candidate.path
        || previous.requested != requested
        || !previous.parent.same_directory(&candidate.parent)?
    {
        return Err(invalid(text::CREATE_CHANGED));
    }
    // 最後の名前は mkdirat で新しく作り、開いた根からだけ雛形を書く。
    // パスから開き直さない（設計書 01-03「道具の定義の共通の規則」）。
    let mut created = context
        .created_projects
        .lock()
        .map_err(|_| ToolError::State)?;
    let root = candidate
        .parent
        .create_dir_root(&candidate.name)
        .map_err(|error| {
            if matches!(error, SafeFsError::AlreadyExists(_)) {
                invalid(text::CREATE_EXISTS)
            } else {
                invalid(&text::create_failure(&error))
            }
        })?;
    let main = ScopeRelPath::new("main.bnt")?;
    let mut wrote = false;
    let result = (|| -> Result<(), ToolError> {
        let mut file = root.create_new_with_mode(&main, 0o644)?;
        wrote = true;
        file.write_all(TEMPLATE.as_bytes())?;
        file.flush()?;
        Ok(())
    })();
    if let Err(error) = result {
        if wrote {
            let _cleanup = root.remove_file(&main);
        }
        let _cleanup = candidate.parent.remove_dir(&candidate.name);
        return Err(invalid(&text::create_failure(&error)));
    }
    let text = text::project_created(root.real_path());
    created.insert(invocation.call_id.into(), root);
    Ok(ToolOutput::success(text))
}
