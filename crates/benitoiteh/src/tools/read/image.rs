//! 画像の見出しを確かめ、上限を超える PNG と JPEG を縮めて保持する（設計書 01-03「画像のファイルの読み取り」）。

use std::io::{Cursor, Read};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard};

use image::codecs::jpeg::JpegEncoder;
use image::codecs::png::PngEncoder;
use image::imageops::FilterType;
use image::{DynamicImage, ImageDecoder, ImageFormat, ImageReader, Limits};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::provider::ToolImage;
use crate::tools::{
    FileVersion, MAX_IMAGE_BYTES, MAX_IMAGE_DIMENSION, ToolContext, ToolError, ToolOutput, text,
};

const MAX_RESIZE_PIXELS: u64 = 40_000_000;
const MAX_DECODED_BYTES: u64 = 160_000_000;
static RESIZE: Mutex<()> = Mutex::new(());

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
    prefix: Vec<u8>,
    format: Format,
    cancelled: &AtomicBool,
) -> Result<ToolOutput, ToolError> {
    if arguments.get("offset").is_some() || arguments.get("limit").is_some() {
        return Err(invalid(text::IMAGE_LINES));
    }
    if !context.image_input {
        return Err(invalid(text::IMAGE_UNSUPPORTED));
    }
    let limit = if matches!(format, Format::Png | Format::Jpeg) {
        super::MAX_FILE_BYTES
    } else {
        MAX_IMAGE_BYTES
    };
    let size = version.size;
    if size > limit {
        return Err(invalid(text::image_too_large(size, limit)));
    }
    // 大きいファイルが錠の前で最大八つ分たまらないよう、寸法を調べる前でも
    // 縮小すると分かるものは読み込みから直列にする（設計書 01-03「画像のファイルの読み取り」）。
    let mut resize_guard = if matches!(format, Format::Png | Format::Jpeg) && size > MAX_IMAGE_BYTES
    {
        Some(lock_resize(cancelled)?)
    } else {
        None
    };
    let mut bytes = prefix;
    let remaining = limit
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
    if size > limit {
        return Err(invalid(text::image_too_large(size, limit)));
    }
    let (format, _, original_width, original_height) = header(&bytes)?;
    let resized = matches!(format, Format::Png | Format::Jpeg)
        && (original_width > MAX_IMAGE_DIMENSION
            || original_height > MAX_IMAGE_DIMENSION
            || size > MAX_IMAGE_BYTES);
    if resized {
        if u64::from(original_width).saturating_mul(u64::from(original_height)) > MAX_RESIZE_PIXELS
        {
            return Err(invalid(text::image_pixels(
                original_width,
                original_height,
                MAX_RESIZE_PIXELS,
            )));
        }
        let format = match format {
            Format::Png => ImageFormat::Png,
            Format::Jpeg => ImageFormat::Jpeg,
            Format::Gif | Format::Webp => return Err(invalid(text::IMAGE_INVALID)),
        };
        if resize_guard.is_none() {
            resize_guard = Some(lock_resize(cancelled)?);
        }
        bytes = resize(&bytes, format, original_width, original_height, cancelled)?;
    }
    drop(resize_guard);
    if resized {
        check_cancelled(cancelled)?;
    }
    // 保存した画像を再開時にも受け付けるよう、変換後も同じ規則で検査する
    // （設計書 01-03「画像のファイルの読み取り」）。
    let (media_type, width, height) = inspect(&bytes)?;
    let size = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
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
    let body = if resized {
        text::read_resized_image(
            media_type,
            width,
            height,
            size,
            original_width,
            original_height,
        )
    } else {
        text::read_image(media_type, width, height, size)
    };
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

/// 読み取りと再開で同じ形式・寸法の制限を確かめる。
pub(crate) fn inspect(bytes: &[u8]) -> Result<(&'static str, u32, u32), ToolError> {
    let (_, media_type, width, height) = header(bytes)?;
    if width > MAX_IMAGE_DIMENSION || height > MAX_IMAGE_DIMENSION {
        return Err(invalid(text::image_dimensions(
            width,
            height,
            MAX_IMAGE_DIMENSION,
        )));
    }
    Ok((media_type, width, height))
}

fn header(bytes: &[u8]) -> Result<(Format, &'static str, u32, u32), ToolError> {
    let format = detect(bytes).ok_or_else(|| invalid(text::IMAGE_INVALID))?;
    let (media_type, dimensions) = match format {
        Format::Png => ("image/png", png(bytes)?),
        Format::Jpeg => ("image/jpeg", jpeg(bytes)),
        Format::Gif => ("image/gif", le16(bytes, 6).zip(le16(bytes, 8))),
        Format::Webp => ("image/webp", webp(bytes)),
    };
    let (width, height) = dimensions.ok_or_else(|| invalid(text::IMAGE_INVALID))?;
    if width == 0 || height == 0 {
        return Err(invalid(text::image_dimensions(
            width,
            height,
            MAX_IMAGE_DIMENSION,
        )));
    }
    Ok((format, media_type, width, height))
}

fn resize(
    bytes: &[u8],
    format: ImageFormat,
    width: u32,
    height: u32,
    cancelled: &AtomicBool,
) -> Result<Vec<u8>, ToolError> {
    let mut reader = ImageReader::with_format(Cursor::new(bytes), format);
    let mut limits = Limits::default();
    limits.max_image_width = Some(width);
    limits.max_image_height = Some(height);
    // PNG のメタデータと行のバッファの制限を、出力する画素の制限と分ける
    // （設計書 01-03「画像のファイルの読み取り」）。
    limits.max_alloc = Some(16 * 1024 * 1024);
    reader.limits(limits);
    let mut decoder = reader
        .into_decoder()
        .map_err(|_| invalid(text::IMAGE_DECODE))?;
    if decoder.total_bytes() > MAX_DECODED_BYTES {
        return Err(invalid(text::IMAGE_DECODE));
    }
    // png 0.18.1 は iCCP の展開の失敗も無視する。上限を超えたプロファイルを
    // 黙って落とした場合を拒む（設計書 01-03「画像のファイルの読み取り」）。
    if format == ImageFormat::Png
        && png_has_icc_profile(bytes)
        && decoder
            .icc_profile()
            .map_err(|_| invalid(text::IMAGE_DECODE))?
            .is_none()
    {
        return Err(invalid(text::IMAGE_DECODE));
    }
    let orientation = decoder
        .orientation()
        .map_err(|_| invalid(text::IMAGE_DECODE))?;
    let mut image = DynamicImage::from_decoder(decoder).map_err(|_| invalid(text::IMAGE_DECODE))?;
    check_cancelled(cancelled)?;
    if image.width() > MAX_IMAGE_DIMENSION || image.height() > MAX_IMAGE_DIMENSION {
        image = image.resize(
            MAX_IMAGE_DIMENSION,
            MAX_IMAGE_DIMENSION,
            FilterType::Triangle,
        );
    }
    check_cancelled(cancelled)?;
    image.apply_orientation(orientation);
    let mut output = Vec::new();
    if has_transparency(&image) {
        DynamicImage::ImageRgba8(image.into_rgba8())
            .write_with_encoder(PngEncoder::new(&mut output))
            .map_err(|_| invalid(text::IMAGE_DECODE))?;
    } else {
        let image = DynamicImage::ImageRgb8(image.into_rgb8());
        for quality in [80, 60, 40] {
            output.clear();
            image
                .write_with_encoder(JpegEncoder::new_with_quality(&mut output, quality))
                .map_err(|_| invalid(text::IMAGE_DECODE))?;
            if u64::try_from(output.len()).unwrap_or(u64::MAX) <= MAX_IMAGE_BYTES {
                return Ok(output);
            }
        }
    }
    let size = u64::try_from(output.len()).unwrap_or(u64::MAX);
    if size > MAX_IMAGE_BYTES {
        return Err(invalid(text::image_too_large_after_resizing(
            size,
            MAX_IMAGE_BYTES,
        )));
    }
    Ok(output)
}

fn lock_resize(cancelled: &AtomicBool) -> Result<MutexGuard<'static, ()>, ToolError> {
    // 画像は共有しないので、毒された錠でも処理を続けられる。待っている間の
    // 取り消しも取得直後に見る（設計書 01-03「画像のファイルの読み取り」）。
    let guard = RESIZE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    check_cancelled(cancelled)?;
    Ok(guard)
}

