//! 書く先の親を保持し、実のパスで範囲と守る場所を判定する（設計書 01-03「書く道具のパスの規則」）。

use std::fs::Metadata;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

use super::{ApprovalStatus, FileVersion, ToolContext, ToolError, ToolOutput, text};
use crate::config::Mode;
use crate::safe_fs::{SafeFsError, ScopeRelPath, ScopeRoot, resolved_path};

pub(super) struct WriteTarget {
    parent: ScopeRoot,
    remaining: ScopeRelPath,
    pub real_path: PathBuf,
}

impl WriteTarget {
    pub fn resolve(context: &ToolContext, requested: &Path) -> Result<Self, ToolError> {
        let mut last_error = ToolError::OutsideScope;
        for root in [&context.project, &context.temporary] {
            match root.resolve_path(context.project.real_path(), requested) {
                Ok(relative) => return Self::from_scope(root, relative),
                Err(error) => last_error = error.into(),
            }
        }
        if context.mode == Mode::Restricted {
            return Err(last_error);
        }
        // 通常モードの範囲外も、確認の後の書き込みに safe_fs を使う。
        let root = ScopeRoot::open("/")?;
        let relative = root.resolve_path(context.project.real_path(), requested)?;
        Self::from_scope(&root, relative)
    }

    fn from_scope(root: &ScopeRoot, relative: ScopeRelPath) -> Result<Self, ToolError> {
        let mut prefix = relative
            .as_path()
            .parent()
            .ok_or(SafeFsError::InvalidPath)?;
        loop {
            let opened = if prefix.as_os_str().is_empty() {
                ScopeRoot::open(root.real_path())
            } else {
                root.open_dir(&ScopeRelPath::new(prefix)?)
            };
            match opened {
                Ok(parent) => {
                    if !parent.real_path().starts_with(root.real_path()) {
                        return Err(ToolError::OutsideScope);
                    }
                    let remaining = ScopeRelPath::new(
                        relative
                            .as_path()
                            .strip_prefix(prefix)
                            .map_err(|_| SafeFsError::InvalidPath)?,
                    )?;
                    let real_path = parent.real_path().join(remaining.as_path());
                    return Self {
                        parent,
                        remaining,
                        real_path,
                    }
                    .with_real_file_name();
                }
                Err(SafeFsError::NotFound(_)) => {
                    prefix = prefix.parent().ok_or(SafeFsError::InvalidPath)?;
                }
                Err(error) => return Err(error.into()),
            }
        }
    }

    fn with_real_file_name(mut self) -> Result<Self, ToolError> {
        let Some(version) = self.version()? else {
            return Ok(self);
        };
        let file = self.parent.open_read(&self.remaining)?;
        let metadata = file.metadata()?;
        check_metadata(&metadata)?;
        if FileVersion::from_metadata(&metadata) != version {
            return Err(ToolError::StaleRead);
        }
        // 大文字小文字を区別しない FS では、要求した最後の名前も実名と異なる。
        // 拡張子と read の記録に、開いたファイルの実名を使う
        // （設計書 01-03「書く道具のパスの規則」「読んでから直す規則」）。
        let real_path = resolved_path(&file)?;
        if real_path.parent() != Some(self.parent.real_path()) {
            return Err(ToolError::StaleRead);
        }
        self.remaining = ScopeRelPath::new(Path::new(
            real_path.file_name().ok_or(SafeFsError::InvalidPath)?,
        ))?;
        self.real_path = real_path;
        Ok(self)
    }

