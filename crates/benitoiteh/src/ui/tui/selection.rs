//! 巻き戻しの時点と範囲は利用者のキーだけで選ぶ（設計書 01-05「巻き戻し」、05-02）。

use super::{
    Action, Command, Screen,
    control::{Decoration, Line, SafeText},
    format, text,
};
use crate::agent::{BranchInfo, BranchPreview, BranchTreeRow, RewindPreview};
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
    Branches {
        rows: Vec<BranchTreeRow>,
        index: usize,
    },
    BranchScope {
        row: BranchTreeRow,
        index: usize,
    },
}

pub(super) enum PendingMove {
    Rewind(RewindPreview),
    Branch(BranchPreview),
}

impl PendingMove {
    pub fn cancelled(&self) -> &'static str {
        match self {
            Self::Rewind(_) => text::REWIND_CANCELLED,
            Self::Branch(_) => text::BRANCH_CANCELLED,
        }
    }
}
impl Selection {
    fn count(&self) -> usize {
        match self {
            Self::Checkpoints { points, .. } => points.len(),
            Self::Scope { point, .. } => scopes(point).len(),
            Self::Branches { rows, .. } => rows.len(),
            Self::BranchScope { row, .. } => row
                .branch
                .as_ref()
                .map_or(0, |branch| branch_scopes(branch).len()),
        }
    }
    fn option(&self, position: usize) -> Option<Line> {
        let value = match self {
            Self::Checkpoints { points, .. } => points.get(position).map(text::checkpoint_row),
            Self::Scope { point, .. } => scopes(point)
                .get(position)
                .map(|scope| text::scope(*scope).to_owned()),
            Self::Branches { rows, .. } => {
                return rows.get(position).map(|row| text::tree_row(&row.row));
            }
            Self::BranchScope { row, .. } => row
                .branch
                .as_ref()
                .and_then(|branch| branch_scopes(branch).get(position).copied())
                .map(|scope| text::scope(scope).to_owned()),
        }?;
        Some(vec![(SafeText::strict(&value, false), Decoration::Plain)])
    }
    pub fn bottom(&self, size: (u16, u16)) -> Vec<Line> {
        let maximum = usize::from(size.1)
            .saturating_sub(1)
            .saturating_sub(usize::from(size.1 >= 3));
        let (title, index) = match self {
            Self::Checkpoints { index, .. } => (text::SELECT_CHECKPOINT, *index),
            Self::Scope { index, .. } => (text::SELECT_SCOPE, *index),
            Self::Branches { index, .. } => (text::SELECT_BRANCH, *index),
            Self::BranchScope { index, .. } => (text::SELECT_BRANCH_SCOPE, *index),
        };
        let line = |value: &str| vec![(format::clipped(value, size.0), Decoration::Plain)];
        let mut rows = vec![line(title)];
        let capacity = maximum.saturating_sub(2);
        rows.extend(window(self.count(), index, capacity, |position, limit| {
            let mut option = vec![(
                SafeText::strict(&text::selection_row(position == index, ""), false),
                Decoration::Dim,
            )];
            option.extend(self.option(position).unwrap_or_default());
            format::wrapped_line(option, size.0, limit)
        }));
        rows.push(line(text::SELECT_KEYS));
        rows.truncate(maximum);
        rows
    }
}
// 高さが分かった行だけを保持し、選択行の前後の窓の外は折り返さない。
fn window(
    count: usize,
    index: usize,
    capacity: usize,
    mut row: impl FnMut(usize, usize) -> Vec<Line>,
) -> Vec<Line> {
    if count == 0 || capacity == 0 {
        return Vec::new();
    }
    let index = index.min(count.saturating_sub(1));
    let selected = row(index, capacity.saturating_add(1));
    let mut used = selected.len();
    let mut visible = std::collections::VecDeque::from([selected]);
    let mut start = index;
    while used < capacity {
        let Some(previous) = start.checked_sub(1) else {
            break;
        };
        let remaining = capacity.saturating_sub(used);
        let lines = row(previous, remaining.saturating_add(1));
        if lines.len() > remaining {
            break;
        }
        used = used.saturating_add(lines.len());
        visible.push_front(lines);
        start = previous;
    }
    let mut next = index.saturating_add(1);
    while used < capacity && next < count {
        let lines = row(next, capacity.saturating_sub(used));
        used = used.saturating_add(lines.len());
        visible.push_back(lines);
        next = next.saturating_add(1);
    }
    visible.into_iter().flatten().take(capacity).collect()
}

