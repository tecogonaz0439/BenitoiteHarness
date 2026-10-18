//! 処理系の起動と、診断を spill に残さない出力処理（設計書 04-01）。
use super::{ToolContext, ToolError, process_output::Output, text};
use crate::benitoite::{
    self, Effect, RunContext,
    diagnostics::{Diagnostic, PreparedDiagnostics, push_line},
    effects,
};
use crate::sandbox::{self, SandboxCommand, SandboxStdin, run::RunCommand};
use crate::session::{Session, SpillInfo};
use std::{
    collections::BTreeSet,
    ffi::OsString,
    io::{self, Write},
    path::Path,
};
pub(super) const DIAGNOSTICS_BYTES: usize = 16 * 1024 * 1024;
pub(super) fn prepare(
    run: &RunContext,
    effects: &BTreeSet<Effect>,
    args: Vec<OsString>,
    piped: bool,
) -> Result<RunCommand, crate::sandbox::SandboxError> {
    let access = effects::restricted_access(effects, &run.processor, &run.allowed_programs);
    let policy = sandbox::restricted_policy(&access, &run.locations, &run.denied_candidates)?;
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
