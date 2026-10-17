//! 端末から独立した対話の画面と、入力・イベントの接続（設計書 05-02）。

mod commands;
mod confirmation;
mod selection;
pub use commands::{Command, NewSession};
mod control;
pub mod history;
pub use confirmation::ConfirmationGuard;
mod format;
mod input;
mod runtime;
mod terminal;
mod text;
mod unicode;
mod worker;

pub use control::restoration;
pub use input::Action;
pub use runtime::{InputSource, TuiError, confirm, run};
pub use terminal::{TerminalGuard, TerminalOutput, TerminalRestore};
pub use worker::{RunControl, Worker};

use std::collections::{BTreeSet, VecDeque};
use std::io::{self, Write};
use std::time::{Duration, Instant};

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
    explicit_history: BTreeSet<usize>,
    rows_above: u16,
    rows_below: u16,
    confirmation: Option<confirmation::Confirmation>,
    confirmation_result: Option<bool>,
    followups: VecDeque<SubmittedInput>,
    original_submission: Option<String>,
    history_file: Option<history::History>,
    active_tools: BTreeSet<String>,
    cancellation_notice_shown: bool,
    selection: Option<selection::Selection>,
    rewind_preview: Option<crate::agent::RewindPreview>,
    rewind_retry: bool,
    operation: Option<&'static str>,
    before_rewind: Option<String>,
    interrupted_rewind: Option<String>,
}

