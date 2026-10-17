//! 起動時の保存量の走査と、spill・objects 共通の予約（設計書 01-05「記録を残す期間」）。

use std::fs::File;
use std::sync::atomic::Ordering;

use rustix::fs::{AtFlags, Dir, FileType, Mode, OFlags, openat, statat};

use super::{Session, SessionError, open_directory_at, scan, sessions_root, text};
use crate::config::paths::DataPaths;

pub(super) const SESSION_LIMIT: u64 = 1024 * 1024 * 1024;
/// 全セッションの保存量の上限（10 GiB）。
pub const TOTAL_LIMIT: u64 = 10 * SESSION_LIMIT;

/// 起動で数えた保存量。通常ファイルの論理的な長さを合計する。
#[derive(Debug, Default)]
pub struct StorageUsage {
    pub total: u64,
    pub current: u64,
    pub current_outputs: u64,
}

/// シンボリックリンクを辿らず、全セッションの通常ファイルを数える。
pub fn storage_usage(
    paths: &DataPaths,
    current: Option<&super::SessionId>,
) -> Result<StorageUsage, SessionError> {
    let root = sessions_root(&paths.sessions_dir)?;
    let mut usage = StorageUsage::default();
    for id in scan::ids(paths)? {
        let directory = match open_directory_at(&root, id.as_str()) {
            Ok(directory) => directory,
            // 消えたものと不正な要素は数えない。リンクの先を読まない。
            Err(_) => continue,
        };
        let bytes = count(&directory, current != Some(&id))?;
        usage.total = usage.total.saturating_add(bytes);
        if current == Some(&id) {
            usage.current = bytes;
            for name in ["spill", "objects"] {
                if let Ok(child) = open_directory_at(&directory, name) {
                    usage.current_outputs =
                        usage.current_outputs.saturating_add(count(&child, false)?);
                }
            }
        }
    }
    Ok(usage)
}

fn count(directory: &File, skip_unreadable: bool) -> Result<u64, SessionError> {
    let mut total = 0_u64;
    let entries = match Dir::read_from(directory) {
        Ok(entries) => entries,
        Err(rustix::io::Errno::NOENT) => return Ok(0),
        Err(_) if skip_unreadable => return Ok(0),
        Err(error) => return Err(error.into()),
    };
    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(rustix::io::Errno::NOENT) => continue,
            Err(_) if skip_unreadable => return Ok(total),
            Err(error) => return Err(error.into()),
        };
        let name = entry.file_name();
        if name.to_bytes() == b"." || name.to_bytes() == b".." {
            continue;
        }
        let stat = match statat(directory, name, AtFlags::SYMLINK_NOFOLLOW) {
            Ok(stat) => stat,
            Err(rustix::io::Errno::NOENT) => continue,
            Err(_) if skip_unreadable => continue,
            Err(error) => return Err(error.into()),
        };
        match FileType::from_raw_mode(stat.st_mode) {
            FileType::RegularFile => {
                total = total.saturating_add(u64::try_from(stat.st_size).unwrap_or(0))
            }
            FileType::Directory => {
                match openat(
                    directory,
                    name,
                    OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                    Mode::empty(),
                ) {
                    Ok(child) => {
                        total = total.saturating_add(count(&File::from(child), skip_unreadable)?)
                    }
                    Err(
                        rustix::io::Errno::NOENT
                        | rustix::io::Errno::LOOP
                        | rustix::io::Errno::NOTDIR,
                    ) => {}
                    Err(_) if skip_unreadable => {}
                    Err(error) => return Err(error.into()),
                }
            }
            FileType::Symlink
            | FileType::Fifo
            | FileType::Socket
            | FileType::CharacterDevice
            | FileType::BlockDevice
            | FileType::Unknown => {}
        }
    }
    Ok(total)
}

impl StorageUsage {
    /// 合計の上限を超えた場合の起動時の診断。
    pub fn warning(&self) -> Option<&'static str> {
        (self.total > TOTAL_LIMIT).then_some(text::TOTAL_LIMIT)
    }
}

impl Session {
    /// 起動で数えた残量を共有の予約に設定する。書き手を使う前に一度だけ呼ぶ。
    pub fn configure_budget(&self, usage: &StorageUsage) {
        let own = SESSION_LIMIT.saturating_sub(usage.current_outputs);
        let global = TOTAL_LIMIT.saturating_sub(usage.total.saturating_sub(usage.current));
        self.inner
            .available
            .store(own.min(global), Ordering::Release);
    }

    /// spill と objects の書き込みを予約し、超過の通知を呼び出し側のために保持する。
    pub fn reserve(&self, bytes: u64) -> bool {
        if self
            .inner
            .available
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |remaining| {
                remaining.checked_sub(bytes)
            })
            .is_ok()
        {
            return true;
        }
        self.inner.limit_notified.store(true, Ordering::Release);
        false
    }

    /// 前に読んだ後の超過をまとめて受け取る。端末への書き込みは UI が持つ。
    pub fn take_storage_notice(&self) -> Option<&'static str> {
        self.inner
            .limit_notified
            .swap(false, Ordering::AcqRel)
            .then_some(text::STORAGE_LIMIT)
    }

    /// 不要になった出力、または失敗した書き込みの予約を返す。
    pub fn release(&self, bytes: u64) {
        let _previous =
            self.inner
                .available
                .fetch_update(Ordering::AcqRel, Ordering::Acquire, |remaining| {
                    Some(remaining.saturating_add(bytes))
                });
    }
}
