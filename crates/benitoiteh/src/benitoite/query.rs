//! 問い合わせの出力を制限し、取り消しを子のグループへ伝える（設計書 04-01「処理系の起動」）。

use super::{BenitoiteError, Processor};
use crate::config::writable::WritableLocations;
use crate::sandbox::run::{self, Leftovers, RunCommand, RunEnd, RunLimits, StopSignal};
use crate::sandbox::{self, ReadAccess, RestrictedAccess, SandboxCommand, SandboxStdin};
use std::collections::BTreeMap;
use std::ffi::OsString;
use std::io::{self, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum QueryLauncher {
    Sandboxed,
    Unsandboxed,
    #[cfg(test)]
    TestDirect,
}

impl QueryLauncher {
    pub(crate) fn environment_policy(self) -> Option<(&'static str, bool)> {
        match self {
            Self::Unsandboxed => None,
            Self::Sandboxed => Some(("restricted", false)),
            #[cfg(test)]
            Self::TestDirect => Some(("restricted", false)),
        }
    }
}

#[derive(Debug)]
pub(crate) struct QueryContext {
    pub helper: PathBuf,
    pub scratch: PathBuf,
    pub placeholders: PathBuf,
    pub env: BTreeMap<OsString, OsString>,
    pub locations: WritableLocations,
    pub denied_candidates: Vec<PathBuf>,
    pub launcher: QueryLauncher,
}

/// 標準エラーの先頭だけを保つか、超過した問い合わせを止めるか。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StderrLimit {
    KeepFirst(usize),
    FailOver(usize),
}

/// 問い合わせの時間の上限と、停止後の猶予。
#[derive(Clone, Copy, Debug)]
pub struct QueryLimits {
    pub timeout: Duration,
    pub grace: Duration,
    pub stderr: StderrLimit,
}
impl Default for QueryLimits {
    fn default() -> Self {
        Self {
            timeout: Duration::from_secs(10),
            grace: Duration::from_secs(10),
            stderr: StderrLimit::KeepFirst(64 * 1024),
        }
    }
}

