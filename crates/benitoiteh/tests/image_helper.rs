//! __image の入出力の契約と変換の安全の上限を、実際の補助の入口で確かめる（設計書 01-03）。
use benitoiteh::tools::MAX_IMAGE_BYTES;
use image::codecs::jpeg::JpegEncoder;
use image::codecs::png::PngEncoder;
use image::{DynamicImage, ImageEncoder, ImageFormat, Rgb, RgbImage, Rgba, RgbaImage};
use serde_json::Value;
use std::error::Error;
use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};
use std::time::Instant;
type TestResult = Result<(), Box<dyn Error>>;

struct Helper {
    directory: PathBuf,
}
impl Helper {
    fn new() -> Result<Self, Box<dyn Error>> {
        let mut random = [0; 16];
        getrandom::fill(&mut random)?;
        let directory =
            std::env::temp_dir().join(format!("bh-image-{:032x}", u128::from_ne_bytes(random)));
        fs::create_dir(&directory)?;
        Ok(Self { directory })
    }
    fn run(&self, bytes: &[u8], args: &[&str]) -> Result<Output, Box<dyn Error>> {
        let mut child = Command::new(fs::canonicalize(env!("CARGO_BIN_EXE_benitoiteh"))?)
            .arg("__image")
            .args(args)
            .env_clear()
            .env("XDG_CONFIG_HOME", &self.directory)
            .env("XDG_STATE_HOME", &self.directory)
            .env("TMPDIR", &self.directory)
            .current_dir(&self.directory)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        let mut input = child.stdin.take().ok_or("missing helper stdin")?;
        std::thread::scope(|scope| -> Result<Output, Box<dyn Error>> {
            let writer = scope.spawn(move || {
                let result = input.write_all(bytes);
                drop(input);
                result
            });
            let output = child.wait_with_output()?;
            let written = writer.join().map_err(|_| "input writer panicked")?;
            if let Err(error) = written
                && error.kind() != std::io::ErrorKind::BrokenPipe
            {
                return Err(error.into());
            }
            Ok(output)
        })
    }
}
impl Drop for Helper {
    fn drop(&mut self) {
        let _removed = fs::remove_dir_all(&self.directory);
    }
}
fn converted(
    output: &Output,
    media: &str,
    dimensions: (u32, u32),
    original: (u32, u32),
) -> Result<DynamicImage, Box<dyn Error>> {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    let end = output
        .stdout
        .iter()
        .position(|byte| *byte == b'\n')
        .ok_or("missing metadata line")?;
    let metadata: Value = serde_json::from_slice(output.stdout.get(..end).ok_or("metadata")?)?;
    assert_eq!(metadata.get("media_type"), Some(&Value::from(media)));
    assert_eq!(metadata.get("width"), Some(&Value::from(dimensions.0)));
    assert_eq!(metadata.get("height"), Some(&Value::from(dimensions.1)));
    assert_eq!(
        metadata.get("original_width"),
        Some(&Value::from(original.0))
    );
    assert_eq!(
        metadata.get("original_height"),
        Some(&Value::from(original.1))
    );
    let payload = output
        .stdout
        .get(end.saturating_add(1)..)
        .ok_or("payload")?;
    assert!(u64::try_from(payload.len())? <= MAX_IMAGE_BYTES);
    assert_eq!(image::guess_format(payload)?.to_mime_type(), media);
    let image = image::load_from_memory(payload)?;
    assert_eq!((image.width(), image.height()), dimensions);
    Ok(image)
}
fn rejected(output: &Output, diagnostic: &str) {
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.starts_with(diagnostic), "{stderr}");
    assert_eq!(stderr.lines().count(), 1);
}

// 関門: 符号化した PNG/JPEG を補助へ渡し、形式、アルファ、16 ビットの変換を独立した期待で守る。
#[test]
fn png_and_jpeg_return_metadata_and_encoded_pixels() -> TestResult {
    let helper = Helper::new()?;
    let source =
        DynamicImage::ImageRgba8(RgbaImage::from_pixel(2400, 1200, Rgba([48, 96, 192, 255])));
    let bytes = png(&source)?;
    let output = helper.run(&bytes, &["png", "2400", "1200"])?;
    converted(&output, "image/jpeg", (2000, 1000), (2400, 1200))?;
    let mut jpeg = Vec::new();
    source
        .to_rgb8()
        .write_with_encoder(JpegEncoder::new_with_quality(&mut jpeg, 80))?;
    converted(
        &helper.run(&jpeg, &["jpeg", "2400", "1200"])?,
        "image/jpeg",
        (2000, 1000),
        (2400, 1200),
    )?;
    let source = DynamicImage::ImageRgba16(image::ImageBuffer::from_pixel(
        2100,
        2100,
        Rgba([12000, 24000, 48000, 32768]),
    ));
    let output = helper.run(&png(&source)?, &["png", "2100", "2100"])?;
    let decoded = converted(&output, "image/png", (2000, 2000), (2100, 2100))?;
    assert!(
        decoded
            .as_rgba8()
            .ok_or("expected eight-bit RGBA")?
            .pixels()
            .all(|pixel| pixel.0[3] > 0 && pixel.0[3] < 255)
    );
    Ok(())
}

