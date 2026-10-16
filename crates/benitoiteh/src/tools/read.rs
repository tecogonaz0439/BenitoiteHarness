//! 行番号と続きの位置を付けてテキストを読む（設計書 01-03「read」）。

use super::{FileVersion, ToolContext, ToolError, text};
use serde_json::Value;
use std::io::Read;
use std::path::Path;

pub(super) const MAX_BYTES: usize = 50 * 1024;

pub(super) fn execute(context: &ToolContext, arguments: &Value) -> Result<String, ToolError> {
    let path = string(arguments, "path")?;
    let offset = number(arguments, "offset", 1, false)?;
    let limit = number(arguments, "limit", 2000, false)?.min(2000);
    let mut opened = context.open_file(Path::new(path), false)?;
    if is_image(&opened.real_path) {
        return Err(ToolError::NotText);
    }
    let version = FileVersion::from_metadata(&opened.file.metadata()?);
    let mut bytes = Vec::new();
    opened.file.read_to_end(&mut bytes)?;
    if binary(&bytes) {
        return Err(ToolError::NotText);
    }
    let input = String::from_utf8_lossy(&bytes);
    let lines: Vec<_> = input.lines().collect();
    // 空のファイルの既定の読み取りだけは成功させる。
    if offset > u64::try_from(lines.len()).unwrap_or(u64::MAX) && !(lines.is_empty() && offset == 1)
    {
        return Err(ToolError::Offset {
            offset,
            total: lines.len(),
        });
    }
    let start = usize::try_from(offset.saturating_sub(1)).map_err(|_| ToolError::Offset {
        offset,
        total: lines.len(),
    })?;
    let mut output = String::new();
    let mut last = start;
    for (index, line) in lines
        .iter()
        .enumerate()
        .skip(start)
        .take(usize::try_from(limit).unwrap_or(2000))
    {
        let line_number = index.saturating_add(1);
        let count = line.chars().count();
        let contents: String = line.chars().take(2000).collect();
        let mut record = text::numbered(line_number, &contents);
        if count > 2000 {
            record.push_str(&text::long_line(line_number, count));
        }
        record.push('\n');
        let footer = if line_number < lines.len() {
            text::read_continuation(start.saturating_add(1), line_number, lines.len())
        } else {
            String::new()
        };
        if output
            .len()
            .saturating_add(record.len())
            .saturating_add(footer.len())
            > MAX_BYTES
        {
            break;
        }
        output.push_str(&record);
        last = line_number;
    }
    if last < lines.len() {
        output.push_str(&text::read_continuation(
            start.saturating_add(1),
            last,
            lines.len(),
        ));
    }
    if version != FileVersion::from_metadata(&opened.file.metadata()?) {
        return Err(ToolError::Changed);
    }
    context.record_read_version(opened.real_path, version)?;
    Ok(output)
}

pub(super) fn binary(bytes: &[u8]) -> bool {
    bytes.iter().take(8 * 1024).any(|byte| *byte == 0)
}

pub(super) fn string<'a>(arguments: &'a Value, name: &str) -> Result<&'a str, ToolError> {
    arguments
        .get(name)
        .and_then(Value::as_str)
        .ok_or_else(|| ToolError::InvalidArguments(text::missing_argument(name)))
}

pub(super) fn number(
    arguments: &Value,
    name: &str,
    default: u64,
    zero_allowed: bool,
) -> Result<u64, ToolError> {
    let Some(value) = arguments.get(name) else {
        return Ok(default);
    };
    let number = value
        .as_u64()
        .filter(|number| zero_allowed || *number > 0)
        .ok_or_else(|| ToolError::InvalidArguments(text::positive(name, zero_allowed)))?;
    Ok(number)
}

fn is_image(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| {
            [
                "png", "jpg", "jpeg", "gif", "webp", "bmp", "tif", "tiff", "ico", "avif", "heic",
                "svg",
            ]
            .contains(&ext.to_ascii_lowercase().as_str())
        })
}
