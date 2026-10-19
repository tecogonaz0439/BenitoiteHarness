//! 認証の保存を記述子相対の操作と排他錠で守る（設計書 02-01「Sign in with ChatGPT」）。

use super::{AuthError, base64url, text};
use rustix::fs::{
    AtFlags, Dir, FileType, FlockOperation, Mode, OFlags, fchmod, flock, mkdirat, open, openat,
    renameat, statat, unlinkat,
};
use serde::{Deserialize, Serialize};
use std::fs::{DirBuilder, File};
use std::io::{Read, Write};
use std::os::unix::fs::{DirBuilderExt, MetadataExt};
use std::path::Path;
use zeroize::Zeroizing;

const LIMIT: u64 = 64 * 1024;
const NAME: &str = "chatgpt.json";

/// Debug に秘密を含めず、解放時に文字列を消すトークン。
#[derive(Clone, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Secret(String);
impl Secret {
    /// 保存または応答から受け取った秘密を包む。
    pub fn new(value: String) -> Self {
        Self(value)
    }
    /// HTTP の要求を作るときだけ値を借りる。
    pub fn expose_secret(&self) -> &str {
        &self.0
    }
}
impl Drop for Secret {
    fn drop(&mut self) {
        zeroize::Zeroize::zeroize(&mut self.0);
    }
}
impl std::fmt::Debug for Secret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(text::REDACTED)
    }
}

/// 表示とアカウントの取り違えの検査に使う情報。
#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
pub struct Account {
    /// メールの宛先。端末へ出すときは制御文字を処理する。
    pub email: Option<String>,
    /// 発行者の中でのアカウントの識別子。
    pub subject: String,
}
/// 保存する許可と期限。時刻は Unix の秒。
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Tokens {
    /// API に渡すトークン。
    pub access_token: Secret,
    /// 更新のときに一度だけ使うトークン。
    pub refresh_token: Secret,
    /// アクセストークンの期限。
    pub expires_at: u64,
    /// 更新を始めてよい最も早い時刻。
    pub earliest_refresh_at: Option<u64>,
    /// 得た許可。
    pub scopes: Vec<String>,
}
/// chatgpt.json の版付きの保存形式。
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Credentials {
    /// 保存形式の版。現在は 1。
    pub version: u8,
    /// 発行されたクライアントの ID。
    pub client_id: String,
    /// code の交換が成功した ID だけを確認済みとする。旧形式は未確認として読む。
    #[serde(default)]
    pub client_confirmed: bool,
    /// この機械で一度だけ作る ID。
    pub ext_agent_host_id: String,
    /// 表示用の情報。
    pub account: Option<Account>,
    /// ログアウト後は空。
    pub tokens: Option<Tokens>,
    /// 最後に保存した時刻。
    pub saved_at: u64,
}

/// 開いた auth/ の記述子。呼び出し側のパスの置き換えを辿り直さない。
pub struct Store {
    directory: File,
}
/// 排他錠を持つ間だけ読み書きできる保存。
pub struct LockedStore<'a> {
    store: &'a Store,
    _lock: File,
}

impl Store {
    /// auth/ のパスを受け取り、親と auth/ の所有者・権限を検査する。
    pub fn open(auth_dir: &Path) -> Result<Self, AuthError> {
        let data_dir = auth_dir.parent().ok_or(AuthError::Storage)?;
        let name = auth_dir.file_name().ok_or(AuthError::Storage)?;
        DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(data_dir)?;
        let parent = File::from(open(
            data_dir,
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::empty(),
        )?);
        check(&parent, true)?;
        match mkdirat(&parent, name, Mode::from_raw_mode(0o700)) {
            Ok(()) | Err(rustix::io::Errno::EXIST) => {}
            Err(error) => return Err(error.into()),
        }
        let directory = File::from(openat(
            &parent,
            name,
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::empty(),
        )?);
        check(&directory, true)?;
        Ok(Self { directory })
    }
    /// 別の open による錠を取り、スレッド間でも排他する。
    pub fn lock(&self) -> Result<LockedStore<'_>, AuthError> {
        let file = File::from(openat(
            &self.directory,
            "chatgpt.lock",
            OFlags::CREATE | OFlags::RDWR | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
            Mode::from_raw_mode(0o600),
        )?);
        check(&file, false)?;
        flock(&file, FlockOperation::LockExclusive)?;
        let locked = LockedStore {
            store: self,
            _lock: file,
        };
        locked.remove_temporary_files()?;
        Ok(locked)
    }
    /// 一度の原子的な保存の内容を読む。
    pub fn load(&self) -> Result<Option<Credentials>, AuthError> {
        let file = match openat(
            &self.directory,
            NAME,
            OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
            Mode::empty(),
        ) {
            Ok(fd) => File::from(fd),
            Err(rustix::io::Errno::NOENT) => return Ok(None),
            Err(error) => return Err(error.into()),
        };
        read_credentials(file)
    }
}
fn read_credentials(file: File) -> Result<Option<Credentials>, AuthError> {
    check(&file, false)?;
    let mut bytes = Zeroizing::new(Vec::new());
    file.take(LIMIT.saturating_add(1)).read_to_end(&mut bytes)?;
    if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > LIMIT {
        return Err(AuthError::Storage);
    }
    let value: Credentials = serde_json::from_slice(&bytes).map_err(|_| AuthError::Storage)?;
    if value.version != 1 {
        return Err(AuthError::Storage);
    }
    Ok(Some(value))
}

