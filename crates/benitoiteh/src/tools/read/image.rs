//! 画像を解かずに見出しを読み、安全な大きさの中身だけを保持する（設計書 01-03「画像のファイルの読み取り」）。

use std::io::Read;

use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::provider::ToolImage;
use crate::tools::{
    FileVersion, MAX_IMAGE_BYTES, MAX_IMAGE_DIMENSION, ToolContext, ToolError, ToolOutput, text,
};

#[derive(Clone, Copy)]
pub(super) enum Format {
    Png,
    Jpeg,
    Gif,
    Webp,
}

pub(super) fn detect(prefix: &[u8]) -> Option<Format> {
    if prefix.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some(Format::Png)
    } else if prefix.starts_with(b"\xff\xd8\xff") {
        Some(Format::Jpeg)
    } else if prefix.starts_with(b"GIF87a") || prefix.starts_with(b"GIF89a") {
        Some(Format::Gif)
    } else if prefix.starts_with(b"RIFF") && prefix.get(8..12) == Some(b"WEBP") {
        Some(Format::Webp)
    } else {
        None
    }
}

pub(super) fn execute(
    context: &ToolContext,
    arguments: &Value,
    mut opened: crate::tools::paths::OpenedFile,
    version: FileVersion,
    size: u64,
    prefix: Vec<u8>,
    format: Format,
) -> Result<ToolOutput, ToolError> {
    if arguments.get("offset").is_some() || arguments.get("limit").is_some() {
        return Err(invalid(text::IMAGE_LINES));
    }
    if !context.image_input {
        return Err(invalid(text::IMAGE_UNSUPPORTED));
    }
    if size > MAX_IMAGE_BYTES {
        return Err(invalid(text::image_too_large(size, MAX_IMAGE_BYTES)));
    }
    let mut bytes = prefix;
    let remaining = MAX_IMAGE_BYTES
        .saturating_add(1)
        .saturating_sub(u64::try_from(bytes.len()).unwrap_or(u64::MAX));
    opened
        .file
        .by_ref()
        .take(remaining)
        .read_to_end(&mut bytes)?;
    if version != FileVersion::from_metadata(&opened.file.metadata()?) {
        return Err(ToolError::Changed);
    }
    let size = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
    if size > MAX_IMAGE_BYTES {
        return Err(invalid(text::image_too_large(size, MAX_IMAGE_BYTES)));
    }
    let (media_type, dimensions) = match format {
        Format::Png => ("image/png", png(&bytes)?),
        Format::Jpeg => ("image/jpeg", jpeg(&bytes)),
        Format::Gif => ("image/gif", le16(&bytes, 6).zip(le16(&bytes, 8))),
        Format::Webp => ("image/webp", webp(&bytes)),
    };
    let (width, height) = dimensions.ok_or_else(|| invalid(text::IMAGE_INVALID))?;
    if width == 0 || height == 0 || width > MAX_IMAGE_DIMENSION || height > MAX_IMAGE_DIMENSION {
        return Err(invalid(text::image_dimensions(
            width,
            height,
            MAX_IMAGE_DIMENSION,
        )));
    }
    let sha256 = if let Some(session) = &context.session {
        let stored = session
            .store_object(&bytes)
            .map_err(|error| invalid(error.to_string()))?;
        stored
            .ok_or_else(|| invalid(crate::session::text::STORAGE_LIMIT))?
            .0
    } else {
        Sha256::digest(&bytes)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect()
    };
    context.record_read_version(opened.real_path, version)?;
    let body = text::read_image(media_type, width, height, size);
    Ok(ToolOutput {
        screen: Some(body.clone()),
        images: vec![ToolImage {
            media_type: media_type.into(),
            sha256,
            width,
            height,
            bytes: size,
            data: Some(bytes.into()),
        }],
        ..ToolOutput::success(body)
    })
}

fn invalid(message: impl Into<String>) -> ToolError {
    ToolError::InvalidArguments(message.into())
}

