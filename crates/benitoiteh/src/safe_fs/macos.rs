//! macOS の範囲制限と単独リンクの指定を渡す（設計書 01-03「道具の定義の共通の規則」「書く道具のパスの規則」、BH-093）。

use std::fs::File;
use std::os::unix::ffi::OsStringExt;
use std::path::{Path, PathBuf};

use rustix::fs::{self, Mode, OFlags};

use super::SafeFsError;

// macOS 27.0.1 の SDK sys/fcntl.h と open(2)、P0-02 の実測で確認した値。
// rustix 1.1.5 に Apple 向けの名前がないため、未定義ビットを保つ API で渡す。
const RESOLVE_BENEATH: OFlags = OFlags::from_bits_retain(0x0000_1000);
const UNIQUE: OFlags = OFlags::from_bits_retain(0x0000_2000);
const EVENT_ONLY: OFlags = OFlags::from_bits_retain(0x0000_8000);
// rustix 1.1.5 はこの名前も Apple 向けには公開しない。SDK sys/errno.h の値。
const NOT_CAPABLE: rustix::io::Errno = rustix::io::Errno::from_raw_os_error(107);

pub(super) fn open(
    root: &File,
    path: &Path,
    flags: OFlags,
    mode: Mode,
) -> Result<File, SafeFsError> {
    fs::openat(root, path, flags | RESOLVE_BENEATH | OFlags::CLOEXEC, mode)
        .map(File::from)
        .map_err(|error| match error {
            NOT_CAPABLE => SafeFsError::OutsideScope,
            rustix::io::Errno::LOOP => SafeFsError::IntermediateSymlink(error.into()),
            other => SafeFsError::from_errno(other),
        })
}

pub(super) fn metadata_file(parent: &File, name: &Path) -> Result<File, SafeFsError> {
    // O_SYMLINK はリンク自体を開く。O_NOFOLLOW を同時に付けるとリンクを拒否してしまう。
    // macOS 27.0.1 では O_EVTONLY だけでは FIFO の相手を待ったため、NONBLOCK も付ける。
    // 信頼しないファイルの種類の確認で待たないためである（設計書 01-03「道具の定義の共通の規則」）。
    open(
        parent,
        name,
        EVENT_ONLY | OFlags::SYMLINK | OFlags::NONBLOCK,
        Mode::empty(),
    )
}

pub(super) fn write_target(parent: &File, name: &Path) -> Result<File, SafeFsError> {
    // name は単独の要素で NOFOLLOW も付くため、NOTCAPABLE は UNIQUE の拒否を表す。
    // 判定のために開くときも切り詰めない（設計書 01-03「書く道具のパスの規則」）。
    // O_EVTONLY だけでは FIFO を待つため、種類を調べて拒否する前にも NONBLOCK が要る
    // （設計書 01-03「道具の定義の共通の規則」）。
    fs::openat(
        parent,
        name,
        EVENT_ONLY
            | UNIQUE
            | RESOLVE_BENEATH
            | OFlags::NOFOLLOW
            | OFlags::NONBLOCK
            | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .map(File::from)
    .map_err(|error| match error {
        NOT_CAPABLE => SafeFsError::MultipleHardLinks,
        rustix::io::Errno::LOOP => SafeFsError::FinalSymlink,
        other => SafeFsError::from_errno(other),
    })
}

pub(super) fn resolved_path(file: &File) -> Result<PathBuf, SafeFsError> {
    let path = fs::getpath(file).map_err(SafeFsError::from_errno)?;
    Ok(PathBuf::from(std::ffi::OsString::from_vec(
        path.into_bytes(),
    )))
}

// /Library/Developer/CommandLineTools/SDKs/MacOSX.sdk/usr/include/sys/fcntl.h:159
// O_NOFOLLOW_ANY = 0x20000000 を 2026-10-17 に確認した（設計書 01-05「辿り方」）。
const NOFOLLOW_ANY: OFlags = OFlags::from_bits_retain(0x2000_0000);

pub(super) fn open_no_follow(
    root: &File,
    path: &Path,
    flags: OFlags,
    mode: Mode,
) -> Result<File, SafeFsError> {
    // この OS は NOFOLLOW_ANY と NOFOLLOW の同時指定を EINVAL にする。
    // 最後の要素も拒む、より強い NOFOLLOW_ANY だけを使う（設計書 01-05「辿り方」）。
    open(root, path, (flags & !OFlags::NOFOLLOW) | NOFOLLOW_ANY, mode)
}
