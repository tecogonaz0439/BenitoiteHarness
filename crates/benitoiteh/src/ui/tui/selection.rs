//! 巻き戻しの時点と範囲は利用者のキーだけで選ぶ（設計書 01-05「巻き戻し」、05-02）。

use super::{
    Action, Command, Screen,
    control::{Decoration, Line, SafeText},
    format, text,
};
use crate::agent::RewindPreview;
use crate::rewind::{CheckpointInfo, RewindScope};
use crossterm::event::{Event, KeyCode, KeyEventKind};

pub(super) enum Selection {
    Checkpoints {
        points: Vec<CheckpointInfo>,
        index: usize,
    },
    Scope {
        point: Box<CheckpointInfo>,
        index: usize,
    },
}
impl Selection {
    fn options(&self) -> Vec<String> {
        match self {
            Self::Checkpoints { points, .. } => points.iter().map(text::checkpoint_row).collect(),
            Self::Scope { point, .. } => scopes(point)
                .into_iter()
                .map(|scope| text::scope(scope).to_owned())
                .collect(),
        }
    }
    pub fn bottom(&self, size: (u16, u16)) -> Vec<Line> {
        let maximum = usize::from(size.1)
            .saturating_sub(1)
            .saturating_sub(usize::from(size.1 >= 3));
        let (title, index) = match self {
            Self::Checkpoints { index, .. } => (text::SELECT_CHECKPOINT, *index),
            Self::Scope { index, .. } => (text::SELECT_SCOPE, *index),
        };
        let line = |value: &str| vec![(format::clipped(value, size.0), Decoration::Plain)];
        let mut rows = vec![line(title)];
        let capacity = maximum.saturating_sub(2);
        let options = self.options();
        let options: Vec<Vec<Line>> = options
            .iter()
            .enumerate()
            .map(|(position, value)| {
                format::wrapped(
                    &SafeText::strict(&text::selection_row(position == index, value), false),
                    size.0,
                    usize::MAX,
                )
                .0
                .into_iter()
                .map(|value| line(&value))
                .collect()
            })
            .collect();
        let mut start = index;
        let mut used = options.get(index).map_or(0, Vec::len);
        while let Some(previous) = start.checked_sub(1) {
            let height = options.get(previous).map_or(0, Vec::len);
            if used.saturating_add(height) > capacity {
                break;
            }
            used = used.saturating_add(height);
            start = previous;
        }
        rows.extend(options.into_iter().skip(start).flatten().take(capacity));
        rows.push(line(text::SELECT_KEYS));
        rows.truncate(maximum);
        rows
    }
}
fn scopes(point: &CheckpointInfo) -> Vec<RewindScope> {
    if point.conversation_available {
        vec![
            RewindScope::FilesAndConversation,
            RewindScope::Files,
            RewindScope::Conversation,
        ]
    } else {
        vec![RewindScope::Files]
    }
}

impl Screen {
    pub(super) fn select_checkpoints(&mut self, mut points: Vec<CheckpointInfo>) {
        self.flush_pending();
        if points.is_empty() {
            self.notice(text::NO_CHECKPOINTS);
            return;
        }
        points.reverse();
        self.selection = Some(Selection::Checkpoints { points, index: 0 });
    }
    pub(super) fn selection_input(&mut self, event: Event) -> Action {
        if super::confirmation::is_interrupt(&event) {
            self.selection = None;
            return Action::None;
        }
        let Event::Key(key) = event else {
            return Action::None;
        };
        if key.kind != KeyEventKind::Press || !key.modifiers.is_empty() {
            return Action::None;
        }
        if key.code == KeyCode::Esc {
            self.selection = None;
            self.notice(text::REWIND_CANCELLED);
            return Action::None;
        }
        let Some(mut selection) = self.selection.take() else {
            return Action::None;
        };
        let count = selection.options().len();
        let index = match &mut selection {
            Selection::Checkpoints { index, .. } | Selection::Scope { index, .. } => index,
        };
        if key.code == KeyCode::Up {
            *index = index.saturating_sub(1);
        } else if key.code == KeyCode::Down {
            *index = index.saturating_add(1).min(count.saturating_sub(1));
        } else if key.code == KeyCode::Enter {
            match selection {
                Selection::Checkpoints { points, index } => {
                    if let Some(point) = points.get(index) {
                        self.selection = Some(Selection::Scope {
                            point: Box::new(point.clone()),
                            index: 0,
                        });
                    }
                }
                Selection::Scope { point, index } => {
                    if let Some(scope) = scopes(&point).get(index) {
                        return Action::Command(Command::PreviewRewind(
                            point.checkpoint.id.clone(),
                            *scope,
                        ));
                    }
                }
            }
            return Action::None;
        }
        self.selection = Some(selection);
        Action::None
    }
    pub(super) fn rewind_confirmation(
        &mut self,
        preview: RewindPreview,
        incomplete: Option<(String, Vec<String>)>,
    ) {
        self.rewind_retry = incomplete.is_some();
        self.open_rewind(&preview, incomplete);
        self.rewind_preview = Some(preview);
    }
    pub(super) fn rewind_result(&mut self, report: &crate::rewind::RewindReport) {
        self.notice(&text::rewind_result(report));
        for failure in report.failures.iter().take(20) {
            self.notice(&text::rewind_failure(failure));
        }
        if report.failures.len() > 20 {
            self.notice(&text::more(report.failures.len().saturating_sub(20)));
        }
        if !report.irreversible.is_empty() {
            self.notice(text::IRREVERSIBLE);
        }
        for operation in report.irreversible.iter().take(20) {
            self.notice(&operation.summary);
        }
        if report.irreversible.len() > 20 {
            self.notice(&text::more(report.irreversible.len().saturating_sub(20)));
        }
    }
}
