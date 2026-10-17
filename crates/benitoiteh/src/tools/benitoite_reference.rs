//! Skill の参照と診断の節を処理系から返す（設計書 01-03「benitoite_explain と benitoite_reference」）。

use super::{ToolContext, ToolError, ToolOutput, read, text};
use crate::benitoite::{
    BenitoiteError,
    skill::{SkillDocuments, query_reason, skill_parts},
};
use crate::config::Mode;
use serde_json::Value;
use std::sync::atomic::AtomicBool;

fn failure(name: &str, error: BenitoiteError) -> ToolError {
    let reason = query_reason(&error);
    ToolError::InvalidArguments(text::skill_failure(name, &reason))
}
fn skill<'a>(context: &'a ToolContext, name: &str) -> Result<&'a SkillDocuments, ToolError> {
    context.skill.as_deref().ok_or_else(|| {
        ToolError::InvalidArguments(text::skill_failure(name, text::SKILL_UNAVAILABLE))
    })
}

pub(super) fn reference(
    context: &ToolContext,
    arguments: &Value,
    cancelled: &AtomicBool,
) -> Result<ToolOutput, ToolError> {
    let offset = read::number(arguments, "offset", 1, false)?;
    let limit = read::number(arguments, "limit", 2000, false)?.min(2000);
    let requested_name = arguments.get("name").and_then(Value::as_str).map(|name| {
        if name == "SKILL" {
            "SKILL.md".into()
        } else {
            format!("references/{name}.md")
        }
    });
    let documents = skill(
        context,
        requested_name.as_deref().unwrap_or(text::SKILL_LIST_QUERY),
    )?;
    let names = documents
        .names(cancelled)
        .map_err(|error| failure(text::SKILL_LIST_QUERY, error))?;
    let mut available = Vec::new();
    if context.mode == Mode::Normal {
        available.push("SKILL");
    }
    available.extend(
        names
            .iter()
            .filter_map(|name| name.strip_prefix("references/")?.strip_suffix(".md")),
    );
    let listing = || format!("{}\n{}", text::SKILL_LIST, available.join("\n"));
    let Some(name) = arguments.get("name").and_then(Value::as_str) else {
        return Ok(ToolOutput::success(listing()));
    };
    if !available.contains(&name) {
        return Ok(ToolOutput::error(text::unknown_skill(name, &listing())));
    }
    let requested = if name == "SKILL" {
        "SKILL.md".into()
    } else {
        format!("references/{name}.md")
    };
    let document = documents
        .document(&requested, cancelled)
        .map_err(|error| failure(&requested, error))?;
    let body = if name == "SKILL" {
        skill_parts(document.text())
            .map_err(|error| failure(&requested, error))?
            .body
    } else {
        document.text()
    };
    Ok(ToolOutput::success(read::split_text(
        body,
        offset,
        limit,
        read::LineStyle::Reference,
    )?))
}

pub(super) fn explain(
    context: &ToolContext,
    arguments: &Value,
    cancelled: &AtomicBool,
) -> Result<ToolOutput, ToolError> {
    let code = read::string(arguments, "code")?;
    let mut bytes = code.bytes();
    if code.len() != 5
        || !bytes.next().is_some_and(|byte| byte.is_ascii_uppercase())
        || !bytes.all(|byte| byte.is_ascii_digit())
    {
        return Err(ToolError::InvalidArguments(text::INVALID_CODE.into()));
    }
    let name = "references/diagnostics.md";
    let documents = skill(context, name)?;
    documents
        .names(cancelled)
        .map_err(|error| failure(text::SKILL_LIST_QUERY, error))?;
    let document = documents
        .document(name, cancelled)
        .map_err(|error| failure(name, error))?;
    let heading = format!("## {code}");
    let mut lines = document.text().lines().skip_while(|line| *line != heading);
    let Some(first) = lines.next() else {
        return Ok(ToolOutput::error(text::missing_code(code)));
    };
    let mut section = vec![first];
    section.extend(lines.take_while(|line| !line.starts_with("## ")));
    Ok(ToolOutput::success(section.join("\n").trim_end()))
}
