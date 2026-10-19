//! 画像の縮小を道具の実行器と保存した中身で確かめる（設計書 01-03「画像のファイルの読み取り」）。

use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::time::SystemTime;

use benitoiteh::config::{Mode, ReadConfig, paths::DataPaths};
use benitoiteh::safe_fs::ScopeRoot;
use benitoiteh::session::{Session, SessionId};
use benitoiteh::tools::exclude::ExcludeRules;
use benitoiteh::tools::{
    ApprovalStatus, FileVersion, Invocation, MAX_IMAGE_BYTES, ToolContext, ToolOutput, ToolSet,
};
use image::codecs::jpeg::JpegEncoder;
use image::codecs::png::PngEncoder;
use image::{DynamicImage, ImageEncoder, ImageFormat, Rgb, RgbImage, Rgba, RgbaImage};
use serde_json::json;

type TestResult = Result<(), Box<dyn Error>>;

// 作成時の関門: 既存の見出しだけの画像テストでは、変換後の形式・寸法・向きや
// 元の版の記録を守れない。公開の道具の結果と objects/ の実データを検査する。
// 内部関数を公開せず、符号化した入力と独立した期待の寸法で退行を捕まえる。
struct Fixture {
    directory: PathBuf,
    project: PathBuf,
    context: ToolContext,
    session: Session,
}

impl Fixture {
    fn new(mode: Mode) -> Result<Self, Box<dyn Error>> {
        let mut random = [0; 16];
        getrandom::fill(&mut random)?;
        let directory = std::env::temp_dir().join(format!(
            "benitoiteh-read-resize-{:032x}",
            u128::from_ne_bytes(random)
        ));
        fs::create_dir(&directory)?;
        let directory = fs::canonicalize(directory)?;
        let project = directory.join("project");
        let temporary = directory.join("temporary");
        fs::create_dir(&temporary)?;
        let paths = DataPaths::from_values(
            Some(&directory),
            Some(&directory.join("config")),
            Some(&directory.join("state")),
            Some(&temporary),
        )?;
        let session = Session::create(&paths, SessionId::generate(SystemTime::now())?)?;
        let spill = session.directory().join("spill");
        for path in [&project, &temporary, &spill] {
            fs::create_dir_all(path)?;
        }
        let context = ToolContext::new(
            mode,
            ScopeRoot::open(&project)?,
            ScopeRoot::open(&temporary)?,
            ScopeRoot::open(&spill)?,
            ExcludeRules::new(
                &project,
                &paths,
                &spill,
                &ReadConfig::default(),
                &ReadConfig::default(),
            )?,
        )
        .with_image_input(true)
        .with_session(session.clone());
        Ok(Self {
            directory,
            project,
            context,
            session,
        })
    }

    fn read(&self, bytes: &[u8]) -> Result<ToolOutput, Box<dyn Error>> {
        fs::write(self.project.join("picture"), bytes)?;
        Ok(ToolSet::new(self.context.mode).execute(
            &self.context,
            "read",
            json!({"path":"picture"}),
        ))
    }

    fn check_image(
        &self,
        result: &ToolOutput,
        format: ImageFormat,
        dimensions: (u32, u32),
        original: (u32, u32),
    ) -> Result<DynamicImage, Box<dyn Error>> {
        assert!(!result.is_error, "{}", result.text);
        assert_eq!(result.images.len(), 1);
        let image = result.images.first().ok_or("missing image")?;
        let bytes = image.data.as_deref().ok_or("missing image bytes")?;
        assert_eq!(image::guess_format(bytes)?, format);
        assert_eq!(image.media_type, format.to_mime_type());
        assert_eq!((image.width, image.height), dimensions);
        assert_eq!(image.bytes, u64::try_from(bytes.len())?);
        assert!(image.bytes <= MAX_IMAGE_BYTES);
        assert_eq!(
            result.text,
            format!(
                "Read image file {} ({}x{}, {} bytes; resized from {}x{})",
                image.media_type, dimensions.0, dimensions.1, image.bytes, original.0, original.1
            )
        );
        assert_eq!(result.screen.as_ref(), Some(&result.text));
        assert_eq!(
            fs::read(self.session.directory().join("objects").join(&image.sha256))?,
            bytes
        );
        let path = self.project.join("picture");
        assert_eq!(
            self.context.read_version(&path)?,
            Some(FileVersion::from_metadata(&fs::metadata(path)?))
        );
        let decoded = image::load_from_memory_with_format(bytes, format)?;
        assert_eq!((decoded.width(), decoded.height()), dimensions);
        Ok(decoded)
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _removed = fs::remove_dir_all(&self.directory);
    }
}

