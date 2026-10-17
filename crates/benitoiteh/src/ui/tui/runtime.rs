//! 待ちと終了を一か所に置き、端末なしでもシグナルと受信の継続を確かめる（実装プラン P4-06）。

use std::io::{self, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use crossterm::event::Event;

use super::{Action, RunControl, Screen, Worker};
use crate::agent::AgentError;

/// 置き換える外部依存は端末の入力だけとする（設計書 07-01「テストの原則」）。
pub trait InputSource {
    /// 指定時間以内に届いた入力をまとめて返す。連続する Resize は一回の描画にまとまる。
    fn poll(&mut self, wait: Duration) -> io::Result<Vec<Event>>;
    /// 読み取った時刻を保つ入力。端末は一件ずつ読み取った直後の時計を添える。
    fn poll_timed(&mut self, wait: Duration) -> io::Result<Vec<(Event, Instant)>> {
        Ok(self
            .poll(wait)?
            .into_iter()
            .map(|event| (event, Instant::now()))
            .collect())
    }
    /// 表示直後のキーと貼り付けを捨て、Resize と Ctrl+C は受ける。
    fn discard_pending(&mut self) -> io::Result<Vec<(Event, Instant)>> {
        Ok(self
            .poll_timed(Duration::ZERO)?
            .into_iter()
            .filter(|(event, _)| {
                matches!(event, Event::Resize(_, _)) || super::confirmation::is_interrupt(event)
            })
            .collect())
    }
    /// 終了シグナルの累計。キーだけを持つ入力元ではゼロとする。
    fn termination_count(&mut self) -> io::Result<usize> {
        Ok(0)
    }
}

/// 画面の書き込みか、ループと記録の失敗。
#[derive(Debug)]
pub enum TuiError {
    /// 端末の入出力。
    Io(io::Error),
    /// ループとセッションの記録。
    Agent(AgentError),
}

impl std::fmt::Display for TuiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(error) => error.fmt(f),
            Self::Agent(error) => error.fmt(f),
        }
    }
}
impl std::error::Error for TuiError {}