fn array<const N: usize>(bytes: &[u8], offset: usize) -> Option<[u8; N]> {
    bytes.get(offset..offset.checked_add(N)?)?.try_into().ok()
}
fn be16(bytes: &[u8], offset: usize) -> Option<u32> {
    Some(u32::from(u16::from_be_bytes(array(bytes, offset)?)))
}
fn le16(bytes: &[u8], offset: usize) -> Option<u32> {
    Some(u32::from(u16::from_le_bytes(array(bytes, offset)?)))
}
fn be32(bytes: &[u8], offset: usize) -> Option<u32> {
    Some(u32::from_be_bytes(array(bytes, offset)?))
}
fn le32(bytes: &[u8], offset: usize) -> Option<u32> {
    Some(u32::from_le_bytes(array(bytes, offset)?))
}
fn le24(bytes: &[u8], offset: usize) -> Option<u32> {
    let [a, b, c] = array(bytes, offset)?;
    Some(u32::from(a) | u32::from(b) << 8 | u32::from(c) << 16)
}

fn png(bytes: &[u8]) -> Result<Option<(u32, u32)>, ToolError> {
    if be32(bytes, 8) != Some(13) || bytes.get(12..16) != Some(b"IHDR") {
        return Ok(None);
    }
    let dimensions = be32(bytes, 16).zip(be32(bytes, 20));
    let mut offset = 8_usize;
    while offset < bytes.len() {
        let Some(length) = be32(bytes, offset).and_then(|n| usize::try_from(n).ok()) else {
            return Ok(None);
        };
        let Some(kind) = bytes.get(offset.saturating_add(4)..offset.saturating_add(8)) else {
            return Ok(None);
        };
        let Some(end) = offset.checked_add(12).and_then(|n| n.checked_add(length)) else {
            return Ok(None);
        };
        if end > bytes.len() {
            return Ok(None);
        }
        if kind == b"acTL" {
            return Err(invalid(text::IMAGE_ANIMATED_PNG));
        }
        if kind == b"IDAT" {
            return Ok(dimensions);
        }
        offset = end;
    }
    Ok(dimensions)
}

fn jpeg(bytes: &[u8]) -> Option<(u32, u32)> {
    let mut offset = 2_usize;
    loop {
        if bytes.get(offset) != Some(&0xff) {
            return None;
        }
        while bytes.get(offset) == Some(&0xff) {
            offset = offset.checked_add(1)?;
        }
        let marker = *bytes.get(offset)?;
        offset = offset.checked_add(1)?;
        if matches!(marker, 0xd0..=0xd8 | 0x01) {
            continue;
        }
        if matches!(marker, 0x00 | 0xd9 | 0xda) {
            return None;
        }
        let length = usize::try_from(be16(bytes, offset)?).ok()?;
        if length < 2 {
            return None;
        }
        let end = offset.checked_add(length)?;
        let segment = bytes.get(offset..end)?;
        if (0xc0..=0xcf).contains(&marker) && ![0xc4, 0xc8, 0xcc].contains(&marker) {
            if length < 8 {
                return None;
            }
            return be16(segment, 5).zip(be16(segment, 3));
        }
        offset = end;
    }
}

fn webp(bytes: &[u8]) -> Option<(u32, u32)> {
    let end = usize::try_from(le32(bytes, 4)?).ok()?.checked_add(8)?;
    let bytes = bytes.get(..end)?;
    let mut offset = 12_usize;
    while offset < bytes.len() {
        let kind = bytes.get(offset..offset.checked_add(4)?)?;
        let length = usize::try_from(le32(bytes, offset.checked_add(4)?)?).ok()?;
        let start = offset.checked_add(8)?;
        let end = start.checked_add(length)?;
        let chunk = bytes.get(start..end)?;
        match kind {
            b"VP8 " => {
                if chunk.get(3..6) != Some(b"\x9d\x01\x2a") {
                    return None;
                }
                return Some((le16(chunk, 6)? & 0x3fff, le16(chunk, 8)? & 0x3fff));
            }
            b"VP8L" => {
                if chunk.first() != Some(&0x2f) {
                    return None;
                }
                let bits = le32(chunk, 1)?;
                return Some((
                    (bits & 0x3fff).saturating_add(1),
                    ((bits >> 14) & 0x3fff).saturating_add(1),
                ));
            }
            b"VP8X" => {
                if length < 10 {
                    return None;
                }
                return Some((
                    le24(chunk, 4)?.saturating_add(1),
                    le24(chunk, 7)?.saturating_add(1),
                ));
            }
            _ => {}
        }
        offset = end.checked_add(length % 2)?;
    }
    None
}
