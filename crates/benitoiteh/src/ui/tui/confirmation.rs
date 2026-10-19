//! 確認の表示と入力の守りを共有する（設計書 03-03「確認の共通の規則」、05-02「確認の画面」）。

use std::time::{Duration, Instant};

use crossterm::event::{Event, KeyCode, KeyEventKind, KeyModifiers};

use crate::approval::{ApprovalAnswer, ApprovalKind, ApprovalOutcome, ApprovalPrompt};

use super::{
    Action, Screen,
    control::{Decoration, Line, SafeText, plain},
    format, text,
};

/// 表示が完了した時刻から判定する。時計は呼び出す側が渡す。
#[derive(Default)]
pub struct ConfirmationGuard {
    shown: Option<Instant>,
}
impl ConfirmationGuard {
    /// write と flush が成功してから一度だけ呼ぶ。
    pub fn presented(&mut self, at: Instant) -> bool {
        if self.shown.is_some() {
            return false;
        }
        self.shown = Some(at);
        true
    }
    /// Ctrl+C は時刻によらず受け、キーと貼り付けは 300 ms 待つ。
    pub fn accepts(&self, event: &Event, read_at: Instant) -> bool {
        if is_interrupt(event) {
            return true;
        }
        if matches!(event, Event::Resize(_, _)) {
            return true;
        }
        self.shown.is_some_and(|shown| {
            read_at.saturating_duration_since(shown) >= Duration::from_millis(300)
                && read_at >= shown
        })
    }
}

pub(super) fn is_interrupt(event: &Event) -> bool {
    matches!(event, Event::Key(key) if key.kind == KeyEventKind::Press
        && key.modifiers.contains(KeyModifiers::CONTROL) && matches!(key.code, KeyCode::Char('c' | 'C')))
}

pub(super) struct Confirmation {
    pub prompt: Option<ApprovalPrompt>,
    rewind: Option<RewindConfirmation>,
    secret: Option<SecretConfirmation>,
    pub guard: ConfirmationGuard,
    pub message: Option<String>,
    message_overflow: bool,
}

struct SecretConfirmation {
    prompt: crate::askpass::Prompt,
    value: zeroize::Zeroizing<String>,
    reveal: bool,
    closed: bool,
}

struct RewindConfirmation {
    facts: Vec<(&'static str, String)>,
    extra: Vec<String>,
    incomplete: bool,
    irreversible_count: usize,
    branch: bool,
}

impl Confirmation {
    fn kind(&self) -> Option<ApprovalKind> {
        self.prompt.as_ref().map(|prompt| prompt.request.kind)
    }
    fn available(size: (u16, u16)) -> usize {
        usize::from(size.1)
            .saturating_sub(1)
            .saturating_sub(usize::from(size.1 >= 3))
    }

    fn title(&self) -> &'static str {
        if self.secret.is_some() {
            return text::SECRET_TITLE;
        }
        if let Some(rewind) = &self.rewind {
            if rewind.branch && rewind.incomplete {
                text::INCOMPLETE_BRANCH_TITLE
            } else if rewind.branch {
                text::BRANCH_TITLE
            } else if rewind.incomplete {
                text::INCOMPLETE_TITLE
            } else {
                text::REWIND_TITLE
            }
        } else {
            self.kind().map_or(text::MODE_CHANGE, text::approval_title)
        }
    }

