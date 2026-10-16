//! UTF-8 の境界を保って道具の出力を制限する（設計書 01-03「出力の切り詰め」）。

use super::text;

/// すべての道具の結果に掛けるバイト数の上限。
pub const MAX_OUTPUT_BYTES: usize = 64 * 1024;

/// 切り詰めた後に残す側。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Side {
    Head,
    Tail,
}

/// 切り詰めた本文と、捨てた量。
#[derive(Debug, Eq, PartialEq)]
pub struct Truncated {
    pub text: String,
    pub truncated: bool,
    pub omitted_bytes: usize,
}

/// 行数とバイト数で先に達した上限まで残し、末尾に指定された印を付ける。
/// 印もバイト数の上限に含める。
pub fn truncate(
    input: &str,
    side: Side,
    max_lines: usize,
    max_bytes: usize,
    marker: &str,
) -> Truncated {
    let by_lines = match side {
        Side::Head => input
            .split_inclusive('\n')
            .take(max_lines)
            .map(str::len)
            .sum::<usize>(),
        Side::Tail => input
            .split_inclusive('\n')
            .rev()
            .take(max_lines)
            .map(str::len)
            .sum::<usize>(),
    };
    if input.len() <= max_bytes && by_lines >= input.len() {
        return Truncated {
            text: input.to_owned(),
            truncated: false,
            omitted_bytes: 0,
        };
    }
    let marker = prefix(marker, max_bytes);
    let budget = max_bytes
        .saturating_sub(marker.len())
        .saturating_sub(1)
        .min(by_lines);
    let kept = match side {
        Side::Head => prefix(input, budget),
        Side::Tail => suffix(input, budget),
    };
    let mut output = kept.to_owned();
    if !output.is_empty()
        && !output.ends_with('\n')
        && output.len() < max_bytes.saturating_sub(marker.len())
    {
        output.push('\n');
    }
    output.push_str(marker);
    Truncated {
        text: output,
        truncated: true,
        omitted_bytes: input.len().saturating_sub(kept.len()),
    }
}

/// 64 KiB を超える結果の先頭と末尾を残し、中ほどを除いたバイト数を添える。
pub fn cap_middle(input: &str) -> String {
    if input.len() <= MAX_OUTPUT_BYTES {
        return input.to_owned();
    }
    // 印の桁数を見積もった余白を取り、UTF-8 の境界に寄せた後の実際の量を示す。
    let budget = MAX_OUTPUT_BYTES.saturating_sub(text::middle_removed(input.len()).len());
    let head = prefix(input, budget / 2);
    let tail = suffix(input, budget.saturating_sub(head.len()));
    let removed = input
        .len()
        .saturating_sub(head.len())
        .saturating_sub(tail.len());
    format!("{head}{}{tail}", text::middle_removed(removed))
}

pub(super) fn prefix(input: &str, bytes: usize) -> &str {
    let mut end = bytes.min(input.len());
    while !input.is_char_boundary(end) {
        end = end.saturating_sub(1);
    }
    input.get(..end).unwrap_or_default()
}

fn suffix(input: &str, bytes: usize) -> &str {
    let mut start = input.len().saturating_sub(bytes);
    while !input.is_char_boundary(start) {
        start = start.saturating_add(1);
    }
    input.get(start..).unwrap_or_default()
}
