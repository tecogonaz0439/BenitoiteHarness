//! 端末から独立した対話の画面と、入力・イベントの接続（設計書 05-02）。

mod control;
mod format;
mod input;
mod runtime;
mod terminal;
mod text;
mod worker;

pub use control::restoration;
pub use input::Action;
pub use runtime::{InputSource, TuiError, run};
pub use terminal::{TerminalGuard, TerminalOutput, TerminalRestore};
pub use worker::{RunControl, Worker};

use std::io::{self, Write};
use std::time::Duration;

use crossterm::event::Event;

use crate::agent::{AgentEvent, Outcome};
use crate::config::Mode;
use control::{Decoration, Frame, Line, SafeText, plain};

/// 起動時と依頼の間に画面へ渡す値。送り先はホストの名前だけを持つ。
#[derive(Clone, Debug)]
pub struct Status {
    pub mode: Mode,
    pub provider: String,
    pub model: String,
    pub host: String,
    pub sandbox: String,
    pub sandbox_disabled: bool,
    pub context_window: Option<u64>,
    pub estimate: u64,
}

/// 入力のイベント列と AgentEvent を受け、Write に一回の描画を出す芯。
pub struct Screen {
    size: (u16, u16),
    pub status: Status,
    input: input::Input,
    pending: String,
    pending_at_line_start: bool,
    reasoning: bool,
    reasoning_lines: usize,
    reasoning_omitted: bool,
    markdown: format::Markdown,
    history: Vec<Line>,
    rows_above: u16,
    rows_below: u16,
}

impl Screen {
    /// 端末の大きさは呼び出す側で求める。ここでは端末へ問い合わせない。
    pub fn new(size: (u16, u16), status: Status) -> Self {
        Self {
            size,
            status,
            input: input::Input::default(),
            pending: String::new(),
            pending_at_line_start: true,
            reasoning: false,
            reasoning_lines: 0,
            reasoning_omitted: false,
            markdown: format::Markdown::default(),
            history: Vec::new(),
            rows_above: 0,
            rows_below: 0,
        }
    }

    /// 最初の依頼より先に、通信先・送る内容・サンドボックス・通知を示す。
    pub fn startup(&mut self, diagnostics: impl IntoIterator<Item = String>) {
        self.notice(&crate::context::transmission_notice(
            &self.status.provider,
            &self.status.model,
            &self.status.host,
        ));
        self.notice(&text::sandbox(&self.status));
        for diagnostic in diagnostics {
            self.notice(&diagnostic);
        }
    }

    /// 生の入力は送信のために保ち、描画するときだけ安全な文へ変える。
    pub fn input(&mut self, event: Event, now: Duration, running: bool) -> Action {
        if let Event::Resize(width, height) = event {
            self.size = (width, height);
            return Action::None;
        }
        self.input.event(event, now, running)
    }

    /// 外から指定した最初の依頼も、同じ履歴と画面の経路を通す。
    pub fn request(&mut self, request: &str) {
        self.input.remember(request);
        self.flush_pending();
        for line in SafeText::new(request, true).as_str().split('\n') {
            self.history
                .push(plain(&format!("{}{line}", text::PROMPT), Decoration::Bold));
        }
    }

    /// 実行を終え、次の Ctrl+C が新しい取り消しになるようにする。
    pub fn complete(&mut self, estimate: u64) {
        self.status.estimate = estimate;
        self.input.complete();
    }

    /// UI が所有する一行の診断。raw のモードで別の書き手を使わない。
    pub fn notice(&mut self, value: &str) {
        self.history.push(plain(value, Decoration::Dim));
    }

