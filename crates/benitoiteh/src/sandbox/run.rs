//! 時間の上限・取り消しと、回収前の子孫の後始末を扱う（設計書 03-02「子プロセスの後始末」）。

use std::error::Error;
use std::fmt;
use std::io::{self, Read, Write};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError, mpsc};
use std::thread;
use std::time::{Duration, Instant};

use rustix::process::{Pid, Signal, WaitId, WaitIdOptions, kill_process_group, waitid};

use super::SandboxError;

/// 最初に送る停止の合図。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StopSignal {
    /// 処理系の中断。
    Interrupt,
    /// シェルの終了要求。
    Terminate,
}

impl StopSignal {
    pub(crate) fn signal(self) -> Signal {
        match self {
            Self::Interrupt => Signal::INT,
            Self::Terminate => Signal::TERM,
        }
    }
}

/// 実行時間と停止後の待機の上限。
#[derive(Clone, Copy, Debug)]
pub struct RunLimits {
    /// 実行時間の上限。
    pub timeout: Duration,
    /// 猶予の前に送る合図。
    pub stop_signal: StopSignal,
    /// 強制終了までの猶予。既定 10 秒。
    pub grace: Duration,
    /// 出力のパイプが閉じるまでの待機。既定 2 秒。
    pub pipe_close_wait: Duration,
}

impl Default for RunLimits {
    fn default() -> Self {
        Self {
            timeout: Duration::from_secs(120),
            stop_signal: StopSignal::Terminate,
            grace: Duration::from_secs(10),
            pipe_close_wait: Duration::from_secs(2),
        }
    }
}

/// 実行を終えた理由。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RunEnd {
    /// 子が自分で終わった。
    Exited,
    /// 時間の上限で止めた。
    TimedOut,
    /// 取り消しで止めた。
    Cancelled,
}

/// 出力のパイプと補助の応答から分かる子孫の残り。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Leftovers {
    /// 残りを示すものはなかった。グループを抜けてパイプを閉じたものは検出できない。
    None,
    /// パイプが閉じないか、Linux の補助が応答しなかった。
    PossiblyRemaining,
}

/// 起動したプログラムの終了状態と後始末の結果。
#[derive(Debug)]
pub struct RunOutcome {
    /// Command のプログラム、または外側の bubblewrap の終了状態。
    pub status: ExitStatus,
    /// 実行を終えた理由。
    pub end: RunEnd,
    /// 残ったプロセスの可能性。
    pub leftovers: Leftovers,
}

/// 起動、合図、待機、入出力の失敗。
#[derive(Debug)]
pub enum RunError {
    /// OS または受け取り先の誤り。
    Io(io::Error),
    /// Linux の準備または後始末の失敗。
    Sandbox(SandboxError),
}

impl fmt::Display for RunError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(f, "{} {error}", text::FAILED),
            Self::Sandbox(error) => error.fmt(f),
        }
    }
}
impl Error for RunError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::Sandbox(error) => Some(error),
        }
    }
}
impl From<io::Error> for RunError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}
impl From<SandboxError> for RunError {
    fn from(error: SandboxError) -> Self {
        Self::Sandbox(error)
    }
}

/// 実行する起動の形。
pub enum RunCommand {
    /// __session で包んだサンドボックスなし、または macOS の sandbox-exec。
    Command(Command),
    /// 制御ソケットを持つ Linux の bubblewrap。
    #[cfg(target_os = "linux")]
    Sandbox(super::PreparedSandbox),
}
impl From<Command> for RunCommand {
    fn from(command: Command) -> Self {
        Self::Command(command)
    }
}
#[cfg(target_os = "linux")]
impl From<super::PreparedSandbox> for RunCommand {
    fn from(command: super::PreparedSandbox) -> Self {
        Self::Sandbox(command)
    }
}