fn check_cancelled(cancelled: &AtomicBool) -> Result<(), ToolError> {
    if cancelled.load(Ordering::Acquire) {
        return Err(invalid(text::IMAGE_CANCELLED));
    }
    Ok(())
}

fn png_has_icc_profile(bytes: &[u8]) -> bool {
    let mut offset = 8_usize;
    while let Some(length) = be32(bytes, offset).and_then(|n| usize::try_from(n).ok()) {
        let Some(kind) = array::<4>(bytes, offset.saturating_add(4)) else {
            return false;
        };
        if &kind == b"IDAT" {
            return false;
        }
        if &kind == b"iCCP" {
            return true;
        }
        let Some(next) = offset.checked_add(12).and_then(|n| n.checked_add(length)) else {
            return false;
        };
        offset = next;
    }
    false
}

fn has_transparency(image: &DynamicImage) -> bool {
    if !image.color().has_alpha() {
        return false;
    }
    if let Some(rgba) = image.as_rgba8() {
        rgba.pixels().any(|pixel| pixel.0[3] < u8::MAX)
    } else if let Some(luma) = image.as_luma_alpha8() {
        luma.pixels().any(|pixel| pixel.0[1] < u8::MAX)
    } else {
        image
            .to_rgba16()
            .pixels()
            .any(|pixel| pixel.0[3] < u16::MAX)
    }
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
