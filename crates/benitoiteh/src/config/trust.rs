//! プロジェクトの信頼の決定を、探す範囲の根ごとに保存する（設計書 03-01「プロジェクトの信頼」）。
//!
//! 決定は状態のディレクトリの `trust/` に、根の実のパスの SHA-256 を名前にしたファイルとして置く。
//! 根ごとにファイルを分け、置き換えは名前の付け替えで行うので、並んで動く `benitoiteh` が
//! 一つのファイルを読み書きし合う競合は起きない。状態のディレクトリは、どのサンドボックスの方針でも
//! 読ませない場所に入るので、サンドボックスの中のコマンドは決定を読めず、偽れない（設計書 03-02「方針の要素」）。

use std::fs::{DirBuilder, File};
use std::io::{self, Read, Write};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt};
use std::path::Path;

use rustix::fs::{Mode, OFlags, open, openat, renameat};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::paths::DataPaths;

/// 決定のファイルの大きさの上限。根のパスと真偽だけを持つ。
const FILE_LIMIT: u64 = 64 * 1024;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Record {
    path: String,
    trusted: bool,
}

/// 保存した信頼の決定。ディレクトリの記述子を保ち、後の操作でもパスを辿り直さない。
pub struct TrustStore {
    directory: File,
}

impl TrustStore {
    /// `trust/` を利用者だけが読み書きできる形で作るか開く。
    pub fn open(paths: &DataPaths) -> io::Result<Self> {
        DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(&paths.data_dir)?;
        let path = paths.data_dir.join("trust");
        match DirBuilder::new().mode(0o700).create(&path) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error),
        }
        let directory = File::from(open(
            &path,
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
        Ok(Self { directory })
    }

    /// 根の決定を読む。決めていなければ `None` を返す。
    pub fn get(&self, root: &Path) -> io::Result<Option<bool>> {
        let file = match openat(
            &self.directory,
            name(root),
            OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::CLOEXEC | OFlags::NONBLOCK,
            Mode::empty(),
        ) {
            Ok(file) => File::from(file),
            Err(rustix::io::Errno::NOENT) => return Ok(None),
            Err(error) => return Err(error.into()),
        };
        let metadata = file.metadata()?;
        if !metadata.is_file()
            || metadata.uid() != rustix::process::geteuid().as_raw()
            || metadata.permissions().mode() & 0o7777 != 0o600
            || metadata.nlink() != 1
            || metadata.len() > FILE_LIMIT
        {
            return Err(invalid());
        }
        let mut bytes = Vec::new();
        file.take(FILE_LIMIT.saturating_add(1))
            .read_to_end(&mut bytes)?;
        let record: Record = serde_json::from_slice(&bytes).map_err(|_| invalid())?;
        // 名前のハッシュが重なっても、別の根の決定を使わない。
        if record.path.as_bytes() != root.as_os_str().as_bytes() {
            return Err(invalid());
        }
        Ok(Some(record.trusted))
    }

    /// 根の決定を、一時のファイルに書いてから名前を付け替えて置く。
    pub fn set(&self, root: &Path, trusted: bool) -> io::Result<()> {
        let path = root.to_str().ok_or_else(invalid)?.to_owned();
        let mut bytes = serde_json::to_vec(&Record { path, trusted }).map_err(|_| invalid())?;
        bytes.push(b'\n');
        let mut random = [0_u8; 16];
        getrandom::fill(&mut random).map_err(|error| io::Error::other(error.to_string()))?;
        let target = name(root);
        let temporary = format!("{target}.{:032x}.tmp", u128::from_ne_bytes(random));
        let mut file = File::from(openat(
            &self.directory,
            temporary.as_str(),
            OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::RUSR | Mode::WUSR,
        )?);
        let written = file.write_all(&bytes).and_then(|()| file.sync_all());
        let renamed = written.and_then(|()| {
            renameat(
                &self.directory,
                temporary.as_str(),
                &self.directory,
                target.as_str(),
            )
            .map_err(io::Error::from)
        });
        if renamed.is_err() {
            let _removed = rustix::fs::unlinkat(
                &self.directory,
                temporary.as_str(),
                rustix::fs::AtFlags::empty(),
            );
        }
        renamed
    }
}

fn name(root: &Path) -> String {
    Sha256::digest(root.as_os_str().as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn invalid() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        super::text::INVALID_TRUST_RECORD,
    )
}

#[cfg(test)]
mod tests;
