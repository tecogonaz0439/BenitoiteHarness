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
        let mut events = Vec::new();
        if crossterm::event::poll(wait)? {
            events.push(crossterm::event::read()?);
            while crossterm::event::poll(std::time::Duration::ZERO)? {
                events.push(crossterm::event::read()?);
            }
        }
        Ok(events)
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
    let exec::Prepared {
        mut status,
        diagnostics,
        agent,
        session,
        panic_record,
    } = exec::prepare(&args, &paths, crate::sandbox::check_availability)
        .map_err(CliError::Preparation)?;
    status.estimate = agent.context_estimate();
    let mut screen = Screen::new(
        crossterm::terminal::size().map_err(CliError::Output)?,
        status,
    );
    screen.startup(
        diagnostics
            .into_iter()
            .chain([text::APPROVAL_NEVER.to_owned()]),
    );
    let restore = TerminalRestore::capture().map_err(CliError::Output)?;
    panic::install(Some(panic_record), Some(Arc::clone(&restore)));
    let result = (|| {
        let _terminal = TerminalGuard::enter(Arc::clone(&restore)).map_err(CliError::Output)?;
        let mut output = restore.output().map_err(CliError::Output)?;
        // 最初の要求が通信を始める前に、送る内容を実際の端末へ出す（設計書 01-04）。
        screen.draw(&mut output, None).map_err(CliError::Output)?;
        restore.set_rows_below(screen.rows_below_cursor());
        let mut worker = Worker::start(agent, session).map_err(CliError::Output)?;
        if let super::args::Request::Text(request) = &args.request
            && !request.trim().is_empty()
        {
            screen.request(request);
            worker
                .submit(request.clone(), RunControl::default())
                .map_err(CliError::Agent)?;
        }
        let terminated = Arc::clone(&signals.cancelled);
        crate::ui::tui::run(
            &mut screen,
            worker,
            &mut TerminalInput { signals },
            &mut output,
            &terminated,
            |rows| restore.set_rows_below(rows),
        )
        .map_err(|error| match error {
            TuiError::Io(error) => CliError::Output(error),
            TuiError::Agent(error) => CliError::Agent(error),
        })
    })();
    // prepare は hook を設定しないので、/new の間も端末の復元を失わない。
    panic::install(None, None);
    result
}