/// 入力とイベントをまとめて一回描く。終了中も、仕事が戻るまで受信を続ける。
pub fn run(
    screen: &mut Screen,
    mut worker: Worker,
    input: &mut impl InputSource,
    output: &mut impl Write,
    terminated: &AtomicBool,
    mut cursor_position: impl FnMut(u16),
) -> Result<u8, TuiError> {
    let started = Instant::now();
    let mut processing_since = worker.is_running().then(Instant::now);
    let mut next_tick = Instant::now();
    let mut exiting = false;
    let mut status = 0;
    let mut error = None;
    let mut output_failed = false;
    let mut input_failed = false;
    let mut redraw = true;
    let mut pending_input = Vec::new();
    loop {
        match input.termination_count() {
            // Worker の Drop は受信先を閉じるだけで join しない。二度目は記録を待たない。
            Ok(count) if count >= 2 => return Ok(130),
            Ok(_) => {}
            Err(failure) => {
                error = Some(TuiError::Io(failure));
                exiting = true;
                worker.cancel();
            }
        }
        if terminated.load(Ordering::Acquire) {
            redraw |= screen.cancellation_notice();
            exiting = true;
            status = 130;
            worker.cancel();
        }
        let was_running = worker.is_running();
        match worker.drain(screen) {
            Ok(changed) => redraw |= changed,
            Err(failure) => {
                screen.notice(&failure.to_string());
                error = Some(TuiError::Agent(failure));
                exiting = true;
                worker.cancel();
                redraw = true;
            }
        }
        if was_running && !worker.is_running() {
            processing_since = None;
        }
        if exiting {
            redraw |= screen.cancellation_notice();
            screen.cancel_confirmation();
        }
        if !exiting
            && !worker.is_running()
            && !screen.confirming()
            && let Some(request) = screen.next_followup()
        {
            screen.show_request(&request);
            if let Err(failure) = worker.submit(request, RunControl::default()) {
                error = Some(TuiError::Agent(failure));
                exiting = true;
            }
            processing_since = Some(Instant::now());
            redraw = true;
        }
        let now = Instant::now();
        if redraw || (worker.is_running() && now >= next_tick) {
            if !output_failed {
                if let Err(failure) =
                    screen.draw(output, processing_since.map(|time| time.elapsed()))
                {
                    error = Some(TuiError::Io(failure));
                    output_failed = true;
                    exiting = true;
                    worker.cancel();
                }
                if !output_failed && screen.presented(Instant::now()) {
                    match input.discard_pending() {
                        Ok(events) => pending_input.extend(events),
                        Err(failure) => {
                            error = Some(TuiError::Io(failure));
                            exiting = true;
                            worker.cancel();
                            screen.cancel_confirmation();
                        }
                    }
                }
                cursor_position(screen.rows_below_cursor());
            }
            redraw = false;
            next_tick = now.checked_add(Duration::from_secs(1)).unwrap_or(now);
        }
        if exiting && !worker.is_running() {
            break;
        }
        let interval = Duration::from_millis(if worker.is_running() { 33 } else { 250 });
        let wait = if worker.is_running() {
            interval.min(next_tick.saturating_duration_since(Instant::now()))
        } else {
            interval
        };
        let events = if !pending_input.is_empty() {
            std::mem::take(&mut pending_input)
        } else if input_failed {
            std::thread::park_timeout(wait);
            Vec::new()
        } else {
            match input.poll_timed(wait) {
                Ok(events) => events,
                Err(failure) => {
                    error = Some(TuiError::Io(failure));
                    input_failed = true;
                    exiting = true;
                    worker.cancel();
                    Vec::new()
                }
            }
        };
        // 外からの INT はキーの Ctrl+C と区別し、次の依頼を始める前にも調べる。
        if terminated.load(Ordering::Acquire) {
            redraw |= screen.cancellation_notice();
            exiting = true;
            status = 130;
            worker.cancel();
        }
        for (event, read_at) in events {
            if exiting {
                match event {
                    Event::Resize(_, _) => {
                        screen.input(event, started.elapsed(), worker.is_running());
                        redraw = true;
                    }
                    Event::Key(key)
                        if key.kind == crossterm::event::KeyEventKind::Press
                            && key
                                .modifiers
                                .contains(crossterm::event::KeyModifiers::CONTROL)
                            && matches!(key.code, crossterm::event::KeyCode::Char('c' | 'd')) =>
                    {
                        return Ok(130);
                    }
                    Event::Key(_)
                    | Event::Paste(_)
                    | Event::FocusGained
                    | Event::FocusLost
                    | Event::Mouse(_) => {}
                }
                continue;
            }
            match screen.input_at(
                event,
                read_at,
                read_at.saturating_duration_since(started),
                worker.is_running(),
            ) {
                Action::None => {}
                Action::Submit(request) => {
                    screen.request(&request);
                    if let Err(failure) = worker.submit(request, RunControl::default()) {
                        error = Some(TuiError::Agent(failure));
                        exiting = true;
                    }
                    processing_since = Some(Instant::now());
                }
                Action::Steer(value) => {
                    screen.remember_input(&value);
                    if let Err(value) = worker.steer(value) {
                        screen.restore_input(value);
                    }
                }
                Action::FollowUp(value) => screen.queue_followup(value),
                Action::CancelStep => {
                    screen.cancellation_notice();
                    worker.cancel_step();
                }
                Action::Cancel => {
                    screen.cancellation_notice();
                    worker.cancel();
                    screen.cancel_confirmation();
                }
                Action::Quit => {
                    screen.cancellation_notice();
                    exiting = true;
                    worker.cancel();
                    screen.cancel_confirmation();
                }
            }
            redraw = true;
        }
    }
    if let Err(failure) = worker.close() {
        error = Some(TuiError::Agent(failure));
    }
    // SIGHUP 等で端末が切れた後の EIO/EOF より、終了シグナルを優先する。
    if status == 130 || terminated.load(Ordering::Acquire) {
        return Ok(130);
    }
    match error {
        Some(error) => Err(error),
        None => Ok(status),
    }
}

/// 再開と巻き戻しに共通の確認の待ち方。端末の待ち行列も同じ順序で捨てる。
pub fn confirm(
    screen: &mut Screen,
    input: &mut impl InputSource,
    output: &mut impl Write,
    terminated: &AtomicBool,
    mut cursor_position: impl FnMut(u16),
) -> io::Result<bool> {
    let started = Instant::now();
    let mut pending = Vec::new();
    loop {
        if terminated.load(Ordering::Acquire) || input.termination_count()? > 0 {
            terminated.store(true, Ordering::Release);
            screen.cancel_confirmation();
            return Ok(false);
        }
        screen.draw(output, None)?;
        let shown = Instant::now();
        cursor_position(screen.rows_below_cursor());
        if screen.presented(shown) {
            pending.extend(input.discard_pending()?);
        }
        if let Some(result) = screen.take_confirmation_result() {
            return Ok(result);
        }
        let events = if pending.is_empty() {
            input.poll_timed(Duration::from_millis(33))?
        } else {
            std::mem::take(&mut pending)
        };
        for (event, read_at) in events {
            match screen.input_at(
                event,
                read_at,
                read_at.saturating_duration_since(started),
                false,
            ) {
                Action::Cancel | Action::Quit => {
                    terminated.store(true, Ordering::Release);
                    screen.cancel_confirmation();
                    return Ok(false);
                }
                Action::None
                | Action::Submit(_)
                | Action::Steer(_)
                | Action::FollowUp(_)
                | Action::CancelStep => {}
            }
        }
    }
}
