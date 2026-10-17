//! 折り返さない行の文字を可視化し、幅を控えめに見積もらない（設計書 05-02「出力の示し方」）。

use std::sync::LazyLock;

use regex::RegexSet;
use unicode_width::UnicodeWidthChar;

// 既存の regex の Unicode の表を使う。手書きの表で Cf や未割り当てを漏らさない。
// 表の構築に失敗しても、非 ASCII を可視化して承認の表示を安全側に保つ。
static PROPERTIES: LazyLock<Result<RegexSet, regex::Error>> = LazyLock::new(|| {
    RegexSet::new([
        r"[\p{Default_Ignorable_Code_Point}\p{Cf}\p{Variation_Selector}\x{115f}\x{1160}\x{3164}\x{ffa0}]",
        r"\p{Cn}",
        r"\p{Extended_Pictographic}",
    ])
});

fn property(ch: char, index: usize) -> bool {
    let mut bytes = [0; 4];
    match &*PROPERTIES {
        Ok(properties) => properties
            .matches(ch.encode_utf8(&mut bytes))
            .matched(index),
        Err(_) => !ch.is_ascii(),
    }
}

pub(super) fn visible(value: &str) -> String {
    let mut result = String::with_capacity(value.len());
    for ch in value.chars() {
        if property(ch, 0) {
            result.push_str(&format!("\\u{{{:x}}}", u32::from(ch)));
        } else {
            result.push(ch);
        }
    }
    result
}

pub(super) fn cells(ch: char) -> usize {
    // 単独の地域指示子や文字として描く絵文字も、二列使う端末に合わせる。
    if matches!(ch, '\u{1f1e6}'..='\u{1f1ff}') || property(ch, 1) || property(ch, 2) {
        2
    } else {
        match ch.width_cjk() {
            // 単独の結合文字を一列で描く端末もあるので、ゼロ幅も下限に使わない。
            Some(0) | None => 2,
            Some(width) => width,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::cells;

    // 関門: 単独の地域指示子と、既定で文字として描く絵文字を一列と数える退行を守る。
    #[test]
    fn regional_indicators_and_extended_pictographs_reserve_two_columns() {
        for ch in ['\u{1f1e6}', '\u{1f1ef}', '\u{1f1ff}', '❤', '☀', '\u{1f5e8}'] {
            assert_eq!(cells(ch), 2, "U+{:04X}", u32::from(ch));
        }
        for ch in ['a', '1', '#'] {
            assert_eq!(cells(ch), 1);
        }
    }
}
