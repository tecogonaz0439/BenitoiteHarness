//! ファイルを作成・置換し、LLM の要約と画面用の差分を分ける（設計書 01-03「write」「読んでから直す規則」）。

use super::{
    ApprovalStatus, ToolContext, ToolError, ToolOutput, read::string, text, write_path::WriteTarget,
};
use serde_json::Value;
use std::path::Path;

pub(super) fn execute(
    context: &ToolContext,
    arguments: &Value,
    approval: ApprovalStatus,
    invocation: super::Invocation<'_>,
) -> Result<ToolOutput, ToolError> {
    let path = string(arguments, "path")?;
    let contents = string(arguments, "content")?;
    let target = WriteTarget::resolve(context, Path::new(path))?;
    let previous = target.version()?;
    if let Some(output) = target.approval(context, approval)? {
        return Ok(output);
    }
    let before = if let Some(version) = &previous {
        target.check_read(context, version)?;
        target.read(version)?
    } else {
        Vec::new()
    };
    context.checkpoint_before(
        crate::rewind::CheckpointReason::Write,
        Some(&target.real_path),
        &invocation,
    )?;
    let real_path = target.real_path.clone();
    target.write(context, previous, contents.as_bytes())?;
    Ok(super::benitoite_check::after_write(
        context,
        path,
        &real_path,
        output(path, &String::from_utf8_lossy(&before), contents),
        invocation.cancelled,
    ))
}

pub(super) fn output(path: &str, before: &str, after: &str) -> ToolOutput {
    let mut output = ToolOutput::success(text::wrote(path, after.lines().count()));
    let diff = similar::TextDiff::from_lines(before, after);
    let mut added = 0usize;
    let mut deleted = 0usize;
    for change in diff.iter_all_changes() {
        match change.tag() {
            similar::ChangeTag::Insert => added = added.saturating_add(1),
            similar::ChangeTag::Delete => deleted = deleted.saturating_add(1),
            similar::ChangeTag::Equal => {}
        }
    }
    output.screen = Some(text::screen_diff(added, deleted));
    output.display = Some(diff.unified_diff().header(path, path).to_string());
    output
}
