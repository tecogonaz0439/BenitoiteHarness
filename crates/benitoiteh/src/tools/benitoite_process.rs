//! 処理系の起動と、診断を spill に残さない出力処理（設計書 04-01）。
use super::{Invocation, ToolContext, ToolError, ToolOutput, process_output::Output, text};
use crate::benitoite::report::InspectionReport;
use crate::benitoite::{
    self, Effect, RunContext,
    diagnostics::{Diagnostic, PreparedDiagnostics, push_line},
    effects,
};
use crate::sandbox::run::{self, Leftovers, RunEnd, RunLimits, StopSignal};
use crate::sandbox::{self, SandboxCommand, SandboxStdin, run::RunCommand};
use crate::session::{Session, SpillInfo};
use serde_json::Value;
use std::os::unix::process::ExitStatusExt;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

pub(super) struct ScriptRun {
    pub(super) timeout: u64,
    pub(super) reason: crate::rewind::CheckpointReason,
    pub(super) automatic: bool,
}

fn failure(error: &dyn std::fmt::Display) -> ToolError {
    ToolError::InvalidArguments(text::run_failure(error))
}

use std::{
    collections::BTreeSet,
    ffi::OsString,
    io::{self, Write},
    path::Path,
};
pub(super) const DIAGNOSTICS_BYTES: usize = 16 * 1024 * 1024;

pub(super) fn restricted_facts(
    context: &ToolContext,
    current: &BTreeSet<Effect>,
    previous: Option<&BTreeSet<Effect>>,
) -> Option<crate::approval::RestrictedFacts> {
    let run = context.run.as_ref()?;
    // 再検査は前の確認を含む集合で実行しうるため、今回の報告だけでは範囲が足りない
    // （設計書 04-01「実行の前の検査の報告」、実装プラン P2-10）。
    let combined: BTreeSet<_> = current
        .iter()
        .chain(previous.into_iter().flatten())
        .copied()
        .collect();
    let access = effects::restricted_access(&combined, &run.processor, &run.allowed_programs);
    let mut writable = Vec::new();
    if access.write_project {
        writable.push(run.locations.project.clone());
    }
    writable.push(run.locations.temporary.clone());
    Some(crate::approval::RestrictedFacts {
        writable,
        network: access.allow_ip,
        process_run: combined.contains(&Effect::ProcessRun),
        programs: access.programs,
    })
}

pub(super) fn prepare(
    run: &RunContext,
    effects: &BTreeSet<Effect>,
    args: Vec<OsString>,
    piped: bool,
    automatic: bool,
) -> Result<RunCommand, crate::sandbox::SandboxError> {
    let access = effects::restricted_access(effects, &run.processor, &run.allowed_programs);
    let policy = sandbox::restricted_policy(&access, &run.locations, &run.denied_candidates)?;
    let policy = if automatic {
        sandbox::scratch_policy(&policy, &run.locations, &run.processor)?
    } else {
        policy
    };
    let command = SandboxCommand {
        policy,
        program: run.processor.clone(),
        args,
        cwd: run.locations.project.clone(),
        env: crate::config::child_environment(
            run.env.clone(),
            &BTreeSet::new(),
            &run.locations.temporary,
            Some(("restricted", access.allow_ip)),
        ),
        stdin: if piped {
            SandboxStdin::Piped
        } else {
            SandboxStdin::Null
        },
    };
    #[cfg(target_os = "linux")]
    {
        command
            .prepare(&run.helper, &run.scratch, &run.placeholders)
            .map(Into::into)
    }
    #[cfg(not(target_os = "linux"))]
    {
        command.to_command(&run.helper).map(Into::into)
    }
}

pub(super) fn render(
    diagnostics: Vec<Diagnostic>,
    context: &ToolContext,
    cancelled: &std::sync::atomic::AtomicBool,
) -> String {
    let rendered =
        PreparedDiagnostics::new(diagnostics, context, cancelled).render(context, None, cancelled);
    let mut body = rendered.text;
    if rendered.unread > 0 {
        push_line(
            &mut body,
            &benitoite::text::unread_diagnostics(rendered.unread),
        );
    }
    body
}