// 送る文と復元する原文を分け、// を外した文が取り消し後にコマンドにならないようにする。
struct SubmittedInput {
    text: String,
    original: String,
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
            explicit_history: BTreeSet::new(),
            rows_above: 0,
            rows_below: 0,
            confirmation: None,
            confirmation_result: None,
            followups: VecDeque::new(),
            original_submission: None,
            history_file: None,
            active_tools: BTreeSet::new(),
            cancellation_notice_shown: false,
            selection: None,
            rewind_preview: None,
            rewind_retry: false,
            operation: None,
            before_rewind: None,
            interrupted_rewind: None,
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
        self.input_at(event, Instant::now(), now, running)
    }

    /// 端末で読んだ時刻を保ち、確認前のキーを後から答えにしない。
    pub fn input_at(
        &mut self,
        event: Event,
        read_at: Instant,
        now: Duration,
        running: bool,
    ) -> Action {
        if let Event::Resize(width, height) = event {
            if self.size != (width, height)
                && let Some(confirmation) = &mut self.confirmation
            {
                confirmation.guard = ConfirmationGuard::default();
            }
            self.size = (width, height);
            return Action::None;
        }
        if self.confirming() {
            return self.confirmation_input(event, read_at, now, running);
        }
        if self.selection.is_some() {
            return self.selection_input(event);
        }
        let action = self.input.event(event, now, running);
        self.command_action(action, running)
    }

    /// 外から指定した最初の依頼も、同じ履歴と画面の経路を通す。
    pub fn request(&mut self, request: &str) {
        self.remember(request);
        self.show_request(request);
    }

    /// 保存済みの後続を、履歴へ重ねて入れずに示す。
    pub fn show_request(&mut self, request: &str) {
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
            AgentEvent::Steered { text } => {
                self.flush_pending();
                for line in SafeText::new(text, true).as_str().split('\n') {
                    self.history
                        .push(plain(&format!("{}{line}", text::PROMPT), Decoration::Bold));
                }
            }
            AgentEvent::TextDelta { text, .. } => self.delta(text, false),
            AgentEvent::ReasoningDelta { text, .. } => self.delta(text, true),
            AgentEvent::TurnStart { .. } => {
                self.cancellation_notice_shown = false;
                self.flush_pending();
                self.reset_reasoning();
            }
            AgentEvent::ToolCall {
                name,
                arguments,
                call_id,
                ..
            } => {
                self.active_tools.insert(call_id.clone());
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
                call_id,
                ..
            } => {
                self.active_tools.remove(call_id);
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
            AgentEvent::ApprovalRequest { .. } => self.flush_pending(),
            AgentEvent::ApprovalResult {
                summary,
                facts,
                justification,
                user_message,
                decision,
                ..
            } => {
                self.flush_pending();
                let summary = super::text::approval(summary, *decision);
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
                if checkpoint.reason == crate::rewind::CheckpointReason::BeforeRewind {
                    self.before_rewind = Some(checkpoint.id.clone());
                }
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

    /// 履歴の読み込みの失敗は一行で示して続ける。
    pub fn load_history(
        &mut self,
        paths: &crate::config::paths::DataPaths,
        project: &std::path::Path,
    ) {
        match history::History::open(paths, project) {
            Ok((file, entries)) => {
                for entry in entries {
                    self.input.remember(&entry);
                }
                self.history_file = Some(file);
            }
            Err(error) => self.notice(&text::history_failed(&error)),
        }
    }

    fn remember(&mut self, value: &str) {
        self.input.remember(value);
        if let Some(file) = &mut self.history_file
            && let Err(error) = file.append(value)
        {
            self.history_file = None;
            self.notice(&text::history_failed(&error));
        }
    }

    /// 操縦と後続は送った時点で履歴に保存する。
    pub fn remember_input(&mut self, value: &str) {
        self.remember(value);
    }

    /// 後続を画面側で保つ。Agent は一連の処理の中では受け取らない。
    pub fn queue_followup(&mut self, value: String) {
        self.remember(&value);
        self.followups.push_back(SubmittedInput {
            original: value.clone(),
            text: value,
        });
    }

    /// 依頼の完了後だけ、次の後続を取り出す。
    pub fn next_followup(&mut self) -> Option<String> {
        self.followups.pop_front().map(|value| value.text)
    }

    /// 取り消しでは後続も入力欄へ戻す。加えられなかった操縦は結果によらず戻す。
    pub fn finished(&mut self, cancelled: bool, mut steering: Vec<String>) {
        if cancelled {
            steering.extend(self.followups.drain(..).map(|value| value.original));
        }
        self.input.restore(steering);
        self.active_tools.clear();
    }

    /// 送り手が先に閉じた操縦も失わない。
    pub fn restore_input(&mut self, value: String) {
        self.input.restore([value]);
    }

    /// 道具の結果がない間に止める場合だけ、不可逆の操作を知らせる。
    pub fn cancellation_notice(&mut self) -> bool {
        if !self.active_tools.is_empty() && !self.cancellation_notice_shown {
            self.cancellation_notice_shown = true;
            self.notice(text::CANCEL_EFFECTS);
            true
        } else {
            false
        }
    }

    /// 再開した会話の最後の三組から依頼・操縦と応答だけを示す。
    pub fn resume_preview(&mut self, conversation: &crate::provider::Conversation) {
        use crate::provider::{AssistantPart, Item, UserPartKind};
        let start = conversation.items.iter().enumerate().rev().filter_map(|(index, item)| {
            matches!(item, Item::UserMessage(message) if message.parts.iter().any(|part| matches!(part.kind, UserPartKind::Request | UserPartKind::Steering))).then_some(index)
        }).nth(2).unwrap_or(0);
        if conversation.items.iter().any(|item| matches!(item, Item::UserMessage(message) if message.parts.iter().any(|part| part.kind == UserPartKind::Summary))) {
            self.notice(text::COMPACTED_HISTORY);
        }
        for item in conversation.items.iter().skip(start) {
            match item {
                Item::UserMessage(message) => {
                    for part in &message.parts {
                        if matches!(part.kind, UserPartKind::Request | UserPartKind::Steering) {
                            for line in SafeText::new(&part.text, true).as_str().split('\n') {
                                self.notice(&format!("{}{line}", text::PROMPT));
                            }
                        }
                    }
                }
                Item::AssistantTurn(turn) => {
                    for part in &turn.parts {
                        if let AssistantPart::Text(value) = part {
                            for line in SafeText::new(value, true).as_str().split('\n') {
                                self.notice(line);
                            }
                        }
                    }
                }
                Item::ToolResult(_) => {}
            }
        }
    }

    /// 中断して端末を戻した後に、回復に使える時点を示す。
    pub fn checkpoint_before_rewind(&self) -> Option<&str> {
        self.interrupted_rewind.as_deref()
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
        let (inputs, (mut cursor_row, mut cursor_column)) = format::wrapped(&value, width, cursor);
        let input_count = inputs
            .len()
            .min((height / 2).max(1))
            .min(maximum.saturating_sub(status_rows));
        let mut start = cursor_row
            .saturating_sub(input_count / 2)
            .min(inputs.len().saturating_sub(input_count));
        let mut inputs: Vec<_> = inputs
            .iter()
            .skip(start)
            .take(input_count)
            .map(|line| plain(line, Decoration::Plain))
            .collect();
        if let Some(selection) = &self.selection {
            inputs = selection.bottom(self.size);
            cursor_row = 0;
            cursor_column = 0;
            start = 0;
        }
        if let Some(confirmation) = &self.confirmation {
            let (rows, cursor) = confirmation.bottom(self.size);
            inputs = rows;
            (cursor_row, cursor_column) = cursor;
            start = 0;
        }
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
                format!(
                    "{}{}{}",
                    text::status(&self.status, elapsed),
                    text::queued(self.followups.len()),
                    self.operation
                        .map_or(String::new(), |value| format!(" | {value}"))
                )
            };
            bottom.push(vec![(format::clipped(&status, width), Decoration::Dim)]);
        }
        let mut frame = Frame::default();
        frame.begin()?;
        frame.clear(self.rows_above)?;
        frame.wrap(true)?;
        for (index, line) in self.history.iter().enumerate() {
            frame.wrap(!self.explicit_history.contains(&index))?;
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
        self.explicit_history.clear();
        self.rows_above =
            u16::try_from(target.min(maximum.saturating_sub(below))).unwrap_or(u16::MAX);
        self.rows_below = u16::try_from(below).unwrap_or(u16::MAX);
        Ok(())
    }
}

#[cfg(test)]
mod tests;
