//! 対話の起動だけが端末の有無と大きさを調べる（設計書 05-01、05-02）。

use std::collections::VecDeque;
use std::io::{self, IsTerminal};
use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::time::{Duration, Instant};

use rustix::event::{PollFd, PollFlags, Timespec, poll};

use crate::config::paths::DataPaths;
use crate::ui::tui::{
    InputSource, RunControl, Screen, TerminalGuard, TerminalRestore, TuiError, WakeNotifier,
    WakeReceiver, Worker, wake_channel,
};

use super::{CliError, args::ExecArgs, exec, panic, signals::Signals, text};

#[derive(Default)]
struct ReaderControl {
    paused: bool,
    polling: bool,
    stop: bool,
}

struct InputShared {
    control: Mutex<ReaderControl>,
    condvar: Condvar,
    queue: Mutex<VecDeque<(crossterm::event::Event, Instant)>>,
    error: Mutex<Option<io::Error>>,
}

struct ReaderPause<'a>(&'a InputShared);

impl Drop for ReaderPause<'_> {
    fn drop(&mut self) {
        let mut ctrl = self
            .0
            .control
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        ctrl.paused = false;
        self.0.condvar.notify_all();
    }
}

fn read_crossterm_batch() -> io::Result<Vec<(crossterm::event::Event, Instant)>> {
    let mut batch = Vec::new();
    let ready = match crossterm::event::poll(Duration::from_millis(25)) {
        Ok(ready) => ready,
        Err(error) if error.kind() == io::ErrorKind::Interrupted => return Ok(batch),
        Err(error) => return Err(error),
    };
    if ready {
        batch.push((crossterm::event::read()?, Instant::now()));
        while crossterm::event::poll(Duration::ZERO)? {
            batch.push((crossterm::event::read()?, Instant::now()));
        }
    }
    Ok(batch)
}

fn keep_after_present(event: &crossterm::event::Event) -> bool {
    matches!(event, crossterm::event::Event::Resize(_, _))
        || matches!(
            event,
            crossterm::event::Event::Key(key)
                if key.kind == crossterm::event::KeyEventKind::Press
                    && key
                        .modifiers
                        .contains(crossterm::event::KeyModifiers::CONTROL)
                    && matches!(key.code, crossterm::event::KeyCode::Char('c' | 'C'))
        )
}

struct TerminalInput<'a> {
    signals: &'a mut Signals,
    worker_wake_rx: WakeReceiver,
    worker_wake_tx: WakeNotifier,
    input_wake_rx: WakeReceiver,
    shared: Arc<InputShared>,
    reader: Option<std::thread::JoinHandle<()>>,
}

impl<'a> TerminalInput<'a> {
    fn new(signals: &'a mut Signals) -> io::Result<Self> {
        let (worker_wake_tx, worker_wake_rx) = wake_channel()?;
        let (input_wake_tx, input_wake_rx) = wake_channel()?;
        let shared = Arc::new(InputShared {
            control: Mutex::new(ReaderControl::default()),
            condvar: Condvar::new(),
            queue: Mutex::new(VecDeque::new()),
            error: Mutex::new(None),
        });
        let reader_shared = Arc::clone(&shared);
        let reader = std::thread::Builder::new()
            .name("tui-input".into())
            .spawn(move || {
                loop {
                    {
                        let mut ctrl = reader_shared
                            .control
                            .lock()
                            .unwrap_or_else(PoisonError::into_inner);
                        while ctrl.paused && !ctrl.stop {
                            ctrl = reader_shared
                                .condvar
                                .wait(ctrl)
                                .unwrap_or_else(PoisonError::into_inner);
                        }
                        if ctrl.stop {
                            break;
                        }
                        ctrl.polling = true;
                    }
                    let result = read_crossterm_batch();
                    {
                        let mut ctrl = reader_shared
                            .control
                            .lock()
                            .unwrap_or_else(PoisonError::into_inner);
                        ctrl.polling = false;
                        reader_shared.condvar.notify_all();
                    }
                    match result {
                        Ok(batch) if !batch.is_empty() => {
                            let mut queue = reader_shared
                                .queue
                                .lock()
                                .unwrap_or_else(PoisonError::into_inner);
                            queue.extend(batch);
                            drop(queue);
                            input_wake_tx.notify();
                        }
                        Ok(_) => {}
                        Err(error) => {
                            *reader_shared
                                .error
                                .lock()
                                .unwrap_or_else(PoisonError::into_inner) = Some(error);
                            input_wake_tx.notify();
                            break;
                        }
                    }
                }
            })?;
        Ok(Self {
            signals,
            worker_wake_rx,
            worker_wake_tx,
            input_wake_rx,
            shared,
            reader: Some(reader),
        })
    }

