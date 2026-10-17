//! 静的検査と書いた後の検査を同じ診断の規則につなぐ（設計書 01-03、04-01）。

use std::ffi::OsString;
use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use serde_json::Value;

use crate::benitoite::{
    BenitoiteError, QueryLimits, StderrLimit,
    diagnostics::{self, CheckClass, PreparedDiagnostics, push_line},
    skill::check_reason,
};
use crate::config::Mode;
use crate::safe_fs::{SafeFsError, ScopeRelPath};

use super::{
    CheckLaunch, CheckOutcome, Invocation, ToolContext, ToolError, ToolOutput, read::string, text,
};

const DIAGNOSTICS_BYTES: usize = 16 * 1024 * 1024;

fn query(
    context: &ToolContext,
    path: &Path,
    tests: bool,
    cancelled: &AtomicBool,
) -> Result<(CheckClass, PreparedDiagnostics), BenitoiteError> {
    let query = context
        .check_query
        .as_ref()
        .ok_or(BenitoiteError::Invalid(text::SKILL_UNAVAILABLE))?;
    let mut args: Vec<OsString> = if tests {
        vec!["test".into(), "--check".into()]
    } else {
        vec!["check".into()]
    };
    args.push("--diagnostics=json".into());
    args.push(path.as_os_str().to_owned());
    let limits = QueryLimits {
        timeout: Duration::from_secs(30),
        stderr: StderrLimit::FailOver(DIAGNOSTICS_BYTES),
        ..QueryLimits::default()
    };
    let (status, stderr) = match query(&args, limits, cancelled) {
        Ok(output) => (0, output.stderr),
        Err(BenitoiteError::Exit {
            code: Some(status),
            stderr,
        }) => (status, stderr),
        Err(error) => return Err(error),
    };
    let parsed = diagnostics::parse(&stderr);
    let class = diagnostics::classify_check(status, &parsed);
    Ok((class, PreparedDiagnostics::new(parsed.diagnostics, context)))
}

pub(super) fn execute(
    context: &ToolContext,
    arguments: &Value,
    invocation: Invocation<'_>,
) -> Result<ToolOutput, ToolError> {
    let path = string(arguments, "path")?;
    if context.check_query.is_none() {
        return Ok(ToolOutput::error(text::check_failure(
            path,
            text::SKILL_UNAVAILABLE,
        )));
    }
    let requested = Path::new(path);
    let real_path =
        if std::fs::metadata(context.requested_path(requested)).is_ok_and(|meta| meta.is_dir()) {
            context.search_path(requested)?
        } else {
            context.open_file(requested, false)?.real_path
        };
    let tests = arguments
        .get("tests")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let mut output = match query(context, &real_path, tests, invocation.cancelled) {
        Ok((class @ (CheckClass::Ok | CheckClass::Warnings | CheckClass::Failed), diagnostics)) => {
            let rendered = diagnostics.render(context, None);
            let mut body = rendered.text;
            if rendered.unread > 0 {
                push_line(
                    &mut body,
                    &crate::benitoite::text::unread_diagnostics(rendered.unread),
                );
            }
            if body.is_empty() {
                body = text::CHECK_OK.into();
            }
            let diagnostics = body.clone();
            let mut output = ToolOutput::success(body);
            output.is_error = class == CheckClass::Failed;
            output.check = Some(CheckOutcome {
                key: real_path,
                failed: class == CheckClass::Failed,
                diagnostics,
            });
            output
        }
        Ok((CheckClass::Usage(reason) | CheckClass::Processor(reason), _)) => {
            ToolOutput::error(text::check_failure(path, &reason))
        }
        Err(BenitoiteError::DiagnosticsLimit) => {
            ToolOutput::error(crate::benitoite::text::DIAGNOSTICS_LIMIT)
        }
        Err(error) => ToolOutput::error(text::check_failure(path, &check_reason(&error))),
    };
    output.check_launch = Some(CheckLaunch::Tool);
    Ok(output)
}

pub(super) fn after_write(
    context: &ToolContext,
    path: &str,
    real_path: &Path,
    mut output: ToolOutput,
    cancelled: &AtomicBool,
) -> ToolOutput {
    if context.mode != Mode::Restricted
        || context.check_query.is_none()
        || real_path.extension().is_none_or(|ext| ext != "bnt")
    {
        return output;
    }
    if !real_path.starts_with(context.project.real_path()) {
        push_line(&mut output.text, &text::not_checked_outside(path));
        return output;
    }
    match nested_package(context, real_path) {
        Ok(true) => {
            push_line(&mut output.text, &text::not_checked_nested(path));
            return output;
        }
        Ok(false) => {}
        Err(error) => {
            push_line(
                &mut output.text,
                &text::after_check_incomplete(path, &diagnostics::one_line(&error.to_string())),
            );
            return output;
        }
    }
    output.check_launch = Some(CheckLaunch::AfterWrite);
    match query(context, context.project.real_path(), true, cancelled) {
        Ok((CheckClass::Ok | CheckClass::Warnings | CheckClass::Failed, diagnostics)) => {
            let rendered = diagnostics.render(context, Some(real_path));
            let mut body = text::after_check(path, rendered.selected == 0);
            if !rendered.text.is_empty() {
                push_line(&mut body, &rendered.text);
            }
            if rendered.other > 0 {
                push_line(&mut body, &text::other_diagnostics(rendered.other));
            }
            if rendered.unread > 0 {
                push_line(
                    &mut body,
                    &crate::benitoite::text::unread_diagnostics(rendered.unread),
                );
            }
            output.check = Some(CheckOutcome {
                key: real_path.into(),
                failed: rendered.failed,
                diagnostics: body.clone(),
            });
            push_line(&mut output.text, &body);
        }
        Ok((CheckClass::Usage(reason) | CheckClass::Processor(reason), _)) => push_line(
            &mut output.text,
            &text::after_check_incomplete(path, &reason),
        ),
        Err(error) => push_line(
            &mut output.text,
            &text::after_check_incomplete(path, &check_reason(&error)),
        ),
    }
    output.text = super::truncate::cap_middle(&output.text);
    output
}

fn nested_package(context: &ToolContext, path: &Path) -> Result<bool, ToolError> {
    for parent in path
        .ancestors()
        .skip(1)
        .take_while(|path| *path != context.project.real_path())
    {
        let relative = parent
            .strip_prefix(context.project.real_path())
            .map_err(|_| ToolError::OutsideScope)?
            .join("package.bnt");
        match context.project.metadata(&ScopeRelPath::new(relative)?) {
            Ok(_) => return Ok(true),
            Err(SafeFsError::NotFound(_)) => {}
            Err(error) => return Err(error.into()),
        }
    }
    Ok(false)
}
