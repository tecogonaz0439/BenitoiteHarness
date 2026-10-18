//! 報告と確認を経て、同じ集合の権限だけで実行する（設計書 01-03、03-02、03-03、04-01）。

use super::{
    ApprovalRequest, ApprovalStatus, ConfirmKey, Invocation, ToolContext, ToolError, ToolOutput,
    benitoite_process::{Stderr, append, finish, prepare, render},
    process_output::Output,
    read::string,
    text,
};
use crate::approval::{self, ApprovalFacts, ApprovalKind};
use crate::benitoite::{
    self, BenitoiteError, Effect, QueryLimits, StderrLimit, effects,
    report::{self, InspectionReport, ReportError, ReportKind},
};
use crate::sandbox::run::{self, Leftovers, RunEnd, RunLimits, StopSignal};
use serde_json::Value;
use std::collections::BTreeSet;
use std::ffi::OsString;
use std::os::unix::process::ExitStatusExt;
use std::path::Path;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

fn failure(error: &dyn std::fmt::Display) -> ToolError {
    ToolError::InvalidArguments(text::run_failure(error))
}

pub(super) fn execute(
    context: &ToolContext,
    arguments: &Value,
    approval: ApprovalStatus,
    invocation: Invocation<'_>,
) -> Result<ToolOutput, ToolError> {
    let path = string(arguments, "path")?;
    let timeout = match arguments.get("timeout") {
        None => 120,
        Some(value) => value
            .as_u64()
            .filter(|n| (1..=600).contains(n))
            .ok_or_else(|| ToolError::InvalidArguments(text::BASH_TIMEOUT.into()))?,
    };
    let opened = context.open_file(Path::new(path), false)?;
    let real_path = &opened.real_path;
    if real_path.extension().is_none_or(|ext| ext != "bnt") || real_path.to_str().is_none() {
        return Err(ToolError::InvalidArguments(text::RUN_PATH.into()));
    }
    let Some(query) = &context.check_query else {
        return Ok(ToolOutput::error(benitoite::text::inspection_failure(
            path,
            text::SKILL_UNAVAILABLE,
        )));
    };
    let report = match query(
        &[
            "check".into(),
            "--report=json".into(),
            real_path.as_os_str().to_owned(),
        ],
        QueryLimits {
            timeout: Duration::from_secs(30),
            stderr: StderrLimit::KeepFirst(64 * 1024),
            ..QueryLimits::default()
        },
        invocation.cancelled,
    ) {
        Ok(output) => report::parse(&output.stdout, Some(0), ReportKind::Run),
        Err(BenitoiteError::Exit { code, stdout, .. }) => {
            report::parse(&stdout, code, ReportKind::Run)
        }
        Err(error) => {
            let message = if matches!(error, BenitoiteError::OutputLimit) {
                error.to_string()
            } else {
                benitoite::text::inspection_failure(path, &benitoite::skill::report_reason(&error))
            };
            return Ok(ToolOutput {
                processor_launches: 1,
                ..ToolOutput::error(message)
            });
        }
    };
    let report = match report {
        Ok(report) => report,
        Err(error) => {
            let message = match error {
                ReportError::Check(diagnostics) => {
                    render(diagnostics, context, invocation.cancelled)
                }
                ReportError::Invalid(reason) => benitoite::text::inspection_failure(path, &reason),
            };
            return Ok(ToolOutput {
                processor_launches: 1,
                ..ToolOutput::error(message)
            });
        }
    };
    let confirmation: BTreeSet<_> = report
        .effects
        .iter()
        .copied()
        .filter(|e| *e != Effect::AssertCheck)
        .collect();
    let mut facts = ApprovalFacts {
        targets: vec![real_path.clone()],
        effects: effects::names(&confirmation, false),
        shell_reference: report.shell_reference && report.effects.contains(&Effect::ProcessRun),
        sources: report.sources.clone(),
        unread_imports: benitoite::diagnostics::unread_imports(
            &report.sources,
            context,
            invocation.cancelled,
        ),
        ..ApprovalFacts::default()
    };
    let key = ConfirmKey::from_facts(ApprovalKind::Effects, &facts).ok_or(ToolError::State)?;
    let previous = invocation.confirmed_effects.get(&key);
    if facts.unread_imports > 0 && context.noninteractive && approval == ApprovalStatus::Approved {
        return Ok(ToolOutput {
            processor_launches: 1,
            ..ToolOutput::error(approval::unread_imports_denied(
                &facts,
                context.project.real_path(),
            ))
        });
    }
    if !previous.is_some_and(|effects| confirmation.is_subset(effects))
        || (facts.unread_imports > 0 && approval == ApprovalStatus::Unapproved)
    {
        if approval == ApprovalStatus::Approved {
            return Ok(ToolOutput {
                processor_launches: 1,
                ..ToolOutput::error(benitoite::text::EFFECTS_CHANGED)
            });
        }
        if let Some(previous) = previous {
            facts.added_effects =
                effects::names(&confirmation.difference(previous).copied().collect(), false);
        }
        let summary = approval::effects_summary(&facts, context.project.real_path());
        let mut output = ToolOutput::needs_approval(ApprovalRequest {
            kind: ApprovalKind::Effects,
            facts,
            summary,
            justification: None,
        });
        output.processor_launches = 1;
        return Ok(output);
    }
    let result = execute_report(context, arguments, real_path, report, timeout, &invocation);
    // 失敗した起動も問い合わせの一回は数える。起動に到達した分は実行側で加算する。
    Ok(match result {
        Ok(mut output) => {
            output.processor_launches = output.processor_launches.saturating_add(1);
            output
        }
        Err(error) => ToolOutput {
            processor_launches: 1,
            ..ToolOutput::error(error.to_string())
        },
    })
}

