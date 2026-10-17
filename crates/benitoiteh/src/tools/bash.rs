//! 通常モードのシェルを確認後に起動し、出力の末尾と保存を管理する（設計書 01-03「bash（通常モード）」）。

use std::ffi::OsString;
use std::io::{self, Write};
use std::os::unix::process::ExitStatusExt;
use std::path::Path;
use std::process::Command;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use serde_json::Value;

use super::{
    ApprovalRequest, ApprovalStatus, Invocation, ShellContext, ToolContext, ToolError, ToolOutput,
    text,
};
use crate::approval::{ApprovalFacts, ApprovalKind};
use crate::sandbox::run::{self, Leftovers, RunCommand, RunEnd, RunLimits};
use crate::sandbox::{SandboxCommand, SandboxStdin};
use crate::session::SpillWriter;

const MAX_BYTES: usize = 50 * 1024;
const MAX_LINES: usize = 2000;

fn failure(error: &dyn std::fmt::Display) -> ToolError {
    ToolError::InvalidArguments(text::bash_failure(error))
}

pub(super) fn execute(
    context: &ToolContext,
    arguments: &Value,
    approval: ApprovalStatus,
    invocation: Invocation<'_>,
) -> Result<ToolOutput, ToolError> {
    let command = arguments
        .get("command")
        .and_then(Value::as_str)
        .ok_or_else(|| ToolError::InvalidArguments(text::missing_argument("command")))?;
    let timeout = match arguments.get("timeout") {
        None => 120,
        Some(value) => value
            .as_u64()
            .filter(|n| (1..=600).contains(n))
            .ok_or_else(|| ToolError::InvalidArguments(text::BASH_TIMEOUT.into()))?,
    };
    let escalate = arguments
        .get("escalate")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    if approval == ApprovalStatus::Unapproved {
        return Ok(ToolOutput::needs_approval(ApprovalRequest {
            kind: if escalate {
                ApprovalKind::Escalate
            } else {
                ApprovalKind::Command
            },
            facts: ApprovalFacts {
                command: Some(command.to_owned()),
                cwd: Some(context.project.real_path().to_owned()),
                sandbox_policy: Some(
                    context
                        .shell
                        .as_ref()
                        .map_or(crate::config::Sandbox::WorkspaceWrite, |shell| {
                            shell.sandbox
                        })
                        .name()
                        .to_owned(),
                ),
                ..ApprovalFacts::default()
            },
            justification: arguments
                .get("justification")
                .and_then(Value::as_str)
                .map(str::to_owned),
            summary: text::bash_approval(command, escalate),
        }));
    }
    let (Some(shell), Some(session)) = (&context.shell, &context.session) else {
        return Err(ToolError::InvalidArguments(text::BASH_CONTEXT.into()));
    };
    let sandboxed = shell.policy.is_some() && !escalate;
    let launch = prepare(shell, context.project.real_path(), command, sandboxed)?;
    let sink = Arc::new(Mutex::new(Output::new(
        session
            .spill_writer(invocation.call_id)
            .map_err(|e| failure(&e))?,
    )));
    context.checkpoint_before(crate::rewind::CheckpointReason::Bash, None, &invocation)?;
    let outcome = run::run(
        launch,
        Vec::new(),
        Arc::clone(&sink),
        Arc::clone(&sink),
        RunLimits {
            timeout: Duration::from_secs(timeout),
            ..RunLimits::default()
        },
        invocation.cancelled,
    );
    // パイプの読み手が残っていても、この錠の中で書き手を抜いた後は出力を捨てる。
    // （設計書 03-02「子プロセスの後始末」）。
    let mut output = sink.lock().unwrap_or_else(PoisonError::into_inner);
    output.closed = true;
    let writer = output.writer.take();
    let outcome = match outcome {
        Ok(outcome) => outcome,
        Err(error) => {
            if let Some(writer) = writer {
                let _removed = writer.discard();
            }
            return Err(failure(&error));
        }
    };
    let (mut contents, first, total, truncated) = output.tail();
    let spill = match writer {
        Some(writer) if truncated => writer.finish().ok(),
        Some(writer) => {
            writer.discard().map_err(|e| failure(&e))?;
            None
        }
        None => None,
    };
    if truncated {
        append(
            &mut contents,
            &text::bash_truncated(
                first,
                total,
                total,
                spill.as_ref().map(|info| (info, session.directory())),
            ),
        );
    }
    let signal = outcome.status.signal();
    let code = outcome
        .status
        .code()
        .unwrap_or_else(|| 128_i32.saturating_add(signal.unwrap_or(0)));
    let blocked = sandbox_denied(
        code,
        signal,
        &String::from_utf8_lossy(&output.bytes),
        sandboxed,
        cfg!(target_os = "linux"),
    );
    append(&mut contents, &text::bash_exit(code));
    match outcome.end {
        RunEnd::TimedOut => append(&mut contents, &text::bash_timed_out(timeout)),
        RunEnd::Cancelled => append(&mut contents, text::BASH_CANCELLED),
        RunEnd::Exited => {}
    }
    if outcome.leftovers == Leftovers::PossiblyRemaining {
        append(&mut contents, text::BASH_LEFTOVERS);
    }
    if blocked {
        append(&mut contents, text::BASH_SANDBOX);
    }
    Ok(ToolOutput {
        spill,
        is_error: code != 0,
        ..ToolOutput::success(contents)
    })
}