enum Running {
    Command(Child),
    #[cfg(target_os = "linux")]
    Sandbox(super::SandboxChild),
}
impl Running {
    fn child(&mut self) -> &mut Child {
        match self {
            Self::Command(child) => child,
            #[cfg(target_os = "linux")]
            Self::Sandbox(child) => child.child_mut(),
        }
    }
    fn stop(&mut self, pid: Pid, signal: StopSignal) -> Result<(), RunError> {
        match self {
            Self::Command(_) => session_signal(pid, signal.signal()).map_err(Into::into),
            #[cfg(target_os = "linux")]
            Self::Sandbox(child) => child.stop(signal).map_err(Into::into),
        }
    }
    fn force(&mut self, pid: Pid) -> Result<bool, RunError> {
        match self {
            Self::Command(_) => {
                session_signal(pid, Signal::KILL)?;
                Ok(false)
            }
            #[cfg(target_os = "linux")]
            Self::Sandbox(child) => {
                child.terminate()?;
                Ok(true)
            }
        }
    }
    fn wait(&mut self, reaped: &mut bool) -> Result<ExitStatus, RunError> {
        let status = self.child().wait()?;
        // 後始末が失敗した場合にも、回収済みの番号へ再び合図を送らない。
        *reaped = true;
        #[cfg(target_os = "linux")]
        if let Self::Sandbox(child) = self {
            child.finish_cleanup()?;
        }
        Ok(status)
    }
}

pub(crate) fn group_signal(pid: Pid, signal: Signal) -> io::Result<()> {
    match kill_process_group(pid, signal) {
        Ok(()) | Err(rustix::io::Errno::SRCH) => Ok(()),
        Err(error) => Err(io::Error::new(
            io::Error::from(error).kind(),
            format!("{} {pid}: {error}", text::GROUP_SIGNAL),
        )),
    }
}

fn session_signal(pid: Pid, signal: Signal) -> io::Result<()> {
    // setsid 前の ESRCH に限り子自身へ送る。既存のグループの頭へ
    // 同じ合図を二度送らない（設計書 03-02「端末からの切り離し」）。
    match kill_process_group(pid, signal) {
        Ok(()) => Ok(()),
        Err(rustix::io::Errno::SRCH) => match rustix::process::kill_process(pid, signal) {
            Ok(()) | Err(rustix::io::Errno::SRCH) => Ok(()),
            Err(error) => Err(io::Error::from(error)),
        },
        Err(error) => Err(io::Error::new(
            io::Error::from(error).kind(),
            format!("{} {pid}: {error}", text::GROUP_SIGNAL),
        )),
    }
}

fn cleanup_exited_group(pid: Pid) -> io::Result<()> {
    match kill_process_group(pid, Signal::KILL) {
        // macOS は回収前のゾンビだけのグループにも EPERM を返す。
        // NOWAIT で終了を観測した後の掃除に限り、送れる相手がいないと扱う。
        // 生きている間の停止と強制終了は group_signal の誤りを返す
        // （設計書 03-02「子プロセスの後始末」）。
        #[cfg(target_os = "macos")]
        Err(rustix::io::Errno::PERM) => Ok(()),
        Ok(()) | Err(rustix::io::Errno::SRCH) => Ok(()),
        Err(error) => Err(io::Error::new(
            io::Error::from(error).kind(),
            format!("{} {pid}: {error}", text::GROUP_SIGNAL),
        )),
    }
}

/// 利用者を待つ時間と、停止の直前に無効にする権限。
pub trait RunPause {
    /// 現在待っている時間も含む、待ち時間の合計。
    fn paused(&self) -> Duration;
    /// 停止の合図の前、および正常終了の観測時に呼ぶ。
    fn stop(&self);
}

/// 呼び出したスレッドで起動して回収する。出力は上限なく読み、受け取り先へ渡す。
/// Linux の PreparedSandbox は、入力があるとき Piped、ないとき Null で準備すること。
pub fn run<O, E>(
    command: impl Into<RunCommand>,
    input: Vec<u8>,
    stdout: Arc<Mutex<O>>,
    stderr: Arc<Mutex<E>>,
    limits: RunLimits,
    cancelled: &AtomicBool,
) -> Result<RunOutcome, RunError>
where
    O: Write + Send + 'static,
    E: Write + Send + 'static,
{
    run_with_pause(command, input, stdout, stderr, limits, cancelled, None)
}