#[test]
fn byte_limits_lower_jpeg_quality_and_reject_large_transparent_png() -> TestResult {
    let helper = Helper::new()?;
    let source = DynamicImage::ImageRgb8(noise(2000, 2000));
    let mut quality_eighty = Vec::new();
    source.write_with_encoder(JpegEncoder::new_with_quality(&mut quality_eighty, 80))?;
    assert!(u64::try_from(quality_eighty.len())? > MAX_IMAGE_BYTES);
    converted(
        &helper.run(&png(&source)?, &["png", "2000", "2000"])?,
        "image/jpeg",
        (2000, 2000),
        (2000, 2000),
    )?;
    let source = DynamicImage::ImageRgb8(noise(1200, 1200));
    let bytes = png(&source)?;
    assert!(u64::try_from(bytes.len())? > MAX_IMAGE_BYTES);
    converted(
        &helper.run(&bytes, &["png", "1200", "1200"])?,
        "image/jpeg",
        (1200, 1200),
        (1200, 1200),
    )?;
    let mut rgba = source.into_rgba8();
    for pixel in rgba.pixels_mut() {
        pixel.0[3] = 128;
    }
    rejected(
        &helper.run(
            &png(&DynamicImage::ImageRgba8(rgba))?,
            &["png", "1200", "1200"],
        )?,
        "too-large ",
    );
    Ok(())
}

#[test]
fn jpeg_and_png_apply_exif_after_resizing() -> TestResult {
    let helper = Helper::new()?;
    let source = DynamicImage::ImageRgb8(RgbImage::from_fn(2400, 800, |x, _| {
        if x < 1200 {
            Rgb([240, 0, 0])
        } else {
            Rgb([0, 0, 240])
        }
    }));
    for format in [ImageFormat::Jpeg, ImageFormat::Png] {
        let mut bytes = Vec::new();
        let kind = if format == ImageFormat::Jpeg {
            let mut encoder = JpegEncoder::new_with_quality(&mut bytes, 80);
            encoder.set_exif_metadata(orientation_exif())?;
            source.write_with_encoder(encoder)?;
            "jpeg"
        } else {
            let mut encoder = PngEncoder::new(&mut bytes);
            encoder.set_exif_metadata(orientation_exif())?;
            source.write_with_encoder(encoder)?;
            "png"
        };
        let output = helper.run(&bytes, &[kind, "2400", "800"])?;
        let image = converted(&output, "image/jpeg", (667, 2000), (2400, 800))?.into_rgb8();
        let top = image.get_pixel(333, 100);
        let bottom = image.get_pixel(333, 1900);
        assert!(top.0[0] > 200 && top.0[2] < 30);
        assert!(bottom.0[2] > 200 && bottom.0[0] < 30);
    }
    Ok(())
}

// 関門: 入力とメタデータの上限・壊れた入力を拒み、失敗時に画像を一部も出さない契約を守る。
#[test]
fn invalid_arguments_corruption_and_input_limit_produce_no_stdout() -> TestResult {
    let helper = Helper::new()?;
    for args in [
        vec![],
        vec!["gif", "1", "1"],
        vec!["png", "0", "1"],
        vec!["png", "x", "1"],
        vec!["png", "1", "1", "extra"],
    ] {
        rejected(&helper.run(&[], &args)?, "arguments");
    }
    rejected(&helper.run(b"not an image", &["png", "1", "1"])?, "decode");
    let mut bytes = png(&DynamicImage::ImageRgb8(RgbImage::from_pixel(
        2100,
        2100,
        Rgb([64, 128, 192]),
    )))?;
    for byte in bytes.get_mut(51..251).ok_or("missing IDAT")? {
        *byte ^= 0xff;
    }
    rejected(&helper.run(&bytes, &["png", "2100", "2100"])?, "decode");
    rejected(
        &helper.run(&vec![0; 64 * 1024 * 1024 + 1], &["png", "1", "1"])?,
        "input",
    );
    rejected(
        &helper.run(&png_header(8001, 5000), &["png", "8001", "5000"])?,
        "decode",
    );
    rejected(
        &helper.run(&png_header(8000, 5000), &["png", "8000", "5000"])?,
        "decode",
    );
    Ok(())
}

