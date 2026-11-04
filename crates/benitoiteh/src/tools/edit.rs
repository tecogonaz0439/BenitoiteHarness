//! 字面と寄せた形で置換し、対象外のバイト列を保つ（設計書 01-03「edit」）。

use std::ops::Range;
use std::path::Path;

use super::{
    ApprovalStatus, ToolContext, ToolError, ToolOutput, read::string, text, write,
    write_path::WriteTarget,
};
use serde_json::Value;

/// 一つの置き換え。
struct Replacement<'a> {
    old: &'a str,
    new: &'a str,
    all: bool,
}

/// 置き換える範囲と、どの置き換えから来たか。
struct Located {
    range: Range<usize>,
    index: usize,
}

pub(super) fn execute(
    context: &ToolContext,
    arguments: &Value,
    approval: ApprovalStatus,
    invocation: super::Invocation<'_>,
) -> Result<ToolOutput, ToolError> {
    let path = string(arguments, "path")?;
    // `edits` は評価用の複数の置き換えの形だけが受ける（設計書 01-03「複数の置き換え」）。
    // 引数の検証が道具の定義にない欄を拒むので、`edits` があれば、その形の道具である。
    let multiple = arguments.get("edits").is_some();
    let replacements = if let Some(edits) = arguments.get("edits") {
        let edits = edits.as_array().ok_or(ToolError::State)?;
        if edits.is_empty() {
            return Err(ToolError::InvalidArguments(text::EDITS_EMPTY.to_owned()));
        }
        edits
            .iter()
            .map(replacement)
            .collect::<Result<Vec<_>, _>>()?
    } else {
        vec![replacement(arguments)?]
    };
    let target = WriteTarget::resolve(context, Path::new(path))?;
    let version = target.version()?;
    if let Some(output) = target.approval(context, approval)? {
        return Ok(output);
    }
    let version = version.ok_or(ToolError::NotRead)?;
    target.check_read(context, &version)?;
    let bytes = target.read(&version)?;
    let before = std::str::from_utf8(&bytes).map_err(|_| ToolError::NotText)?;
    let body = before.strip_prefix('\u{feff}').unwrap_or(before);
    // 一つの形の誤りの文は変えず、複数の形ではどの要素かを先に示す。
    let failure = |index: usize, message: String| {
        ToolError::InvalidArguments(if multiple {
            text::edit_item(index, &message)
        } else {
            message
        })
    };
    let mut located = Vec::new();
    let mut replaced = Vec::with_capacity(replacements.len());
    for (index, replacement) in replacements.iter().enumerate() {
        if replacement.old.is_empty() {
            return Err(failure(index, text::EMPTY_OLD.to_owned()));
        }
        // 複数の形では、変えない要素があっても全体が変われば書く（Pi の edit-diff.ts に倣う）。
        if !multiple && replacement.old == replacement.new {
            return Err(ToolError::InvalidArguments(text::edit_unchanged(path)));
        }
        let ranges = locate(body, replacement.old)?;
        if ranges.is_empty() {
            return Err(failure(index, text::edit_not_found(path)));
        }
        if ranges.len() > 1 && !replacement.all {
            return Err(failure(index, text::edit_multiple(path, ranges.len())));
        }
        located.extend(ranges.into_iter().map(|range| Located { range, index }));
        replaced.push(line_endings(replacement.new, body));
    }
    // どの要素も元のファイルで照合し、重なる範囲を拒む。理由: 前の置き換えの結果に
    // 次の要素が合う形にすると、要素の順で結果が変わり、LLM が予想しにくい。
    located.sort_by_key(|item| (item.range.start, item.index));
    for pair in located.windows(2) {
        if let [previous, current] = pair
            && previous.range.end > current.range.start
        {
            return Err(ToolError::InvalidArguments(text::edit_overlap(
                previous.index.min(current.index),
                previous.index.max(current.index),
                path,
            )));
        }
    }
    let mut after = String::new();
    if before.starts_with('\u{feff}') {
        after.push('\u{feff}');
    }
    let mut cursor = 0;
    for item in located {
        after.push_str(body.get(cursor..item.range.start).ok_or(ToolError::State)?);
        after.push_str(replaced.get(item.index).ok_or(ToolError::State)?);
        cursor = item.range.end;
    }
    after.push_str(body.get(cursor..).ok_or(ToolError::State)?);
    if after == before {
        return Err(ToolError::InvalidArguments(text::edit_unchanged(path)));
    }
    context.checkpoint_before(
        crate::rewind::CheckpointReason::Edit,
        Some(&target.real_path),
        &invocation,
    )?;
    let real_path = target.real_path.clone();
    target.write(context, Some(version), after.as_bytes())?;
    Ok(super::benitoite_check::after_write(
        context,
        path,
        &real_path,
        write::output(path, before, &after),
        invocation.cancelled,
    ))
}