fn execute_report(
    context: &ToolContext,
    arguments: &Value,
    path: &Path,
    report: InspectionReport,
    timeout: u64,
    invocation: &Invocation<'_>,
) -> Result<ToolOutput, ToolError> {
    let (Some(run), Some(session)) = (&context.run, &context.session) else {
        return Err(ToolError::InvalidArguments(text::RUN_CONTEXT.into()));
    };
    let mut args = vec![
        "run".into(),
        "--diagnostics=json".into(),
        format!(
            "--allow-effects={}",
            effects::names(&report.effects, true).join(",")
        )
        .into(),
        path.as_os_str().to_owned(),
    ];
    if let Some(values) = arguments.get("args").and_then(Value::as_array) {
        args.extend(values.iter().filter_map(Value::as_str).map(OsString::from));
    }
    let input = arguments.get("stdin").and_then(Value::as_str);
    let command = prepare(run, &report.effects, args, input.is_some()).map_err(|e| failure(&e))?;
    let stdout = Arc::new(Mutex::new(Output::new(
        session
            .spill_writer(&format!("{}-stdout", invocation.call_id))
            .map_err(|e| failure(&e))?,
        25 * 1024,
        1000,
    )));
    let stderr = Arc::new(Mutex::new(Stderr::new(
        Output::new(
            session
                .spill_writer(&format!("{}-stderr", invocation.call_id))
                .map_err(|e| failure(&e))?,
            25 * 1024,
            1000,
        ),
        vec![path.to_owned()],
    )));
    context.checkpoint_before(
        crate::rewind::CheckpointReason::BenitoiteRun,
        None,
        invocation,
    )?;
    let outcome = run::run(
        command,
        input.unwrap_or_default().as_bytes().to_vec(),
        Arc::clone(&stdout),
        Arc::clone(&stderr),
        RunLimits {
            timeout: Duration::from_secs(timeout),
            stop_signal: StopSignal::Interrupt,
            ..RunLimits::default()
        },
        invocation.cancelled,
    );
    // 出力の後始末の失敗で、起動の集計や残存プロセスでの停止を取り消さない。
    // （設計書 03-02「子プロセスの後始末」、07-02「測る項目」）。
    let possibly_remaining = outcome
        .as_ref()
        .is_ok_and(|outcome| outcome.leftovers == Leftovers::PossiblyRemaining);
    let result: Result<ToolOutput, ToolError> = (|| {
        let mut stdout = stdout.lock().unwrap_or_else(PoisonError::into_inner);
        let mut stderr = stderr.lock().unwrap_or_else(PoisonError::into_inner);
        stderr.end_line()?;
        stderr.closed = true;
        let (stdout_text, spill) = finish(&mut stdout, session)?;
        let (stderr_text, stderr_spill) = finish(&mut stderr.other, session)?;
        let outcome = match outcome {
            Ok(outcome) => outcome,
            Err(error) => {
                return Ok(ToolOutput {
                    processor_launches: 1,
                    spill,
                    stderr_spill,
                    ..ToolOutput::error(text::run_failure(&error))
                });
            }
        };
        let code = outcome
            .status
            .code()
            .unwrap_or_else(|| 128_i32.saturating_add(outcome.status.signal().unwrap_or(0)));
        let changed = code == 2 && stderr.effects_changed;
        let mut body = String::new();
        if !stdout_text.is_empty() {
            append(&mut body, text::RUN_STDOUT);
            append(&mut body, &stdout_text);
        }
        if !stderr_text.is_empty() {
            append(&mut body, text::RUN_STDERR);
            append(&mut body, &stderr_text);
        }
        append(
            &mut body,
            &render(
                std::mem::take(&mut stderr.diagnostics),
                context,
                invocation.cancelled,
            ),
        );
        if stderr.omitted > 0 {
            append(
                &mut body,
                &text::run_diagnostics_omitted(stderr.omitted, stderr.omitted_error),
            );
        }
        if changed {
            append(&mut body, benitoite::text::EFFECTS_CHANGED);
        }
        append(&mut body, &text::bash_exit(code));
        match outcome.end {
            RunEnd::TimedOut => append(&mut body, &text::run_timed_out(timeout)),
            RunEnd::Cancelled => append(&mut body, text::RUN_CANCELLED),
            RunEnd::Exited => {}
        }
        let stop_run = if outcome.leftovers == Leftovers::PossiblyRemaining {
            append(&mut body, text::RUN_LEFTOVERS);
            Some(crate::agent::text::LEFTOVER_PROCESSES.into())
        } else {
            None
        };
        Ok(ToolOutput {
            screen: Some(text::screen_exit(code)),
            is_error: code != 0 || outcome.end != RunEnd::Exited || stop_run.is_some(),
            spill,
            stderr_spill,
            stop_run,
            processor_launches: 1,
            ..ToolOutput::success(body)
        })
    })();
    Ok(result.unwrap_or_else(|error| {
        let mut output = ToolOutput {
            processor_launches: 1,
            ..ToolOutput::error(error.to_string())
        };
        if possibly_remaining {
            append(&mut output.text, text::RUN_LEFTOVERS);
            output.stop_run = Some(crate::agent::text::LEFTOVER_PROCESSES.into());
        }
        output
    }))
}
