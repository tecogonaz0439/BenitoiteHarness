//! 要求したパスを開き、記述子の実のパスを再検査する（設計書 01-03「道具の定義の共通の規則」）。

use super::{ToolContext, ToolError, text};
use crate::config::Mode;
use crate::safe_fs::{ScopeRelPath, ScopeRoot, resolved_path};
use std::fs::File;
use std::path::{Path, PathBuf};

pub(super) struct OpenedFile {
    pub file: File,
    pub real_path: PathBuf,
}

impl ToolContext {
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
        let mut roots = if search {
            vec![&self.project]
        } else {
            vec![&self.project, &self.temporary, &self.spill]
        };
        if !search {
            roots.extend(&self.skill_roots);
        }
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
        let file = File::from(
            rustix::fs::open(
                real_path,
                rustix::fs::OFlags::RDONLY
                    | rustix::fs::OFlags::NONBLOCK
                    | rustix::fs::OFlags::CLOEXEC,
                rustix::fs::Mode::empty(),
            )
            .map_err(std::io::Error::from)?,
        );
        self.check_file(file, None)
    }

    pub(super) fn check_file(
        &self,
        file: File,
        scope: Option<&ScopeRoot>,
    ) -> Result<OpenedFile, ToolError> {
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

    pub(super) fn search_path(&self, requested: &Path) -> Result<SearchPath<'_>, ToolError> {
        let absolute = self.requested_path(requested);
        let real_path = std::fs::canonicalize(&absolute)?;
        if let Some(reason) = self.exclude.reason(&real_path) {
            return Err(ToolError::Excluded(reason));
        }
        if self.mode == Mode::Restricted {
            if real_path == self.project.real_path()
                && (requested == Path::new(".") || absolute == self.project.real_path())
            {
                return Ok(SearchPath {
                    real_path,
                    scope: Some(&self.project),
                    relative: None,
                });
            }
            // 起点を開く処理は検索側で行うが、従来の相対パスの構文と範囲の
            // 検査は残す（設計書 01-03「道具の定義の共通の規則」）。
            for root in std::iter::once(&self.project).chain(&self.skill_roots) {
                // 根そのものには ScopeRelPath を作れない。絶対パスの構文は
                // 一つ下の候補で確かめ、起点は保持した記述子を複製する。
                if real_path == root.real_path() {
                    root.resolve_path(self.project.real_path(), &requested.join("SKILL.md"))?;
                    return Ok(SearchPath {
                        real_path,
                        scope: Some(root),
                        relative: None,
                    });
                }
                if root
                    .resolve_path(self.project.real_path(), requested)
                    .is_ok()
                    && real_path.starts_with(root.real_path())
                {
                    // resolve_path は最後のリンクの名前を保つ。検索の起点には
                    // 実のファイルの親を使い、記述子もその親から開く（設計書 01-03「glob」）。
                    let relative = ScopeRelPath::new(
                        real_path
                            .strip_prefix(root.real_path())
                            .map_err(|_| ToolError::OutsideScope)?,
                    )?;
                    return Ok(SearchPath {
                        real_path,
                        scope: Some(root),
                        relative: Some(relative),
                    });
                }
            }
            return Err(ToolError::OutsideScope);
        }
        // 通常モードは従来どおり範囲を限らない。保持した根に当たるときは
        // 検索もその記述子から始める（設計書 01-04「利用者の Agent Skill」）。
        for root in std::iter::once(&self.project).chain(&self.skill_roots) {
            if let Ok(relative) = real_path.strip_prefix(root.real_path()) {
                return Ok(SearchPath {
                    scope: Some(root),
                    relative: if relative.as_os_str().is_empty() {
                        None
                    } else {
                        Some(ScopeRelPath::new(relative)?)
                    },
                    real_path,
                });
            }
        }
        Ok(SearchPath {
            real_path,
            scope: None,
            relative: None,
        })
    }
}

pub(super) struct SearchPath<'a> {
    pub real_path: PathBuf,
    pub scope: Option<&'a ScopeRoot>,
    pub relative: Option<ScopeRelPath>,
}

impl crate::benitoite::diagnostics::DiagnosticSources for ToolContext {
    fn exclusion_cost(&self, file: &str) -> Option<usize> {
        self.exclude
            .diagnostic_cost(&self.requested_path(Path::new(file)))
    }
    fn resolve(&self, file: &str) -> crate::benitoite::diagnostics::DiagnosticFile {
        use crate::benitoite::diagnostics::{bounded_path, relative_text};
        if !bounded_path(file) || self.exclusion_cost(file).is_none() {
            return crate::benitoite::diagnostics::DiagnosticFile {
                path: file.into(),
                display: None,
                exists: false,
                relative: None,
            };
        }
        let requested = self.requested_path(Path::new(file));
        let relative = requested
            .strip_prefix(self.project.real_path())
            .ok()
            .map(Path::to_path_buf);
        // 除外した候補の存在は調べない。後ろの部分も読み取りの設定だけで判定する。
        // （設計書 04-01「終了状態と診断の受け渡し」、BH-029）。
        let relative_string = relative.as_ref().and_then(|path| relative_text(path));
        if self
            .exclude
            .diagnostic_hidden(&requested, relative_string.as_deref())
        {
            return crate::benitoite::diagnostics::DiagnosticFile {
                path: requested,
                display: None,
                exists: false,
                relative,
            };
        }
        match self.open_file(&requested, false) {
            Ok(opened) => {
                let display = opened
                    .real_path
                    .strip_prefix(self.project.real_path())
                    .unwrap_or(&opened.real_path)
                    .to_path_buf();
                crate::benitoite::diagnostics::DiagnosticFile {
                    path: opened.real_path,
                    display: Some(display),
                    exists: true,
                    relative,
                }
            }
            Err(_) => crate::benitoite::diagnostics::DiagnosticFile {
                path: requested,
                display: None,
                exists: false,
                relative,
            },
        }
    }

    fn line(&self, file: &Path, line: u64) -> Option<crate::benitoite::diagnostics::SourceLine> {
        super::read::diagnostic_line(self.open_file(file, false).ok()?, line)
            .ok()
            .flatten()
    }

    fn content_hash(
        &self,
        file: &Path,
        cancelled: &std::sync::atomic::AtomicBool,
    ) -> Option<String> {
        super::read::content_hash(self.open_file(file, false).ok()?, cancelled).ok()
    }
}