pub(super) fn finish(
    output: &mut Output,
    session: &Session,
) -> Result<(String, Option<SpillInfo>), ToolError> {
    output.closed = true;
    let (mut body, first, total, truncated) = output.tail();
    let spill = match output.writer.take() {
        Some(writer) if truncated => writer.finish().ok(),
        Some(writer) => {
            writer
                .discard()
                .map_err(|e| ToolError::InvalidArguments(e.to_string()))?;
            None
        }
        None => None,
    };
    if truncated {
        append(
            &mut body,
            &text::bash_truncated(
                first,
                total,
                total,
                spill.as_ref().map(|s| (s, session.directory())),
            ),
        );
    }
    Ok((body, spill))
}

pub(super) fn append(body: &mut String, line: &str) {
    if line.is_empty() {
        return;
    }
    if !body.is_empty() && !body.ends_with('\n') {
        body.push('\n');
    }
    body.push_str(line);
}

// 診断を末尾の保持と分ける。診断を大量に書かれても D1 の可否判定を省略しない。
pub(super) struct Stderr {
    pub(super) other: Output,
    line: Vec<u8>,
    pub(super) diagnostics: Vec<Diagnostic>,
    diagnostic_bytes: usize,
    pub(super) effects_changed: bool,
    targets: Vec<(std::path::PathBuf, bool)>,
    pub(super) omitted: usize,
    // 省略した診断も集計を失わない。道具の終了判定には処理系の終了値を使う。
    pub(super) omitted_error: bool,
    oversized: bool,
    forwarding: bool,
    pub(super) closed: bool,
}
impl Stderr {
    pub(super) fn new(other: Output, targets: Vec<std::path::PathBuf>) -> Self {
        Self {
            other,
            line: Vec::new(),
            diagnostics: Vec::new(),
            diagnostic_bytes: 0,
            effects_changed: false,
            targets: targets
                .into_iter()
                .map(|p| {
                    let directory = p.is_dir();
                    (p, directory)
                })
                .collect(),
            omitted: 0,
            omitted_error: false,
            oversized: false,
            forwarding: false,
            closed: false,
        }
    }
    pub(super) fn end_line(&mut self) -> io::Result<()> {
        if self.oversized {
            self.omitted = self.omitted.saturating_add(1);
        } else if !self.forwarding && !self.line.is_empty() {
            match benitoite::diagnostics::parse_line(&self.line) {
                Some(d) => {
                    self.effects_changed |= d.code.as_deref() == Some("E0512")
                        && d.primary.as_ref().is_some_and(|span| {
                            self.targets.iter().any(|(path, directory)| {
                                if *directory {
                                    Path::new(&span.file).starts_with(path)
                                } else {
                                    Path::new(&span.file) == path
                                }
                            })
                        });
                    self.diagnostic_bytes = self.diagnostic_bytes.saturating_add(self.line.len());
                    if self.diagnostic_bytes <= DIAGNOSTICS_BYTES
                        && self.diagnostics.len() < benitoite::diagnostics::MAX_RESOLVED_DIAGNOSTICS
                    {
                        self.diagnostics.push(d);
                    } else {
                        self.omitted = self.omitted.saturating_add(1);
                        self.omitted_error |= d.severity == "error";
                    }
                }
                None => self.other.write_all(&self.line)?,
            }
        }
        self.line.clear();
        self.oversized = false;
        self.forwarding = false;
        Ok(())
    }
}
impl Write for Stderr {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if self.closed {
            return Ok(bytes.len());
        }
        for part in bytes.split_inclusive(|b| *b == b'\n') {
            if self.forwarding {
                self.other.write_all(part)?;
            } else if !self.oversized {
                if self.line.len().saturating_add(part.len()) <= DIAGNOSTICS_BYTES {
                    self.line.extend_from_slice(part);
                } else if self.line.iter().copied().find(|b| !b.is_ascii_whitespace()) == Some(b'{')
                {
                    self.line.clear();
                    self.oversized = true;
                } else {
                    self.other.write_all(&self.line)?;
                    self.line.clear();
                    self.other.write_all(part)?;
                    self.forwarding = true;
                }
            }
            if part.ends_with(b"\n") {
                self.end_line()?;
            }
        }
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

pub(super) fn execute_report(
    context: &ToolContext,
    arguments: &Value,
    path: &Path,
    report: InspectionReport,
    options: ScriptRun,
    invocation: &Invocation<'_>,
) -> Result<ToolOutput, ToolError> {
    let timeout = options.timeout;
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
    let command = prepare(
        run,
        &report.effects,
        args,
        input.is_some(),
        options.automatic,
    )
    .map_err(|e| failure(&e))?;
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
    context.checkpoint_before(options.reason, None, invocation)?;
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