    fn facts(&self, width: u16, compact: bool) -> Vec<FactRow> {
        if let Some(secret) = &self.secret {
            let prompt = &secret.prompt;
            let cwd = SafeText::path(&prompt.facts.cwd);
            let report = text::secret_helper(
                prompt.kind.name(),
                prompt.pid,
                prompt.ppid,
                prompt.ssh_prompt.as_deref(),
            );
            let mut rows = Vec::new();
            for (label, value) in [
                (text::SECRET_HELPER, report.as_str()),
                (text::SECRET_CALL, prompt.facts.call_id.as_str()),
                (text::COMMAND, prompt.facts.command.as_str()),
                (text::DIRECTORY, cwd.as_str()),
                (text::SECRET_MODE, prompt.facts.mode),
                (text::WARNING, text::SECRET_WARNING),
            ] {
                rows.extend(fact(label, value, width, compact));
            }
            return rows;
        }
        if let Some(rewind) = &self.rewind {
            let mut rows: Vec<_> = rewind
                .facts
                .iter()
                .flat_map(|(label, value)| fact(label, value, width, compact))
                .collect();
            if rewind.irreversible_count > 0 {
                rows.extend(
                    wrapped_plain(
                        &text::irreversible_count(rewind.irreversible_count),
                        width,
                        Decoration::Plain,
                    )
                    .into_iter()
                    .map(|line| FactRow {
                        line,
                        characters: 0,
                    }),
                );
            }
            return rows;
        }
        if let Some(prompt) = &self.prompt {
            let request = &prompt.request;
            let cwd = request.facts.cwd.as_deref().map(SafeText::path);
            let path = request.facts.path.as_deref().map(SafeText::path);
            let mut rows = Vec::new();
            for (label, value) in [
                (text::COMMAND, request.facts.command.as_deref()),
                (text::DIRECTORY, cwd.as_ref().map(SafeText::as_str)),
                (text::PATH, path.as_ref().map(SafeText::as_str)),
                (text::SANDBOX, request.facts.sandbox_policy.as_deref()),
                (text::URL, request.facts.url.as_deref()),
                (text::DOMAIN, request.facts.domain.as_deref()),
            ] {
                if let Some(value) = value {
                    rows.extend(fact(label, value, width, compact));
                }
            }
            if matches!(
                request.kind,
                ApprovalKind::Effects | ApprovalKind::TestEffects
            ) {
                let targets = request
                    .facts
                    .targets
                    .iter()
                    .map(|path| SafeText::path(path).as_str().to_owned())
                    .collect::<Vec<_>>()
                    .join("\n");
                rows.extend(fact(text::TARGETS, &targets, width, compact));
                if request.facts.unread_imports > 0 {
                    rows.extend(fact(
                        text::UNREAD_IMPORTS,
                        &request.facts.unread_imports.to_string(),
                        width,
                        compact,
                    ));
                }
                let mut ordered: Vec<_> = request.facts.effects.iter().collect();
                ordered.sort_by_key(|name| (!request.facts.added_effects.contains(name), *name));
                let effects = if ordered.is_empty() {
                    text::NO_EFFECTS.into()
                } else {
                    ordered
                        .into_iter()
                        .map(|name| {
                            text::effect_line(name, request.facts.added_effects.contains(name))
                        })
                        .collect::<Vec<_>>()
                        .join("\n")
                };
                rows.extend(fact(text::EFFECTS, &effects, width, compact));
                if let Some(restricted) = &request.facts.restricted {
                    rows.extend(fact(
                        text::SANDBOX,
                        &text::sandbox_scope(restricted),
                        width,
                        compact,
                    ));
                    if restricted.process_run {
                        let mut programs = restricted
                            .programs
                            .iter()
                            .map(|path| SafeText::path(path).as_str().to_owned())
                            .collect::<Vec<_>>();
                        programs.push(text::OTHER_PROGRAMS_BLOCKED.into());
                        programs.push(text::PROCESS_RUN_SCOPE.into());
                        if cfg!(target_os = "macos")
                            && restricted.programs.first().is_some_and(|processor| {
                                restricted
                                    .programs
                                    .iter()
                                    .any(|program| program != processor)
                            })
                        {
                            programs.push(text::MACOS_PROCESS_GROUPS.into());
                        }
                        rows.extend(fact(text::PROGRAMS, &programs.join("\n"), width, compact));
                    }
                }
                if request.facts.shell_reference {
                    rows.extend(fact(text::SHELL, text::SHELL_REFERENCE, width, compact));
                }
            }
            rows
        } else {
            fact(text::WARNING, text::MODE_CHANGE_WARNING, width, compact)
        }
    }

