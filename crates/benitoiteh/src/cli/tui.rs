//! 対話の起動だけが端末の有無と大きさを調べる（設計書 05-01、05-02）。

use std::io::{self, IsTerminal};
use std::sync::Arc;

use crate::config::paths::DataPaths;
use crate::ui::tui::{
    InputSource, RunControl, Screen, TerminalGuard, TerminalRestore, TuiError, Worker,
};

use super::{CliError, args::ExecArgs, exec, panic, signals::Signals, text};

struct TerminalInput<'a> {
    signals: &'a mut Signals,
}

impl InputSource for TerminalInput<'_> {
    fn termination_count(&mut self) -> io::Result<usize> {
        self.signals.termination_count()
    }
    fn poll(&mut self, wait: std::time::Duration) -> io::Result<Vec<crossterm::event::Event>> {
        Ok(self
            .poll_timed(wait)?
            .into_iter()
            .map(|(event, _)| event)
            .collect())
    }
    fn poll_timed(
        &mut self,
        wait: std::time::Duration,
    ) -> io::Result<Vec<(crossterm::event::Event, std::time::Instant)>> {
        let mut events = Vec::new();
        if crossterm::event::poll(wait)? {
            events.push((crossterm::event::read()?, std::time::Instant::now()));
            while crossterm::event::poll(std::time::Duration::ZERO)? {
                events.push((crossterm::event::read()?, std::time::Instant::now()));
            }
        }
        Ok(events)
    }
    fn discard_pending(
        &mut self,
    ) -> io::Result<Vec<(crossterm::event::Event, std::time::Instant)>> {
        // OS の待ち行列を先に捨て、次に crossterm の読み込み済みの列を空にする。
        rustix::termios::tcflush(io::stdin(), rustix::termios::QueueSelector::IFlush)?;
        Ok(self.poll_timed(std::time::Duration::ZERO)?.into_iter().filter(|(event, _)| {
            matches!(event, crossterm::event::Event::Resize(_, _)) || matches!(event, crossterm::event::Event::Key(key) if key.kind == crossterm::event::KeyEventKind::Press && key.modifiers.contains(crossterm::event::KeyModifiers::CONTROL) && matches!(key.code, crossterm::event::KeyCode::Char('c' | 'C')))
        }).collect())
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
    let mut mode_declined = false;
    let mut interrupted_before = None;
    let result = (|| {
        let _terminal = TerminalGuard::enter(Arc::clone(&restore)).map_err(CliError::Output)?;
        let mut output = restore.output().map_err(CliError::Output)?;
        let terminated = Arc::clone(&signals.cancelled);
        let mut confirmation_error = None;
        let prepared = exec::prepare(
            &args,
            None,
            &paths,
            crate::sandbox::check_availability,
            &mut |change| {
                let mut screen = Screen::new(
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
                    },
                );
                screen.notice(change.session_id.as_str());
                screen.open_mode_change();
                match crate::ui::tui::confirm(
                    &mut screen,
                    &mut TerminalInput { signals },
                    &mut output,
                    &terminated,
                    |rows| restore.set_rows_below(rows),
                ) {
                    Ok(answer) => answer,
                    Err(error) => {
                        confirmation_error = Some(error);
                        false
                    }
                }
            },
        );
        if let Some(error) = confirmation_error {
            return Err(CliError::Output(error));
        }
        let exec::Prepared {
            mut status,
            diagnostics,
            agent,
            session,
            panic_record,
        } = match prepared {
            Ok(prepared) => prepared,
            Err(exec::PreparationError {
                error: exec::StartupError::Declined,
                ..
            }) => {
                mode_declined = !terminated.load(std::sync::atomic::Ordering::Acquire);
                return Ok(if mode_declined { 0 } else { 130 });
            }
            Err(error) => return Err(CliError::Preparation(error)),
        };
        panic::install(Some(panic_record), Some(Arc::clone(&restore)));
        status.estimate = agent.context_estimate();
        let mut screen = Screen::new(
            crossterm::terminal::size().map_err(CliError::Output)?,
            status,
        );
        screen.startup(diagnostics);
        // 再開でも記録したプロジェクトを使う。Agent の環境は prepare が実のパスで作る。
        screen.load_history(&paths, agent.project_dir());
        if args.resume.is_some() || args.continue_session {
            screen.resume_preview(agent.conversation());
        }
        screen.draw(&mut output, None).map_err(CliError::Output)?;
        restore.set_rows_below(screen.rows_below_cursor());
        let (approver, approvals) = crate::approval::InteractiveApprover::channel();
        let mut worker = Worker::start(agent.with_approver(Box::new(approver)), session)
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
            &mut TerminalInput { signals },
            &mut output,
            &terminated,
            |rows| restore.set_rows_below(rows),
            &mut |current| {
                let exec::Prepared {
                    agent,
                    session,
                    status,
                    diagnostics,
                    panic_record,
                } = exec::prepare_new(&args, current, &paths, crate::sandbox::check_availability)
                    .map_err(|error| {
                    error
                        .diagnostics
                        .iter()
                        .chain(std::iter::once(&error.error.to_string()))
                        .cloned()
                        .collect::<Vec<_>>()
                        .join("\n")
                })?;
                let restore = Arc::clone(&restore);
                Ok(
                    crate::ui::tui::NewSession::new(agent, session, status, diagnostics)
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
    if mode_declined {
        crate::ui::ExecOutput::diagnostic(text::MODE_CHANGE_DECLINED).map_err(CliError::Output)?;
    }
    if let Some(checkpoint) = interrupted_before {
        crate::ui::ExecOutput::diagnostic(&text::checkpoint_before_rewind(&checkpoint))
            .map_err(CliError::Output)?;
    }
    result
}
