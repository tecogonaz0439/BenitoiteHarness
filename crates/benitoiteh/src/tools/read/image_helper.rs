//! 画像の変換を主のプロセスから隔離する補助の入口（設計書 01-03「画像のファイルの読み取り」）。

use super::{Format, MAX_RESIZE_PIXELS, OutputHeader, array, be32, header, inspect};
use crate::tools::{MAX_IMAGE_BYTES, MAX_IMAGE_DIMENSION};
use image::codecs::jpeg::JpegEncoder;
use image::codecs::png::PngEncoder;
use image::imageops::FilterType;
use image::{DynamicImage, ImageDecoder, ImageFormat, ImageReader, Limits};
use std::ffi::OsString;
use std::io::{self, Cursor, Read, Write};
use std::process::ExitCode;

const MAX_INPUT_BYTES: u64 = 64 * 1024 * 1024;
const MAX_DECODED_BYTES: u64 = 160_000_000;

enum HelperError {
    Arguments,
    #[cfg(target_os = "linux")]
    Memory,
    Input,
    Decode,
    TooLarge(u64),
    Output,
}

/// 通常の CLI の設定と panic hook を通らず、固定のプロトコルだけを扱う。
pub(crate) fn run_helper(args: impl Iterator<Item = OsString>) -> ExitCode {
    match convert(args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            let message = match error {
                HelperError::Arguments => text::ARGUMENTS,
                #[cfg(target_os = "linux")]
                HelperError::Memory => text::MEMORY,
                HelperError::Input => text::INPUT,
                HelperError::Decode => text::DECODE,
                HelperError::TooLarge(size) => {
                    let _shown = writeln!(io::stderr().lock(), "{} {size}", text::TOO_LARGE);
                    return ExitCode::FAILURE;
                }
                HelperError::Output => text::OUTPUT,
            };
            let _shown = writeln!(io::stderr().lock(), "{message}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(target_os = "linux")]
fn limit_memory() -> Result<(), HelperError> {
    use rustix::process::{Resource, Rlimit, setrlimit};
    setrlimit(
        Resource::As,
        Rlimit {
            current: Some(1 << 30),
            maximum: Some(1 << 30),
        },
    )
    .map_err(|_| HelperError::Memory)
}

fn convert(mut args: impl Iterator<Item = OsString>) -> Result<(), HelperError> {
    // macOS は意味のある RLIMIT_AS を拒むため Linux にだけ掛ける（BH-113）。
    #[cfg(target_os = "linux")]
    limit_memory()?;
    let format = match args.next().as_deref().and_then(std::ffi::OsStr::to_str) {
        Some("png") => ImageFormat::Png,
        Some("jpeg") => ImageFormat::Jpeg,
        _ => return Err(HelperError::Arguments),
    };
    let mut dimension = || {
        args.next()
            .and_then(|value| value.to_str()?.parse::<u32>().ok())
            .filter(|value| *value > 0)
            .ok_or(HelperError::Arguments)
    };
    let width = dimension()?;
    let height = dimension()?;
    if args.next().is_some() {
        return Err(HelperError::Arguments);
    }
    let mut bytes = Vec::new();
    io::stdin()
        .lock()
        .take(MAX_INPUT_BYTES.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|_| HelperError::Input)?;
    if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > MAX_INPUT_BYTES {
        return Err(HelperError::Input);
    }
    let (detected, _, original_width, original_height) =
        header(&bytes).map_err(|_| HelperError::Decode)?;
    if !matches!(
        (detected, format),
        (Format::Png, ImageFormat::Png) | (Format::Jpeg, ImageFormat::Jpeg)
    ) || (width, height) != (original_width, original_height)
        || u64::from(width).saturating_mul(u64::from(height)) > MAX_RESIZE_PIXELS
    {
        return Err(HelperError::Decode);
    }
    let bytes = resize(&bytes, format, width, height)?;
    let (media_type, width, height) = inspect(&bytes).map_err(|_| HelperError::Decode)?;
    let metadata = OutputHeader {
        media_type: media_type.into(),
        width,
        height,
        original_width,
        original_height,
    };
    let metadata = serde_json::to_vec(&metadata).map_err(|_| HelperError::Output)?;
    let mut output = io::stdout().lock();
    output
        .write_all(&metadata)
        .and_then(|()| output.write_all(b"\n"))
        .and_then(|()| output.write_all(&bytes))
        .map_err(|_| HelperError::Output)
}

fn resize(
    bytes: &[u8],
    format: ImageFormat,
    width: u32,
    height: u32,
) -> Result<Vec<u8>, HelperError> {
    let mut reader = ImageReader::with_format(Cursor::new(bytes), format);
    let mut limits = Limits::default();
    limits.max_image_width = Some(width);
    limits.max_image_height = Some(height);
    // PNG のメタデータと行のバッファの制限を、出力する画素の制限と分ける
    // （設計書 01-03「画像のファイルの読み取り」）。
    limits.max_alloc = Some(16 * 1024 * 1024);
    reader.limits(limits);
    let mut decoder = reader.into_decoder().map_err(|_| HelperError::Decode)?;
    if decoder.total_bytes() > MAX_DECODED_BYTES {
        return Err(HelperError::Decode);
    }
    // png 0.18.1 は iCCP の展開の失敗も無視する。上限を超えたプロファイルを
    // 黙って落とした場合を拒む（設計書 01-03「画像のファイルの読み取り」）。
    if format == ImageFormat::Png
        && png_has_icc_profile(bytes)
        && decoder
            .icc_profile()
            .map_err(|_| HelperError::Decode)?
            .is_none()
    {
        return Err(HelperError::Decode);
    }
    let orientation = decoder.orientation().map_err(|_| HelperError::Decode)?;
    let mut image = DynamicImage::from_decoder(decoder).map_err(|_| HelperError::Decode)?;
    if image.width() > MAX_IMAGE_DIMENSION || image.height() > MAX_IMAGE_DIMENSION {
        image = image.resize(
            MAX_IMAGE_DIMENSION,
            MAX_IMAGE_DIMENSION,
            FilterType::Triangle,
        );
    }
    image.apply_orientation(orientation);
    let mut output = Vec::new();
    if has_transparency(&image) {
        DynamicImage::ImageRgba8(image.into_rgba8())
            .write_with_encoder(PngEncoder::new(&mut output))
            .map_err(|_| HelperError::Decode)?;
    } else {
        let image = DynamicImage::ImageRgb8(image.into_rgb8());
        for quality in [80, 60, 40] {
            output.clear();
            image
                .write_with_encoder(JpegEncoder::new_with_quality(&mut output, quality))
                .map_err(|_| HelperError::Decode)?;
            if u64::try_from(output.len()).unwrap_or(u64::MAX) <= MAX_IMAGE_BYTES {
                return Ok(output);
            }
        }
    }
    let size = u64::try_from(output.len()).unwrap_or(u64::MAX);
    if size > MAX_IMAGE_BYTES {
        return Err(HelperError::TooLarge(size));
    }
    Ok(output)
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

mod text {
    //! 補助が返す固定の失敗の種類。入力の中身や OS の診断を含めない。
    pub const ARGUMENTS: &str = "arguments";
    #[cfg(target_os = "linux")]
    pub const MEMORY: &str = "memory";
    pub const INPUT: &str = "input";
    pub const DECODE: &str = "decode";
    pub const TOO_LARGE: &str = "too-large";
    pub const OUTPUT: &str = "output";
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::limit_memory;
    // 関門: 上限の宣言だけでなく、子の確保が実際に失敗することを確かめる。
    #[test]
    fn address_space_limit_rejects_two_gib_allocation() -> Result<(), Box<dyn std::error::Error>> {
        const CHILD: &str = "BENITOITEH_IMAGE_MEMORY_CHILD";
        if std::env::var_os(CHILD).is_some() {
            assert!(limit_memory().is_ok());
            let mut bytes = Vec::<u8>::new();
            assert!(bytes.try_reserve_exact(2 * 1024 * 1024 * 1024).is_err());
            return Ok(());
        }
        let status = std::process::Command::new(std::env::current_exe()?)
            .args([
                "--exact",
                "tools::read::image::helper::tests::address_space_limit_rejects_two_gib_allocation",
            ])
            .env(CHILD, "1")
            .status()?;
        assert!(status.success());
        Ok(())
    }
}
