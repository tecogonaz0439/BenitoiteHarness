//! 診断とテスト結果の文字列を、偽の行を作れない表示へ変える（設計書 04-01）。
use crate::tools::text;

fn escaped(c: char, quoted: bool) -> String {
    match c {
        '\n' => "\\n".into(),
        '"' if quoted => "\\\"".into(),
        '\\' if quoted => "\\\\".into(),
        c if c.is_control()
            || matches!(c, '\u{2028}' | '\u{2029}' | '\u{61c}' | '\u{200e}' | '\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}') =>
        {
            if quoted {
                format!("\\u{:04x}", u32::from(c))
            } else {
                format!("\\u{{{:x}}}", u32::from(c))
            }
        }
        c => c.to_string(),
    }
}
fn bounded(value: &str, quoted: bool, bytes: usize) -> String {
    let mut result = if quoted {
        String::from("\"")
    } else {
        String::new()
    };
    let mut count = usize::from(quoted);
    let reserve = text::TEST_TRUNCATED
        .len()
        .saturating_add(usize::from(quoted));
    for c in value.chars() {
        let part = escaped(c, quoted);
        count = count.saturating_add(part.chars().count());
        if count > 1980
            || result
                .len()
                .saturating_add(part.len())
                .saturating_add(reserve)
                > bytes
        {
            result.push_str(text::TEST_TRUNCATED);
            break;
        }
        result.push_str(&part);
    }
    if quoted {
        result.push('"');
    }
    result
}
pub(crate) fn one_line(value: &str) -> String {
    bounded(value, false, 8000)
}
pub(crate) fn quoted(value: &str) -> String {
    bounded(value, true, 8000)
}
pub(crate) fn line_with_budget(value: &str, bytes: usize) -> String {
    bounded(value, false, bytes)
}
pub(crate) fn quoted_with_budget(value: &str, bytes: usize) -> String {
    bounded(value, true, bytes)
}

// 引用するコードの字下げだけはタブを保ち、偽の行と双方向の表示を作らせない。
pub(crate) fn source_line(value: &str) -> String {
    value
        .chars()
        .take(2000)
        .map(|c| {
            if c == '\t' {
                "\t".into()
            } else {
                escaped(c, false)
            }
        })
        .collect()
}