    fn answer_keys(&self) -> &'static str {
        if let Some(secret) = &self.secret {
            return if secret.prompt.touch() {
                text::SECRET_TOUCH_KEYS
            } else if secret.prompt.confirm() {
                text::SECRET_CONFIRM_KEYS
            } else {
                text::SECRET_KEYS
            };
        }
        self.rewind.as_ref().map_or_else(
            || text::answer_keys(self.kind()),
            |rewind| {
                if rewind.branch && rewind.incomplete {
                    text::INCOMPLETE_BRANCH_KEYS
                } else if rewind.branch {
                    text::BRANCH_KEYS
                } else if rewind.incomplete {
                    text::INCOMPLETE_KEYS
                } else {
                    text::REWIND_KEYS
                }
            },
        )
    }
    fn decline_keys(&self) -> &'static str {
        if self.secret.is_some() {
            return text::SECRET_DECLINE_KEYS;
        }
        if self.rewind.is_some() {
            text::REWIND_DECLINE
        } else {
            text::decline_keys(self.kind())
        }
    }

    pub fn fits(&self, size: (u16, u16)) -> bool {
        // 空白をまとめる前の全文で判定する。見かけの短さで承認を許さない
        // （設計書 03-03「確認の共通の規則」）。説明の高さは数えない。
        let width = size.0.max(1);
        // 一列しかない本文で二列の文字を代わりの空白にして許可してはならない。
        if content_width(width, "│   ") < 2 {
            return false;
        }
        let facts = self.facts(width, false);
        let widths_fit = facts.iter().all(|row| {
            row.line
                .iter()
                .flat_map(|(value, _)| value.as_str().chars())
                .map(super::unicode::cells)
                .fold(0usize, usize::saturating_add)
                <= usize::from(width)
        });
        widths_fit
            && wrapped_plain(self.title(), width, Decoration::Dim)
                .len()
                .saturating_add(facts.len())
                .saturating_add(usize::from(self.secret.is_some()))
                .saturating_add(usize::from(self.secret.as_ref().is_some_and(|secret| {
                    !secret.prompt.touch() && !secret.prompt.confirm()
                })))
                .saturating_add(wrapped_plain(text::APPROVAL_BORDER, width, Decoration::Dim).len())
                .saturating_add(wrapped_plain(self.answer_keys(), width, Decoration::Error).len())
                <= Self::available(size)
    }

    fn body(&self, size: (u16, u16), key_rows: usize) -> Vec<Line> {
        let width = size.0.max(1);
        let maximum = Self::available(size).saturating_sub(key_rows);
        let mut rows = wrapped_plain(self.title(), width, Decoration::Dim);
        if self.message_overflow {
            rows.extend(wrapped_plain(
                text::MESSAGE_TOO_LONG,
                width,
                Decoration::Error,
            ));
        }
        let facts = self.facts(width, true);
        let border = wrapped_plain(text::APPROVAL_BORDER, width, Decoration::Dim);
        if !self.fits(size) {
            // 通知の桁数で折り返しも変わるため、最も長い通知を先に予約する。
            let full = self.facts(width, false);
            let all_characters = full
                .iter()
                .map(|row| row.characters)
                .fold(0usize, usize::saturating_add);
            let reserve = wrapped_plain(
                &text::facts_do_not_fit(full.len(), all_characters),
                width,
                Decoration::Error,
            )
            .len();
            let capacity = if width < 8 {
                0
            } else {
                maximum
                    .saturating_sub(rows.len())
                    .saturating_sub(border.len())
                    .saturating_sub(reserve)
            };
            let omitted_lines = full.len().saturating_sub(capacity);
            let omitted_characters = full
                .iter()
                .skip(capacity)
                .map(|row| row.characters)
                .fold(0usize, usize::saturating_add);
            rows.extend(facts.into_iter().take(capacity).map(|row| row.line));
            rows.extend(wrapped_plain(
                &text::facts_do_not_fit(omitted_lines, omitted_characters),
                width,
                Decoration::Error,
            ));
            rows.extend(border);
            rows.truncate(maximum);
            return rows;
        }
        if let Some(secret) = &self.secret {
            let spare = maximum
                .saturating_sub(rows.len())
                .saturating_sub(facts.len())
                .saturating_sub(border.len());
            let source = prefixed_details(&secret.prompt.prompt, width);
            let mut details = wrapped_plain(text::SECRET_PROMPT, width, Decoration::Dim);
            if source.len() <= 10 && details.len().saturating_add(source.len()) <= spare {
                details.extend(source);
            } else if spare > 0 {
                // 事実を押し出さず、短い画面でも省略の行を先に予約する。
                details.truncate(spare.saturating_sub(1));
                let shown = source
                    .len()
                    .min(10)
                    .min(spare.saturating_sub(details.len()).saturating_sub(1));
                details.extend(source.iter().take(shown).cloned());
                details.push(vec![(
                    format::clipped(
                        &text::details_omitted(source.len().saturating_sub(shown)),
                        width,
                    ),
                    Decoration::Dim,
                )]);
            }
            rows.extend(details.into_iter().take(spare));
        }
        if let Some(rewind) = &self.rewind {
            let spare = maximum
                .saturating_sub(rows.len())
                .saturating_sub(facts.len())
                .saturating_sub(border.len());
            let extra: Vec<_> = rewind
                .extra
                .iter()
                .flat_map(|value| prefixed_details(value, width))
                .collect();
            if extra.len() <= spare {
                rows.extend(extra);
            } else if spare > 0 {
                let shown = spare.saturating_sub(1);
                let omitted = extra.len().saturating_sub(shown);
                rows.extend(extra.into_iter().take(shown));
                rows.push(vec![(
                    format::clipped(&text::details_omitted(omitted), width),
                    Decoration::Dim,
                )]);
            }
        }
        if let Some(reason) = self
            .prompt
            .as_ref()
            .and_then(|prompt| prompt.request.justification.as_ref())
        {
            let heading = wrapped_plain(text::MODEL_REASON_HEADING, width, Decoration::Dim);
            let omitted = wrapped_plain(text::JUSTIFICATION_OMITTED, width, Decoration::Dim);
            let budget = maximum
                .saturating_sub(rows.len())
                .saturating_sub(facts.len())
                .saturating_sub(border.len())
                .saturating_sub(heading.len())
                .saturating_sub(omitted.len())
                .min(10);
            let mut explanation = Vec::new();
            let safe = SafeText::strict(reason, true);
            let mut truncated = false;
            'source: for source in safe.as_str().split('\n') {
                for line in format::wrapped(
                    &SafeText::strict(source, false),
                    content_width(width, "|   "),
                    usize::MAX,
                )
                .0
                {
                    if explanation.len() >= budget {
                        truncated = true;
                        break 'source;
                    }
                    explanation.push(plain(&format!("|   {line}"), Decoration::Dim));
                }
            }
            // 短い端末では説明の見出しも省き、事実を押し出さない。
            let spare = maximum
                .saturating_sub(rows.len())
                .saturating_sub(facts.len())
                .saturating_sub(border.len());
            let mut reason_rows = heading;
            reason_rows.extend(explanation);
            if truncated {
                reason_rows.extend(omitted);
            }
            rows.extend(reason_rows.into_iter().take(spare));
        }
        rows.extend(facts.into_iter().map(|row| row.line));
        rows.extend(border);
        rows.truncate(maximum);
        rows
    }

    pub fn bottom(&self, size: (u16, u16)) -> (Vec<Line>, (usize, u16)) {
        let width = size.0.max(1);
        let available = Self::available(size);
        let value = self.message.as_ref().map_or_else(
            || {
                if self.fits(size) {
                    self.answer_keys()
                } else {
                    self.decline_keys()
                }
                .to_owned()
            },
            |message| format!("{}{message}", text::DECLINE_MESSAGE),
        );
        if self.secret.as_ref().is_some_and(|secret| secret.closed) {
            return (
                wrapped_plain(text::SECRET_CLOSED, width, Decoration::Error)
                    .into_iter()
                    .take(available)
                    .collect(),
                (0, 0),
            );
        }
        if let Some(secret) = &self.secret {
            let mut keys = wrapped_plain(&value, width, Decoration::Error);
            if !secret.prompt.touch() && !secret.prompt.confirm() {
                let mut display = zeroize::Zeroizing::new(String::with_capacity(
                    crate::askpass::MAX_SECRET.saturating_add(text::SECRET_VALUE.len()),
                ));
                display.push_str(text::SECRET_VALUE);
                if secret.reveal {
                    display.push_str(&secret.value);
                } else {
                    display.extend(std::iter::repeat_n('*', secret.value.chars().count()));
                }
                // この作業用の行だけを Zeroizing で消す。format::clipped が作る SafeText、
                // 描画フレーム、端末の写しは消去を保証しない（設計書 03-04「段 2」）。
                keys.push(vec![(format::clipped(&display, width), Decoration::Plain)]);
            }
            keys.truncate(available);
            let mut rows = self.body(size, keys.len());
            let cursor = (rows.len().saturating_add(keys.len()).saturating_sub(1), 0);
            rows.extend(keys);
            rows.truncate(available);
            return (rows, cursor);
        }
        let safe = SafeText::strict(&value, false);
        let (mut keys, mut cursor) = format::wrapped(
            &safe,
            width,
            if self.message.is_some() {
                safe.as_str().len()
            } else {
                0
            },
        );
        if self.message.is_some() {
            keys = vec![keys.pop().unwrap_or_default()];
            cursor.0 = 0;
        }
        // 極端に小さい端末でも下の部分をはみ出させない。断るキーは常に受ける。
        keys.truncate(available);
        let mut rows = self.body(size, keys.len());
        cursor.0 = cursor.0.saturating_add(rows.len());
        rows.extend(keys.into_iter().map(|line| plain(&line, Decoration::Error)));
        rows.truncate(available);
        (rows, cursor)
    }
}

