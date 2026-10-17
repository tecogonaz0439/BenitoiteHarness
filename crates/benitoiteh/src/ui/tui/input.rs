//! 端末を持たずに入力と履歴を管理する（設計書 05-02「入力の操作」、実装プラン P4-06）。

use std::time::Duration;

use crossterm::event::{Event, KeyCode, KeyEventKind, KeyModifiers};

/// 入力の結果。処理中の Enter は入力欄を保つ。
#[derive(Debug, PartialEq)]
pub enum Action {
    /// 入力またはカーソルだけを更新した。
    None,
    /// 新しい依頼。
    Submit(String),
    /// 処理中に加える操縦。
    Steer(String),
    /// 処理の後に送る依頼。
    FollowUp(String),
    /// 入力欄からだけ実行できるコマンド。
    Command(super::commands::Command),
    /// 道具の段階だけを取り消す。
    CancelStep,
    /// 今の依頼の取り消し。
    Cancel,
    /// 取り消して終える。
    Quit,
}

#[derive(Default)]
pub(super) struct Input {
    pub text: String,
    pub cursor: usize,
    pub quit_hint: bool,
    history: Vec<String>,
    history_index: Option<usize>,
    draft: String,
    last_interrupt: Option<Duration>,
    cancelling: bool,
    last_escape: Option<Duration>,
}

impl Input {
    pub fn remember(&mut self, request: &str) {
        self.history.push(request.to_owned());
    }

    pub fn complete(&mut self) {
        self.cancelling = false;
    }

    pub fn event(&mut self, event: Event, now: Duration, running: bool) -> Action {
        match event {
            Event::Paste(text) => {
                self.reset_interrupt();
                self.insert(&text.replace("\r\n", "\n").replace('\r', "\n"));
            }
            Event::Key(key) if key.kind == KeyEventKind::Press => {
                let control = key.modifiers.contains(KeyModifiers::CONTROL);
                if key.code == KeyCode::Esc {
                    return self.escape(now, running, key.modifiers.contains(KeyModifiers::ALT));
                }
                if control && key.code == KeyCode::Char('c') {
                    if running {
                        if self.cancelling {
                            return Action::Quit;
                        }
                        self.cancelling = true;
                        return Action::Cancel;
                    }
                    if !self.text.is_empty() {
                        self.text.clear();
                        self.cursor = 0;
                        self.reset_interrupt();
                    } else if self
                        .last_interrupt
                        .is_some_and(|prior| now.saturating_sub(prior) <= Duration::from_secs(1))
                    {
                        return Action::Quit;
                    } else {
                        self.last_interrupt = Some(now);
                        self.quit_hint = true;
                    }
                    return Action::None;
                }
                self.reset_interrupt();
                if control && key.code == KeyCode::Char('d') && self.text.is_empty() {
                    return Action::Quit;
                }
                if key.modifiers.contains(KeyModifiers::ALT) && key.code != KeyCode::Enter {
                    return Action::None;
                }
                match key.code {
                    KeyCode::Enter if key.modifiers.contains(KeyModifiers::SHIFT) => {
                        self.insert("\n")
                    }
                    KeyCode::Char('j') if control => self.insert("\n"),
                    KeyCode::Enter if !self.text.trim().is_empty() => {
                        let request = std::mem::take(&mut self.text);
                        self.cursor = 0;
                        self.history_index = None;
                        self.draft.clear();
                        return if !running {
                            Action::Submit(request)
                        } else if key.modifiers.contains(KeyModifiers::ALT) {
                            Action::FollowUp(request)
                        } else {
                            Action::Steer(request)
                        };
                    }
                    KeyCode::Char(ch) if !control => self.insert(&ch.to_string()),
                    KeyCode::Left => self.cursor = self.previous(),
                    KeyCode::Right => self.cursor = self.next(),
                    KeyCode::Backspace => {
                        let previous = self.previous();
                        self.text.drain(previous..self.cursor);
                        self.cursor = previous;
                    }
                    KeyCode::Delete => {
                        self.text.drain(self.cursor..self.next());
                    }
                    KeyCode::Home => {
                        self.cursor = self
                            .text
                            .get(..self.cursor)
                            .unwrap_or_default()
                            .rfind('\n')
                            .map_or(0, |index| index.saturating_add(1));
                    }
                    KeyCode::End => {
                        self.cursor = self
                            .text
                            .get(self.cursor..)
                            .unwrap_or_default()
                            .find('\n')
                            .map_or(self.text.len(), |offset| self.cursor.saturating_add(offset));
                    }
                    KeyCode::Up if !self.text.contains('\n') => self.history(true),
                    KeyCode::Down if !self.text.contains('\n') => self.history(false),
                    KeyCode::Enter
                    | KeyCode::Up
                    | KeyCode::Down
                    | KeyCode::Char(_)
                    | KeyCode::PageUp
                    | KeyCode::PageDown
                    | KeyCode::Tab
                    | KeyCode::BackTab
                    | KeyCode::Insert
                    | KeyCode::F(_)
                    | KeyCode::Null
                    | KeyCode::Esc
                    | KeyCode::CapsLock
                    | KeyCode::ScrollLock
                    | KeyCode::NumLock
                    | KeyCode::PrintScreen
                    | KeyCode::Pause
                    | KeyCode::Menu
                    | KeyCode::KeypadBegin
                    | KeyCode::Media(_)
                    | KeyCode::Modifier(_) => {}
                }
            }
            Event::Key(_)
            | Event::FocusGained
            | Event::FocusLost
            | Event::Mouse(_)
            | Event::Resize(_, _) => {}
        }
        Action::None
    }