    fn pause_reader(&self) -> ReaderPause<'_> {
        let mut ctrl = self
            .shared
            .control
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        ctrl.paused = true;
        while ctrl.polling {
            ctrl = self
                .shared
                .condvar
                .wait(ctrl)
                .unwrap_or_else(PoisonError::into_inner);
        }
        ReaderPause(&self.shared)
    }
}

impl Drop for TerminalInput<'_> {
    fn drop(&mut self) {
        {
            let mut ctrl = self
                .shared
                .control
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            ctrl.stop = true;
            ctrl.paused = false;
            self.shared.condvar.notify_all();
        }
        if let Some(reader) = self.reader.take() {
            let _joined = reader.join();
        }
    }
}

impl InputSource for TerminalInput<'_> {
    fn worker_waker(&self) -> Option<WakeNotifier> {
        Some(self.worker_wake_tx.clone())
    }
    fn clear_worker_wake(&mut self) {
        self.worker_wake_rx.drain();
    }
    fn termination_count(&mut self) -> io::Result<usize> {
        self.signals.termination_count()
    }
    /// raw のモードを戻してエディタを動かし、終えたら raw に戻して残った入力を捨てる
    /// （設計書 05-02「入力の操作」）。
    fn suspend(&mut self, task: &mut dyn FnMut() -> io::Result<bool>) -> io::Result<bool> {
        let _pause = self.pause_reader();
        crossterm::terminal::disable_raw_mode()?;
        let result = task();
        crossterm::terminal::enable_raw_mode()?;
        rustix::termios::tcflush(io::stdin(), rustix::termios::QueueSelector::IFlush)?;
        result
    }
    fn poll(&mut self, wait: Duration) -> io::Result<Vec<crossterm::event::Event>> {
        Ok(self
            .poll_timed(wait)?
            .into_iter()
            .map(|(event, _)| event)
            .collect())
    }
    fn poll_timed(
        &mut self,
        wait: Duration,
    ) -> io::Result<Vec<(crossterm::event::Event, Instant)>> {
        let mut wake_worker = false;
        let deadline = Instant::now().checked_add(wait);
        loop {
            self.input_wake_rx.drain();
            {
                let mut queue = self
                    .shared
                    .queue
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner);
                if !queue.is_empty() {
                    return Ok(queue.drain(..).collect());
                }
            }
            if let Some(error) = self
                .shared
                .error
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .take()
            {
                return Err(error);
            }
            if wake_worker {
                return Ok(Vec::new());
            }
            let remaining = deadline.map_or(Duration::ZERO, |end| {
                end.saturating_duration_since(Instant::now())
            });
            if remaining.is_zero() {
                return Ok(Vec::new());
            }
            let timeout = Timespec::try_from(remaining).map_err(|_| io::Error::other("timeout"))?;
            let signal_fd = self.signals.wake_fd();
            let mut fds = Vec::with_capacity(3);
            fds.push(PollFd::new(&self.input_wake_rx, PollFlags::IN));
            fds.push(PollFd::new(&self.worker_wake_rx, PollFlags::IN));
            if let Some(signal_fd) = &signal_fd {
                fds.push(PollFd::new(signal_fd, PollFlags::IN));
            }
            match poll(&mut fds, Some(&timeout)) {
                Ok(_) => {
                    let input_ready = fds.first().is_some_and(|fd| !fd.revents().is_empty());
                    wake_worker = fds
                        .get(1..)
                        .unwrap_or_default()
                        .iter()
                        .any(|fd| !fd.revents().is_empty());
                    if !input_ready {
                        return Ok(Vec::new());
                    }
                }
                Err(rustix::io::Errno::INTR) => return Ok(Vec::new()),
                Err(error) => return Err(error.into()),
            }
        }
    }
    fn discard_pending(&mut self) -> io::Result<Vec<(crossterm::event::Event, Instant)>> {
        // 先に入力スレッドを止めたうえで OS の待ち行列・キュー・内部バッファを順に捨てる。
        let _pause = self.pause_reader();
        let mut kept: Vec<_> = {
            let mut queue = self
                .shared
                .queue
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            queue
                .drain(..)
                .filter(|(event, _)| keep_after_present(event))
                .collect()
        };
        rustix::termios::tcflush(io::stdin(), rustix::termios::QueueSelector::IFlush)?;
        while crossterm::event::poll(Duration::ZERO)? {
            let event = crossterm::event::read()?;
            if keep_after_present(&event) {
                kept.push((event, Instant::now()));
            }
        }
        Ok(kept)
    }
}