/// 問い合わせの全標準出力と、指定した上限までの標準エラー。
#[derive(Debug)]
pub struct QueryOutput {
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

impl Processor {
    /// 利用者のスクリプトを実行しない問い合わせを、固定した形で起動する。
    pub fn query(
        &self,
        args: &[OsString],
        limits: QueryLimits,
        cancelled: &AtomicBool,
    ) -> Result<QueryOutput, BenitoiteError> {
        let context = &self.context;
        // --no-sandbox とテストも方針の検証を通し、不正な場所を起動へ渡さない。
        let policy = sandbox::restricted_policy(
            &RestrictedAccess {
                write_project: false,
                allow_ip: false,
                programs: vec![self.path.clone()],
                readable: ReadAccess::All,
            },
            &context.locations,
            &context.denied_candidates,
        )?;
        let command: RunCommand = match context.launcher {
            QueryLauncher::Sandboxed => {
                let command = SandboxCommand {
                    policy,
                    program: self.path.clone(),
                    args: args.into(),
                    cwd: context.locations.project.clone(),
                    env: context.env.clone(),
                    stdin: SandboxStdin::Null,
                };
                #[cfg(target_os = "linux")]
                {
                    command
                        .prepare(&context.helper, &context.scratch, &context.placeholders)?
                        .into()
                }
                #[cfg(not(target_os = "linux"))]
                {
                    let _ = (&context.scratch, &context.placeholders);
                    command.to_command(&context.helper)?.into()
                }
            }
            QueryLauncher::Unsandboxed => {
                let mut command = sandbox::session::command(&context.helper, &self.path);
                command
                    .args(args)
                    .current_dir(&context.locations.project)
                    .env_clear()
                    .envs(&context.env);
                command.into()
            }
            #[cfg(test)]
            QueryLauncher::TestDirect => {
                use std::os::unix::process::CommandExt;
                let mut command = std::process::Command::new(&self.path);
                command
                    .args(args)
                    .current_dir(&context.locations.project)
                    .env_clear()
                    .envs(&context.env)
                    .process_group(0);
                command.into()
            }
        };
        let stop = Arc::new(AtomicBool::new(cancelled.load(Ordering::Acquire)));
        let exceeded = Arc::new(AtomicBool::new(false));
        let stdout = Arc::new(Mutex::new(Output {
            bytes: Vec::new(),
            cap: 16 * 1024 * 1024,
            overflow: Some((Arc::clone(&exceeded), Arc::clone(&stop))),
        }));
        let diagnostics_exceeded = Arc::new(AtomicBool::new(false));
        let (cap, overflow) = match limits.stderr {
            StderrLimit::KeepFirst(cap) => (cap, None),
            StderrLimit::FailOver(cap) => (
                cap,
                Some((Arc::clone(&diagnostics_exceeded), Arc::clone(&stop))),
            ),
        };
        let stderr = Arc::new(Mutex::new(Output {
            bytes: Vec::new(),
            cap,
            overflow,
        }));
        let done = AtomicBool::new(false);
        let outcome = std::thread::scope(|scope| {
            let watcher = std::thread::Builder::new().spawn_scoped(scope, || {
                while !done.load(Ordering::Acquire) {
                    if cancelled.load(Ordering::Acquire) {
                        stop.store(true, Ordering::Release);
                        break;
                    }
                    std::thread::park_timeout(Duration::from_millis(10));
                }
            })?;
            let outcome = run::run(
                command,
                Vec::new(),
                Arc::clone(&stdout),
                Arc::clone(&stderr),
                RunLimits {
                    timeout: limits.timeout,
                    grace: limits.grace,
                    stop_signal: StopSignal::Interrupt,
                    ..RunLimits::default()
                },
                &stop,
            );
            done.store(true, Ordering::Release);
            watcher.thread().unpark();
            let _joined = watcher.join();
            outcome.map_err(BenitoiteError::from)
        });
        let outcome = outcome?;
        if outcome.leftovers != Leftovers::None {
            return Err(BenitoiteError::Invalid(super::text::LEFTOVERS));
        }
        if exceeded.load(Ordering::Acquire) {
            return Err(BenitoiteError::OutputLimit);
        }
        if diagnostics_exceeded.load(Ordering::Acquire) {
            return Err(BenitoiteError::DiagnosticsLimit);
        }
        match outcome.end {
            RunEnd::TimedOut => return Err(BenitoiteError::TimedOut),
            RunEnd::Cancelled => return Err(BenitoiteError::Cancelled),
            RunEnd::Exited => {}
        }
        let stdout =
            std::mem::take(&mut stdout.lock().unwrap_or_else(PoisonError::into_inner).bytes);
        let stderr =
            std::mem::take(&mut stderr.lock().unwrap_or_else(PoisonError::into_inner).bytes);
        if !outcome.status.success() {
            return Err(BenitoiteError::Exit {
                code: outcome.status.code(),
                stderr,
            });
        }
        Ok(QueryOutput { stdout, stderr })
    }
}

struct Output {
    bytes: Vec<u8>,
    cap: usize,
    overflow: Option<(Arc<AtomicBool>, Arc<AtomicBool>)>,
}
impl Write for Output {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let remaining = self.cap.saturating_sub(self.bytes.len());
        if let Some((exceeded, stop)) = &self.overflow {
            if exceeded.load(Ordering::Acquire) {
                return Ok(bytes.len());
            }
            if bytes.len() > remaining {
                exceeded.store(true, Ordering::Release);
                stop.store(true, Ordering::Release);
            }
        }
        self.bytes
            .extend_from_slice(bytes.get(..bytes.len().min(remaining)).unwrap_or_default());
        // run の読み手は EOF まで排出するが、超過後は保持も解釈もしない。
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
