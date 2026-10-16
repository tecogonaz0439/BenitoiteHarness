//! Linux の openat2 で根からの解決を制限する（設計書 01-03「道具の定義の共通の規則」）。

use std::fs::File;
use std::os::fd::AsRawFd;
use std::path::{Path, PathBuf};

use rustix::fs::{self, Mode, OFlags, ResolveFlags};

use super::SafeFsError;

pub(super) fn open(
    root: &File,
    path: &Path,
    flags: OFlags,
    mode: Mode,
) -> Result<File, SafeFsError> {
    // ENOSYS を含め、制約のない openat に戻さない（設計書 01-03「道具の定義の共通の規則」）。
    fs::openat2(
        root,
        path,
        flags | OFlags::CLOEXEC,
        mode,
        ResolveFlags::BENEATH | ResolveFlags::NO_MAGICLINKS,
    )
    .map(File::from)
    .map_err(|error| match error {
        rustix::io::Errno::XDEV => SafeFsError::OutsideScope,
        rustix::io::Errno::LOOP => SafeFsError::IntermediateSymlink(error.into()),
        other => SafeFsError::from_errno(other),
    })
}

pub(super) fn metadata_file(parent: &File, name: &Path) -> Result<File, SafeFsError> {
    open(parent, name, OFlags::PATH | OFlags::NOFOLLOW, Mode::empty())
}

pub(super) fn write_target(parent: &File, name: &Path) -> Result<File, SafeFsError> {
    // O_PATH は FIFO を待たず、最後のリンク自体を開く。種類と nlink は記述子で調べる。
    // 元のファイルは変更せず、一時ファイルから置き換える（設計書 01-03「書く道具のパスの規則」）。
    metadata_file(parent, name)
}

pub(super) fn resolved_path(file: &File) -> Result<PathBuf, SafeFsError> {
    std::fs::read_link(format!("/proc/self/fd/{}", file.as_raw_fd())).map_err(SafeFsError::from)
}