struct FactRow {
    line: Line,
    characters: usize,
}

fn wrapped_plain(value: &str, width: u16, decoration: Decoration) -> Vec<Line> {
    format::wrapped(&SafeText::strict(value, false), width, usize::MAX)
        .0
        .into_iter()
        .map(|line| plain(&line, decoration))
        .collect()
}

// 信頼しない一覧も説明と同じ印で囲み、改行や折り返しで事実・回答の行を作らせない。
fn prefixed_details(value: &str, width: u16) -> Vec<Line> {
    SafeText::strict(value, true)
        .as_str()
        .split('\n')
        .flat_map(|source| {
            format::wrapped(
                &SafeText::strict(source, false),
                content_width(width, "|   "),
                usize::MAX,
            )
            .0
            .into_iter()
            .map(|line| plain(&format!("|   {line}"), Decoration::Dim))
        })
        .collect()
}

fn content_width(width: u16, prefix: &str) -> u16 {
    let columns = prefix
        .chars()
        .map(super::unicode::cells)
        .fold(0usize, usize::saturating_add);
    width
        .saturating_sub(u16::try_from(columns).unwrap_or(u16::MAX))
        .max(1)
}

fn compact_spaces(value: &str) -> String {
    let mut result = String::new();
    let mut spaces = 0usize;
    for ch in value.chars().chain(std::iter::once('\0')) {
        if ch == ' ' {
            spaces = spaces.saturating_add(1);
            continue;
        }
        if spaces >= 8 {
            result.push_str(&text::spaces(spaces));
        } else {
            result.extend(std::iter::repeat_n(' ', spaces));
        }
        spaces = 0;
        if ch != '\0' {
            result.push(ch);
        }
    }
    result
}

