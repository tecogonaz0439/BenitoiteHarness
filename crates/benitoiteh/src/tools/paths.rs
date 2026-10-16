//! 要求したパスを開き、記述子の実のパスを再検査する（設計書 01-03「道具の定義の共通の規則」）。

use super::{ToolContext, ToolError, text};
use crate::config::Mode;
use crate::safe_fs::{ScopeRoot, resolved_path};
use std::fs::File;
use std::path::{Path, PathBuf};

pub(super) struct OpenedFile {
    pub file: File,
    pub real_path: PathBuf,
}

impl ToolContext {
    pub(super) fn open_walk_file(
        &self,
        root: &ScopeRoot,
        path: &Path,
    ) -> Result<OpenedFile, ToolError> {
        let relative = root.resolve_path(root.real_path(), path)?;
        self.check_file(root.open_read(&relative)?, Some(root))
    }

    pub(super) fn requested_path(&self, requested: &Path) -> PathBuf {
        if requested.is_absolute() {
            requested.to_path_buf()
        } else {
            self.project.real_path().join(requested)
        }
    }

    pub(super) fn open_file(
        &self,
        requested: &Path,
        search: bool,
    ) -> Result<OpenedFile, ToolError> {
        let roots = if search {
            vec![&self.project]
        } else {
            vec![&self.project, &self.temporary, &self.spill]
        };
        let mut last_error = ToolError::OutsideScope;
        for root in roots {
            match root.resolve_path(self.project.real_path(), requested) {
                Ok(relative) => {
                    let file = match root.open_read(&relative) {
                        Ok(file) => file,
                        Err(error) => {
                            last_error = error.into();
                            continue;
                        }
                    };
                    return self.check_file(file, Some(root));
                }
                Err(error) => last_error = error.into(),
            }
        }
        if self.mode == Mode::Restricted {
            return Err(last_error);
        }
        // 通常モードの範囲を限らない読み取りだけは標準のパス解決を使える。
        let real_path = std::fs::canonicalize(self.requested_path(requested))?;
        if let Some(reason) = self.exclude.reason(&real_path) {
            return Err(ToolError::Excluded(reason));
        }
        if !std::fs::metadata(&real_path)?.is_file() {
            return Err(ToolError::InvalidArguments(text::REGULAR_FILE.to_owned()));
        }
        self.check_file(File::open(real_path)?, None)
    }

    fn check_file(&self, file: File, scope: Option<&ScopeRoot>) -> Result<OpenedFile, ToolError> {
        let real_path = resolved_path(&file)?;
        if scope.is_some_and(|root| !real_path.starts_with(root.real_path())) {
            return Err(ToolError::OutsideScope);
        }
        if let Some(reason) = self.exclude.reason(&real_path) {
            return Err(ToolError::Excluded(reason));
        }
        if !file.metadata()?.is_file() {
            return Err(ToolError::InvalidArguments(text::REGULAR_FILE.to_owned()));
        }
        Ok(OpenedFile { file, real_path })
    }

    pub(super) fn search_path(&self, requested: &Path) -> Result<PathBuf, ToolError> {
        let absolute = self.requested_path(requested);
        let real_path = std::fs::canonicalize(&absolute)?;
        if let Some(reason) = self.exclude.reason(&real_path) {
            return Err(ToolError::Excluded(reason));
        }
        if self.mode == Mode::Restricted {
            if real_path == self.project.real_path()
                && (requested == Path::new(".") || absolute == self.project.real_path())
            {
                return Ok(real_path);
            }
            let relative = self
                .project
                .resolve_path(self.project.real_path(), requested)?;
            if std::fs::metadata(&absolute)?.is_dir() {
                let opened = self.project.open_dir(&relative)?;
                if !opened.real_path().starts_with(self.project.real_path()) {
                    return Err(ToolError::OutsideScope);
                }
            } else {
                self.open_file(requested, true)?;
            }
        }
        Ok(real_path)
    }
}
