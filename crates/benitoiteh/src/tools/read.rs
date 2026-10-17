//! 行番号と続きの位置を付けてテキストを読む（設計書 01-03「read」）。

use super::{FileVersion, ToolContext, ToolError, text};
use serde_json::Value;
use std::io::Read;
use std::path::Path;

pub(super) const MAX_BYTES: usize = 50 * 1024;
pub(super) const MAX_FILE_BYTES: u64 = 64 * 1024 * 1024;

pub(super) fn execute(context: &ToolContext, arguments: &Value) -> Result<String, ToolError> {
    let path = string(arguments, "path")?;
    let offset = number(arguments, "offset", 1, false)?;
    let limit = number(arguments, "limit", 2000, false)?.min(2000);
    let mut opened = context.open_file(Path::new(path), false)?;
    let metadata = opened.file.metadata()?;
    if metadata.len() > MAX_FILE_BYTES {
        return Err(ToolError::InvalidArguments(text::READ_TOO_LARGE.into()));
    }
    if is_image(&opened.real_path) {
        return Err(ToolError::NotText);
    }
    let version = FileVersion::from_metadata(&metadata);
    let mut lines = ReadLines {
        offset,
        limit,
        total: 0,
        count: 0,
        prefix: String::new(),
        last_cr: false,
        records: Vec::new(),
        bytes: 0,
        collecting: true,
    };
    let mut buffer = [0; 8192];
    let mut pending = Vec::new();
    let mut bytes_read = 0u64;
    loop {
        let count = opened.file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        let part = buffer.get(..count).unwrap_or_default();
        if bytes_read < 8192
            && binary(
                part.get(
                    ..8192usize
                        .saturating_sub(usize::try_from(bytes_read).unwrap_or(8192))
                        .min(count),
                )
                .unwrap_or_default(),
            )
        {
            return Err(ToolError::NotText);
        }
        bytes_read = bytes_read.saturating_add(u64::try_from(count).unwrap_or(u64::MAX));
        if bytes_read > MAX_FILE_BYTES {
            return Err(ToolError::InvalidArguments(text::READ_TOO_LARGE.into()));
        }
        pending.extend_from_slice(part);
        // 塊の境の未完の UTF-8 だけを次へ回し、置換文字を余分に作らない。
        // 長い行も先頭の 2,000 文字だけを保つ（設計書 01-03「read」）。
        let mut end = pending.len();
        let mut checked = 0usize;
        while let Some(rest) = pending.get(checked..) {
            match std::str::from_utf8(rest) {
                Ok(_) => break,
                Err(error) => {
                    checked = checked.saturating_add(error.valid_up_to());
                    match error.error_len() {
                        Some(length) => checked = checked.saturating_add(length),
                        None => {
                            end = checked;
                            break;
                        }
                    }
                }
            }
        }
        for ch in String::from_utf8_lossy(pending.get(..end).unwrap_or_default()).chars() {
            lines.character(ch);
        }
        pending.drain(..end);
    }
    for ch in String::from_utf8_lossy(&pending).chars() {
        lines.character(ch);
    }
    if lines.count > 0 {
        lines.finish(false);
    }
    if offset > u64::try_from(lines.total).unwrap_or(u64::MAX) && !(lines.total == 0 && offset == 1)
    {
        return Err(ToolError::Offset {
            offset,
            total: lines.total,
        });
    }
    let start = usize::try_from(offset.saturating_sub(1)).map_err(|_| ToolError::Offset {
        offset,
        total: lines.total,
    })?;
    let mut output = String::new();
    let mut last = start;
    for (number, record) in lines.records {
        let footer = if number < lines.total {
            text::read_continuation(start.saturating_add(1), number, lines.total)
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
        last = number;
    }
    if last < lines.total {
        output.push_str(&text::read_continuation(
            start.saturating_add(1),
            last,
            lines.total,
        ));
    }
    if version != FileVersion::from_metadata(&opened.file.metadata()?) {
        return Err(ToolError::Changed);
    }
    context.record_read_version(opened.real_path, version)?;
    Ok(output)
}

struct ReadLines {
    offset: u64,
    limit: u64,
    total: usize,
    count: usize,
    prefix: String,
    last_cr: bool,
    records: Vec<(usize, String)>,
    bytes: usize,
    collecting: bool,
}
impl ReadLines {
    fn selected(&self) -> bool {
        let number = u64::try_from(self.total)
            .unwrap_or(u64::MAX)
            .saturating_add(1);
        self.collecting && number >= self.offset && number.saturating_sub(self.offset) < self.limit
    }
    fn character(&mut self, ch: char) {
        if ch == '\n' {
            self.finish(true);
            return;
        }
        self.count = self.count.saturating_add(1);
        if self.selected() && self.count <= 2000 {
            self.prefix.push(ch);
        }
        self.last_cr = ch == '\r';
    }
    fn finish(&mut self, newline: bool) {
        if newline && self.last_cr {
            if self.count <= 2000 {
                self.prefix.pop();
            }
            self.count = self.count.saturating_sub(1);
        }
        if self.selected() {
            let number = self.total.saturating_add(1);
            let mut record = text::numbered(number, &self.prefix);
            if self.count > 2000 {
                record.push_str(&text::long_line(number, self.count));
            }
            record.push('\n');
            self.bytes = self.bytes.saturating_add(record.len());
            if self.bytes <= MAX_BYTES {
                self.records.push((number, record));
            } else {
                self.collecting = false;
            }
        }
        self.total = self.total.saturating_add(1);
        self.prefix.clear();
        self.count = 0;
        self.last_cr = false;
    }
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
