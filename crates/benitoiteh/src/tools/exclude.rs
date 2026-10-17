//! 実のパスで既定と追加の除外を判定する（設計書 01-03「読まないファイル」）。

use super::{ToolError, text};
use crate::config::{ReadConfig, paths::DataPaths};
use globset::{Glob, GlobMatcher};
use std::path::{Path, PathBuf};

/// 利用者とプロジェクトの設定が、既定の除外を減らせない規則。
#[derive(Clone, Debug)]
pub struct ExcludeRules {
    project: PathBuf,
    config: PathBuf,
    data: PathBuf,
    spill: PathBuf,
    additional: Vec<(String, GlobMatcher)>,
}

impl ExcludeRules {
    /// 存在しない設定ディレクトリも、存在する祖先の実のパスから固定する。
    pub fn new(
        project: &Path,
        paths: &DataPaths,
        spill: &Path,
        user: &ReadConfig,
        project_config: &ReadConfig,
    ) -> Result<Self, ToolError> {
        let mut additional = Vec::new();
        for pattern in user.exclude.iter().chain(&project_config.exclude) {
            let matcher = Glob::new(pattern)
                .map_err(|error| ToolError::Pattern(text::pattern(&error)))?
                .compile_matcher();
            additional.push((pattern.clone(), matcher));
        }
        Ok(Self {
            project: normalize_existing(project)?,
            config: normalize_existing(&paths.config_dir)?,
            data: normalize_existing(&paths.data_dir)?,
            spill: normalize_existing(spill)?,
            additional,
        })
    }

    /// 開いたものの実のパスについて、除外の理由を返す。
    pub fn reason(&self, real_path: &Path) -> Option<String> {
        self.default_reason(real_path)
            .or_else(|| self.additional_reason(real_path))
    }

    fn default_reason(&self, real_path: &Path) -> Option<String> {
        for component in real_path.components() {
            let name = component.as_os_str().to_string_lossy();
            if secret_component(&name) {
                return Some(format!("{} ({name})", text::DEFAULT_SECRET));
            }
        }
        self.directory_reason(real_path)
    }

    fn directory_reason(&self, real_path: &Path) -> Option<String> {
        if !real_path.starts_with(&self.spill) {
            if real_path.starts_with(&self.config) {
                return Some(text::CONFIG_DIRECTORY.to_owned());
            }
            if real_path.starts_with(&self.data) {
                return Some(text::DATA_DIRECTORY.to_owned());
            }
        }
        None
    }

    fn additional_reason(&self, real_path: &Path) -> Option<String> {
        for (pattern, matcher) in &self.additional {
            for ancestor in real_path.ancestors() {
                let relative = ancestor.strip_prefix(&self.project).unwrap_or(ancestor);
                if matcher.is_match(ancestor)
                    || matcher.is_match(relative)
                    || ancestor
                        .file_name()
                        .is_some_and(|name| matcher.is_match(Path::new(name)))
                {
                    return Some(pattern.clone());
                }
            }
        }
        None
    }

    /// 診断の照合量を、後ろの部分の要素数の和と設定した型の数から見積もる。
    pub(crate) fn diagnostic_cost(&self, path: &Path) -> Option<usize> {
        let (path, prefix) = match path.strip_prefix(&self.project) {
            Ok(relative) => (relative, self.project.components().count()),
            Err(_) => (path, 0),
        };
        let count = path.components().take(33).count();
        if count > 32 {
            return None;
        }
        Some(
            count
                .saturating_mul(count.saturating_add(1))
                .checked_div(2)
                .unwrap_or(usize::MAX)
                .saturating_add(count.saturating_mul(prefix))
                .saturating_mul(self.additional.len()),
        )
    }

    /// 秘密の要素名は全体で一度だけ調べ、後ろの部分にはディレクトリと型だけを当てる。
    pub(crate) fn diagnostic_hidden(&self, path: &Path, relative: Option<&str>) -> bool {
        if self.default_reason(path).is_some() || self.additional_reason(path).is_some() {
            return true;
        }
        relative.is_some_and(|relative| {
            let mut candidate = self.project.to_string_lossy().into_owned();
            candidate.push('/');
            let prefix = candidate.len();
            crate::benitoite::diagnostics::suffixes(relative).any(|suffix| {
                candidate.truncate(prefix);
                candidate.push_str(suffix);
                let candidate = Path::new(&candidate);
                self.directory_reason(candidate).is_some()
                    || self.additional_reason(candidate).is_some()
            })
        })
    }

    /// 除外されていない親の直下の一要素だけを判定する。木の走査は除外した親へ降りない。
    pub(crate) fn reason_entry(&self, real_path: &Path) -> Option<String> {
        let name = real_path.file_name()?.to_string_lossy();
        if secret_component(&name) {
            return Some(format!("{} ({name})", text::DEFAULT_SECRET));
        }
        if !real_path.starts_with(&self.spill) {
            if real_path.starts_with(&self.config) {
                return Some(text::CONFIG_DIRECTORY.into());
            }
            if real_path.starts_with(&self.data) {
                return Some(text::DATA_DIRECTORY.into());
            }
        }
        let relative = real_path.strip_prefix(&self.project).unwrap_or(real_path);
        self.additional
            .iter()
            .find(|(_, matcher)| {
                matcher.is_match(real_path)
                    || matcher.is_match(relative)
                    || matcher.is_match(Path::new(name.as_ref()))
            })
            .map(|(pattern, _)| pattern.clone())
    }

    pub(super) fn descend(&self, real_path: &Path) -> bool {
        // data/ 自体の除外で spill/ への走査まで切らない（設計書 01-03「読まないファイル」）。
        self.reason(real_path).is_none()
            || (self.spill.starts_with(real_path)
                && !real_path
                    .components()
                    .any(|part| secret_component(&part.as_os_str().to_string_lossy())))
    }
}

fn secret_component(name: &str) -> bool {
    (name == ".env" || name.starts_with(".env.")) && name != ".env.example" && name != ".env.sample"
        || [
            ".ssh",
            ".gnupg",
            ".aws",
            ".netrc",
            ".git-credentials",
            ".git",
        ]
        .contains(&name)
        || [".pem", ".key", ".p12", ".pfx"]
            .iter()
            .any(|ending| name.ends_with(ending))
        || ["id_rsa", "id_ecdsa", "id_ed25519"]
            .iter()
            .any(|prefix| name.starts_with(prefix))
}

fn normalize_existing(path: &Path) -> Result<PathBuf, ToolError> {
    match std::fs::canonicalize(path) {
        Ok(path) => Ok(path),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let parent = path.parent().ok_or(ToolError::OutsideScope)?;
            let name = path.file_name().ok_or(ToolError::OutsideScope)?;
            Ok(normalize_existing(parent)?.join(name))
        }
        Err(error) => Err(error.into()),
    }
}
