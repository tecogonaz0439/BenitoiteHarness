//! 上限付きで gitignore を読み、有効な行だけを組み立てる（設計書 01-03「glob」、01-05「辿り方」）。

use super::{SafeFsError, ScopeRelPath, ScopeRoot, text};
use ignore::gitignore::{Gitignore, GitignoreBuilder};
use rustix::fs::FileType;
use std::fmt;
use std::io::Read;
use std::path::Path;

/// `.gitignore` 一つを読み取る量の固定の上限。
pub(crate) const MAX_IGNORE_BYTES: u64 = 1024 * 1024;

/// 読めない規則と、中断された読み取りを呼び出し元へ返す。
#[derive(Debug)]
pub(crate) enum IgnoreError {
    File(SafeFsError),
    Io(std::io::Error),
    Invalid,
    Utf8(std::str::Utf8Error),
    Build(ignore::Error),
    Stopped,
}

impl fmt::Display for IgnoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::File(error) => error.fmt(f),
            Self::Io(error) => error.fmt(f),
            Self::Invalid => f.write_str(text::IGNORE_INVALID),
            Self::Utf8(error) => error.fmt(f),
            Self::Build(error) => error.fmt(f),
            Self::Stopped => f.write_str(text::IGNORE_STOPPED),
        }
    }
}

/// リンクを辿らないディレクトリから、通常ファイルだけを上限付きで読む。
pub(crate) fn load_gitignore(
    directory: &ScopeRoot,
    rules_root: &Path,
    max_bytes: u64,
    mut stopped: impl FnMut() -> bool,
) -> Result<Option<Gitignore>, IgnoreError> {
    let local = ScopeRelPath::new(".gitignore").map_err(IgnoreError::File)?;
    let stat = match directory.stat(&local) {
        Ok(stat) => stat,
        Err(SafeFsError::NotFound(_)) => return Ok(None),
        Err(error) => return Err(IgnoreError::File(error)),
    };
    let max_bytes = max_bytes.min(MAX_IGNORE_BYTES);
    if FileType::from_raw_mode(stat.st_mode) != FileType::RegularFile
        || u64::try_from(stat.st_size).unwrap_or(u64::MAX) > max_bytes
    {
        return Err(IgnoreError::Invalid);
    }
    let mut file = directory.open_read(&local).map_err(IgnoreError::File)?;
    let metadata = file.metadata().map_err(IgnoreError::Io)?;
    if !metadata.is_file() || metadata.len() > max_bytes {
        return Err(IgnoreError::Invalid);
    }
    let mut bytes = Vec::new();
    let mut buffer = [0; 8192];
    loop {
        if stopped() {
            return Err(IgnoreError::Stopped);
        }
        let count = file.read(&mut buffer).map_err(IgnoreError::Io)?;
        if count == 0 {
            break;
        }
        if u64::try_from(bytes.len().saturating_add(count)).unwrap_or(u64::MAX) > max_bytes {
            return Err(IgnoreError::Invalid);
        }
        bytes.extend_from_slice(buffer.get(..count).unwrap_or_default());
    }
    build_gitignore(rules_root, &bytes, stopped).map(Some)
}

/// 保存済みの中身にも同じ BOM と不正な行の扱いを適用する。
pub(crate) fn build_gitignore(
    directory: &Path,
    bytes: &[u8],
    mut stopped: impl FnMut() -> bool,
) -> Result<Gitignore, IgnoreError> {
    let body = std::str::from_utf8(bytes).map_err(IgnoreError::Utf8)?;
    let body = body.strip_prefix('\u{feff}').unwrap_or(body);
    let mut builder = GitignoreBuilder::new(directory);
    for line in body.lines() {
        if stopped() {
            return Err(IgnoreError::Stopped);
        }
        // 壊れた一行で他の有効な規則を捨てない（設計書 01-03「glob」）。
        let _invalid_line = builder.add_line(Some(directory.join(".gitignore")), line);
    }
    builder.build().map_err(IgnoreError::Build)
}

/// 明示の除外は true、否定は false、該当しないときは None を返す。
pub(crate) fn decision(ignore: &Gitignore, path: &Path, directory: bool) -> Option<bool> {
    let matched = ignore.matched(path, directory);
    if matched.is_ignore() {
        Some(true)
    } else if matched.is_whitelist() {
        Some(false)
    } else {
        None
    }
}
