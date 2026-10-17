//! OS が範囲制限の印を無視しないことを起動時に確かめる（設計書 01-03「道具の定義の共通の規則」）。

use std::fs;
use std::io;
use std::os::unix::fs::DirBuilderExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use rustix::fs::{Mode, OFlags};

use super::{SafeFsError, ScopeRoot, os, text};

/// 実の小さな木で OS の印を検査する。失敗したら呼び出し元は起動を止める。
pub fn verify_platform_support(temporary_root: &Path) -> Result<(), SafeFsError> {
    let tree = CheckTree::create(temporary_root)?;
    let root_path = tree.0.join("scope");
    fs::create_dir(&root_path)?;
    fs::write(tree.0.join("outside"), b"outside")?;
    fs::write(root_path.join("inside"), b"inside")?;
    let scope = ScopeRoot::open(&root_path)?;
    // Linux で openat2 が ENOSYS の場合も、この正の確認を通らず起動を止める。
    // 検査中に FIFO に差し替わっても待たない（設計書 01-03「道具の定義の共通の規則」）。
    let _inside = os::open(
        &scope.directory,
        Path::new("inside"),
        OFlags::RDONLY | OFlags::NONBLOCK,
        Mode::empty(),
    )?;
    // ScopeRelPath の構文検査で先に拒否せず、OS 自身の拒否を確かめる。
    match os::open(
        &scope.directory,
        Path::new("../outside"),
        OFlags::RDONLY | OFlags::NONBLOCK,
        Mode::empty(),
    ) {
        Err(SafeFsError::OutsideScope) => {}
        Ok(_) => return Err(unsupported(text::BENEATH_UNSUPPORTED)),
        Err(error) => return Err(error),
    }
    std::os::unix::fs::symlink("inside", root_path.join("link"))?;
    match os::open_no_follow(
        &scope.directory,
        Path::new("link"),
        OFlags::RDONLY | OFlags::NONBLOCK,
        Mode::empty(),
    ) {
        Err(SafeFsError::IntermediateSymlink(_)) | Err(SafeFsError::FinalSymlink) => {}
        Ok(_) => return Err(unsupported(text::NOFOLLOW_UNSUPPORTED)),
        Err(error) => return Err(error),
    }
    let _inside_no_follow = os::open_no_follow(
        &scope.directory,
        Path::new("inside"),
        OFlags::RDONLY | OFlags::NONBLOCK,
        Mode::empty(),
    )?;
    fs::create_dir(root_path.join("nested"))?;
    fs::write(root_path.join("nested/value"), b"inside")?;
    std::os::unix::fs::symlink("nested", root_path.join("linkdir"))?;
    match os::open_no_follow(
        &scope.directory,
        Path::new("linkdir/value"),
        OFlags::RDONLY | OFlags::NONBLOCK,
        Mode::empty(),
    ) {
        Err(SafeFsError::IntermediateSymlink(_)) | Err(SafeFsError::FinalSymlink) => {}
        Ok(_) => return Err(unsupported(text::NOFOLLOW_UNSUPPORTED)),
        Err(error) => return Err(error),
    }
    #[cfg(target_os = "macos")]
    {
        fs::hard_link(root_path.join("inside"), root_path.join("hard"))?;
        // nlink の事後検査で隠さず、O_UNIQUE を付けた openat の拒否を確認する。
        match os::write_target(&scope.directory, Path::new("inside")) {
            Err(SafeFsError::MultipleHardLinks) => {}
            Ok(_) => return Err(unsupported(text::UNIQUE_UNSUPPORTED)),
            Err(error) => return Err(error),
        }
    }
    fs::remove_dir_all(&tree.0)?;
    Ok(())
}

fn unsupported(message: &'static str) -> SafeFsError {
    SafeFsError::Io(io::Error::new(io::ErrorKind::Unsupported, message))
}

static COUNTER: AtomicU64 = AtomicU64::new(0);

struct CheckTree(PathBuf);

impl CheckTree {
    fn create(parent: &Path) -> Result<Self, SafeFsError> {
        for _attempt in 0..128 {
            let nonce = COUNTER.fetch_add(1, Ordering::Relaxed);
            let path = parent.join(format!(
                ".benitoiteh-fs-check-{}-{nonce}",
                std::process::id()
            ));
            match fs::DirBuilder::new().mode(0o700).create(&path) {
                Ok(()) => return Ok(Self(path)),
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                Err(error) => return Err(error.into()),
            }
        }
        Err(SafeFsError::AlreadyExists(io::Error::new(
            io::ErrorKind::AlreadyExists,
            text::TEMPORARY_COLLISIONS,
        )))
    }
}

impl Drop for CheckTree {
    fn drop(&mut self) {
        // 検査の途中で失敗しても、呼び出し元へ元の誤りを返し、木は片付ける。
        let _cleanup = fs::remove_dir_all(&self.0);
    }
}