impl LockedStore<'_> {
    // 錠の中なので、別の保存が今書いている一時ファイルを消さない。
    fn remove_temporary_files(&self) -> Result<(), AuthError> {
        let mut removed = false;
        for entry in Dir::read_from(&self.store.directory)? {
            let entry = entry?;
            let name = entry.file_name();
            let bytes = name.to_bytes();
            if !bytes.starts_with(b".chatgpt-") || !bytes.ends_with(b".tmp") {
                continue;
            }
            let metadata = match statat(&self.store.directory, name, AtFlags::SYMLINK_NOFOLLOW) {
                Ok(metadata) => metadata,
                Err(rustix::io::Errno::NOENT) => continue,
                Err(error) => return Err(error.into()),
            };
            if FileType::from_raw_mode(metadata.st_mode) == FileType::RegularFile
                && metadata.st_uid == rustix::process::geteuid().as_raw()
            {
                // 残った一時ファイルを消せなくても、保存全体を使えなくしない。
                if unlinkat(&self.store.directory, name, AtFlags::empty()).is_ok() {
                    removed = true;
                }
            }
        }
        if removed {
            self.store.directory.sync_all()?;
        }
        Ok(())
    }
    /// 錠を取った後の最新の内容を読む。
    pub fn load(&self) -> Result<Option<Credentials>, AuthError> {
        self.store.load()
    }
    /// 0600 の一時ファイルを同期し、名前を替えてディレクトリも同期する。
    pub fn save(&self, credentials: &Credentials) -> Result<(), AuthError> {
        // 既存のファイルの型・権限も検査してから置き換える。
        let _previous = self.load()?;
        let mut random = [0; 16];
        getrandom::fill(&mut random).map_err(|_| AuthError::Random)?;
        let temporary = format!(".chatgpt-{}.tmp", base64url::encode(&random));
        let result = (|| {
            let mut file = File::from(openat(
                &self.store.directory,
                temporary.as_str(),
                OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                Mode::from_raw_mode(0o600),
            )?);
            fchmod(&file, Mode::from_raw_mode(0o600))?;
            check(&file, false)?;
            let bytes =
                Zeroizing::new(serde_json::to_vec(credentials).map_err(|_| AuthError::Storage)?);
            if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > LIMIT {
                return Err(AuthError::Storage);
            }
            file.write_all(&bytes)?;
            file.sync_all()?;
            renameat(
                &self.store.directory,
                temporary.as_str(),
                &self.store.directory,
                NAME,
            )?;
            self.store.directory.sync_all()?;
            Ok(())
        })();
        if result.is_err() {
            let _removed = unlinkat(&self.store.directory, temporary.as_str(), AtFlags::empty());
        }
        result
    }
}
fn check(file: &File, directory: bool) -> Result<(), AuthError> {
    let metadata = file.metadata()?;
    let valid_kind = if directory {
        metadata.is_dir()
    } else {
        metadata.is_file() && metadata.nlink() == 1
    };
    if !valid_kind
        || metadata.uid() != rustix::process::geteuid().as_raw()
        || metadata.mode() & 0o7777 != if directory { 0o700 } else { 0o600 }
    {
        return Err(AuthError::Storage);
    }
    Ok(())
}

#[cfg(test)]
mod tests;
