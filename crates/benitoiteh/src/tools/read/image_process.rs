//! 画像の補助の起動、出力の上限と検証、後始末（設計書 01-03「画像のファイルの読み取り」、03-02）。

use std::collections::BTreeMap;
use std::io::{self, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use super::{Format, OutputHeader, check_cancelled, inspect, invalid};
use crate::sandbox::run::{self, Leftovers, RunCommand, RunEnd, RunError, RunLimits, StopSignal};
use crate::sandbox::{self, SandboxCommand, SandboxError, SandboxStdin};
use crate::tools::{ImageHelperContext, MAX_IMAGE_BYTES, ToolError, text};

pub(super) enum ProcessError {
    Unavailable,
    Tool(ToolError),
}
impl From<ToolError> for ProcessError {
    fn from(error: ToolError) -> Self {
        Self::Tool(error)
    }
}
impl From<io::Error> for ProcessError {
    fn from(error: io::Error) -> Self {
        Self::Tool(ToolError::Io(error))
    }
}
impl From<SandboxError> for ProcessError {
    fn from(error: SandboxError) -> Self {
        match error {
            SandboxError::Unavailable => Self::Unavailable,
            SandboxError::Io(error) => error.into(),
            SandboxError::InvalidPolicy(_) | SandboxError::Setup { .. } => {
                invalid(text::IMAGE_HELPER_UNAVAILABLE).into()
            }
        }
    }
}

pub(super) fn resize(
    context: &ImageHelperContext,
    bytes: Vec<u8>,
    format: Format,
    width: u32,
    height: u32,
    cancelled: &AtomicBool,
) -> Result<Vec<u8>, ProcessError> {
    check_cancelled(cancelled)?;
    let policy = sandbox::image_policy(&context.helper)?;
    let format = match format {
        Format::Png => "png",
        Format::Jpeg => "jpeg",
        Format::Gif | Format::Webp => return Err(invalid(text::IMAGE_DECODE).into()),
    };
    let command = SandboxCommand {
        policy,
        program: context.helper.clone(),
        args: vec![
            "__image".into(),
            format.into(),
            width.to_string().into(),
            height.to_string().into(),
        ],
        cwd: PathBuf::from("/"),
        env: BTreeMap::new(),
        stdin: SandboxStdin::Piped,
    };
    let timeout = Duration::from_secs(30);
    #[cfg(test)]
    let timeout = context
        .test_command
        .as_ref()
        .map_or(timeout, |(_, limit)| *limit);
    #[cfg(test)]
    let command: RunCommand = if let Some((script, _)) = &context.test_command {
        use std::os::unix::process::CommandExt;
        let mut direct = std::process::Command::new("/bin/sh");
        direct
            .args(["-c", script])
            .current_dir("/")
            .env_clear()
            .process_group(0);
        direct.into()
    } else {
        prepare(command, context)?
    };
    #[cfg(not(test))]
    let command = prepare(command, context)?;

    let stop = Arc::new(AtomicBool::new(cancelled.load(Ordering::Acquire)));
    let exceeded = Arc::new(AtomicBool::new(false));
    let stdout = Arc::new(Mutex::new(Output {
        bytes: Vec::new(),
        cap: usize::try_from(MAX_IMAGE_BYTES)
            .unwrap_or(0)
            .saturating_add(4096),
        overflow: Some((Arc::clone(&exceeded), Arc::clone(&stop))),
    }));
    let stderr = Arc::new(Mutex::new(Output {
        bytes: Vec::new(),
        cap: 4096,
        overflow: None,
    }));
    let done = AtomicBool::new(false);
    let outcome = std::thread::scope(|scope| -> Result<_, ProcessError> {
        let watcher = std::thread::Builder::new().spawn_scoped(scope, || {
            while !done.load(Ordering::Acquire) {
                if cancelled.load(Ordering::Acquire) {
                    stop.store(true, Ordering::Release);
                    break;
                }
                std::thread::park_timeout(Duration::from_millis(10));
            }
        })?;
        let result = run::run(
            command,
            bytes,
            Arc::clone(&stdout),
            Arc::clone(&stderr),
            RunLimits {
                timeout,
                grace: Duration::from_secs(1),
                stop_signal: StopSignal::Terminate,
                ..RunLimits::default()
            },
            &stop,
        );
        done.store(true, Ordering::Release);
        watcher.thread().unpark();
        let _joined = watcher.join();
        result.map_err(|error| match error {
            RunError::Io(error) => ProcessError::from(error),
            RunError::Sandbox(error) => ProcessError::from(error),
        })
    });
    // 出力の超過による停止と本物の取り消しを区別する。途中の出力は使わない。
    if exceeded.load(Ordering::Acquire) {
        return Err(invalid(text::IMAGE_DECODE).into());
    }
    check_cancelled(cancelled)?;
    let outcome = outcome?;
    if outcome.leftovers != Leftovers::None || outcome.end != RunEnd::Exited {
        return Err(invalid(text::IMAGE_DECODE).into());
    }
    if !outcome.status.success() {
        let stderr = stderr.lock().unwrap_or_else(PoisonError::into_inner);
        // 固定の種類と十進の数字だけを信頼し、診断の生の文は LLM に渡さない。
        if let Some(size) = too_large(&stderr.bytes) {
            return Err(
                invalid(text::image_too_large_after_resizing(size, MAX_IMAGE_BYTES)).into(),
            );
        }
        return Err(invalid(text::IMAGE_DECODE).into());
    }
    let bytes = std::mem::take(&mut stdout.lock().unwrap_or_else(PoisonError::into_inner).bytes);
    validate(bytes, width, height).map_err(Into::into)
}

fn prepare(
    command: SandboxCommand,
    context: &ImageHelperContext,
) -> Result<RunCommand, ProcessError> {
    #[cfg(target_os = "linux")]
    {
        Ok(command
            .prepare(&context.helper, &context.scratch, &context.placeholders)?
            .into())
    }
    #[cfg(not(target_os = "linux"))]
    {
        Ok(command.to_command(&context.helper)?.into())
    }
}

fn too_large(bytes: &[u8]) -> Option<u64> {
    let line = std::str::from_utf8(bytes).ok()?.strip_suffix('\n')?;
    let number = line.strip_prefix("too-large ")?;
    if number.is_empty() || !number.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    number
        .parse::<u64>()
        .ok()
        .filter(|size| *size > MAX_IMAGE_BYTES)
}

fn validate(
    bytes: Vec<u8>,
    original_width: u32,
    original_height: u32,
) -> Result<Vec<u8>, ToolError> {
    let bad = || invalid(text::IMAGE_DECODE);
    let end = bytes
        .iter()
        .take(4096)
        .position(|byte| *byte == b'\n')
        .ok_or_else(bad)?;
    let metadata: OutputHeader =
        serde_json::from_slice(bytes.get(..end).ok_or_else(bad)?).map_err(|_| bad())?;
    let payload = bytes.get(end.saturating_add(1)..).ok_or_else(bad)?;
    if u64::try_from(payload.len()).unwrap_or(u64::MAX) > MAX_IMAGE_BYTES
        || !matches!(metadata.media_type.as_str(), "image/png" | "image/jpeg")
        || (metadata.original_width, metadata.original_height) != (original_width, original_height)
    {
        return Err(bad());
    }
    let (media_type, width, height) = inspect(payload).map_err(|_| bad())?;
    if media_type != metadata.media_type || (width, height) != (metadata.width, metadata.height) {
        return Err(bad());
    }
    Ok(payload.into())
}

struct Output {
    bytes: Vec<u8>,
    cap: usize,
    overflow: Option<(Arc<AtomicBool>, Arc<AtomicBool>)>,
}
impl Write for Output {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let remaining = self.cap.saturating_sub(self.bytes.len());
        if let Some((exceeded, stop)) = &self.overflow {
            if exceeded.load(Ordering::Acquire) {
                return Ok(bytes.len());
            }
            if bytes.len() > remaining {
                exceeded.store(true, Ordering::Release);
                stop.store(true, Ordering::Release);
            }
        }
        self.bytes
            .extend_from_slice(bytes.get(..bytes.len().min(remaining)).unwrap_or_default());
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Mode, ReadConfig, paths::DataPaths};
    use crate::safe_fs::ScopeRoot;
    use crate::session::{Session, SessionId};
    use crate::tools::{ApprovalStatus, Invocation, ToolContext, ToolSet, exclude::ExcludeRules};
    use serde_json::json;
    use std::collections::BTreeSet;
    use std::fs;
    use std::os::unix::fs::DirBuilderExt;
    use std::time::SystemTime;
    type TestResult = Result<(), Box<dyn std::error::Error>>;

    struct TestDirectory(PathBuf);
    impl TestDirectory {
        fn new(parent: &std::path::Path) -> io::Result<Self> {
            let mut random = [0; 16];
            getrandom::fill(&mut random).map_err(io::Error::other)?;
            let path = parent.join(format!("image-test-{:032x}", u128::from_ne_bytes(random)));
            std::fs::DirBuilder::new().mode(0o700).create(&path)?;
            Ok(Self(std::fs::canonicalize(path)?))
        }
    }
    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _removed = std::fs::remove_dir_all(&self.0);
        }
    }

    struct Fixture {
        directory: TestDirectory,
        project: PathBuf,
        private: PathBuf,
        session: Session,
        context: ToolContext,
    }
    impl Fixture {
        fn new(script: String, timeout: Duration) -> Result<Self, Box<dyn std::error::Error>> {
            let directory = TestDirectory::new(&std::env::temp_dir())?;
            let project = directory.0.join("project");
            let temporary = directory.0.join("temporary");
            fs::create_dir(&project)?;
            fs::create_dir(&temporary)?;
            let paths = DataPaths::from_values(
                Some(&directory.0),
                Some(&directory.0.join("config")),
                Some(&directory.0.join("state")),
                Some(&temporary),
            )?;
            let session = Session::create(&paths, SessionId::generate(SystemTime::now())?)?;
            let spill = session.directory().join("spill");
            let private = session.directory().join("sandbox");
            fs::create_dir(&private)?;
            let context = ToolContext::new(
                Mode::Restricted,
                ScopeRoot::open(&project)?,
                ScopeRoot::open(session.temporary_dir())?,
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
            .with_session(session.clone())
            .with_image_helper(ImageHelperContext {
                helper: fs::canonicalize(std::env::current_exe()?)?,
                scratch: private.clone(),
                placeholders: private.clone(),
                test_command: Some((script, timeout)),
            });
            let mut bytes = Vec::new();
            image::DynamicImage::ImageRgb8(image::RgbImage::from_pixel(
                2400,
                1,
                image::Rgb([1, 2, 3]),
            ))
            .write_with_encoder(image::codecs::png::PngEncoder::new(&mut bytes))?;
            fs::write(project.join("picture"), bytes)?;
            Ok(Self {
                directory,
                project,
                private,
                session,
                context,
            })
        }
        fn read(&self, cancelled: &AtomicBool) -> crate::tools::ToolOutput {
            let tools = ToolSet::new(Mode::Restricted);
            let call = tools.validate("read", json!({"path":"picture"}));
            match call {
                Ok(call) => tools.execute_validated(
                    &self.context,
                    &call,
                    ApprovalStatus::Unapproved,
                    Invocation {
                        call_id: "image-helper",
                        cancelled,
                        confirmed_effects: &BTreeMap::new(),
                        session_domains: &BTreeSet::new(),
                    },
                ),
                Err(error) => crate::tools::ToolOutput::error(error.to_string()),
            }
        }
        fn no_saved_image(&self, result: &crate::tools::ToolOutput, expected: &str) -> TestResult {
            assert!(
                result.is_error && result.text.contains(expected),
                "{}",
                result.text
            );
            assert!(result.images.is_empty());
            assert_eq!(
                self.context.read_version(&self.project.join("picture"))?,
                None
            );
            assert_eq!(
                fs::read_dir(self.session.directory().join("objects"))?.count(),
                0
            );
            assert_eq!(fs::read_dir(&self.private)?.count(), 0);
            assert!(self.directory.0.exists());
            Ok(())
        }
    }

    // 関門: 補助の正常な変換では捕まらない、信頼しない応答の拒否と保存前の停止を
    // 公開の道具の結果・objects・版の記録で確かめる。差し替えはこの cfg(test) だけ。
    #[test]
    fn invalid_helper_results_never_store_images_or_versions() -> TestResult {
        let mut payload = Vec::new();
        image::DynamicImage::ImageRgb8(image::RgbImage::from_pixel(1, 1, image::Rgb([1, 2, 3])))
            .write_with_encoder(image::codecs::png::PngEncoder::new(&mut payload))?;
        let octal: String = payload.iter().map(|byte| format!("\\{byte:03o}")).collect();
        let correct = json!({"media_type":"image/png","width":1,"height":1,"original_width":2400,"original_height":1});
        let mut scripts = Vec::new();
        for (key, value) in [
            ("media_type", json!("image/jpeg")),
            ("width", json!(2)),
            ("height", json!(2)),
            ("original_width", json!(2401)),
            ("original_height", json!(2)),
            ("extra", json!(true)),
        ] {
            let mut metadata = correct.clone();
            metadata
                .as_object_mut()
                .ok_or("metadata object")?
                .insert(key.into(), value);
            scripts.push(format!("printf '%s\\n' '{metadata}'; printf '{octal}'"));
        }
        scripts.extend([
            "printf 'not-json\\n'".into(),
            "printf '{}'".into(),
            format!("printf '%s%4096s\\n' '{correct}' ''; printf '{octal}'"),
            format!("printf '%s\\n' '{correct}'; /usr/bin/head -c 3932161 /dev/zero"),
            "/usr/bin/head -c 4000000 /dev/zero; /bin/sleep 5".into(),
            "kill -KILL $$".into(),
            "/bin/sleep 5".into(),
            "printf 'untrusted secret\\n' >&2; exit 1".into(),
            "printf 'too-large 1\\n' >&2; exit 1".into(),
            "printf 'too-large 4000000\\nextra\\n' >&2; exit 1".into(),
        ]);
        for script in scripts {
            let timeout = if script == "/bin/sleep 5" {
                Duration::from_millis(100)
            } else {
                Duration::from_secs(3)
            };
            let overflow = script.starts_with("/usr/bin/head -c 4000000");
            let fixture = Fixture::new(script, timeout)?;
            let start = std::time::Instant::now();
            fixture.no_saved_image(&fixture.read(&AtomicBool::new(false)), text::IMAGE_DECODE)?;
            if overflow {
                assert!(start.elapsed() < Duration::from_secs(2));
            }
        }
        let fixture = Fixture::new(
            format!("printf '%s\\n' '{correct}'; printf '{octal}'"),
            Duration::from_secs(3),
        )?;
        let output = fixture.read(&AtomicBool::new(false));
        assert!(!output.is_error, "{}", output.text);
        assert_eq!(output.images.len(), 1);
        assert!(
            fixture
                .context
                .read_version(&fixture.project.join("picture"))?
                .is_some()
        );
        assert_eq!(fs::read_dir(&fixture.private)?.count(), 0);
        let fixture = Fixture::new(
            "printf 'too-large 4000000\\n' >&2; exit 1".into(),
            Duration::from_secs(2),
        )?;
        fixture.no_saved_image(
            &fixture.read(&AtomicBool::new(false)),
            &text::image_too_large_after_resizing(4_000_000, MAX_IMAGE_BYTES),
        )?;
        Ok(())
    }

    // 関門: 変換の失敗を扱う既存のテストでは、起動前の方針検証の誤りを区別できない。
    // 実際の不正な方針を公開の read に渡し、誤った案内と画像・版の保存を防ぐ。
    #[test]
    fn helper_policy_preparation_failure_is_reported_without_decoding_or_saving() -> TestResult {
        let mut fixture = Fixture::new(String::new(), Duration::from_secs(2))?;
        fixture
            .context
            .image_helper
            .as_mut()
            .ok_or("helper context")?
            .helper = fixture.directory.0.join(".").join("helper");
        fixture.no_saved_image(
            &fixture.read(&AtomicBool::new(false)),
            "The image resizing helper could not be started.",
        )
    }

    #[test]
    fn cancellation_stops_running_helper_without_creating_workdir() -> TestResult {
        let mut fixture = Fixture::new(String::new(), Duration::from_secs(10))?;
        let ready = fixture.directory.0.join("ready");
        fixture
            .context
            .image_helper
            .as_mut()
            .ok_or("helper context")?
            .test_command = Some((
            format!("printf ready > '{}'; /bin/sleep 5", ready.display()),
            Duration::from_secs(10),
        ));
        let cancelled = AtomicBool::new(false);
        std::thread::scope(|scope| -> TestResult {
            let reader = scope.spawn(|| fixture.read(&cancelled));
            let start = std::time::Instant::now();
            loop {
                if ready.exists() {
                    break;
                }
                if start.elapsed() > Duration::from_secs(2) {
                    return Err("helper did not start".into());
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            cancelled.store(true, Ordering::Release);
            let result = reader.join().map_err(|_| "read thread failed")?;
            fixture.no_saved_image(&result, text::IMAGE_CANCELLED)
        })
    }

    #[test]
    #[ignore = "manual release measurement; launches the real image helper without the OS sandbox"]
    fn measure_parent_read_with_direct_image_helper() -> TestResult {
        let exe = fs::canonicalize(
            std::env::var_os("BENITOITEH_IMAGE_BENCH_BINARY")
                .ok_or("set BENITOITEH_IMAGE_BENCH_BINARY")?,
        )?;
        let script = format!(
            "exec '{}' __image png 2400 1200",
            exe.to_string_lossy().replace('\'', "'\\''")
        );
        let fixture = Fixture::new(script, Duration::from_secs(30))?;
        let source = image::DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(
            2400,
            1200,
            image::Rgba([48, 96, 192, 255]),
        ));
        let mut bytes = Vec::new();
        source.write_with_encoder(image::codecs::png::PngEncoder::new(&mut bytes))?;
        for _ in 0..5 {
            let start = std::time::Instant::now();
            fs::write(fixture.project.join("picture"), &bytes)?;
            let output = fixture.read(&AtomicBool::new(false));
            eprintln!(
                "P5-15 parent read with direct helper: {:?}",
                start.elapsed()
            );
            assert!(!output.is_error, "{}", output.text);
        }
        Ok(())
    }
}