fn png(image: &DynamicImage) -> Result<Vec<u8>, image::ImageError> {
    let mut bytes = Vec::new();
    image.write_with_encoder(PngEncoder::new(&mut bytes))?;
    Ok(bytes)
}

#[test]
fn oversized_opaque_png_becomes_jpeg_and_records_original_file_version() -> TestResult {
    let fixture = Fixture::new(Mode::Restricted)?;
    // 色の型にアルファがあっても、実際に透明でなければ JPEG にする。
    let source =
        DynamicImage::ImageRgba8(RgbaImage::from_pixel(2400, 1200, Rgba([48, 96, 192, 255])));
    let result = fixture.read(&png(&source)?)?;
    fixture.check_image(&result, ImageFormat::Jpeg, (2000, 1000), (2400, 1200))?;
    Ok(())
}

#[test]
fn oversized_transparent_png_preserves_alpha_in_eight_bit_png() -> TestResult {
    let fixture = Fixture::new(Mode::Normal)?;
    // 16 ビットのアルファも検査し、送る PNG は 8 ビットに揃える。
    let source = DynamicImage::ImageRgba16(image::ImageBuffer::from_pixel(
        2100,
        2100,
        Rgba([12000, 24000, 48000, 32768]),
    ));
    let result = fixture.read(&png(&source)?)?;
    let decoded = fixture.check_image(&result, ImageFormat::Png, (2000, 2000), (2100, 2100))?;
    let rgba = decoded.as_rgba8().ok_or("expected eight-bit RGBA PNG")?;
    assert!(
        rgba.pixels()
            .all(|pixel| pixel.0[3] > 0 && pixel.0[3] < 255)
    );
    Ok(())
}