    pub fn approval(
        &self,
        context: &ToolContext,
        approval: ApprovalStatus,
    ) -> Result<Option<ToolOutput>, ToolError> {
        let within = [&context.project, &context.temporary]
            .into_iter()
            .find_map(|root| self.real_path.strip_prefix(root.real_path()).ok());
        let protected = within.is_some_and(|path| {
            path.components().any(|part| {
                let name = part.as_os_str().as_encoded_bytes();
                name.eq_ignore_ascii_case(b".git") || name.eq_ignore_ascii_case(b".benitoiteh")
            })
        });
        if context.mode == Mode::Restricted {
            if within.is_none() {
                return Err(ToolError::OutsideScope);
            }
            if protected {
                return Err(ToolError::ProtectedWrite);
            }
            if self.real_path.extension().is_none_or(|ext| ext != "bnt") {
                return Err(ToolError::RestrictedExtension);
            }
        } else if (within.is_none() || protected) && approval == ApprovalStatus::Unapproved {
            return Ok(Some(ToolOutput::needs_approval(text::write_approval(
                &self.real_path,
                protected,
            ))));
        }
        Ok(None)
    }

    pub fn version(&self) -> Result<Option<FileVersion>, ToolError> {
        match self.parent.metadata(&self.remaining) {
            Ok(metadata) => {
                check_metadata(&metadata)?;
                Ok(Some(FileVersion::from_metadata(&metadata)))
            }
            Err(SafeFsError::NotFound(_)) => Ok(None),
            Err(error) => Err(error.into()),
        }
    }

    pub fn check_read(
        &self,
        context: &ToolContext,
        current: &FileVersion,
    ) -> Result<(), ToolError> {
        match context.read_version(&self.real_path)? {
            None => Err(ToolError::NotRead),
            Some(version) if &version != current => Err(ToolError::StaleRead),
            Some(_) => Ok(()),
        }
    }

    pub fn read(&self, version: &FileVersion) -> Result<Vec<u8>, ToolError> {
        use std::io::Read;
        let mut file = self.parent.open_read(&self.remaining)?;
        let mut contents = Vec::new();
        check_metadata(&file.metadata()?)?;
        if FileVersion::from_metadata(&file.metadata()?) != *version {
            return Err(ToolError::StaleRead);
        }
        file.read_to_end(&mut contents)?;
        if FileVersion::from_metadata(&file.metadata()?) != *version {
            return Err(ToolError::StaleRead);
        }
        Ok(contents)
    }

    pub fn write(
        mut self,
        context: &ToolContext,
        previous: Option<FileVersion>,
        contents: &[u8],
    ) -> Result<(), ToolError> {
        // 未作成の親も一段ずつ開く。判定した親をパスから開き直さない
        // （設計書 01-03「書く道具のパスの規則」）。
        let parent_path = self
            .remaining
            .as_path()
            .parent()
            .ok_or(SafeFsError::InvalidPath)?;
        let mut expected_parent = self.parent.real_path().to_path_buf();
        for component in parent_path.components() {
            let name = ScopeRelPath::new(Path::new(component.as_os_str()))?;
            match self.parent.create_dir(&name) {
                Ok(()) | Err(SafeFsError::AlreadyExists(_)) => {}
                Err(error) => return Err(error.into()),
            }
            self.parent = self.parent.open_dir(&name)?;
            expected_parent.push(component);
            if self.parent.real_path() != expected_parent {
                return Err(ToolError::StaleRead);
            }
        }
        let name = self
            .remaining
            .as_path()
            .file_name()
            .ok_or(SafeFsError::InvalidPath)?;
        self.remaining = ScopeRelPath::new(Path::new(name))?;
        if self.parent.real_path().join(self.remaining.as_path()) != self.real_path {
            return Err(ToolError::StaleRead);
        }
        if self.version()? != previous {
            return Err(ToolError::StaleRead);
        }
        self.parent.write(&self.remaining, contents, false)?;
        let version = self.version()?.ok_or(ToolError::StaleRead)?;
        context.record_read_version(self.real_path, version)
    }
}

fn check_metadata(metadata: &Metadata) -> Result<(), ToolError> {
    if metadata.file_type().is_symlink() {
        return Err(SafeFsError::FinalSymlink.into());
    }
    if !metadata.is_file() {
        return Err(SafeFsError::NotRegularFile.into());
    }
    if metadata.nlink() > 1 {
        return Err(SafeFsError::MultipleHardLinks.into());
    }
    Ok(())
}
