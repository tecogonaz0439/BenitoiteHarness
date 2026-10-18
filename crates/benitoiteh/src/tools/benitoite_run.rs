//! 報告と確認を経て、同じ集合の権限だけで実行する（設計書 01-03、03-02、03-03、04-01）。

use super::{
    ApprovalRequest, ApprovalStatus, ConfirmKey, Invocation, ToolContext, ToolError, ToolOutput,
    benitoite_process::{ScriptRun, execute_report, render},
    read::string,
    text,
};
use crate::approval::{self, ApprovalFacts, ApprovalKind};
use crate::benitoite::{
    self, BenitoiteError, Effect, QueryLimits, StderrLimit, effects,
    report::{self, ReportError, ReportKind},
};
use serde_json::Value;
use std::collections::BTreeSet;
use std::path::Path;
use std::time::Duration;

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
    execute_script(
        context,
        arguments,
        ScriptPath {
            real: real_path,
            display: path,
        },
        approval,
        invocation,
        timeout,
        false,
    )
}

pub(super) struct ScriptPath<'a> {
    pub(super) real: &'a Path,
    pub(super) display: &'a str,
}

// 一時スクリプトも同じ報告の検証と再検査を使い、初回の自動実行だけを分ける。
// （設計書 03-03「一時的なスクリプト」）。
pub(super) fn execute_script(
    context: &ToolContext,
    arguments: &Value,
    source: ScriptPath<'_>,
    approval: ApprovalStatus,
    invocation: Invocation<'_>,
    timeout: u64,
    scratch: bool,
) -> Result<ToolOutput, ToolError> {
    let real_path = source.real;
    let path = source.display;
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
        targets: vec![real_path.to_owned()],
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
    let automatic = scratch
        && approval == ApprovalStatus::Unapproved
        && facts.unread_imports == 0
        && confirmation.is_subset(
            &context
                .run
                .as_ref()
                .map(|run| run.scratch_auto_effects.clone())
                .unwrap_or_else(|| effects::scratch_auto_effects(&[])),
        );
    let retry_automatic =
        if scratch && context.noninteractive && approval == ApprovalStatus::Approved {
            context.take_scratch_automatic(invocation.call_id)?
        } else {
            BTreeSet::new()
        };
    let confirmed = previous.is_some_and(|effects| {
        confirmation
            .iter()
            .all(|effect| effects.contains(effect) || retry_automatic.contains(effect))
    });
    let needs_confirmation = if scratch && approval == ApprovalStatus::Unapproved {
        !automatic
    } else {
        !confirmed || (facts.unread_imports > 0 && approval == ApprovalStatus::Unapproved)
    };
    if needs_confirmation {
        if approval == ApprovalStatus::Approved {
            return Ok(ToolOutput {
                processor_launches: 1,
                ..ToolOutput::error(benitoite::text::EFFECTS_CHANGED)
            });
        }
        if let Some(previous) = previous.filter(|_| !scratch) {
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
    let result = execute_report(
        context,
        arguments,
        real_path,
        report,
        ScriptRun {
            timeout,
            reason: if scratch {
                crate::rewind::CheckpointReason::BenitoiteScratch
            } else {
                crate::rewind::CheckpointReason::BenitoiteRun
            },
            // File.Read を自動の範囲だけで許した非対話の呼び直しも、読み取りを限る。
            // 対話で利用者が確認した範囲とは分ける（設計書 03-03「非対話での扱い」）。
            automatic: automatic || retry_automatic.contains(&Effect::FileRead),
        },
        &invocation,
    );
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