#[test]
fn png_exceeding_only_byte_limit_keeps_dimensions_and_becomes_jpeg() -> TestResult {
    let fixture = Fixture::new(Mode::Restricted)?;
    let bytes = png(&DynamicImage::ImageRgb8(noise(1200, 1200)))?;
    assert!(u64::try_from(bytes.len())? > MAX_IMAGE_BYTES);
    let result = fixture.read(&bytes)?;
    fixture.check_image(&result, ImageFormat::Jpeg, (1200, 1200), (1200, 1200))?;
    Ok(())
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

// 作成時の関門: 通常の縮小の成功では、品質の再試行と変換後のバイト数の
// 上限を確かめられない。品質 80 で超える入力を示し、道具の結果の上限を守る。
#[test]
fn jpeg_quality_is_reduced_when_quality_eighty_exceeds_byte_limit() -> TestResult {
    let fixture = Fixture::new(Mode::Restricted)?;
    let source = DynamicImage::ImageRgb8(noise(2000, 2000));
    let mut quality_eighty = Vec::new();
    source.write_with_encoder(JpegEncoder::new_with_quality(&mut quality_eighty, 80))?;
    assert!(u64::try_from(quality_eighty.len())? > MAX_IMAGE_BYTES);
    let bytes = png(&source)?;
    let result = fixture.read(&bytes)?;
    fixture.check_image(&result, ImageFormat::Jpeg, (2000, 2000), (2000, 2000))?;
    Ok(())
}

#[test]
fn transparent_png_that_still_exceeds_byte_limit_is_rejected() -> TestResult {
    let fixture = Fixture::new(Mode::Normal)?;
    let mut rgba = DynamicImage::ImageRgb8(noise(1200, 1200)).into_rgba8();
    for pixel in rgba.pixels_mut() {
        pixel.0[3] = 128;
    }
    let bytes = png(&DynamicImage::ImageRgba8(rgba))?;
    assert!(u64::try_from(bytes.len())? > MAX_IMAGE_BYTES);
    let result = fixture.read(&bytes)?;
    assert!(result.is_error);
    assert!(
        result.text.contains("Image size after resizing is"),
        "{}",
        result.text
    );
    assert!(result.text.contains("3932160"));
    assert!(result.images.is_empty());
    assert_eq!(
        fixture
            .context
            .read_version(&fixture.project.join("picture"))?,
        None
    );
    assert_eq!(
        fs::read_dir(fixture.session.directory().join("objects"))?.count(),
        0
    );
    Ok(())
}

fn orientation_exif() -> Vec<u8> {
    let mut bytes = b"MM\0\x2a\0\0\0\x08\0\x01\x01\x12\0\x03\0\0\0\x01".to_vec();
    bytes.extend(6_u16.to_be_bytes());
    bytes.extend([0; 6]);
    bytes
}

#[test]
fn oversized_jpeg_and_png_apply_exif_orientation_after_resizing() -> TestResult {
    let fixture = Fixture::new(Mode::Normal)?;
    let source = DynamicImage::ImageRgb8(RgbImage::from_fn(2400, 800, |x, _| {
        if x < 1200 {
            Rgb([240, 0, 0])
        } else {
            Rgb([0, 0, 240])
        }
    }));
    for format in [ImageFormat::Jpeg, ImageFormat::Png] {
        let mut bytes = Vec::new();
        if format == ImageFormat::Jpeg {
            let mut encoder = JpegEncoder::new_with_quality(&mut bytes, 80);
            encoder.set_exif_metadata(orientation_exif())?;
            source.write_with_encoder(encoder)?;
        } else {
            let mut encoder = PngEncoder::new(&mut bytes);
            encoder.set_exif_metadata(orientation_exif())?;
            source.write_with_encoder(encoder)?;
        }
        let result = fixture.read(&bytes)?;
        assert!(!result.is_error, "{}", result.text);
        let image = result.images.first().ok_or("missing oriented image")?;
        assert_eq!(image.height, 2000);
        assert!(image.width < image.height);
        let decoded = fixture
            .check_image(&result, ImageFormat::Jpeg, (image.width, 2000), (2400, 800))?
            .into_rgb8();
        // 向きの値を捨てただけでなく、赤い左半分が上へ回ったことを確かめる。
        let top = decoded.get_pixel(image.width / 2, 100);
        let bottom = decoded.get_pixel(image.width / 2, 1900);
        assert!(top.0[0] > 200 && top.0[2] < 30, "{format:?}: {top:?}");
        assert!(
            bottom.0[2] > 200 && bottom.0[0] < 30,
            "{format:?}: {bottom:?}"
        );
    }
    Ok(())
}

fn png_header(width: u32, height: u32) -> Vec<u8> {
    let mut bytes = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
    bytes.extend(width.to_be_bytes());
    bytes.extend(height.to_be_bytes());
    bytes.extend([8, 2, 0, 0, 0, 0, 0, 0, 0]);
    bytes.extend(b"\0\0\0\0IDAT\0\0\0\0");
    bytes
}

// 作成時の関門: 従来の画素数と入力バイト数の検査だけでは、メタデータの
// 展開量と 16 ビット画像の出力バイト数を抑えられない。小さい圧縮入力から
// 有効な大きいデータを宣言し、道具が保存する前に拒否することを確かめる。
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

#[test]
fn resizing_rejects_iccp_expanding_to_five_hundred_mib() -> TestResult {
    let original = png(&DynamicImage::ImageRgb8(RgbImage::from_pixel(
        2400,
        1,
        Rgb([0, 0, 0]),
    )))?;
    for size in [1024 * 1024, 500 * 1024 * 1024] {
        let fixture = Fixture::new(Mode::Restricted)?;
        let mut bytes = original.clone();
        let mut profile = b"profile\0\0".to_vec();
        profile.extend(zlib_zeros(size)?);
        bytes.splice(33..33, png_chunk(b"iCCP", &profile)?);
        assert!(bytes.len() < 1024 * 1024);
        let result = fixture.read(&bytes)?;
        if size == 1024 * 1024 {
            fixture.check_image(&result, ImageFormat::Jpeg, (2000, 1), (2400, 1))?;
        } else {
            assert!(result.is_error, "{}", result.text);
            assert!(
                result.text.contains("The image could not be decoded."),
                "{}",
                result.text
            );
            assert!(result.images.is_empty());
            assert_eq!(
                fixture
                    .context
                    .read_version(&fixture.project.join("picture"))?,
                None
            );
            assert_eq!(
                fs::read_dir(fixture.session.directory().join("objects"))?.count(),
                0
            );
        }
    }
    Ok(())
}

#[test]
fn resizing_rejects_rgba16_exceeding_decoded_byte_limit() -> TestResult {
    let fixture = Fixture::new(Mode::Normal)?;
    let mut ihdr = 6324_u32.to_be_bytes().to_vec();
    ihdr.extend(6324_u32.to_be_bytes());
    ihdr.extend([16, 6, 0, 0, 0]);
    let mut bytes = b"\x89PNG\r\n\x1a\n".to_vec();
    bytes.extend(png_chunk(b"IHDR", &ihdr)?);
    // 有効な走査線も圧縮して置く。バイト数の検査を失うとデコードと縮小が成功し、
    // 壊れた画素の検査で偶然通る否定のテストにならないようにする。
    let raw_size = 6324_usize
        .saturating_mul(8)
        .saturating_add(1)
        .saturating_mul(6324);
    bytes.extend(png_chunk(b"IDAT", &zlib_zeros(raw_size)?)?);
    bytes.extend(png_chunk(b"IEND", &[])?);
    assert!(u64::try_from(bytes.len())? < MAX_IMAGE_BYTES);
    let result = fixture.read(&bytes)?;
    assert!(result.is_error, "{}", result.text);
    assert!(
        result.text.contains("The image could not be decoded."),
        "{}",
        result.text
    );
    assert!(result.images.is_empty());
    assert_eq!(
        fixture
            .context
            .read_version(&fixture.project.join("picture"))?,
        None
    );
    assert_eq!(
        fs::read_dir(fixture.session.directory().join("objects"))?.count(),
        0
    );
    Ok(())
}

// 作成時の関門: 通常の read の入口は取り消しを常に偽で作る。ループと同じ
// 公開の入口へ真の印を渡し、読み取り前と読み取り後の両方の錠の取り方を覆う。
#[test]
fn cancelled_resizing_does_not_store_images_or_record_versions() -> TestResult {
    let original = png(&DynamicImage::ImageRgb8(RgbImage::from_pixel(
        2400,
        1,
        Rgb([0, 0, 0]),
    )))?;
    for large_file in [false, true] {
        let fixture = Fixture::new(Mode::Restricted)?;
        let mut bytes = original.clone();
        if large_file {
            bytes.resize(usize::try_from(MAX_IMAGE_BYTES)?.saturating_add(1), 0);
        }
        fs::write(fixture.project.join("picture"), &bytes)?;
        let tools = ToolSet::new(Mode::Restricted);
        let call = tools.validate("read", json!({"path":"picture"}))?;
        let result = tools.execute_validated(
            &fixture.context,
            &call,
            ApprovalStatus::Unapproved,
            Invocation {
                call_id: "cancel-resize",
                cancelled: &AtomicBool::new(true),
                confirmed_effects: &BTreeMap::new(),
                session_domains: &BTreeSet::new(),
            },
        );
        assert!(result.is_error, "{large_file}: {}", result.text);
        assert!(result.text.contains("cancelled"), "{}", result.text);
        assert!(result.images.is_empty());
        assert_eq!(
            fixture
                .context
                .read_version(&fixture.project.join("picture"))?,
            None
        );
        assert_eq!(
            fs::read_dir(fixture.session.directory().join("objects"))?.count(),
            0
        );
    }
    Ok(())
}

#[test]
fn resizing_rejects_excessive_pixels_and_corrupt_images_without_storing_them() -> TestResult {
    let fixture = Fixture::new(Mode::Restricted)?;
    for (width, expected) in [
        (
            8001,
            "Image has 8001x5000 pixels; the limit for resizing is 40000000 pixels.",
        ),
        (8000, "The image could not be decoded."),
    ] {
        let result = fixture.read(&png_header(width, 5000))?;
        assert!(result.is_error);
        assert!(result.text.contains(expected), "{}", result.text);
        assert!(result.images.is_empty());
    }
    let mut corrupt = png(&DynamicImage::ImageRgb8(RgbImage::from_pixel(
        2100,
        2100,
        Rgb([64, 128, 192]),
    )))?;
    // メタデータを持たない PNG の IDAT の中を壊す。見出しの拒否で通さない。
    for byte in corrupt.get_mut(51..251).ok_or("missing IDAT bytes")? {
        *byte ^= 0xff;
    }
    let result = fixture.read(&corrupt)?;
    assert!(result.is_error);
    assert!(
        result.text.contains("The image could not be decoded."),
        "{}",
        result.text
    );
    assert!(result.images.is_empty());

    // メタデータの大きさの拒否は、画像を解くより前に働く。
    fs::write(fixture.project.join("picture"), png_header(2400, 1200))?;
    fs::OpenOptions::new()
        .write(true)
        .open(fixture.project.join("picture"))?
        .set_len(64 * 1024 * 1024 + 1)?;
    let result =
        ToolSet::new(Mode::Restricted).execute(&fixture.context, "read", json!({"path":"picture"}));
    assert!(result.is_error);
    assert!(result.text.contains("67108865") && result.text.contains("67108864"));
    assert!(result.images.is_empty());
    assert_eq!(
        fixture
            .context
            .read_version(&fixture.project.join("picture"))?,
        None
    );
    assert_eq!(
        fs::read_dir(fixture.session.directory().join("objects"))?.count(),
        0
    );
    Ok(())
}