/// 時間の上限から対話の待機を除き、子を止める前に呼び出しの権限を失効させる。
pub fn run_with_pause<O, E>(
    command: impl Into<RunCommand>,
    input: Vec<u8>,
    stdout: Arc<Mutex<O>>,
    stderr: Arc<Mutex<E>>,
    limits: RunLimits,
    cancelled: &AtomicBool,
    pause: Option<&dyn RunPause>,
) -> Result<RunOutcome, RunError>
where
    O: Write + Send + 'static,
    E: Write + Send + 'static,
{
    let mut child = match command.into() {
        RunCommand::Command(mut command) => {
            command.stdout(Stdio::piped()).stderr(Stdio::piped());
            command.stdin(if input.is_empty() {
                Stdio::null()
            } else {
                Stdio::piped()
            });
            Running::Command(command.spawn()?)
        }
        #[cfg(target_os = "linux")]
        RunCommand::Sandbox(mut command) => {
            command.capture_output();
            Running::Sandbox(command.spawn()?)
        }
    };
    let pid = Pid::from_child(child.child());
    let mut execution = Execution {
        child: &mut child,
        pid,
        reaped: false,
    };
    execution.execute(input, stdout, stderr, limits, cancelled, pause)
}

struct Execution<'a> {
    child: &'a mut Running,
    pid: Pid,
    reaped: bool,
}
impl Drop for Execution<'_> {
    fn drop(&mut self) {
        if !self.reaped {
            // 入出力や待機の失敗でも、回収前の番号だけに送り、子を残さない。
            let _force = self.child.force(self.pid);
            let observed = loop {
                match waitid(
                    WaitId::Pid(self.pid),
                    WaitIdOptions::EXITED | WaitIdOptions::NOWAIT | WaitIdOptions::NOHANG,
                ) {
                    Err(rustix::io::Errno::INTR) => continue,
                    result => break result.is_ok_and(|status| status.is_some()),
                }
            };
            let _kill = if observed {
                cleanup_exited_group(self.pid)
            } else {
                group_signal(self.pid, Signal::KILL)
            };
            let _wait = self.child.wait(&mut self.reaped);
        }
    }
}
impl Execution<'_> {
    fn execute<O: Write + Send + 'static, E: Write + Send + 'static>(
        &mut self,
        input: Vec<u8>,
        stdout: Arc<Mutex<O>>,
        stderr: Arc<Mutex<E>>,
        limits: RunLimits,
        cancelled: &AtomicBool,
        pause: Option<&dyn RunPause>,
    ) -> Result<RunOutcome, RunError> {
        let pid = self.pid;
        let child = &mut self.child;
        let (ended_tx, ended) = mpsc::channel();
        thread::Builder::new().spawn(move || {
            let result = loop {
                match waitid(
                    WaitId::Pid(pid),
                    WaitIdOptions::EXITED | WaitIdOptions::NOWAIT,
                ) {
                    Err(rustix::io::Errno::INTR) => continue,
                    value => break value.map(|_| ()).map_err(io::Error::from),
                }
            };
            let _sent = ended_tx.send(result);
        })?;
        let (output_tx, output_rx) = mpsc::channel();
        read_output(child.child().stdout.take(), stdout, output_tx.clone())?;
        read_output(child.child().stderr.take(), stderr, output_tx)?;
        let input_done = match &mut **child {
            Running::Command(child) => write_input(child.stdin.take(), input)?,
            #[cfg(target_os = "linux")]
            Running::Sandbox(child) => write_input(child.take_stdin(), input)?,
        };
        let start = Instant::now();
        let end = loop {
            match ended.try_recv() {
                Ok(result) => {
                    result?;
                    break RunEnd::Exited;
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    return Err(io::Error::other(text::WAITER).into());
                }
                Err(mpsc::TryRecvError::Empty) => {}
            }
            if cancelled.load(Ordering::Acquire) {
                break RunEnd::Cancelled;
            }
            let elapsed = start
                .elapsed()
                .saturating_sub(pause.map_or(Duration::ZERO, RunPause::paused));
            if elapsed >= limits.timeout {
                break RunEnd::TimedOut;
            }
            match ended.recv_timeout(
                limits
                    .timeout
                    .saturating_sub(elapsed)
                    .min(Duration::from_millis(50)),
            ) {
                Ok(result) => {
                    result?;
                    break RunEnd::Exited;
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    return Err(io::Error::other(text::WAITER).into());
                }
            }
        };
        if let Some(pause) = pause {
            pause.stop();
        }
        let mut leftovers = Leftovers::None;
        if end != RunEnd::Exited {
            child.stop(pid, limits.stop_signal)?;
            if !wait_for_end(&ended, limits.grace)? {
                let controlled = child.force(pid)?;
                let observed = controlled && wait_for_end(&ended, Duration::from_secs(5))?;
                if !observed {
                    if controlled {
                        group_signal(pid, Signal::KILL)?;
                        leftovers = Leftovers::PossiblyRemaining;
                    }
                    ended.recv().map_err(|_| io::Error::other(text::WAITER))??;
                }
            }
        }
        // この時点まで Child::wait/try_wait は使わず、番号の使い回しを防ぐ。
        // 強制終了のときも、別スレッドの NOWAIT の通知を待ってから回収する。
        cleanup_exited_group(pid)?;
        let status = child.wait(&mut self.reaped)?;
        let start = Instant::now();
        let mut output_error = None;
        for _ in 0..2 {
            match output_rx.recv_timeout(limits.pipe_close_wait.saturating_sub(start.elapsed())) {
                Ok(Ok(())) => {}
                Ok(Err(error)) => {
                    output_error.get_or_insert(error);
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    leftovers = Leftovers::PossiblyRemaining;
                    break;
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    output_error.get_or_insert_with(|| io::Error::other(text::READER));
                    break;
                }
            }
        }
        if let Some(error) = output_error {
            return Err(error.into());
        }
        // stdin のスレッドは待たず、すでに判明した EPIPE 以外の失敗だけを返す。
        if let Ok(result) = input_done.try_recv() {
            result?;
        }
        Ok(RunOutcome {
            status,
            end,
            leftovers,
        })
    }
}