fn branch_scopes(branch: &BranchInfo) -> Vec<RewindScope> {
    if branch.checkpoint.is_some() {
        vec![RewindScope::FilesAndConversation, RewindScope::Conversation]
    } else {
        vec![RewindScope::Conversation]
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
    pub(super) fn select_branches(&mut self, rows: Vec<BranchTreeRow>) {
        self.flush_pending();
        if rows.is_empty() {
            self.notice(text::NO_BRANCHES);
            return;
        }
        let index = rows
            .iter()
            .position(|row| row.row.is_current)
            .unwrap_or_default();
        self.selection = Some(Selection::Branches { rows, index });
    }
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
            let cancelled = if matches!(
                self.selection,
                Some(Selection::Branches { .. } | Selection::BranchScope { .. })
            ) {
                text::BRANCH_CANCELLED
            } else {
                text::REWIND_CANCELLED
            };
            self.selection = None;
            self.notice(cancelled);
            return Action::None;
        }
        let Some(mut selection) = self.selection.take() else {
            return Action::None;
        };
        let count = selection.count();
        let index = match &mut selection {
            Selection::Checkpoints { index, .. }
            | Selection::Scope { index, .. }
            | Selection::Branches { index, .. }
            | Selection::BranchScope { index, .. } => index,
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
                Selection::Branches { rows, index } => {
                    if let Some(row) = rows.get(index)
                        && row.branch.is_some()
                    {
                        self.selection = Some(Selection::BranchScope {
                            row: row.clone(),
                            index: 0,
                        });
                    }
                }
                Selection::BranchScope { row, index } => {
                    if let Some(branch) = &row.branch
                        && let Some(scope) = branch_scopes(branch).get(index)
                    {
                        return Action::Command(Command::PreviewBranch(branch.target, *scope));
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
        self.move_confirmation(PendingMove::Rewind(preview), incomplete);
    }
    pub(super) fn move_confirmation(
        &mut self,
        preview: PendingMove,
        incomplete: Option<(String, Vec<String>)>,
    ) {
        self.rewind_retry = incomplete.is_some();
        match &preview {
            PendingMove::Rewind(preview) => self.open_rewind(preview, incomplete),
            PendingMove::Branch(preview) => self.open_branch(preview, incomplete),
        }
        self.pending_move = Some(preview);
    }
    pub(super) fn move_result(
        &mut self,
        report: &crate::rewind::RewindReport,
        branch_scope: Option<RewindScope>,
    ) {
        if let Some(request) = &report.request {
            let input = if request.starts_with('/') {
                format!("/{request}")
            } else {
                request.clone()
            };
            self.restore_input(input);
            self.notice(text::REQUEST_RESTORED);
        }
        if let Some(scope) = branch_scope {
            self.notice(&text::branch_result(report, scope));
        } else {
            self.notice(&text::rewind_result(report));
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    // 関門: 見えない行まで折り返す退行を、処理した行の番号と窓の内容で検出する。
    #[test]
    fn window_wraps_only_nearby_options_and_limits_the_last_one() {
        let mut visited = Vec::new();
        let rows = window(10_000, 5000, 5, |position, limit| {
            visited.push(position);
            format::wrapped_line(
                vec![(
                    SafeText::strict(&format!("{position}:{}", "x".repeat(65)), false),
                    Decoration::Plain,
                )],
                40,
                limit,
            )
        });
        assert_eq!(visited, [5000, 4999, 4998, 5001]);
        assert_eq!(rows.len(), 5);
        let text: Vec<_> = rows
            .iter()
            .map(|line| {
                line.iter()
                    .map(|(value, _)| value.as_str())
                    .collect::<String>()
            })
            .collect();
        assert!(text.first().is_some_and(|value| value.starts_with("4999:")));
        assert!(text.get(2).is_some_and(|value| value.starts_with("5000:")));
        assert!(text.last().is_some_and(|value| value.starts_with("5001:")));
    }
}