    /// イベント内の信頼しない値は、すべて SafeText の境界を通す。
    pub fn event(&mut self, event: &AgentEvent) {
        match event {
            AgentEvent::TextDelta { text, .. } => self.delta(text, false),
            AgentEvent::ReasoningDelta { text, .. } => self.delta(text, true),
            AgentEvent::TurnStart { .. } => {
                self.flush_pending();
                self.reset_reasoning();
            }
            AgentEvent::ToolCall {
                name, arguments, ..
            } => {
                self.flush_pending();
                self.reset_reasoning();
                self.history
                    .push(format::tool_call(name, arguments, self.size.0));
            }
            AgentEvent::ToolResult {
                summary,
                is_error,
                screen,
                display,
                ..
            } => {
                self.flush_pending();
                let fallback =
                    text::event_detail(if *is_error { text::ERROR } else { text::OK }, summary);
                let value = screen.as_deref().unwrap_or(&fallback);
                self.history.push(vec![(
                    format::clipped(value, self.size.0),
                    if *is_error {
                        Decoration::Error
                    } else {
                        Decoration::Dim
                    },
                )]);
                if let Some(display) = display {
                    self.history.extend(format::diff(display));
                }
            }
            AgentEvent::SessionStart {
                provider,
                model,
                host,
                notice,
                instruction_files,
                skipped_instructions,
                ..
            } => {
                self.status.provider.clone_from(provider);
                self.status.model.clone_from(model);
                self.status.host.clone_from(host);
                // notice に base_url を組み込まず、イベントが持つ通知も可視化する。
                if let Some(notice) = notice {
                    self.notice(notice);
                }
                for path in instruction_files {
                    self.notice(&text::instruction_loaded(path));
                }
                for skipped in skipped_instructions {
                    self.notice(&text::instruction_skipped(&skipped.path, &skipped.reason));
                }
            }
            AgentEvent::ApprovalRequest {
                summary,
                facts,
                justification,
                user_message,
                ..
            }
            | AgentEvent::ApprovalResult {
                summary,
                facts,
                justification,
                user_message,
                ..
            } => {
                self.flush_pending();
                let summary = if let AgentEvent::ApprovalResult { decision, .. } = event {
                    super::text::approval(summary, *decision)
                } else {
                    summary.clone()
                };
                let detail = text::approval(
                    &summary,
                    facts,
                    justification.as_deref(),
                    user_message.as_deref(),
                );
                self.history.push(plain(&detail, Decoration::Error));
            }
            AgentEvent::Error { message, .. } => {
                self.flush_pending();
                self.history.push(plain(message, Decoration::Error));
            }
            AgentEvent::Retry { reason, .. } => {
                self.flush_pending();
                self.notice(&text::event_detail(text::RETRY, reason));
            }
            AgentEvent::Compaction { estimate_after, .. } => {
                self.status.estimate = *estimate_after;
                self.notice(&text::event_detail(text::COMPACTION, estimate_after));
            }
            AgentEvent::Checkpoint { checkpoint } => {
                self.notice(&super::text::checkpoint(checkpoint))
            }
            AgentEvent::HardLinks { paths } => self.notice(&super::text::hard_links(paths)),
            AgentEvent::SessionEnd { outcome, reason } => {
                self.flush_pending();
                if *outcome == Outcome::Cancelled {
                    self.history
                        .push(plain(text::INTERRUPTED, Decoration::Error));
                }
                self.notice(reason);
            }
            AgentEvent::TurnEnd { .. } => self.flush_pending(),
        }
    }

    fn delta(&mut self, text: &str, reasoning: bool) {
        if self.reasoning != reasoning {
            self.flush_pending();
            if !reasoning {
                self.reset_reasoning();
            }
            self.reasoning = reasoning;
        }
        let safe = SafeText::new(text, true);
        for part in safe.as_str().split_inclusive('\n') {
            // 長い推論は先頭三行と一行の省略の印だけにする。
            if reasoning && self.reasoning_lines >= 3 {
                if !self.reasoning_omitted && !part.is_empty() {
                    self.history
                        .push(plain(text::REASONING_OMITTED, Decoration::Dim));
                    self.reasoning_omitted = true;
                }
                continue;
            }
            self.pending
                .push_str(part.strip_suffix('\n').unwrap_or(part));
            if part.ends_with('\n') {
                self.commit_pending();
            }
        }
    }

    fn commit_pending(&mut self) {
        let pending = std::mem::take(&mut self.pending);
        if self.reasoning {
            if self.reasoning_lines < 3 {
                self.history.push(plain(&pending, Decoration::Dim));
                self.reasoning_lines = self.reasoning_lines.saturating_add(1);
            }
        } else if self.pending_at_line_start {
            self.history
                .push(self.markdown.line(&SafeText::new(&pending, false)));
        } else {
            self.history.push(plain(&pending, Decoration::Plain));
        }
        self.pending_at_line_start = true;
    }

    fn flush_pending(&mut self) {
        if !self.pending.is_empty() {
            self.commit_pending();
        }
        self.pending_at_line_start = true;
    }

    fn reset_reasoning(&mut self) {
        self.reasoning_lines = 0;
        self.reasoning_omitted = false;
    }