pub(super) fn run(args: ExecArgs) -> Result<u8, CliError> {
    // 設定や DataPaths より先に拒み、セッションのディレクトリを作らない。
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        return Err(CliError::Startup(exec::StartupError::Invalid(
            text::TERMINAL_REQUIRED,
        )));
    }
    let mut signals = Signals::interactive().map_err(CliError::Output)?;
    let cancelled = Arc::clone(&signals.cancelled);
    let result = run_interactive(args, &mut signals);
    if cancelled.load(std::sync::atomic::Ordering::Acquire) {
        Ok(130)
    } else {
        result
    }
}

fn run_interactive(args: ExecArgs, signals: &mut Signals) -> Result<u8, CliError> {
    let paths = DataPaths::from_env()
        .map_err(|error| CliError::Startup(exec::StartupError::Config(error)))?;
    let restore = TerminalRestore::capture().map_err(CliError::Output)?;
    panic::install(None, Some(Arc::clone(&restore)));
    let mut declined_message: Option<&'static str> = None;
    let mut interrupted_before = None;
    let result = (|| {
        let _terminal = TerminalGuard::enter(Arc::clone(&restore)).map_err(CliError::Output)?;
        let output = restore.output().map_err(CliError::Output)?;
        let terminated = Arc::clone(&signals.cancelled);
        // 起動のときの確認（モードの変更、処理系の変更、プロジェクトの信頼）が、同じ端末を順に使う。
        let shared = std::cell::RefCell::new((signals, output, None::<io::Error>));
        let startup_screen = || {
            Screen::new(
                crossterm::terminal::size().unwrap_or((80, 24)),
                crate::ui::tui::Status {
                    mode: crate::config::Mode::Normal,
                    provider: String::new(),
                    model: String::new(),
                    host: String::new(),
                    sandbox: String::new(),
                    sandbox_disabled: false,
                    context_window: None,
                    estimate: 0,
                    templates: Default::default(),
                },
            )
        };
        let ask = |screen: &mut Screen| {
            let mut shared = shared.borrow_mut();
            let (signals, output, error) = &mut *shared;
            let mut input = match TerminalInput::new(signals) {
                Ok(input) => input,
                Err(failure) => {
                    *error = Some(failure);
                    return false;
                }
            };
            match crate::ui::tui::confirm(screen, &mut input, output, &terminated, |rows| {
                restore.set_rows_below(rows)
            }) {
                Ok(answer) => answer,
                Err(failure) => {
                    *error = Some(failure);
                    false
                }
            }
        };
        let mut mode_change = |change: &exec::ModeChange| {
            let mut screen = startup_screen();
            screen.notice(change.session_id.as_str());
            screen.open_mode_change();
            ask(&mut screen)
        };
        let mut processor_change = |change: &exec::ProcessorChange| {
            let mut screen = startup_screen();
            screen.open_processor_change(
                &change.path,
                &change.previous_version,
                &change.previous_sha256,
                &change.current_version,
                &change.current_sha256,
            );
            ask(&mut screen)
        };
        let mut trust = |request: &exec::TrustRequest| {
            let mut screen = startup_screen();
            screen.open_trust(&request.root, &trust_loads(request), &request.added);
            let _answered = ask(&mut screen);
            if terminated.load(std::sync::atomic::Ordering::Acquire) || shared.borrow().2.is_some()
            {
                return None;
            }
            // 断った答えと取り消し（Esc）は、この起動だけ信頼しない（設計書 03-01「プロジェクトの信頼」）。
            Some(match screen.take_trust_result() {
                Some(crate::ui::tui::TrustChoice::Remember(trusted)) => {
                    exec::TrustAnswer::Remember(trusted)
                }
                Some(crate::ui::tui::TrustChoice::Session(trusted)) => {
                    exec::TrustAnswer::Session(trusted)
                }
                None => exec::TrustAnswer::Session(false),
            })
        };
        let prepared = exec::prepare(
            &args,
            None,
            &paths,
            crate::sandbox::check_availability_with_bubblewrap,
            crate::benitoite::QueryLauncher::Sandboxed,
            exec::StartupPrompts {
                mode_change: &mut mode_change,
                processor_change: &mut processor_change,
                trust: &mut trust,
            },
        );
        let (signals, mut output, confirmation_error) = shared.into_inner();
        if let Some(error) = confirmation_error {
            return Err(CliError::Output(error));
        }
        let exec::Prepared {
            mut status,
            diagnostics,
            agent,
            session,
            panic_record,
            notifications,
            provider_keys,
        } = match prepared {
            Ok(prepared) => prepared,
            Err(exec::PreparationError {
                error: exec::StartupError::Declined,
                ..
            }) => {
                let cancelled = terminated.load(std::sync::atomic::Ordering::Acquire);
                if !cancelled {
                    declined_message = Some(text::MODE_CHANGE_DECLINED);
                }
                return Ok(if cancelled { 130 } else { 0 });
            }
            Err(exec::PreparationError {
                error: exec::StartupError::ProcessorDeclined,
                ..
            }) => {
                let cancelled = terminated.load(std::sync::atomic::Ordering::Acquire);
                if !cancelled {
                    declined_message = Some(text::PROCESSOR_CHANGE_DECLINED);
                }
                return Ok(if cancelled { 130 } else { 0 });
            }
            Err(error) => return Err(CliError::Preparation(error)),
        };
        panic::install(Some(panic_record), Some(Arc::clone(&restore)));
        status.estimate = agent.context_estimate();
        let mut screen = Screen::new(
            crossterm::terminal::size().map_err(CliError::Output)?,
            status,
        )
        .with_notifier(crate::ui::tui::Notifier::new(&notifications))
        .with_editor(editor_command())
        .with_provider_keys(provider_keys);
        screen.startup(diagnostics);
        // 再開でも記録したプロジェクトを使う。Agent の環境は prepare が実のパスで作る。
        screen.load_history(&paths, agent.project_dir());
        if args.resume.is_some() || args.continue_session {
            screen.resume_preview(agent.conversation());
        }
        screen.draw(&mut output, None).map_err(CliError::Output)?;
        restore.set_rows_below(screen.rows_below_cursor());
        let mut input = TerminalInput::new(signals).map_err(CliError::Output)?;
        let worker_wake = input.worker_waker();
        let (mut approver, approvals) = crate::approval::InteractiveApprover::channel();
        if let Some(wake) = &worker_wake {
            let wake = wake.clone();
            approver = approver.with_wake(Arc::new(move || wake.notify()));
        }
        let mut worker = Worker::start_with_wake(
            agent.with_interactive_approver(approver),
            session,
            worker_wake,
        )
        .map_err(CliError::Output)?
        .with_approval_requests(approvals);
        if let super::args::Request::Text(request) = &args.request
            && !request.trim().is_empty()
        {
            screen.request(request);
            worker
                .submit(request.clone(), RunControl::default())
                .map_err(CliError::Agent)?;
        }
        let result = crate::ui::tui::run(
            &mut screen,
            worker,
            &mut input,
            &mut output,
            &terminated,
            |rows| restore.set_rows_below(rows),
            &mut |current, source, target| {
                let (prepared, request) = match target {
                    Some(target) => exec::prepare_fork(
                        &args,
                        current,
                        source,
                        &paths,
                        target,
                        crate::sandbox::check_availability_with_bubblewrap,
                        crate::benitoite::QueryLauncher::Sandboxed,
                    ),
                    None => exec::prepare_new(
                        &args,
                        current,
                        &paths,
                        crate::sandbox::check_availability_with_bubblewrap,
                        crate::benitoite::QueryLauncher::Sandboxed,
                    )
                    .map(|prepared| (prepared, None)),
                }
                .map_err(|error| {
                    error
                        .diagnostics
                        .iter()
                        .chain(std::iter::once(&error.error.to_string()))
                        .cloned()
                        .collect::<Vec<_>>()
                        .join("\n")
                })?;
                // 知らせ方は起動のときの設定を使い続ける。
                let exec::Prepared {
                    agent,
                    session,
                    status,
                    diagnostics,
                    panic_record,
                    ..
                } = prepared;
                let restore = Arc::clone(&restore);
                Ok(
                    crate::ui::tui::NewSession::new(agent, session, status, diagnostics)
                        .with_request(request)
                        .on_start(move || panic::install(Some(panic_record), Some(restore))),
                )
            },
        );
        if matches!(result, Ok(130)) {
            interrupted_before = screen.checkpoint_before_rewind().map(str::to_owned);
        }
        result.map_err(|error| match error {
            TuiError::Io(error) => CliError::Output(error),
            TuiError::Agent(error) => CliError::Agent(error),
        })
    })();
    panic::install(None, None);
    // raw のモードを戻した後でだけ、断った旨を通常の診断の書き先へ出す。
    if let Some(message) = declined_message {
        crate::ui::ExecOutput::diagnostic(message).map_err(CliError::Output)?;
    }
    if let Some(checkpoint) = interrupted_before {
        crate::ui::ExecOutput::diagnostic(&text::checkpoint_before_rewind(&checkpoint))
            .map_err(CliError::Output)?;
    }
    result
}

/// 信頼の確認に示す、確かめた置き場所（設計書 03-01「プロジェクトの信頼」）。
fn trust_loads(request: &exec::TrustRequest) -> Vec<&'static str> {
    [
        (request.skills, "project skills (.agents/skills)"),
        (request.plugins, "project plugins (.agents/plugins)"),
        (
            request.prompts,
            "project prompt templates (.agents/prompts)",
        ),
        (
            request.model,
            "the model setting in .benitoiteh/config.toml",
        ),
    ]
    .into_iter()
    .filter_map(|(present, name)| present.then_some(name))
    .collect()
}

/// `VISUAL`、なければ `EDITOR` を空白で分けたもの（`code -w` のような引数を許す）。
fn editor_command() -> Option<Vec<String>> {
    ["VISUAL", "EDITOR"].into_iter().find_map(|name| {
        let value = std::env::var(name).ok()?;
        let command = value
            .split_whitespace()
            .map(str::to_owned)
            .collect::<Vec<_>>();
        (!command.is_empty()).then_some(command)
    })
}
