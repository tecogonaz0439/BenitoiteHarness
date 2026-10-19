//! 画像の補助を使えない場合と取り消しを公開の道具で確かめる（設計書 01-03）。
#[allow(dead_code)]
#[path = "support/image_resize.rs"]
mod fixture;
use benitoiteh::config::Mode;
use benitoiteh::tools::{ApprovalStatus, Invocation, MAX_IMAGE_BYTES, ToolSet};
use fixture::{Fixture, png};
use image::{DynamicImage, Rgb, RgbImage};
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::sync::atomic::AtomicBool;
type TestResult = Result<(), Box<dyn std::error::Error>>;

// 関門: 補助の利用不可で上限を超える入力を読まず、縮めない画像を保存する契約を守る。
#[test]
fn unavailable_helper_preserves_small_images_and_rejects_resize_limits() -> TestResult {
    for mode in [Mode::Restricted, Mode::Normal] {
        let fixture = Fixture::new(mode, false)?;
        let small = png(&DynamicImage::ImageRgb8(RgbImage::from_pixel(
            1,
            1,
            Rgb([1, 2, 3]),
        )))?;
        let result = fixture.read(&small)?;
        assert!(!result.is_error, "{}", result.text);
        assert_eq!(
            result
                .images
                .first()
                .and_then(|image| image.data.as_deref()),
            Some(small.as_slice())
        );
        let large = png(&DynamicImage::ImageRgb8(RgbImage::from_pixel(
            2400,
            1,
            Rgb([1, 2, 3]),
        )))?;
        let result = fixture.read(&large)?;
        assert!(
            result.is_error && result.text.contains("2400x1"),
            "{}",
            result.text
        );
        fs::write(fixture.project.join("picture"), &small)?;
        fs::OpenOptions::new()
            .write(true)
            .open(fixture.project.join("picture"))?
            .set_len(64 * 1024 * 1024 + 1)?;
        let result =
            ToolSet::new(mode).execute(&fixture.context, "read", json!({"path":"picture"}));
        assert!(
            result.is_error && result.text.contains("67108865") && result.text.contains("3932160"),
            "{}",
            result.text
        );
    }
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
        let fixture = Fixture::new(Mode::Restricted, true)?;
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

// 関門: 補助の入口の上限だけでは、主のプロセスが巨大な入力を先に読む退行を捕まえられない。
// 補助を利用できる文脈でも、見出しと元のファイルの大きさだけで拒むことを守る。
#[test]
fn parent_rejects_pixel_and_file_limits_before_launching_helper() -> TestResult {
    let fixture = Fixture::new(Mode::Restricted, true)?;
    let mut bytes = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
    bytes.extend(8001_u32.to_be_bytes());
    bytes.extend(5000_u32.to_be_bytes());
    bytes.extend([8, 2, 0, 0, 0, 0, 0, 0, 0]);
    bytes.extend(b"\0\0\0\0IDAT\0\0\0\0");
    let output = fixture.read(&bytes)?;
    assert!(
        output.is_error && output.text.contains("40000000 pixels"),
        "{}",
        output.text
    );
    fs::OpenOptions::new()
        .write(true)
        .open(fixture.project.join("picture"))?
        .set_len(64 * 1024 * 1024 + 1)?;
    let output =
        ToolSet::new(Mode::Restricted).execute(&fixture.context, "read", json!({"path":"picture"}));
    assert!(
        output.is_error && output.text.contains("67108865") && output.text.contains("67108864"),
        "{}",
        output.text
    );
    assert!(output.images.is_empty());
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
    assert_eq!(
        fs::read_dir(fixture.session.directory().join("sandbox"))?.count(),
        0
    );
    Ok(())
}
