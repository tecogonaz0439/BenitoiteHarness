//! 実のパスで書ける場所の規則を検査する（設計書 03-02「書ける場所の検査」）。

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
    /// 自身の実行ファイルを含む。
    Executable,
    /// 作成先が守る名前の中にある。
    ProtectedName,
    /// 足した場所が一時ディレクトリの根か、その祖先である。
    TemporaryRoot,
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
                    WritableRule::Executable => text::WRITABLE_EXECUTABLE,
                    WritableRule::ProtectedName => text::WRITABLE_PROTECTED_NAME,
                    WritableRule::TemporaryRoot => text::WRITABLE_TEMPORARY_ROOT,
                };
                write!(f, "{message}: {} ({})", path.display(), other.display())?;
                if *rule == WritableRule::TemporaryRoot {
                    write!(f, ". {}", text::writable_temporary_root_hint(other))?;
                }
                Ok(())
            }
        }
    }
}
impl std::error::Error for WritableError {}

/// 検査に通った実のパス。サンドボックスにはこの値を渡す。
#[derive(Clone, Debug)]
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
    let executable = std::env::current_exe().map_err(|source| WritableError::Resolve {
        path: PathBuf::new(),
        source,
    })?;
    validate_writable_with_executable(paths, project, temporary, additional, &executable)
}

/// 起動する実行ファイルを明示し、同じ実パスの検査を行う。
pub fn validate_writable_with_executable(
    paths: &DataPaths,
    project: &Path,
    temporary: &Path,
    additional: &[PathBuf],
    executable: &Path,
) -> Result<WritableLocations, WritableError> {
    let executable =
        std::fs::canonicalize(executable).map_err(|source| WritableError::Resolve {
            path: executable.to_path_buf(),
            source,
        })?;
    let locations = WritableLocations {
        project: real_existing(project)?,
        temporary: real_future(temporary)?,
        additional: additional
            .iter()
            .map(|path| real_existing(path))
            .collect::<Result<_, _>>()?,
    };
    validate_locations(paths, locations, &executable)
}

/// 開いた実の親と未作成の名前に、起動時と同じ書ける場所の規則を当てる。
pub fn validate_project_candidate(
    paths: &DataPaths,
    candidate: &Path,
    temporary: &Path,
    executable: &Path,
) -> Result<WritableLocations, WritableError> {
    if candidate.components().any(|part| {
        let name = part.as_os_str().as_encoded_bytes();
        name.eq_ignore_ascii_case(b".git") || name.eq_ignore_ascii_case(b".benitoiteh")
    }) {
        return Err(reject(WritableRule::ProtectedName, candidate, candidate));
    }
    let executable =
        std::fs::canonicalize(executable).map_err(|source| WritableError::Resolve {
            path: executable.into(),
            source,
        })?;
    validate_locations(
        paths,
        WritableLocations {
            project: candidate.into(),
            temporary: real_future(temporary)?,
            additional: Vec::new(),
        },
        &executable,
    )
}

fn validate_locations(
    paths: &DataPaths,
    locations: WritableLocations,
    executable: &Path,
) -> Result<WritableLocations, WritableError> {
    let home = real_existing(&paths.home)?;
    let config = real_future(&paths.config_dir)?;
    let data = real_future(&paths.data_dir)?;
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
    for path in std::iter::once(&locations.project)
        .chain(std::iter::once(&locations.temporary))
        .chain(&locations.additional)
    {
        if executable.starts_with(path) {
            return Err(reject(WritableRule::Executable, path, executable));
        }
    }
    // 根に書けると、同じ根のほかのセッションの一時ディレクトリを書き換えられ、根の差し替えで
    // 削除を根の外へ向けられる（設計書 03-02「書ける場所の検査」）。doctor は試しのディレクトリを
    // 渡すので、locations.temporary の親ではなく paths.temporary_root と比べる。
    if !locations.additional.is_empty() {
        let temporary_root = real_future(&paths.temporary_root)?;
        if let Some(path) = locations
            .additional
            .iter()
            .find(|additional| temporary_root.starts_with(additional))
        {
            return Err(reject(WritableRule::TemporaryRoot, path, &temporary_root));
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    struct Fixture(PathBuf);
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _removed = fs::remove_dir_all(&self.0);
        }
    }

    // 関門: 足した場所が一時ディレクトリの根か祖先なら断り、根の下は通す。根を
    // locations.temporary の親で比べる退行（doctor の試しのディレクトリで素通りする）や、
    // 祖先を見落とす退行を、実のパスの表で捕まえる。macOS の /var の別名は実のパスに直す。
    #[test]
    fn additional_roots_must_not_contain_the_temporary_root()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut nonce = [0; 8];
        getrandom::fill(&mut nonce).map_err(io::Error::other)?;
        let root = std::env::temp_dir().join(format!("bh-w-{:016x}", u64::from_ne_bytes(nonce)));
        fs::create_dir(&root)?;
        let fixture = Fixture(fs::canonicalize(&root)?);
        let base = &fixture.0;
        for directory in ["home", "project", "tmp/T/build", "bin"] {
            fs::create_dir_all(base.join(directory))?;
        }
        fs::write(base.join("bin/benitoiteh"), b"")?;
        let paths = DataPaths::from_values(
            Some(&base.join("home")),
            Some(&base.join("config")),
            Some(&base.join("state")),
            Some(&base.join("tmp/T")),
        )
        .map_err(|error| io::Error::other(error.to_string()))?;
        // doctor と同じく、根の下の別のディレクトリを一時ディレクトリとして渡す。
        let temporary = base.join("tmp/T/benitoiteh-doctor-0/write");
        for (additional, rejected) in [("tmp/T", true), ("tmp", true), ("tmp/T/build", false)] {
            let result = validate_writable_with_executable(
                &paths,
                &base.join("project"),
                &temporary,
                &[base.join(additional)],
                &base.join("bin/benitoiteh"),
            );
            match result {
                Err(WritableError::Rejected { rule, path, other }) if rejected => {
                    assert_eq!(rule, WritableRule::TemporaryRoot, "{additional}");
                    assert_eq!(path, base.join(additional));
                    assert_eq!(other, base.join("tmp/T"));
                }
                Ok(locations) if !rejected => {
                    assert_eq!(locations.additional, vec![base.join(additional)]);
                }
                other => return Err(format!("{additional}: {other:?}").into()),
            }
        }
        Ok(())
    }
}
