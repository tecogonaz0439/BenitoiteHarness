//! 画像の補助を本物の OS のサンドボックスで起動する（設計書 01-03、03-02）。
#[allow(dead_code)]
#[path = "support/image_resize.rs"]
mod fixture;
use benitoiteh::config::Mode;
use fixture::{Fixture, png};
use image::{DynamicImage, ImageFormat, Rgba, RgbaImage};
type TestResult = Result<(), Box<dyn std::error::Error>>;
// 関門: 直接起動では守れない、サンドボックスの起動・保存・元の版の記録を守る。
#[test]
fn oversized_opaque_png_becomes_jpeg_and_records_original_file_version() -> TestResult {
    let fixture = Fixture::new(Mode::Restricted, true)?;
    // 色の型にアルファがあっても、実際に透明でなければ JPEG にする。
    let source =
        DynamicImage::ImageRgba8(RgbaImage::from_pixel(2400, 1200, Rgba([48, 96, 192, 255])));
    let result = fixture.read(&png(&source)?)?;
    fixture.check_image(&result, ImageFormat::Jpeg, (2000, 1000), (2400, 1200))?;
    assert_eq!(
        std::fs::read_dir(fixture.session.directory().join("sandbox"))?.count(),
        0
    );
    Ok(())
}

#[test]
fn oversized_transparent_png_preserves_alpha_in_eight_bit_png() -> TestResult {
    let fixture = Fixture::new(Mode::Normal, true)?;
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
    assert_eq!(
        std::fs::read_dir(fixture.session.directory().join("sandbox"))?.count(),
        0
    );
    Ok(())
}

// 関門: 方針の文字列ではなく、同じ方針で実際のホームのファイルと通信を拒む。
// 囮のホームを一時ディレクトリ内に作り、設定ファイルや利用者のデータを読まない。
#[test]
fn image_policy_denies_home_reads_and_network_connections() -> TestResult {
    use benitoiteh::sandbox::run::{self, Leftovers, RunCommand, RunLimits};
    use benitoiteh::sandbox::{self, SandboxCommand, SandboxStdin};
    use std::collections::BTreeMap;
    use std::fs;
    use std::io;
    use std::net::TcpListener;
    use std::path::Path;
    use std::sync::atomic::AtomicBool;
    use std::sync::{Arc, Mutex};
    let fixture = Fixture::new(Mode::Normal, true)?;
    let home = fixture.directory.join("home");
    fs::create_dir(&home)?;
    let secret = home.join("secret");
    fs::write(&secret, "IMAGE_HOME_SECRET")?;
    assert_eq!(fs::read(&secret)?, b"IMAGE_HOME_SECRET");
    let private = fixture.session.directory().join("sandbox");
    let work = private.join("probe-work");
    fs::create_dir(&work)?;
    let helper = fs::canonicalize(env!("CARGO_BIN_EXE_benitoiteh"))?;
    let probe = std::env::current_exe()?
        .parent()
        .and_then(Path::parent)
        .ok_or("target directory")?
        .join("sandbox-probe");
    let probe = fs::canonicalize(probe)?;
    let listener = TcpListener::bind("127.0.0.1:0")?;
    listener.set_nonblocking(true)?;
    for args in [
        vec!["read".into(), secret.into_os_string()],
        vec![
            "tcp-connect".into(),
            listener.local_addr()?.to_string().into(),
        ],
    ] {
        let command = SandboxCommand {
            policy: sandbox::image_policy(&probe, &work)?,
            program: probe.clone(),
            args,
            cwd: work.clone(),
            env: BTreeMap::from([("HOME".into(), home.clone().into_os_string())]),
            stdin: SandboxStdin::Null,
        };
        #[cfg(target_os = "linux")]
        let command: RunCommand = command
            .prepare(&helper, &private, &fixture.directory.join("placeholders"))?
            .into();
        #[cfg(not(target_os = "linux"))]
        let command: RunCommand = command.to_command(&helper)?.into();
        let output = Arc::new(Mutex::new(Vec::<u8>::new()));
        let diagnostics = Arc::new(Mutex::new(Vec::<u8>::new()));
        let result = run::run(
            command,
            Vec::new(),
            Arc::clone(&output),
            Arc::clone(&diagnostics),
            RunLimits::default(),
            &AtomicBool::new(false),
        )?;
        assert_eq!(result.leftovers, Leftovers::None);
        assert!(!result.status.success());
        // 起動失敗で偶然通さず、探査が PermissionDenied を受けた終了状態を確かめる。
        assert_eq!(
            result.status.code(),
            Some(10),
            "{:?}",
            diagnostics.lock().map_err(|_| "diagnostics lock")?
        );
        assert!(
            !String::from_utf8_lossy(&output.lock().map_err(|_| "output lock")?)
                .contains("IMAGE_HOME_SECRET")
        );
    }
    assert!(matches!(listener.accept(),Err(error) if error.kind()==io::ErrorKind::WouldBlock));
    Ok(())
}

#[test]
fn failed_resize_does_not_store_or_record_and_removes_workdir() -> TestResult {
    use benitoiteh::tools::MAX_IMAGE_BYTES;
    let fixture = Fixture::new(Mode::Normal, true)?;
    let mut state = 0x4d59_5df4_d0f3_3173_u64;
    let source = DynamicImage::ImageRgba8(RgbaImage::from_fn(1200, 1200, |_, _| {
        let mut rgba = [0, 0, 0, 128];
        for byte in rgba.iter_mut().take(3) {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            *byte = state.to_le_bytes()[0];
        }
        Rgba(rgba)
    }));
    let bytes = png(&source)?;
    assert!(u64::try_from(bytes.len())? > MAX_IMAGE_BYTES);
    let output = fixture.read(&bytes)?;
    assert!(
        output.is_error && output.text.contains("Image size after resizing is"),
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
        std::fs::read_dir(fixture.session.directory().join("objects"))?.count(),
        0
    );
    assert_eq!(
        std::fs::read_dir(fixture.session.directory().join("sandbox"))?.count(),
        0
    );
    Ok(())
}

#[test]
#[ignore = "manual release measurement of sandbox startup and resizing"]
fn measure_sandbox_helper_startup_and_resize() -> TestResult {
    let fixture = Fixture::new(Mode::Restricted, true)?;
    let bytes = png(&DynamicImage::ImageRgba8(RgbaImage::from_pixel(
        2400,
        1200,
        Rgba([48, 96, 192, 255]),
    )))?;
    for _ in 0..5 {
        let start = std::time::Instant::now();
        let output = fixture.read(&bytes)?;
        eprintln!("P5-15 sandbox helper resize: {:?}", start.elapsed());
        assert!(!output.is_error, "{}", output.text);
    }
    Ok(())
}
