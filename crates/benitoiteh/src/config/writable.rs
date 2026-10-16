//! 実のパスで書ける場所の四つの規則を検査する（設計書 03-02「書ける場所の検査」）。

use std::fmt;
use std::io;
use std::path::{Component, Path, PathBuf};

use super::{paths::DataPaths, text};

/// 拒否の理由を呼び出し元が区別できる規則。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WritableRule {
    /// 根、ホーム、ホームの祖先。
    RootOrHome,
    /// ホームの隠しディレクトリと macOS Library。
    HomePrivate,
    /// 設定・状態と重なる。
    ConfigOrData,
    /// プロジェクトと一時領域が重なる。
    ProjectTemporary,
}

/// 実パスへの変換の失敗、または拒否した規則と場所。
#[derive(Debug)]
pub enum WritableError {
    /// パスを安全に実パスへ変換できなかった。
    Resolve { path: PathBuf, source: io::Error },
    /// 規則に当たった二つのパス。
    Rejected {
        rule: WritableRule,
        path: PathBuf,
        other: PathBuf,
    },
}

impl fmt::Display for WritableError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Resolve { path, source } => write!(f, "{}: {source}", path.display()),
            Self::Rejected { rule, path, other } => {
                let message = match rule {
                    WritableRule::RootOrHome => text::ROOT_OR_HOME,
                    WritableRule::HomePrivate => text::HOME_PRIVATE,
                    WritableRule::ConfigOrData => text::CONFIG_OR_DATA,
                    WritableRule::ProjectTemporary => text::PROJECT_TEMP,
                };
                write!(f, "{message}: {} ({})", path.display(), other.display())
            }
        }
    }
}
impl std::error::Error for WritableError {}

/// 検査に通った実のパス。サンドボックスにはこの値を渡す。
#[derive(Debug)]
pub struct WritableLocations {
    pub project: PathBuf,
    pub temporary: PathBuf,
    pub additional: Vec<PathBuf>,
}

/// 通常モードでだけ追加の場所を渡す。まだない一時領域は実の TMPDIR に ID を連結して調べる。
pub fn validate_writable(
    paths: &DataPaths,
    project: &Path,
    temporary: &Path,
    additional: &[PathBuf],
) -> Result<WritableLocations, WritableError> {
    let home = real_existing(&paths.home)?;
    let config = real_future(&paths.config_dir)?;
    let data = real_future(&paths.data_dir)?;
    let locations = WritableLocations {
        project: real_existing(project)?,
        temporary: real_future(temporary)?,
        additional: additional
            .iter()
            .map(|path| real_existing(path))
            .collect::<Result<_, _>>()?,
    };
    for path in std::iter::once(&locations.project)
        .chain(std::iter::once(&locations.temporary))
        .chain(&locations.additional)
    {
        if home.starts_with(path) || path.parent().is_none() {
            return Err(reject(WritableRule::RootOrHome, path, &home));
        }
        if let Ok(relative) = path.strip_prefix(&home) {
            let first = relative.components().next();
            if first.is_some_and(|component| {
                let name = component.as_os_str();
                use std::os::unix::ffi::OsStrExt;
                name.as_bytes().starts_with(b".")
                    || (cfg!(target_os = "macos") && name == "Library")
            }) {
                return Err(reject(WritableRule::HomePrivate, path, &home));
            }
        }
        for protected in [&config, &data] {
            if overlaps(path, protected) {
                return Err(reject(WritableRule::ConfigOrData, path, protected));
            }
        }
    }
    if overlaps(&locations.project, &locations.temporary) {
        return Err(reject(
            WritableRule::ProjectTemporary,
            &locations.project,
            &locations.temporary,
        ));
    }
    Ok(locations)
}

fn reject(rule: WritableRule, path: &Path, other: &Path) -> WritableError {
    WritableError::Rejected {
        rule,
        path: path.to_path_buf(),
        other: other.to_path_buf(),
    }
}

fn overlaps(left: &Path, right: &Path) -> bool {
    left.starts_with(right) || right.starts_with(left)
}

fn real_existing(path: &Path) -> Result<PathBuf, WritableError> {
    let real = std::fs::canonicalize(path).map_err(|source| WritableError::Resolve {
        path: path.to_path_buf(),
        source,
    })?;
    if !real.is_dir() {
        return Err(WritableError::Resolve {
            path: path.to_path_buf(),
            source: io::Error::new(io::ErrorKind::NotADirectory, text::DIRECTORY_REQUIRED),
        });
    }
    Ok(real)
}

// まだない設定と一時領域の末尾だけを保存する。リンクを未作成の名前と取り違えない。
fn real_future(path: &Path) -> Result<PathBuf, WritableError> {
    if !path.is_absolute()
        || path
            .components()
            .any(|component| matches!(component, Component::ParentDir))
    {
        return Err(WritableError::Resolve {
            path: path.to_path_buf(),
            source: io::Error::new(io::ErrorKind::InvalidInput, text::INVALID_PATH),
        });
    }
    let mut prefix = path;
    let mut suffix = Vec::new();
    loop {
        match std::fs::symlink_metadata(prefix) {
            Ok(_) => {
                let mut real = real_existing(prefix)?;
                for name in suffix.into_iter().rev() {
                    real.push(name);
                }
                return Ok(real);
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                let Some(name) = prefix.file_name() else {
                    return Err(WritableError::Resolve {
                        path: path.to_path_buf(),
                        source: error,
                    });
                };
                suffix.push(name);
                let Some(parent) = prefix.parent() else {
                    return Err(WritableError::Resolve {
                        path: path.to_path_buf(),
                        source: error,
                    });
                };
                prefix = parent;
            }
            Err(source) => {
                return Err(WritableError::Resolve {
                    path: path.to_path_buf(),
                    source,
                });
            }
        }
    }
}