#[test]
fn icc_and_decoded_pixels_have_independent_allocation_limits() -> TestResult {
    let helper = Helper::new()?;
    let original = png(&DynamicImage::ImageRgb8(RgbImage::from_pixel(
        2400,
        1,
        Rgb([0, 0, 0]),
    )))?;
    for size in [1024 * 1024, 500 * 1024 * 1024] {
        let mut bytes = original.clone();
        let mut profile = b"profile\0\0".to_vec();
        profile.extend(zlib_zeros(size)?);
        bytes.splice(33..33, png_chunk(b"iCCP", &profile)?);
        assert!(bytes.len() < 1024 * 1024);
        let output = helper.run(&bytes, &["png", "2400", "1"])?;
        if size == 1024 * 1024 {
            converted(&output, "image/jpeg", (2000, 1), (2400, 1))?;
        } else {
            rejected(&output, "decode");
        }
    }
    let mut ihdr = 6324_u32.to_be_bytes().to_vec();
    ihdr.extend(6324_u32.to_be_bytes());
    ihdr.extend([16, 6, 0, 0, 0]);
    let mut bytes = b"\x89PNG\r\n\x1a\n".to_vec();
    bytes.extend(png_chunk(b"IHDR", &ihdr)?);
    let raw_size = 6324_usize
        .saturating_mul(8)
        .saturating_add(1)
        .saturating_mul(6324);
    bytes.extend(png_chunk(b"IDAT", &zlib_zeros(raw_size)?)?);
    bytes.extend(png_chunk(b"IEND", &[])?);
    rejected(&helper.run(&bytes, &["png", "6324", "6324"])?, "decode");
    Ok(())
}

#[test]
#[ignore = "manual release timing; does not include the OS sandbox"]
fn measure_helper_startup_and_resize() -> TestResult {
    let helper = Helper::new()?;
    let bytes = png(&DynamicImage::ImageRgba8(RgbaImage::from_pixel(
        2400,
        1200,
        Rgba([48, 96, 192, 255]),
    )))?;
    for _ in 0..5 {
        let start = Instant::now();
        let output = helper.run(&bytes, &["png", "2400", "1200"])?;
        eprintln!("P5-15 direct helper resize: {:?}", start.elapsed());
        assert!(output.status.success());
    }
    Ok(())
}

fn png(image: &DynamicImage) -> Result<Vec<u8>, image::ImageError> {
    let mut bytes = Vec::new();
    image.write_with_encoder(PngEncoder::new(&mut bytes))?;
    Ok(bytes)
}

fn noise(width: u32, height: u32) -> RgbImage {
    let mut state = 0x4d59_5df4_d0f3_3173_u64;
    RgbImage::from_fn(width, height, |_, _| {
        let mut rgb = [0; 3];
        for byte in &mut rgb {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            *byte = state.to_le_bytes()[0];
        }
        Rgb(rgb)
    })
}

fn orientation_exif() -> Vec<u8> {
    let mut bytes = b"MM\0\x2a\0\0\0\x08\0\x01\x01\x12\0\x03\0\0\0\x01".to_vec();
    bytes.extend(6_u16.to_be_bytes());
    bytes.extend([0; 6]);
    bytes
}

fn png_header(width: u32, height: u32) -> Vec<u8> {
    let mut bytes = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
    bytes.extend(width.to_be_bytes());
    bytes.extend(height.to_be_bytes());
    bytes.extend([8, 2, 0, 0, 0, 0, 0, 0, 0]);
    bytes.extend(b"\0\0\0\0IDAT\0\0\0\0");
    bytes
}

fn png_chunk(kind: &[u8; 4], data: &[u8]) -> Result<Vec<u8>, Box<dyn Error>> {
    let mut bytes = u32::try_from(data.len())?.to_be_bytes().to_vec();
    bytes.extend(kind);
    bytes.extend(data);
    let mut crc = u32::MAX;
    for byte in kind.iter().chain(data) {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            crc = (crc >> 1) ^ if crc & 1 == 0 { 0 } else { 0xedb8_8320 };
        }
    }
    bytes.extend((!crc).to_be_bytes());
    Ok(bytes)
}

fn zlib_zeros(size: usize) -> Result<Vec<u8>, Box<dyn Error>> {
    // Python の zlib.compressobj(9, DEFLATED, -15) で 1 MiB の 0 を圧縮し、
    // Z_FULL_FLUSH したブロック。辞書を空に戻すので連結して繰り返せる。
    let mut block = b"\xec\xc1\x31\x01\0\0\0\xc2\xa0\xf5\x4f\x6d\x08\x5f\xa0".to_vec();
    block.resize(1031, 0);
    block.extend(b"\x3e\x03\0\0\xff\xff");
    let mut bytes = b"\x78\xda".to_vec();
    for _ in 0..size / (1024 * 1024) {
        bytes.extend(&block);
    }
    let mut remaining = size % (1024 * 1024);
    while remaining > 0 {
        let length = u16::try_from(remaining.min(usize::from(u16::MAX)))?;
        bytes.push(0); // 非最終の無圧縮ブロック。
        bytes.extend(length.to_le_bytes());
        bytes.extend((!length).to_le_bytes());
        bytes.resize(bytes.len().saturating_add(usize::from(length)), 0);
        remaining = remaining.saturating_sub(usize::from(length));
    }
    bytes.extend(b"\x03\0"); // 最終の空のブロック。
    let adler = ((u32::try_from(size)? % 65521) << 16) | 1;
    bytes.extend(adler.to_be_bytes());
    Ok(bytes)
}
