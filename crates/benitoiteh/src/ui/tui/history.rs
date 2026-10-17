//! プロジェクトごとの履歴を錠の中で読み、追記・置き換えする（設計書 05-02「入力の操作」）。

use std::fs::{self, DirBuilder, File};
use std::io::{self, BufRead, BufReader, Read, Write};
use std::os::unix::{
    ffi::OsStrExt,
    fs::{DirBuilderExt, MetadataExt, PermissionsExt},
};
use std::path::Path;

use rustix::fs::{AtFlags, FlockOperation, Mode, OFlags, flock, open, openat, renameat, unlinkat};
use sha2::{Digest, Sha256};

use super::text;
use crate::config::paths::DataPaths;

const ENTRY_LIMIT: usize = 64 * 1024;
const FILE_LIMIT: u64 = 64 * 1024 * 1024;
const COUNT: usize = 500;

/// 履歴の入出力か、権限・型・上限の検査の失敗。
#[derive(Debug)]
pub enum HistoryError {
    /// 履歴を安全に扱えなかった。
    Io(io::Error),
}
impl std::fmt::Display for HistoryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(error) => error.fmt(f),
        }
    }
}
impl std::error::Error for HistoryError {}
impl From<io::Error> for HistoryError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}
impl From<rustix::io::Errno> for HistoryError {
    fn from(error: rustix::io::Errno) -> Self {
        Self::Io(error.into())
    }
}

/// ディレクトリの記述子を保ち、後の操作でもパスを辿り直さない。
pub struct History {
    directory: File,
    name: String,
}
impl History {
    /// 実のプロジェクトのパスをハッシュにし、最新の 500 件を読む。
    pub fn open(paths: &DataPaths, project: &Path) -> Result<(Self, Vec<String>), HistoryError> {
        match DirBuilder::new().mode(0o700).create(&paths.history_dir) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error.into()),
        }
        let directory = File::from(open(
            &paths.history_dir,
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::empty(),
        )?);
        let metadata = directory.metadata()?;
        if !metadata.is_dir()
            || metadata.uid() != rustix::process::geteuid().as_raw()
            || metadata.permissions().mode() & 0o7777 != 0o700
        {
            return Err(invalid());
        }
        let real = fs::canonicalize(project)?;
        let history = Self {
            directory,
            name: Sha256::digest(real.as_os_str().as_bytes())
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect(),
        };
        let _lock = history.lock()?;
        let entries = history.read(&mut history.file()?)?;
        Ok((history, entries))
    }

    fn checked(&self, name: &str, flags: OFlags) -> Result<File, HistoryError> {
        let file = File::from(openat(
            &self.directory,
            name,
            flags | OFlags::CREATE | OFlags::NOFOLLOW | OFlags::CLOEXEC | OFlags::NONBLOCK,
            Mode::RUSR | Mode::WUSR,
        )?);
        let metadata = file.metadata()?;
        if !metadata.is_file()
            || metadata.uid() != rustix::process::geteuid().as_raw()
            || metadata.permissions().mode() & 0o7777 != 0o600
            || metadata.nlink() != 1
        {
            return Err(invalid());
        }
        Ok(file)
    }
    fn file(&self) -> Result<File, HistoryError> {
        self.checked(&self.name, OFlags::RDWR | OFlags::APPEND)
    }
    fn lock(&self) -> Result<File, HistoryError> {
        let file = self.checked(&format!("{}.lock", self.name), OFlags::RDWR)?;
        flock(&file, FlockOperation::LockExclusive)?;
        Ok(file)
    }
    fn read(&self, file: &mut File) -> Result<Vec<String>, HistoryError> {
        if file.metadata()?.len() > FILE_LIMIT {
            return Err(invalid());
        }
        let mut entries = std::collections::VecDeque::new();
        let mut reader = BufReader::new(file.take(FILE_LIMIT.saturating_add(1)));
        let mut bytes = Vec::new();
        let mut total = 0u64;
        loop {
            bytes.clear();
            let count = reader.read_until(b'\n', &mut bytes)?;
            if count == 0 {
                break;
            }
            total = total.saturating_add(u64::try_from(count).unwrap_or(u64::MAX));
            if total > FILE_LIMIT {
                return Err(invalid());
            }
            if !bytes.ends_with(b"\n") {
                return Err(invalid());
            }
            let entry: String = serde_json::from_slice(&bytes).map_err(|_| invalid())?;
            if entry.len() <= ENTRY_LIMIT {
                entries.push_back(entry);
                if entries.len() > COUNT {
                    entries.pop_front();
                }
            }
        }
        Ok(entries.into_iter().collect())
    }

    /// 書き手ごとに開き直し、錠の中で全書き手の最新の記録を残す。
    pub fn append(&mut self, entry: &str) -> Result<(), HistoryError> {
        if entry.len() > ENTRY_LIMIT {
            return Ok(());
        }
        let _lock = self.lock()?;
        let mut file = self.file()?;
        let mut entries = self.read(&mut file)?;
        let mut bytes = serde_json::to_vec(entry).map_err(|_| invalid())?;
        bytes.push(b'\n');
        if entries.len() < COUNT
            && file
                .metadata()?
                .len()
                .saturating_add(u64::try_from(bytes.len()).unwrap_or(u64::MAX))
                <= FILE_LIMIT
        {
            file.write_all(&bytes)?;
            return Ok(());
        }
        entries.push(entry.to_owned());
        let temporary = format!(
            "{}.{}.tmp",
            self.name,
            crate::session::SessionId::generate(std::time::SystemTime::now())
                .map_err(|_| invalid())?
        );
        let result = (|| {
            let mut output = self.checked(&temporary, OFlags::WRONLY | OFlags::EXCL)?;
            // エスケープで大きくなる入力でも、次の起動で読める大きさに収める。
            // 新しい側から選び、件数と保存時のバイト数の両方を守る（設計書 05-02「入力の操作」）。
            let mut encoded = Vec::new();
            let mut total = 0u64;
            for value in entries.iter().rev().take(COUNT) {
                let mut bytes = serde_json::to_vec(value).map_err(|_| invalid())?;
                bytes.push(b'\n');
                total = total.saturating_add(u64::try_from(bytes.len()).unwrap_or(u64::MAX));
                if total > FILE_LIMIT {
                    break;
                }
                encoded.push(bytes);
            }
            for bytes in encoded.iter().rev() {
                output.write_all(bytes)?;
            }
            output.sync_all()?;
            renameat(
                &self.directory,
                temporary.as_str(),
                &self.directory,
                self.name.as_str(),
            )?;
            Ok(())
        })();
        if result.is_err() {
            let _removed = unlinkat(&self.directory, temporary.as_str(), AtFlags::empty());
        }
        result
    }
}

fn invalid() -> HistoryError {
    io::Error::new(io::ErrorKind::PermissionDenied, text::HISTORY_UNSAFE).into()
}
