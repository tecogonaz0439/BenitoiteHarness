//! 通常モードのシェルを確認後に起動し、出力の末尾と保存を管理する（設計書 01-03「bash（通常モード）」）。

use std::ffi::OsString;
use std::os::unix::process::ExitStatusExt;
use std::path::Path;

use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use serde_json::Value;

use super::process_output::Output;
use super::{
    ApprovalRequest, ApprovalStatus, Invocation, ShellContext, ToolContext, ToolError, ToolOutput,
    text,
};
use crate::approval::{ApprovalFacts, ApprovalKind};
use crate::sandbox::run::{self, Leftovers, RunCommand, RunEnd, RunLimits};
use crate::sandbox::{SandboxCommand, SandboxStdin};

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
    let mediated = !sandboxed
        && (shell.sandbox == crate::config::Sandbox::DangerFullAccess
            || (escalate && shell.policy.is_some() && approval == ApprovalStatus::Approved));
    let mut changed_directory = shell
        .askpass
        .as_ref()
        .is_some_and(|broker| !broker.valid_directory());
    let secret_call = if mediated && !changed_directory {
        shell
            .askpass
            .as_ref()
            .map(|broker| {
                broker.begin(crate::askpass::Facts {
                    call_id: invocation.call_id.into(),
                    command: command.into(),
                    cwd: context.project.real_path().into(),
                    mode: if shell.sandbox == crate::config::Sandbox::DangerFullAccess {
                        crate::askpass::text::NORMAL_FULL
                    } else {
                        crate::askpass::text::NORMAL_ESCALATED
                    },
                })
            })
            .transpose()
            .or_else(|error| {
                if matches!(error, crate::askpass::AskpassError::ChangedDirectory) {
                    changed_directory = true;
                    Ok(None)
                } else {
                    Err(error)
                }
            })
            .map_err(|error| failure(&error))?
            .flatten()
    } else {
        None
    };
    let launch = prepare(
        shell,
        context.project.real_path(),
        command,
        sandboxed,
        secret_call.as_ref(),
        &mut changed_directory,
    )?;
    let captured = if let Some(call) = &secret_call {
        Output::mediated(MAX_BYTES, MAX_LINES, call.redactor())
    } else {
        Output::new(
            session
                .spill_writer(invocation.call_id)
                .map_err(|e| failure(&e))?,
            MAX_BYTES,
            MAX_LINES,
        )
    };
    let sink = Arc::new(Mutex::new(captured));
    let stdout = Arc::new(Mutex::new(super::process_output::StreamOutput::new(
        Arc::clone(&sink),
        secret_call.as_ref().map(|call| call.redactor()),
    )));
    let stderr = Arc::new(Mutex::new(super::process_output::StreamOutput::new(
        Arc::clone(&sink),
        secret_call.as_ref().map(|call| call.redactor()),
    )));
    context.checkpoint_before(crate::rewind::CheckpointReason::Bash, None, &invocation)?;
    let outcome = run::run_with_pause(
        launch,
        Vec::new(),
        Arc::clone(&stdout),
        Arc::clone(&stderr),
        RunLimits {
            timeout: Duration::from_secs(timeout),
            ..RunLimits::default()
        },
        invocation.cancelled,
        secret_call.as_ref().map(|call| call as &dyn run::RunPause),
    );
    if let Some(call) = &secret_call {
        call.finish();
    }
    // 流れごとの保留を先に流し、混合列の二段目も切り詰め前に確定する。
    stdout
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .finish()?;
    stderr
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .finish()?;
    // パイプの読み手が残っていても、この錠の中で書き手を抜いた後は出力を捨てる。
    // （設計書 03-02「子プロセスの後始末」）。
    let mut output = sink.lock().unwrap_or_else(PoisonError::into_inner);
    output.finish_redaction()?;
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
    if changed_directory {
        append(&mut contents, crate::askpass::text::CHANGED_DIRECTORY);
    }
    if secret_call.is_some() {
        append(&mut contents, crate::askpass::text::SPILL_DISCARDED);
    }
    if let Some(call) = &secret_call {
        for event in call.events() {
            append(&mut contents, &event);
        }
    }
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
        screen: Some(text::screen_exit(code)),
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
    secret_call: Option<&crate::askpass::CallGuard>,
    changed_directory: &mut bool,
) -> Result<RunCommand, ToolError> {
    let path = if Path::new("/bin/bash").is_file() {
        Path::new("/bin/bash")
    } else {
        Path::new("/bin/sh")
    };
    let mut env = shell.env.clone();
    if *changed_directory
        || shell
            .askpass
            .as_ref()
            .is_some_and(|broker| !broker.valid_directory())
    {
        *changed_directory = true;
        crate::askpass::clear_environment(&mut env);
    } else if let Some(call) = secret_call {
        call.environment(&mut env);
    }
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
            env,
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
        env.remove(std::ffi::OsStr::new("BENITOITEH_SANDBOX"));
        env.remove(std::ffi::OsStr::new("BENITOITEH_SANDBOX_NETWORK_DISABLED"));
        let mut launch = crate::sandbox::session::command(&shell.helper, path);
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
