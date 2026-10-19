//! セッションの後始末を、残してよい空の askpass の親と区別する（設計書 07-01）。

use std::fs;
use std::io;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::Path;

/// 一時の根には、自分の 0700 の空の親だけを許す。セッションや補助の残存は拒む。
pub fn check_temporary_root(root: &Path) -> io::Result<()> {
    let uid = rustix::process::geteuid().as_raw();
    let parent_name = format!("bha-{uid}");
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path)?;
        if entry.file_name() != std::ffi::OsStr::new(&parent_name)
            || !metadata.is_dir()
            || metadata.uid() != uid
            || metadata.permissions().mode() & 0o7777 != 0o700
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "Unexpected entry left in temporary root: {}",
                    path.display()
                ),
            ));
        }
        if fs::read_dir(&path)?.next().transpose()?.is_some() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("Askpass helper remains under {}", path.display()),
            ));
        }
    }
    Ok(())
}