fn fact(label: &str, value: &str, width: u16, compact: bool) -> Vec<FactRow> {
    let mut rows: Vec<_> = wrapped_plain(
        &text::fact_heading(label, value.split('\n').count()),
        width,
        Decoration::Dim,
    )
    .into_iter()
    .map(|line| FactRow {
        line,
        characters: 0,
    })
    .collect();
    let safe = SafeText::strict(value, true);
    let value = if compact {
        compact_spaces(safe.as_str())
    } else {
        safe.as_str().to_owned()
    };
    for source in value.split('\n') {
        for line in format::wrapped(
            &SafeText::strict(source, false),
            content_width(width, "│   "),
            usize::MAX,
        )
        .0
        {
            rows.push(FactRow {
                characters: line.chars().count(),
                line: plain(&format!("│   {line}"), Decoration::Plain),
            });
        }
    }
    rows
}

impl Screen {
    /// 値を通常の入力と履歴から切り離して、同じ入力の守りを掛ける。
    pub fn open_secret(&mut self, prompt: crate::askpass::Prompt) {
        self.flush_pending();
        self.confirmation = Some(Confirmation {
            prompt: None,
            rewind: None,
            secret: Some(SecretConfirmation {
                prompt,
                value: zeroize::Zeroizing::new(String::with_capacity(crate::askpass::MAX_SECRET)),
                reveal: false,
                closed: false,
            }),
            guard: ConfirmationGuard::default(),
            message: None,
            message_overflow: false,
        });
    }

    /// 呼び出しの失効やタッチの通知の切断を、描画の前に反映する。
    pub fn close_expired_secret(&mut self) -> bool {
        self.close_expired_secret_at(Instant::now())
    }

    pub(super) fn close_expired_secret_at(&mut self, at: Instant) -> bool {
        let finished = self.finish_secret_results();
        let Some(confirmation) = &mut self.confirmation else {
            return finished;
        };
        let Some(secret) = &mut confirmation.secret else {
            return finished;
        };
        if secret.closed || secret.prompt.is_open() {
            return finished;
        }
        if secret.prompt.touch() {
            self.answer_secret(crate::askpass::Answer::Cancelled, at);
            return true;
        }
        zeroize::Zeroize::zeroize(&mut secret.value);
        secret.reveal = false;
        secret.closed = true;
        confirmation.guard = ConfirmationGuard::default();
        self.discard_secret_input = true;
        true
    }

    pub(super) fn finish_secret_results(&mut self) -> bool {
        let pending = std::mem::take(&mut self.pending_secret_results);
        let mut changed = false;
        for prompt in pending {
            if let Some(outcome) = prompt.outcome() {
                self.notice(&text::secret_history(&prompt.facts.call_id, outcome));
                changed = true;
            } else {
                self.pending_secret_results.push(prompt);
            }
        }
        changed
    }

    fn answer_secret(&mut self, answer: crate::askpass::Answer, at: Instant) {
        if let Some(confirmation) = self.confirmation.take()
            && let Some(secret) = confirmation.secret
        {
            if matches!(answer, crate::askpass::Answer::Cancelled) {
                self.secret_resume_after = at.checked_add(Duration::from_millis(300));
                self.discard_secret_input = true;
            }
            // キューへ送れたことと、補助へ書けたことを同一視しない。
            // 実際の転送の結果を broker が確定してから履歴へ写す。
            let _sent = secret.prompt.reply.send(answer);
            self.pending_secret_results.push(secret.prompt);
            self.finish_secret_results();
        }
    }

    /// 確認前のイベントを読み終えた画面に要求を渡す。
    pub fn open_approval(&mut self, prompt: ApprovalPrompt) {
        self.flush_pending();
        self.confirmation = Some(Confirmation {
            prompt: Some(prompt),
            rewind: None,
            secret: None,
            guard: ConfirmationGuard::default(),
            message: None,
            message_overflow: false,
        });
    }

    /// 再開時のモードの変更にも、同じ入力の守りを掛ける。
    pub fn open_mode_change(&mut self) {
        self.flush_pending();
        self.confirmation_result = None;
        self.confirmation = Some(Confirmation {
            prompt: None,
            rewind: None,
            secret: None,
            guard: ConfirmationGuard::default(),
            message: None,
            message_overflow: false,
        });
    }

    pub(super) fn open_rewind(
        &mut self,
        preview: &crate::agent::RewindPreview,
        incomplete: Option<(String, Vec<String>)>,
    ) {
        self.flush_pending();
        self.confirmation_result = None;
        let mut facts = vec![
            (text::CHECKPOINT, text::checkpoint_row(&preview.selected)),
            (text::SCOPE, text::scope(preview.scope).into()),
        ];
        if preview.scope != crate::rewind::RewindScope::Conversation {
            facts.push((text::BACKUP, text::BACKUP_NOTICE.into()));
        }
        if !preview.selected.checkpoint.incomplete.is_empty() {
            facts.push((text::WARNING, text::INCOMPLETE_TARGET.into()));
        }
        self.open_move(facts, &preview.irreversible, incomplete, false);
    }