fn append(output: &mut String, line: &str) {
    if !output.is_empty() && !output.ends_with('\n') {
        output.push('\n');
    }
    output.push_str(line);
}

fn shell_arguments(program: &Path, command: &str) -> Vec<OsString> {
    let mut args = Vec::new();
    if !program
        .file_name()
        .is_some_and(|name| ["bash", "sh", "dash"].iter().any(|known| name == *known))
    {
        args.push("sh".into());
    }
    args.extend([OsString::from("-c"), OsString::from(command)]);
    args
}

fn prepare(
    shell: &ShellContext,
    cwd: &Path,
    command: &str,
    sandboxed: bool,
) -> Result<RunCommand, ToolError> {
    let path = if Path::new("/bin/bash").is_file() {
        Path::new("/bin/bash")
    } else {
        Path::new("/bin/sh")
    };
    if sandboxed {
        let program = path.canonicalize()?;
        let command = SandboxCommand {
            policy: shell
                .policy
                .clone()
                .ok_or_else(|| ToolError::InvalidArguments(text::BASH_CONTEXT.into()))?,
            args: shell_arguments(&program, command),
            program,
            cwd: cwd.to_owned(),
            env: shell.env.clone(),
            stdin: SandboxStdin::Null,
        };
        #[cfg(target_os = "linux")]
        {
            command
                .prepare(&shell.helper, &shell.scratch, &shell.placeholders)
                .map(Into::into)
                .map_err(|e| failure(&e))
        }
        #[cfg(not(target_os = "linux"))]
        {
            command
                .to_command(&shell.helper)
                .map(Into::into)
                .map_err(|e| failure(&e))
        }
    } else {
        let mut env = shell.env.clone();
        env.remove(std::ffi::OsStr::new("BENITOITEH_SANDBOX"));
        env.remove(std::ffi::OsStr::new("BENITOITEH_SANDBOX_NETWORK_DISABLED"));
        let mut launch = Command::new(path);
        launch
            .arg("-c")
            .arg(command)
            .current_dir(cwd)
            .env_clear()
            .envs(env);
        Ok(launch.into())
    }
}

fn sandbox_denied(
    code: i32,
    signal: Option<i32>,
    tail: &str,
    sandboxed: bool,
    linux: bool,
) -> bool {
    if !sandboxed || [0, 2, 126, 127].contains(&code) {
        return false;
    }
    let tail = tail.to_ascii_lowercase();
    (linux && (code == 159 || signal == Some(31)))
        || [
            "operation not permitted",
            "permission denied",
            "read-only file system",
            "sandbox",
        ]
        .iter()
        .any(|needle| tail.contains(needle))
}

