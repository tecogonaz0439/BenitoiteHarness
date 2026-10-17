//! 安全にした文へ行ごとの装飾を加える（設計書 05-02「出力の示し方」）。

use serde_json::Value;
use unicode_width::UnicodeWidthChar;

use super::control::{Decoration, Line, SafeText, plain};
use super::text;

pub(super) fn clipped(value: &str, width: u16) -> SafeText {
    let value = SafeText::new(value, false);
    let mut columns = 0usize;
    SafeText::new(
        &value
            .as_str()
            .chars()
            .take_while(|ch| {
                columns = columns.saturating_add(ch.width().unwrap_or(0));
                columns <= usize::from(width)
            })
            .collect::<String>(),
        false,
    )
}

/// 折り返し位置とカーソルは、制御文字の可視化とタブの展開の後の幅で数える。
pub(super) fn wrapped(value: &SafeText, width: u16, cursor: usize) -> (Vec<String>, (usize, u16)) {
    let width = usize::from(width.max(1));
    let mut rows = vec![String::new()];
    let mut row = 0usize;
    let mut column = 0usize;
    let mut position = (0, 0);
    for (index, ch) in value.as_str().char_indices() {
        let cells = ch.width().unwrap_or(0).min(width);
        if ch != '\n' && column.saturating_add(cells) > width {
            rows.push(String::new());
            row = row.saturating_add(1);
            column = 0;
        }
        if index == cursor {
            position = (row, u16::try_from(column).unwrap_or(u16::MAX));
        }
        if ch == '\n' {
            rows.push(String::new());
            row = row.saturating_add(1);
            column = 0;
        } else {
            if let Some(line) = rows.last_mut() {
                // 幅が一列しかない端末では二列の文字を空白にして、端末任せの折り返しを防ぐ。
                if ch.width().unwrap_or(0) > width {
                    line.push(' ');
                } else {
                    line.push(ch);
                }
            }
            column = column.saturating_add(cells);
        }
    }
    if cursor == value.as_str().len() {
        if column >= width {
            rows.push(String::new());
            row = row.saturating_add(1);
            column = 0;
        }
        position = (row, u16::try_from(column).unwrap_or(u16::MAX));
    }
    (rows, position)
}

#[derive(Default)]
pub(super) struct Markdown {
    fence: Option<(char, usize)>,
}

impl Markdown {
    pub fn line(&mut self, value: &SafeText) -> Line {
        let line = value.as_str();
        let trimmed = line.trim_start();
        if let Some(marker @ ('`' | '~')) = trimmed.chars().next() {
            let count = trimmed.chars().take_while(|ch| *ch == marker).count();
            if count >= 3 {
                match self.fence {
                    Some((prior, length))
                        if marker == prior
                            && count >= length
                            && trimmed
                                .get(count..)
                                .is_some_and(|tail| tail.trim().is_empty()) =>
                    {
                        self.fence = None
                    }
                    None => self.fence = Some((marker, count)),
                    Some(_) => {}
                }
                return plain(line, Decoration::Plain);
            }
        }
        if self.fence.is_some() {
            return plain(line, Decoration::Plain);
        }
        let default = if trimmed.starts_with('#') {
            Decoration::Bold
        } else {
            Decoration::Plain
        };
        inline(line, default)
    }
}

// 対になった印だけを先に決める。コード内の星印はトークンにしない。
// 再帰を使わず、信頼しない長い行でも呼び出しの深さを増やさない（設計書 05-02）。
fn inline(line: &str, default: Decoration) -> Line {
    let mut tokens: Vec<(&str, Option<usize>, bool)> = Vec::new();
    let trimmed = line.trim_start();
    let mut remaining = line;
    if trimmed.starts_with("* ") {
        let length = line.len().saturating_sub(trimmed.len()).saturating_add(2);
        tokens.push((line.get(..length).unwrap_or_default(), None, false));
        remaining = line.get(length..).unwrap_or_default();
    }
    while !remaining.is_empty() {
        if let Some(tail) = remaining.strip_prefix('`')
            && let Some(end) = tail.find('`')
        {
            tokens.push((tail.get(..end).unwrap_or_default(), None, true));
            remaining = tail.get(end.saturating_add(1)..).unwrap_or_default();
            continue;
        }
        if remaining.starts_with('*') {
            let length = remaining.chars().take_while(|ch| *ch == '*').count();
            tokens.push((
                remaining.get(..length).unwrap_or_default(),
                (length <= 2).then_some(length),
                false,
            ));
            remaining = remaining.get(length..).unwrap_or_default();
        } else {
            let length = remaining
                .char_indices()
                .skip(1)
                .find(|(_, ch)| matches!(ch, '*' | '`'))
                .map_or(remaining.len(), |(index, _)| index);
            tokens.push((remaining.get(..length).unwrap_or_default(), None, false));
            remaining = remaining.get(length..).unwrap_or_default();
        }
    }
    let mut paired = vec![false; tokens.len()];
    for marker in [1, 2] {
        let mut open = None;
        for (index, (_, kind, _)) in tokens.iter().enumerate() {
            if *kind != Some(marker) {
                continue;
            }
            if let Some(prior) = open.take() {
                if let Some(value) = paired.get_mut(prior) {
                    *value = true;
                }
                if let Some(value) = paired.get_mut(index) {
                    *value = true;
                }
            } else {
                open = Some(index);
            }
        }
    }
    let mut single = false;
    let mut double = false;
    let mut spans: Vec<(String, Decoration)> = Vec::new();
    for (index, (content, kind, code)) in tokens.into_iter().enumerate() {
        if paired.get(index) == Some(&true) {
            if kind == Some(1) {
                single = !single;
            } else {
                double = !double;
            }
            continue;
        }
        let style = if code {
            Decoration::Code
        } else if single || double {
            Decoration::Bold
        } else {
            default
        };
        if let Some((text, prior_style)) = spans.last_mut()
            && *prior_style == style
        {
            text.push_str(content);
        } else {
            spans.push((content.to_owned(), style));
        }
    }
    spans
        .into_iter()
        .map(|(text, style)| (SafeText::new(&text, false), style))
        .collect()
}

pub(super) fn tool_call(name: &str, arguments: &Value, width: u16) -> Line {
    let value = text::tool_call(name, arguments);
    vec![(clipped(&value, width), Decoration::Dim)]
}

pub(super) fn diff(value: &str) -> Vec<Line> {
    // 先に制御文字を変える。差分の見出しにあるパスも例外にしない。
    let safe = SafeText::new(value, true);
    let mut lines: Vec<_> = safe
        .as_str()
        .lines()
        .take(200)
        .map(|line| {
            plain(
                line,
                if line.starts_with('+') {
                    Decoration::Added
                } else if line.starts_with('-') {
                    Decoration::Removed
                } else {
                    Decoration::Plain
                },
            )
        })
        .collect();
    let count = safe.as_str().lines().count();
    if count > 200 {
        lines.push(plain(
            &text::more_lines(count.saturating_sub(200)),
            Decoration::Dim,
        ));
    }
    lines
}