fn wait_for_end(ended: &mpsc::Receiver<io::Result<()>>, timeout: Duration) -> io::Result<bool> {
    match ended.recv_timeout(timeout) {
        Ok(result) => result.map(|()| true),
        Err(mpsc::RecvTimeoutError::Timeout) => Ok(false),
        Err(mpsc::RecvTimeoutError::Disconnected) => Err(io::Error::other(text::WAITER)),
    }
}

fn write_input<W: Write + Send + 'static>(
    writer: Option<W>,
    input: Vec<u8>,
) -> io::Result<mpsc::Receiver<io::Result<()>>> {
    let (done, result) = mpsc::channel();
    if let Some(mut writer) = writer {
        thread::Builder::new().spawn(move || {
            // 待たない。逃げた子が読み口を持ち続けても、呼び出し元を止めない。
            let written = match writer.write_all(&input) {
                Err(error) if error.kind() == io::ErrorKind::BrokenPipe => Ok(()),
                result => result,
            };
            drop(writer);
            let _sent = done.send(written);
        })?;
    } else if !input.is_empty() {
        return Err(io::Error::other(text::STDIN));
    } else {
        let _sent = done.send(Ok(()));
    }
    Ok(result)
}

fn read_output<R: Read + Send + 'static, W: Write + Send + 'static>(
    reader: Option<R>,
    sink: Arc<Mutex<W>>,
    done: mpsc::Sender<io::Result<()>>,
) -> io::Result<()> {
    let mut reader = reader.ok_or_else(|| io::Error::other(text::READER))?;
    thread::Builder::new().spawn(move || {
        let mut buffer = [0; 8192];
        let mut failure = None;
        loop {
            match reader.read(&mut buffer) {
                Ok(0) => break,
                Ok(count) => {
                    if failure.is_none() {
                        let mut writer = sink.lock().unwrap_or_else(PoisonError::into_inner);
                        if let Err(error) =
                            writer.write_all(buffer.get(..count).unwrap_or_default())
                        {
                            failure = Some(error);
                        }
                    }
                }
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(error) => {
                    failure.get_or_insert(error);
                    break;
                }
            }
        }
        // 受け取り先の失敗でも EOF まで読み、子の書き込みを詰まらせない。
        let _sent = done.send(match failure {
            Some(error) => Err(error),
            None => Ok(()),
        });
    })?;
    Ok(())
}

mod text {
    //! 実行の失敗に使う英文。
    pub const GROUP_SIGNAL: &str = "Could not signal process group";
    pub const FAILED: &str = "Could not run the child process:";
    pub const WAITER: &str = "The child exit observer disconnected.";
    pub const READER: &str = "The child output reader is unavailable.";
    pub const STDIN: &str = "Prepare the sandbox with piped standard input when supplying input.";
}