    pub fn escape(&mut self, now: Duration, running: bool, doubled: bool) -> Action {
        let twice = doubled
            || self
                .last_escape
                .is_some_and(|prior| now.saturating_sub(prior) <= Duration::from_secs(1));
        self.last_escape = Some(now);
        if running && twice {
            self.cancelling = true;
            Action::Cancel
        } else if running {
            Action::CancelStep
        } else {
            Action::None
        }
    }

    pub fn restore(&mut self, values: impl IntoIterator<Item = String>) {
        let mut values: Vec<_> = values.into_iter().collect();
        if !self.text.is_empty() {
            values.push(std::mem::take(&mut self.text));
        }
        self.text = values.join("\n");
        self.cursor = self.text.len();
    }

    fn reset_interrupt(&mut self) {
        self.last_interrupt = None;
        self.quit_hint = false;
    }

    fn insert(&mut self, text: &str) {
        self.text.insert_str(self.cursor, text);
        self.cursor = self.cursor.saturating_add(text.len());
    }

    fn previous(&self) -> usize {
        self.text
            .get(..self.cursor)
            .unwrap_or_default()
            .char_indices()
            .next_back()
            .map_or(0, |(index, _)| index)
    }

    fn next(&self) -> usize {
        self.text
            .get(self.cursor..)
            .unwrap_or_default()
            .chars()
            .next()
            .map_or(self.cursor, |ch| self.cursor.saturating_add(ch.len_utf8()))
    }

    fn history(&mut self, up: bool) {
        if up {
            if self.history_index.is_none() {
                self.draft = self.text.clone();
            }
            self.history_index = self
                .history_index
                .unwrap_or(self.history.len())
                .checked_sub(1)
                .or(self.history_index);
        } else if let Some(index) = self.history_index {
            self.history_index =
                (index.saturating_add(1) < self.history.len()).then_some(index.saturating_add(1));
        } else {
            return;
        }
        self.text = self
            .history_index
            .and_then(|index| self.history.get(index))
            .unwrap_or(&self.draft)
            .clone();
        self.cursor = self.text.len();
    }
}