struct Output {
    writer: Option<SpillWriter>,
    bytes: Vec<u8>,
    total_bytes: usize,
    partial_first: bool,
    newlines: usize,
    ends_newline: bool,
    closed: bool,
}
impl Output {
    fn new(writer: SpillWriter) -> Self {
        Self {
            writer: Some(writer),
            bytes: Vec::new(),
            total_bytes: 0,
            partial_first: false,
            newlines: 0,
            ends_newline: false,
            closed: false,
        }
    }
    fn tail(&self) -> (String, usize, usize, bool) {
        let total = self
            .newlines
            .saturating_add(usize::from(self.total_bytes != 0 && !self.ends_newline));
        let mut bytes = self.bytes.as_slice();
        let mut truncated = self.total_bytes > bytes.len();
        let mut cut_in_line = self.partial_first;
        // バイト上限で最初の行が欠けたときは、最後の一行だけの場合を除き返さない。
        // （設計書 01-03「出力の切り詰め」）。
        if self.partial_first
            && let Some(index) = bytes.iter().position(|b| *b == b'\n')
            && index.saturating_add(1) < bytes.len()
        {
            bytes = bytes.get(index.saturating_add(1)..).unwrap_or_default();
            cut_in_line = false;
        }
        let mut lines = bytes
            .iter()
            .filter(|b| **b == b'\n')
            .count()
            .saturating_add(usize::from(!bytes.is_empty() && !bytes.ends_with(b"\n")));
        while lines > MAX_LINES {
            let Some(index) = bytes.iter().position(|b| *b == b'\n') else {
                break;
            };
            bytes = bytes.get(index.saturating_add(1)..).unwrap_or_default();
            lines = lines.saturating_sub(1);
            truncated = true;
            cut_in_line = false;
        }
        if cut_in_line {
            while bytes.first().is_some_and(|b| b & 0xc0 == 0x80) {
                bytes = bytes.get(1..).unwrap_or_default();
            }
        }
        let mut contents = String::from_utf8_lossy(bytes).into_owned();
        if contents.len() > MAX_BYTES {
            let mut start = contents.len().saturating_sub(MAX_BYTES);
            while !contents.is_char_boundary(start) {
                start = start.saturating_add(1);
            }
            if let Some(index) = contents.get(start..).and_then(|s| s.find('\n')) {
                let next = start.saturating_add(index).saturating_add(1);
                if next < contents.len() {
                    start = next;
                }
            }
            contents = contents.get(start..).unwrap_or_default().to_owned();
            lines = contents.lines().count();
            truncated = true;
        }
        (
            contents,
            total.saturating_sub(lines).saturating_add(1),
            total,
            truncated,
        )
    }
}
impl Write for Output {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if self.closed {
            return Ok(bytes.len());
        }
        if let Some(writer) = &mut self.writer
            && writer.write_all(bytes).is_err()
            && let Some(writer) = self.writer.take()
        {
            let _removed = writer.discard();
        }
        self.total_bytes = self.total_bytes.saturating_add(bytes.len());
        self.newlines = self
            .newlines
            .saturating_add(bytes.iter().filter(|b| **b == b'\n').count());
        if let Some(last) = bytes.last() {
            self.ends_newline = *last == b'\n';
        }
        self.bytes.extend_from_slice(bytes);
        let excess = self.bytes.len().saturating_sub(MAX_BYTES);
        if excess != 0 {
            self.partial_first = self.bytes.get(excess.saturating_sub(1)) != Some(&b'\n');
            self.bytes.drain(..excess);
        }
        let mut lines = self
            .bytes
            .iter()
            .filter(|b| **b == b'\n')
            .count()
            .saturating_add(usize::from(!self.ends_newline));
        let mut start = 0;
        while lines > MAX_LINES {
            let Some(index) = self
                .bytes
                .get(start..)
                .and_then(|rest| rest.iter().position(|b| *b == b'\n'))
            else {
                break;
            };
            start = start.saturating_add(index).saturating_add(1);
            lines = lines.saturating_sub(1);
        }
        if start != 0 {
            self.bytes.drain(..start);
            self.partial_first = false;
        }
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn shell_launch_supports_multicall_executables() {
        for name in ["bash", "sh", "dash", "busybox"] {
            let args = shell_arguments(&Path::new("/bin").join(name), "echo ok");
            let expected: Vec<OsString> = if name == "busybox" {
                vec!["sh".into(), "-c".into(), "echo ok".into()]
            } else {
                vec!["-c".into(), "echo ok".into()]
            };
            assert_eq!(args, expected);
        }
    }
    #[test]
    fn denial_detection_excludes_shell_errors_and_unsandboxed_commands() {
        for linux in [false, true] {
            for code in [0, 1, 2, 125, 126, 127, 159] {
                for sandboxed in [false, true] {
                    assert_eq!(
                        sandbox_denied(code, None, "PERMISSION DENIED", sandboxed, linux),
                        sandboxed && ![0, 2, 126, 127].contains(&code)
                    );
                    assert_eq!(
                        sandbox_denied(code, None, "", sandboxed, linux),
                        sandboxed && linux && code == 159
                    );
                }
            }
        }
        assert!(sandbox_denied(128, Some(31), "", true, true));
    }
}
