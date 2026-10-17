//! ハッシュ名の中身と一覧を、容量を予約して保存する（設計書 01-05「巻き戻し」）。

use std::fs::File;
use std::io::{Read, Write};
use std::os::unix::fs::MetadataExt;

use rustix::fs::{AtFlags, OFlags};
use sha2::{Digest, Sha256};

use super::{Session, SessionError, create_file, id, open_existing_file, text};

impl Session {
    // 保存した名前の再確認は省き、復元時には必ず中身をハッシュと照合する。
    pub(crate) fn object_known(&self, hash: &str) -> bool {
        self.inner
            .known_objects
            .lock()
            .is_ok_and(|known| known.contains(hash))
    }
    fn remember_object(&self, hash: &str) {
        if let Ok(mut known) = self.inner.known_objects.lock() {
            known.insert(hash.to_owned());
        }
    }

    /// SHA-256 名で一度だけ保存する。容量不足は None、値は名前と新規保存の有無。
    pub fn store_object(&self, bytes: &[u8]) -> Result<Option<(String, bool)>, SessionError> {
        let hash = id::hex(&Sha256::digest(bytes));
        let size = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
        match self.open_object(&hash, size) {
            Ok(file) => {
                if file.metadata()?.len() != size {
                    return Err(SessionError::Invalid(text::INVALID_OBJECT));
                }
                self.remember_object(&hash);
                return Ok(Some((hash, false)));
            }
            Err(SessionError::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
        if !self.reserve(size) {
            return Ok(None);
        }
        let mut random = [0; 16];
        if let Err(error) = getrandom::fill(&mut random) {
            self.release(size);
            return Err(SessionError::Random(error));
        }
        let temporary = format!("tmp-{}", id::hex(&random));
        let result = (|| {
            let mut file = create_file(&self.inner.objects, &temporary, OFlags::WRONLY)?;
            file.write_all(bytes)?;
            file.flush()?;
            rustix::fs::renameat(&self.inner.objects, &temporary, &self.inner.objects, &hash)?;
            self.remember_object(&hash);
            Ok(Some((hash, true)))
        })();
        if result.is_err() {
            let _removed = rustix::fs::unlinkat(&self.inner.objects, &temporary, AtFlags::empty());
            self.release(size);
        }
        result
    }

    /// 信頼しない中身を NOFOLLOW・NONBLOCK で開き、通常ファイル、単独リンク、長さを確かめる。
    pub fn open_object(&self, hash: &str, limit: u64) -> Result<File, SessionError> {
        if hash.len() != 64
            || !hash
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err(SessionError::Invalid(text::INVALID_OBJECT));
        }
        let file = open_existing_file(&self.inner.objects, hash, OFlags::RDONLY)?;
        let metadata = file.metadata()?;
        if !metadata.is_file() || metadata.nlink() != 1 || metadata.len() > limit {
            return Err(SessionError::Invalid(text::INVALID_OBJECT));
        }
        Ok(file)
    }

    /// 一覧を上限付きで読み、名前の SHA-256 と照合する。
    pub fn read_object(&self, hash: &str, limit: u64) -> Result<Vec<u8>, SessionError> {
        let file = self.open_object(hash, limit)?;
        let mut bytes = Vec::new();
        file.take(limit.saturating_add(1)).read_to_end(&mut bytes)?;
        if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > limit
            || id::hex(&Sha256::digest(&bytes)) != hash
        {
            return Err(SessionError::Invalid(text::INVALID_OBJECT));
        }
        self.remember_object(hash);
        Ok(bytes)
    }
}