    pub(super) fn open_branch(
        &mut self,
        preview: &crate::agent::BranchPreview,
        incomplete: Option<(String, Vec<String>)>,
    ) {
        self.flush_pending();
        self.confirmation_result = None;
        let mut facts = vec![
            (text::BRANCH_TARGET, text::branch_target(&preview.selected)),
            (text::SCOPE, text::scope(preview.scope).into()),
        ];
        if let Some(point) = &preview.checkpoint {
            facts.push((text::CHECKPOINT, point.checkpoint.id.clone()));
            if !point.checkpoint.incomplete.is_empty() {
                facts.push((text::WARNING, text::INCOMPLETE_TARGET.into()));
            }
        }
        if preview.scope != crate::rewind::RewindScope::Conversation {
            facts.push((text::BACKUP, text::BACKUP_NOTICE.into()));
        }
        self.open_move(facts, &preview.irreversible, incomplete, true);
    }

    fn open_move(
        &mut self,
        mut facts: Vec<(&'static str, String)>,
        irreversible: &[crate::rewind::IrreversibleOperation],
        incomplete: Option<(String, Vec<String>)>,
        branch: bool,
    ) {
        let mut extra = Vec::new();
        if !irreversible.is_empty() {
            extra.push(text::IRREVERSIBLE.into());
        }
        extra.extend(
            irreversible
                .iter()
                .take(20)
                .map(|operation| operation.summary.clone()),
        );
        if irreversible.len() > 20 {
            extra.push(text::more(irreversible.len().saturating_sub(20)));
        }
        let retry = incomplete.is_some();
        if let Some((id, reasons)) = incomplete {
            facts.push((text::BACKUP, id));
            facts.push((text::WARNING, text::INCOMPLETE_BACKUP.into()));
            extra.push(text::INCOMPLETE_REASONS.into());
            let count = reasons.len();
            extra.extend(reasons.into_iter().take(20));
            if count > 20 {
                extra.push(text::more(count.saturating_sub(20)));
            }
        }
        self.confirmation = Some(Confirmation {
            prompt: None,
            secret: None,
            rewind: Some(RewindConfirmation {
                facts,
                extra,
                incomplete: retry,
                irreversible_count: irreversible.len(),
                branch,
            }),
            guard: ConfirmationGuard::default(),
            message: None,
            message_overflow: false,
        });
    }

    /// 成功した描画の直後に呼び、真なら入力元の待ち行列も捨てる。
    pub fn presented(&mut self, at: Instant) -> bool {
        let discard = std::mem::take(&mut self.discard_secret_input);
        let presented = self
            .confirmation
            .as_mut()
            .is_some_and(|confirmation| confirmation.guard.presented(at));
        discard || presented
    }

    /// 確認中かどうか。
    pub fn confirming(&self) -> bool {
        self.confirmation.is_some()
    }

    /// モードの変更の確かめの結果を一回だけ読む。
    pub fn take_confirmation_result(&mut self) -> Option<bool> {
        self.confirmation_result.take()
    }

    /// 呼び出す側が取り消しの印を立てた後で、待っている Agent へ答えを届ける。
    pub fn cancel_confirmation(&mut self) {
        self.cancel_confirmation_at(Instant::now());
    }

    pub(super) fn cancel_confirmation_at(&mut self, at: Instant) {
        if self
            .confirmation
            .as_ref()
            .is_some_and(|confirmation| confirmation.secret.is_some())
        {
            self.answer_secret(crate::askpass::Answer::Cancelled, at);
        } else {
            self.answer(ApprovalOutcome::Cancelled, None);
        }
    }

    fn answer(&mut self, outcome: ApprovalOutcome, message: Option<String>) {
        if self
            .confirmation
            .as_ref()
            .is_some_and(|confirmation| confirmation.secret.is_some())
        {
            self.answer_secret(crate::askpass::Answer::Cancelled, Instant::now());
            return;
        }
        if let Some(confirmation) = self.confirmation.take() {
            let start = self.history.len();
            self.history.extend(confirmation.body(self.size, 0));
            self.explicit_history.extend(start..self.history.len());
            if let Some(prompt) = confirmation.prompt {
                let _sent = prompt.reply.send(ApprovalAnswer { outcome, message });
            } else {
                self.confirmation_result = Some(outcome == ApprovalOutcome::Approved);
            }
        }
    }

    pub(super) fn confirmation_input(
        &mut self,
        event: Event,
        read_at: Instant,
        now: Duration,
        running: bool,
    ) -> Action {
        self.close_expired_secret_at(read_at);
        let Some(confirmation) = &mut self.confirmation else {
            return Action::None;
        };
        if confirmation
            .secret
            .as_ref()
            .is_some_and(|secret| secret.closed)
        {
            if is_interrupt(&event) {
                self.answer_secret(crate::askpass::Answer::Cancelled, read_at);
                return Action::Cancel;
            }
            if matches!(&event, Event::Key(key) if key.kind == KeyEventKind::Press && (key.code == KeyCode::Esc || (key.code == KeyCode::Char('d') && key.modifiers.contains(KeyModifiers::CONTROL))))
            {
                self.answer_secret(crate::askpass::Answer::Cancelled, read_at);
            }
            return Action::None;
        }
        if !confirmation.guard.accepts(&event, read_at) {
            return Action::None;
        }
        if confirmation.secret.is_some()
            && (is_interrupt(&event)
                || matches!(&event, Event::Key(key) if key.kind == KeyEventKind::Press && key.code == KeyCode::Char('d') && key.modifiers.contains(KeyModifiers::CONTROL)))
        {
            let interrupt = is_interrupt(&event);
            self.answer_secret(crate::askpass::Answer::Cancelled, read_at);
            return if interrupt {
                Action::Cancel
            } else {
                Action::None
            };
        }
        if is_interrupt(&event) {
            return Action::Cancel;
        }
        if matches!(&event, Event::Key(key) if key.kind == KeyEventKind::Press && key.code == KeyCode::Char('d') && key.modifiers.contains(KeyModifiers::CONTROL))
        {
            return if confirmation.fits(self.size) {
                Action::Quit
            } else {
                Action::None
            };
        }
        if confirmation.secret.is_some() {
            return self.secret_input(event, read_at);
        }
        if let Event::Key(key) = &event
            && key.kind == KeyEventKind::Press
            && key.code == KeyCode::Esc
        {
            let action = self
                .input
                .escape(now, running, key.modifiers.contains(KeyModifiers::ALT));
            if action == Action::Cancel {
                return action;
            }
            confirmation.message_overflow = false;
            if confirmation.message.take().is_none() {
                self.answer(ApprovalOutcome::Denied, None);
            }
            return Action::None;
        }
        if let Some(message) = &mut confirmation.message {
            match event {
                Event::Paste(value) => {
                    let value = value.replace("\r\n", " ").replace(['\n', '\r'], " ");
                    if message.len().saturating_add(value.len()) <= 64 * 1024 {
                        message.push_str(&value);
                        confirmation.message_overflow = false;
                    } else {
                        confirmation.message_overflow = true;
                    }
                }
                Event::Key(key) if key.kind == KeyEventKind::Press => {
                    if key.code == KeyCode::Enter && key.modifiers.is_empty() {
                        let message = message.clone();
                        self.answer(ApprovalOutcome::DeniedWithMessage, Some(message));
                    } else if key.code == KeyCode::Backspace {
                        message.pop();
                        confirmation.message_overflow = false;
                    } else if let KeyCode::Char(ch) = key.code
                        && (key.modifiers & !KeyModifiers::SHIFT).is_empty()
                    {
                        if message.len().saturating_add(ch.len_utf8()) <= 64 * 1024 {
                            message.push(ch);
                            confirmation.message_overflow = false;
                        } else {
                            confirmation.message_overflow = true;
                        }
                    }
                }
                Event::FocusGained
                | Event::FocusLost
                | Event::Key(_)
                | Event::Mouse(_)
                | Event::Resize(_, _) => {}
            }
            return Action::None;
        }
        let Event::Key(key) = event else {
            return Action::None;
        };
        if key.kind != KeyEventKind::Press || !(key.modifiers & !KeyModifiers::SHIFT).is_empty() {
            return Action::None;
        }
        let KeyCode::Char(ch) = key.code else {
            return Action::None;
        };
        match ch.to_ascii_lowercase() {
            'f' if confirmation.fits(self.size)
                && confirmation
                    .rewind
                    .as_ref()
                    .is_some_and(|rewind| rewind.incomplete) =>
            {
                self.answer(ApprovalOutcome::Approved, None)
            }
            'y' if confirmation.fits(self.size)
                && !confirmation
                    .rewind
                    .as_ref()
                    .is_some_and(|rewind| rewind.incomplete) =>
            {
                self.answer(ApprovalOutcome::Approved, None)
            }
            'n' => self.answer(ApprovalOutcome::Denied, None),
            'a' if confirmation.fits(self.size)
                && confirmation.kind() == Some(ApprovalKind::Command) =>
            {
                self.answer(ApprovalOutcome::ApprovedForSession, None)
            }
            's' if confirmation.fits(self.size)
                && confirmation.kind() == Some(ApprovalKind::WebFetch) =>
            {
                self.answer(ApprovalOutcome::ApprovedForSession, None)
            }
            'm' if confirmation.kind().is_some() => confirmation.message = Some(String::new()),
            _ => {}
        }
        Action::None
    }
    fn secret_input(&mut self, event: Event, read_at: Instant) -> Action {
        let Some(confirmation) = &mut self.confirmation else {
            return Action::None;
        };
        let fits = confirmation.fits(self.size);
        let Some(secret) = &mut confirmation.secret else {
            return Action::None;
        };
        if !secret.prompt.is_open() {
            self.close_expired_secret();
            return Action::None;
        }
        match event {
            Event::Paste(value) if !secret.prompt.touch() && !secret.prompt.confirm() => {
                let value = zeroize::Zeroizing::new(value);
                let value = value.trim_end_matches(['\n', '\r']);
                if !value.contains(['\n', '\r', '\0'])
                    && secret.value.len().saturating_add(value.len()) <= 4096
                {
                    secret.value.push_str(value);
                }
            }
            Event::Key(key) if key.kind == KeyEventKind::Press => match key.code {
                KeyCode::Esc => self.answer_secret(crate::askpass::Answer::Cancelled, read_at),
                KeyCode::Char('n' | 'N') if secret.prompt.confirm() => {
                    self.answer_secret(crate::askpass::Answer::Declined, read_at)
                }
                KeyCode::Char('y' | 'Y')
                    if secret.prompt.confirm()
                        && fits
                        && (key.modifiers & !KeyModifiers::SHIFT).is_empty() =>
                {
                    self.answer_secret(crate::askpass::Answer::Yes, read_at)
                }
                KeyCode::Enter
                    if !secret.prompt.confirm()
                        && !secret.prompt.touch()
                        && fits
                        && key.modifiers.is_empty() =>
                {
                    let value = std::mem::replace(
                        &mut secret.value,
                        zeroize::Zeroizing::new(String::new()),
                    );
                    self.answer_secret(crate::askpass::Answer::Secret(value), read_at);
                }
                KeyCode::Tab if !secret.prompt.confirm() && !secret.prompt.touch() => {
                    secret.reveal = !secret.reveal
                }
                KeyCode::Backspace => {
                    secret.value.pop();
                }
                KeyCode::Char(ch)
                    if !secret.prompt.confirm()
                        && !secret.prompt.touch()
                        && !matches!(ch, '\n' | '\r' | '\0')
                        && (key.modifiers & !KeyModifiers::SHIFT).is_empty() =>
                {
                    if secret.value.len().saturating_add(ch.len_utf8()) <= 4096 {
                        secret.value.push(ch);
                    }
                }
                KeyCode::BackTab
                | KeyCode::Enter
                | KeyCode::Tab
                | KeyCode::Null
                | KeyCode::Char(_)
                | KeyCode::Left
                | KeyCode::Right
                | KeyCode::Up
                | KeyCode::Down
                | KeyCode::Home
                | KeyCode::End
                | KeyCode::PageUp
                | KeyCode::PageDown
                | KeyCode::Delete
                | KeyCode::Insert
                | KeyCode::F(_)
                | KeyCode::CapsLock
                | KeyCode::ScrollLock
                | KeyCode::NumLock
                | KeyCode::PrintScreen
                | KeyCode::Pause
                | KeyCode::Menu
                | KeyCode::KeypadBegin
                | KeyCode::Media(_)
                | KeyCode::Modifier(_) => {}
            },
            Event::FocusGained
            | Event::FocusLost
            | Event::Key(_)
            | Event::Mouse(_)
            | Event::Resize(_, _)
            | Event::Paste(_) => {}
        }
        Action::None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    // 関門: 入力の上限まで追加しても、平文を入れた領域の再確保を起こさない。
    #[test]
    fn secret_buffer_keeps_the_same_allocation_up_to_its_limit()
    -> Result<(), Box<dyn std::error::Error>> {
        let f = crate::askpass::tests::Fixture::new()?;
        let receiver = f.broker.take_requests().ok_or("missing receiver")?;
        let _stream = f.request(None, "password")?;
        let mut screen = Screen::new(
            (120, 60),
            super::super::Status {
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
        screen.open_secret(receiver.recv()?);
        let shown = Instant::now();
        screen.presented(shown);
        let allocation = screen
            .confirmation
            .as_ref()
            .and_then(|c| c.secret.as_ref())
            .ok_or("missing secret")?
            .value
            .as_ptr();
        for _ in 0..8 {
            screen.input_at(
                Event::Paste("x".repeat(512)),
                shown + Duration::from_secs(1),
                Duration::ZERO,
                true,
            );
            let secret = screen
                .confirmation
                .as_ref()
                .and_then(|c| c.secret.as_ref())
                .ok_or("missing secret")?;
            assert_eq!(secret.value.as_ptr(), allocation);
            assert!(secret.value.capacity() >= crate::askpass::MAX_SECRET);
        }
        assert_eq!(
            screen
                .confirmation
                .as_ref()
                .and_then(|c| c.secret.as_ref())
                .ok_or("missing secret")?
                .value
                .len(),
            crate::askpass::MAX_SECRET
        );
        screen.cancel_confirmation();
        Ok(())
    }
}
