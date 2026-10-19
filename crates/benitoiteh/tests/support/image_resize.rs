//! 画像の道具の公開の入口と保存結果を確かめる準備（設計書 01-03）。
use std::error::Error;
use std::fs;
use std::os::unix::fs::DirBuilderExt;
use std::path::PathBuf;
use std::time::SystemTime;

use benitoiteh::config::{Mode, ReadConfig, paths::DataPaths};
use benitoiteh::safe_fs::ScopeRoot;
use benitoiteh::session::{Session, SessionId};
use benitoiteh::tools::exclude::ExcludeRules;
use benitoiteh::tools::{FileVersion, MAX_IMAGE_BYTES, ToolContext, ToolOutput, ToolSet};
use image::codecs::png::PngEncoder;
use image::{DynamicImage, ImageFormat};
use serde_json::json;

pub struct Fixture {
    pub directory: PathBuf,
    pub project: PathBuf,
    pub context: ToolContext,
    pub session: Session,
}

impl Fixture {
    pub fn new(mode: Mode, available: bool) -> Result<Self, Box<dyn Error>> {
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
        let mut context = ToolContext::new(
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
        if available {
            let private = session.directory().join("sandbox");
            fs::create_dir(&private)?;
            let placeholders = directory.join("placeholders");
            fs::DirBuilder::new().mode(0o700).create(&placeholders)?;
            context = context.with_image_helper(benitoiteh::tools::ImageHelperContext {
                helper: fs::canonicalize(env!("CARGO_BIN_EXE_benitoiteh"))?,
                scratch: private.clone(),
                placeholders,
                work_parent: private,
            });
        }
        Ok(Self {
            directory,
            project,
            context,
            session,
        })
    }

    pub fn read(&self, bytes: &[u8]) -> Result<ToolOutput, Box<dyn Error>> {
        fs::write(self.project.join("picture"), bytes)?;
        Ok(ToolSet::new(self.context.mode).execute(
            &self.context,
            "read",
            json!({"path":"picture"}),
        ))
    }

    pub fn check_image(
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

pub fn png(image: &DynamicImage) -> Result<Vec<u8>, image::ImageError> {
    let mut bytes = Vec::new();
    image.write_with_encoder(PngEncoder::new(&mut bytes))?;
    Ok(bytes)
}
