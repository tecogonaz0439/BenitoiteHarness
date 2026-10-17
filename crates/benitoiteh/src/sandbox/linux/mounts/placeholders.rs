//! 場所ごとの錠と記録を保持し、最後の子が終わってから片付ける（設計書 03-02「Linux のファイルシステムの見え方」）。

use std::ffi::OsString;
use std::fs::{self, DirBuilder, File, OpenOptions};
use std::io::{self, Write};
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};

use rustix::fs::{FlockOperation, Mode, OFlags, flock, open};
use sha2::{Digest, Sha256};

use crate::sandbox::{SandboxError, text};

pub(super) struct Store(PathBuf);

pub(super) struct Placeholder {
    file: File,
    path: PathBuf,
    record: PathBuf,
    missing: bool,
}

impl Store {
    pub fn prepare(path: &Path, writable: &[PathBuf]) -> Result<Self, SandboxError> {
        match fs::symlink_metadata(path) {
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                DirBuilder::new()
                    .recursive(true)
                    .mode(0o700)
                    .create(path)
                    .map_err(failure)?;
            }
            Err(error) => return Err(failure(error)),
        }
        let metadata = fs::symlink_metadata(path).map_err(failure)?;
        if !metadata.is_dir()
            || metadata.uid() != rustix::process::geteuid().as_raw()
            || metadata.mode() & 0o077 != 0
        {
            return Err(SandboxError::InvalidPolicy(text::INVALID_PLACEHOLDERS));
        }
        let real = fs::canonicalize(path).map_err(failure)?;
        if writable.iter().any(|root| real.starts_with(root)) {
            return Err(SandboxError::InvalidPolicy(text::WRITABLE_PLACEHOLDERS));
        }
        let store = Self(real);
        store.reap().map_err(failure)?;
        Ok(store)
    }

    pub fn lock(&self, path: PathBuf) -> io::Result<Placeholder> {
        let hash = path_hash(&path);
        let file = self.open_lock(&hash)?;
        flock(&file, FlockOperation::LockShared)?;
        Ok(Placeholder {
            file,
            path,
            record: self.0.join(format!("{hash}.path")),
            missing: false,
        })
    }

    fn open_lock(&self, hash: &str) -> io::Result<File> {
        // 子ごとに新しく開く。複製した記述子では別の子の錠を維持できない。
        Ok(open(
            self.0.join(format!("{hash}.lock")),
            OFlags::RDWR | OFlags::CREATE | OFlags::CLOEXEC | OFlags::NOFOLLOW,
            Mode::RUSR | Mode::WUSR,
        )?
        .into())
    }

    fn reap(&self) -> io::Result<()> {
        for entry in fs::read_dir(&self.0)? {
            let entry = entry?;
            let name = entry.file_name();
            let Some(hash) = name.as_bytes().strip_suffix(b".path") else {
                continue;
            };
            let Ok(hash) = std::str::from_utf8(hash) else {
                remove_record(&entry.path())?;
                continue;
            };
            // 名前をパスとして解釈させず、ハッシュだけを錠の名前に使う。
            if hash.len() != 64
                || !hash
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            {
                remove_record(&entry.path())?;
                continue;
            }
            let file = self.open_lock(hash)?;
            if !exclusive(&file)? {
                continue;
            }
            match fs::read(entry.path())
                .ok()
                .and_then(|bytes| recorded_path(hash, bytes))
            {
                Some(path) => remove_placeholder(&path, &entry.path())?,
                None => remove_record(&entry.path())?,
            }
            // 次の共有錠を取る前に、ここで排他錠の記述子を閉じる。
        }
        Ok(())
    }
}

impl Placeholder {
    pub fn record_missing(&mut self) -> io::Result<()> {
        let mut random = [0; 16];
        getrandom::fill(&mut random).map_err(io::Error::other)?;
        let temporary = self
            .record
            .with_extension(format!("path.{:032x}.tmp", u128::from_ne_bytes(random)));
        let result = (|| {
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&temporary)?;
            file.write_all(self.path.as_os_str().as_bytes())?;
            fs::rename(&temporary, &self.record)
        })();
        if result.is_err() {
            let _cleanup = fs::remove_file(&temporary);
        }
        result?;
        self.missing = true;
        Ok(())
    }

    pub fn retain_lock(self) {
        // 錠の記述子だけを残し、パスなどのメモリは解放する（設計書 03-02）。
        std::mem::forget(self.file);
    }

    pub fn finish(self) -> io::Result<()> {
        // 掛け直しの失敗で共有錠も外れうる。失敗後は記述子を閉じるだけにする。
        if !exclusive(&self.file)? {
            return Ok(());
        }
        let recorded = match fs::symlink_metadata(&self.record) {
            Ok(_) => true,
            Err(error) if error.kind() == io::ErrorKind::NotFound => false,
            Err(error) => return Err(error),
        };
        if self.missing || recorded {
            remove_placeholder(&self.path, &self.record)?;
        }
        Ok(())
    }
}

pub(super) fn failure(error: io::Error) -> SandboxError {
    SandboxError::Setup {
        stage: text::PLACEHOLDERS,
        detail: error.to_string(),
    }
}

fn exclusive(file: &File) -> io::Result<bool> {
    match flock(file, FlockOperation::NonBlockingLockExclusive) {
        Ok(()) => Ok(true),
        Err(rustix::io::Errno::WOULDBLOCK) => Ok(false),
        Err(error) => Err(error.into()),
    }
}

fn path_hash(path: &Path) -> String {
    Sha256::digest(path.as_os_str().as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn recorded_path(hash: &str, bytes: Vec<u8>) -> Option<PathBuf> {
    let path = PathBuf::from(OsString::from_vec(bytes));
    (path.is_absolute() && !path.as_os_str().as_bytes().contains(&0) && path_hash(&path) == hash)
        .then_some(path)
}

fn remove_placeholder(path: &Path, record: &Path) -> io::Result<()> {
    match fs::remove_dir(path) {
        Ok(()) => {}
        Err(error)
            if matches!(
                error.kind(),
                io::ErrorKind::NotFound
                    | io::ErrorKind::DirectoryNotEmpty
                    | io::ErrorKind::NotADirectory
            ) => {}
        Err(error) => return Err(error),
    }
    remove_record(record)
}

fn remove_record(path: &Path) -> io::Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}