fn replacement(value: &Value) -> Result<Replacement<'_>, ToolError> {
    Ok(Replacement {
        old: string(value, "old_string")?,
        new: string(value, "new_string")?,
        all: value
            .get("replace_all")
            .and_then(Value::as_bool)
            .unwrap_or(false),
    })
}

/// 字面で探し、なければ寄せた形で探して、元のバイト列の範囲を返す。
fn locate(body: &str, old: &str) -> Result<Vec<Range<usize>>, ToolError> {
    let exact = matches(body, old);
    if !exact.is_empty() {
        return Ok(exact);
    }
    let normalized = Normalized::new(body);
    let needle = Normalized::new(old);
    matches_nonempty(&normalized.text, &needle.text)
        .into_iter()
        .map(|range| {
            let start = normalized
                .original
                .get(range.start)
                .ok_or(ToolError::State)?
                .start;
            let end = normalized
                .original
                .get(range.end.saturating_sub(1))
                .ok_or(ToolError::State)?
                .end;
            Ok(start..end)
        })
        .collect()
}

fn matches(input: &str, needle: &str) -> Vec<Range<usize>> {
    input
        .match_indices(needle)
        .map(|(start, found)| start..start.saturating_add(found.len()))
        .collect()
}

fn matches_nonempty(input: &str, needle: &str) -> Vec<Range<usize>> {
    if needle.is_empty() {
        Vec::new()
    } else {
        matches(input, needle)
    }
}

fn line_endings(new: &str, original: &str) -> String {
    let lf = new.replace("\r\n", "\n").replace('\r', "\n");
    let crlf = original
        .find('\n')
        .is_some_and(|index| original.as_bytes().get(index.saturating_sub(1)) == Some(&b'\r'));
    if crlf { lf.replace('\n', "\r\n") } else { lf }
}

struct Normalized {
    text: String,
    original: Vec<Range<usize>>,
}

impl Normalized {
    fn new(input: &str) -> Self {
        let mut units: Vec<(char, Range<usize>)> = Vec::new();
        let mut chars = input.char_indices().peekable();
        while let Some((start, ch)) = chars.next() {
            let mut end = start.saturating_add(ch.len_utf8());
            let ch = if ch == '\r' {
                if chars.peek().is_some_and(|(_, ch)| *ch == '\n')
                    && let Some((index, _)) = chars.next()
                {
                    end = index.saturating_add(1);
                }
                '\n'
            } else {
                ch
            };
            if ch == '\n' {
                trim_line_end(&mut units);
            }
            units.push((fold(ch), start..end));
        }
        trim_line_end(&mut units);
        let mut text = String::new();
        let mut original = Vec::new();
        // 変換後の各バイトを元の文字の範囲に対応付け、未変更の行を正規化しない
        // （設計書 01-03「edit」）。
        for (ch, range) in units {
            text.push(ch);
            original.extend(std::iter::repeat_n(range, ch.len_utf8()));
        }
        Self { text, original }
    }
}

fn trim_line_end(units: &mut Vec<(char, Range<usize>)>) {
    while units
        .last()
        .is_some_and(|(ch, _)| *ch != '\n' && ch.is_whitespace())
    {
        units.pop();
    }
}

fn fold(ch: char) -> char {
    // Pi 4954d760504c11df9b2cbdc040346107b6a8153d の edit-diff.ts の
    // 引用符・ダッシュの一覧に揃える（設計書 08-01「システムプロンプトと道具」）。
    match ch {
        '\u{2018}'..='\u{201b}' => '\'',
        '\u{201c}'..='\u{201f}' => '"',
        '\u{2010}'..='\u{2015}' | '\u{2212}' => '-',
        other => other,
    }
}