    /// 入力位置から下の行数。panic hook も同じ復元の列を組み立てる。
    pub fn rows_below_cursor(&self) -> u16 {
        self.rows_below
    }

    /// 一つのフレームをメモリに作り、write と flush を各一回だけ呼ぶ。
    pub fn draw(&mut self, writer: &mut impl Write, elapsed: Option<Duration>) -> io::Result<()> {
        let width = self.size.0.max(1);
        let height = usize::from(self.size.1);
        let maximum = height.saturating_sub(1);
        let status_rows = usize::from(height >= 3);
        let value = SafeText::new(&format!("{}{}", text::PROMPT, self.input.text), true);
        let prefix = self.input.text.get(..self.input.cursor).unwrap_or_default();
        let cursor = text::PROMPT
            .len()
            .saturating_add(SafeText::new(prefix, true).as_str().len());
        let (inputs, (cursor_row, cursor_column)) = format::wrapped(&value, width, cursor);
        let input_count = inputs
            .len()
            .min((height / 2).max(1))
            .min(maximum.saturating_sub(status_rows));
        let start = cursor_row
            .saturating_sub(input_count / 2)
            .min(inputs.len().saturating_sub(input_count));
        let inputs: Vec<_> = inputs
            .iter()
            .skip(start)
            .take(input_count)
            .map(|line| plain(line, Decoration::Plain))
            .collect();
        let capacity = maximum
            .saturating_sub(inputs.len())
            .saturating_sub(status_rows);
        let mut provisional = Vec::new();
        if !self.pending.is_empty() {
            let safe = SafeText::new(&self.pending, true);
            let (mut lines, _) = format::wrapped(&safe, width, usize::MAX);
            let overflow = lines.len().saturating_sub(capacity);
            if overflow > 0 {
                self.pending_at_line_start = false;
            }
            for line in lines.drain(..overflow) {
                if !self.reasoning || self.reasoning_lines < 3 {
                    self.history.push(plain(
                        &line,
                        if self.reasoning {
                            Decoration::Dim
                        } else {
                            Decoration::Plain
                        },
                    ));
                    if self.reasoning {
                        self.reasoning_lines = self.reasoning_lines.saturating_add(1);
                    }
                } else if !self.reasoning_omitted {
                    self.history
                        .push(plain(text::REASONING_OMITTED, Decoration::Dim));
                    self.reasoning_omitted = true;
                }
            }
            self.pending = lines.join("");
            if self.reasoning && self.reasoning_lines >= 3 {
                if !self.pending.is_empty() && !self.reasoning_omitted {
                    self.history
                        .push(plain(text::REASONING_OMITTED, Decoration::Dim));
                    self.reasoning_omitted = true;
                }
                self.pending.clear();
            } else {
                provisional = lines
                    .iter()
                    .map(|line| {
                        plain(
                            line,
                            if self.reasoning {
                                Decoration::Dim
                            } else {
                                Decoration::Plain
                            },
                        )
                    })
                    .collect();
            }
        }
        let provisional_count = provisional.len();
        let mut bottom = provisional;
        bottom.extend(inputs);
        if status_rows > 0 {
            let status = if self.input.quit_hint {
                text::QUIT_HINT.to_owned()
            } else {
                text::status(&self.status, elapsed)
            };
            bottom.push(vec![(format::clipped(&status, width), Decoration::Dim)]);
        }
        let mut frame = Frame::default();
        frame.begin()?;
        frame.clear(self.rows_above)?;
        frame.wrap(true)?;
        for line in &self.history {
            frame.line(line)?;
            frame.newline();
        }
        frame.wrap(false)?;
        for (index, line) in bottom.iter().enumerate() {
            if index > 0 {
                frame.newline();
            }
            frame.line(line)?;
        }
        let target = provisional_count
            .saturating_add(cursor_row.saturating_sub(start))
            .min(bottom.len().saturating_sub(1));
        let below = bottom.len().saturating_sub(1).saturating_sub(target);
        frame.up(u16::try_from(below).unwrap_or(u16::MAX))?;
        frame.column(cursor_column.min(width.saturating_sub(1)))?;
        frame.finish(writer)?;
        self.history.clear();
        self.rows_above = u16::try_from(target).unwrap_or(u16::MAX);
        self.rows_below = u16::try_from(below).unwrap_or(u16::MAX);
        Ok(())
    }
}

#[cfg(test)]
mod tests;
