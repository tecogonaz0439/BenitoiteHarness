//! セッションの記録、ロック、一時領域、保持期間の掃除（設計書 01-05、05-01「データの置き場所」）。

mod files;
mod id;
mod objects;
mod quota;
mod record;
mod remove;
mod replay;
mod resume;
mod scan;
mod storage;
pub(crate) mod text;

pub use files::FileHistory;
pub use id::SessionId;
pub use quota::{StorageUsage, TOTAL_LIMIT, storage_usage};
pub use record::*;
pub use replay::{SessionTree, read_records, replay, request_text};
pub use resume::{ReasoningContext, ResumeState};
pub use scan::{
    SessionEntry, continue_session, list_sessions, previous_benitoite, recorded_benitoite,
};
pub use storage::{SessionWriter, SpillWriter};

use std::fmt;
use std::fs::{self, DirBuilder, File};
use std::io;
use std::os::unix::fs::{DirBuilderExt, MetadataExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

use rustix::fs::{FlockOperation, Mode, OFlags, flock, open, openat};

use crate::config::paths::DataPaths;
use crate::safe_fs::{SafeFsError, ScopeRelPath, ScopeRoot};

/// 記録の一行と、一つの出力の保存量の上限（末尾の印を除く）。
pub const MAX_BYTES: usize = 16 * 1024 * 1024;
/// 設定で変えない場合の保持期間。
pub const DEFAULT_RETENTION_DAYS: u64 = 30;

/// セッションの所有・形式・入出力の誤り。
#[derive(Debug)]
pub enum SessionError {
    /// OS の入出力の失敗。
    Io(io::Error),
    /// データの範囲を越える読み取りなど。
    SafeFs(SafeFsError),
    /// 記録の JSON の誤り。
    Json(serde_json::Error),
    /// OS の乱数の取得の失敗。
    Random(getrandom::Error),
    /// 構文・権限・形式の制約。
    Invalid(&'static str),
    /// ほかのセッションがロックを保持している。
    Locked,
}

impl fmt::Display for SessionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(f, "{} {error}", text::IO_ERROR),
            Self::SafeFs(error) => error.fmt(f),
            Self::Json(error) => write!(f, "{} {error}", text::JSON_ERROR),
            Self::Random(error) => write!(f, "{} {error}", text::RANDOM_ERROR),
            Self::Invalid(message) => f.write_str(message),
            Self::Locked => f.write_str(text::LOCKED),
        }
    }
}

impl std::error::Error for SessionError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::SafeFs(error) => Some(error),
            Self::Json(error) => Some(error),
            Self::Random(_) | Self::Invalid(_) | Self::Locked => None,
        }
    }
}

impl From<io::Error> for SessionError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}
impl From<SafeFsError> for SessionError {
    fn from(error: SafeFsError) -> Self {
        Self::SafeFs(error)
    }
}
impl From<serde_json::Error> for SessionError {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error)
    }
}
impl From<rustix::io::Errno> for SessionError {
    fn from(error: rustix::io::Errno) -> Self {
        Self::Io(error.into())
    }
}

/// ロックと一時領域の寿命を共有するハンドル。最後の書き手が終わるまでロックを保つ。
#[derive(Clone, Debug)]
pub struct Session {
    inner: Arc<SessionState>,
}

#[derive(Debug)]
struct SessionState {
    id: SessionId,
    path: PathBuf,
    spill: File,
    objects: File,
    known_objects: Mutex<std::collections::HashSet<String>>,
    log: Mutex<storage::LogState>,
    _lock: File,
    temporary: PathBuf,
    temporary_identity: (u64, u64),
    available: AtomicU64,
    limit_notified: AtomicBool,
    resume: Mutex<Option<ResumeState>>,
}

impl Session {
    /// 既存の記録を排他に開く。失敗しても記録を削除しない。
    pub fn open(paths: &DataPaths, id: &SessionId) -> Result<Self, SessionError> {
        let sessions = sessions_root(&paths.sessions_dir)?;
        let directory = open_directory_at(&sessions, id.as_str())?;
        let lock = open_existing_file(&directory, "lock", OFlags::RDWR)?;
        try_lock(&lock)?;
        if lock.metadata()?.nlink() == 0 {
            return Err(io::Error::from(io::ErrorKind::NotFound).into());
        }
        let path = crate::safe_fs::resolved_path(&directory)?;
        let record =
            open_existing_file(&directory, "session.jsonl", OFlags::WRONLY | OFlags::APPEND)?;
        let reader_file = open_existing_file(&directory, "session.jsonl", OFlags::RDONLY)?;
        if record.metadata()?.ino() != reader_file.metadata()?.ino() {
            return Err(SessionError::Invalid(text::UNSAFE_PATH));
        }
        let root = ScopeRoot::open(&path)?;
        let (records, complete) = replay::read_records_from(&root, reader_file)?;
        if !matches!(records.first(), Some(Record::Header(header)) if header.session_id == id.as_str())
        {
            return Err(SessionError::Invalid(text::INVALID_FORMAT));
        }
        let resume = ResumeState::from_records(&records)?;
        let spill = open_directory_at(&directory, "spill")?;
        match rustix::fs::mkdirat(&directory, "objects", Mode::RWXU) {
            Ok(()) | Err(rustix::io::Errno::EXIST) => {}
            Err(error) => return Err(error.into()),
        }
        let objects = open_directory_at(&directory, "objects")?;
        let temporary = paths.temporary_root.join(format!("benitoiteh-{id}"));
        match fs::symlink_metadata(&temporary) {
            Ok(metadata) if private_metadata(&metadata, true) => remove::directory(&temporary)?,
            Ok(_) => return Err(SessionError::Invalid(text::UNSAFE_PATH)),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        DirBuilder::new().mode(0o700).create(&temporary)?;
        let metadata = fs::symlink_metadata(&temporary)?;
        if !private_metadata(&metadata, true) {
            return Err(SessionError::Invalid(text::UNSAFE_PATH));
        }
        Ok(Self {
            inner: Arc::new(SessionState {
                id: id.clone(),
                path,
                spill,
                objects,
                known_objects: Mutex::new(std::collections::HashSet::new()),
                log: Mutex::new(storage::LogState::resumed(record, complete)),
                _lock: lock,
                temporary,
                temporary_identity: (metadata.dev(), metadata.ino()),
                available: AtomicU64::new(quota::SESSION_LIMIT),
                limit_notified: AtomicBool::new(false),
                resume: Mutex::new(Some(resume)),
            }),
        })
    }

    /// 開くときに一度解析した復元状態を、起動処理へ移す。
    pub fn take_resume_state(&self) -> Result<ResumeState, SessionError> {
        self.inner
            .resume
            .lock()
            .map_err(|_| SessionError::Invalid(text::INVALID_FORMAT))?
            .take()
            .ok_or(SessionError::Invalid(text::INVALID_FORMAT))
    }

    /// 排他に作ったデータ領域をロックしてから、共有の一時領域を作る。
    pub fn create(paths: &DataPaths, id: SessionId) -> Result<Self, SessionError> {
        let sessions = sessions_root(&paths.sessions_dir)?;
        rustix::fs::mkdirat(&sessions, id.as_str(), Mode::RWXU)?;
        let directory = open_directory_at(&sessions, id.as_str())?;
        let path = crate::safe_fs::resolved_path(&directory)?;
        let result = (|| {
            let lock = create_file(&directory, "lock", OFlags::RDWR)?;
            try_lock(&lock)?;
            rustix::fs::mkdirat(&directory, "spill", Mode::RWXU)?;
            let spill = open_directory_at(&directory, "spill")?;
            rustix::fs::mkdirat(&directory, "objects", Mode::RWXU)?;
            let objects = open_directory_at(&directory, "objects")?;
            let record = create_file(&directory, "session.jsonl", OFlags::WRONLY | OFlags::APPEND)?;
            let temporary = paths.temporary_root.join(format!("benitoiteh-{id}"));
            DirBuilder::new().mode(0o700).create(&temporary)?;
            let metadata = fs::symlink_metadata(&temporary)?;
            if !private_metadata(&metadata, true) {
                return Err(SessionError::Invalid(text::UNSAFE_PATH));
            }
            Ok(Self {
                inner: Arc::new(SessionState {
                    id,
                    path: path.clone(),
                    spill,
                    objects,
                    known_objects: Mutex::new(std::collections::HashSet::new()),
                    log: Mutex::new(storage::LogState::new(record)),
                    _lock: lock,
                    temporary,
                    temporary_identity: (metadata.dev(), metadata.ino()),
                    available: AtomicU64::new(quota::SESSION_LIMIT),
                    limit_notified: AtomicBool::new(false),
                    resume: Mutex::new(None),
                }),
            })
        })();
        if result.is_err() {
            // 一時領域の作成に競合した場合、その既存の領域には触れない。
            // セッションのデータだけは自分が排他に作った（設計書 01-05「一時ディレクトリ」）。
            let _cleanup = remove::directory(&path);
        }
        result
    }

    /// このセッションの ID。
    pub fn id(&self) -> &SessionId {
        &self.inner.id
    }

    /// セッションの実のディレクトリ。
    pub fn directory(&self) -> &Path {
        &self.inner.path
    }

    /// 子プロセスの TMPDIR に渡すディレクトリ。
    pub fn temporary_dir(&self) -> &Path {
        &self.inner.temporary
    }

    /// 追記専用の書き手を開く。書き手もロックと一時領域の寿命を保持する。
    pub fn writer(&self) -> Result<SessionWriter, SessionError> {
        Ok(SessionWriter::open(self.clone()))
    }

    /// 内部の呼び出し ID から出力用の保存先を作る。
    pub fn spill_writer(&self, tool_id: &str) -> Result<SpillWriter, SessionError> {
        SpillWriter::open(self.clone(), tool_id)
    }
}

impl Drop for SessionState {
    fn drop(&mut self) {
        if fs::symlink_metadata(&self.temporary)
            .is_ok_and(|metadata| (metadata.dev(), metadata.ino()) == self.temporary_identity)
        {
            // 一時領域の中のリンクの先は消さない。ロックはこの後に解放される。
            // （設計書 01-05「一時ディレクトリ」）。失敗時は次の起動の掃除で再試行する。
            let _cleanup = remove::directory(&self.temporary);
        }
    }
}

/// 起動時の掃除の件数と、削除できなかった項目。
#[derive(Debug, Default, Eq, PartialEq)]
pub struct CleanupReport {
    pub temporary_removed: u64,
    pub sessions_removed: u64,
    pub failures: Vec<CleanupFailure>,
}

/// 一項目の掃除の失敗。誤りの文にファイルのパスは含めない。
#[derive(Debug, Eq, PartialEq)]
pub struct CleanupFailure {
    /// 一覧の読み取り自体が失敗した場合は ID を得られない。
    pub id: Option<SessionId>,
    /// 一時ディレクトリの失敗か。
    pub temporary: bool,
    /// OS の短い診断、または固定の検証の誤り。
    pub reason: String,
}

impl CleanupFailure {
    /// 起動時に端末の制御文字を除いて表示するための文。
    pub fn diagnostic(&self) -> String {
        text::cleanup_failed(self)
    }
}

/// 動いていないセッションの一時領域と、保持期間を過ぎた記録を掃除する。
///
/// 不正な名前・所有者・権限・リンク、およびロックがないセッションは保守的に残す。
/// 根の一覧を開けなければ失敗し、項目ごとの失敗は報告して次へ進む。
pub fn cleanup(
    paths: &DataPaths,
    retention_days: u64,
    now: SystemTime,
) -> Result<CleanupReport, SessionError> {
    let sessions = sessions_root(&paths.sessions_dir)?;
    let temporary_entries = fs::read_dir(&paths.temporary_root)?;
    let session_entries = fs::read_dir(&paths.sessions_dir)?;
    let mut report = CleanupReport::default();
    let retention = Duration::from_secs(retention_days.saturating_mul(86400));
    for (temporary, entries) in [(true, temporary_entries), (false, session_entries)] {
        for entry in entries {
            let entry = match entry {
                Ok(entry) => entry,
                Err(error) => {
                    report.failures.push(CleanupFailure {
                        id: None,
                        temporary,
                        reason: error.to_string(),
                    });
                    continue;
                }
            };
            let name = entry.file_name();
            let Some(id) = name.to_str().and_then(|name| {
                let name = if temporary {
                    name.strip_prefix("benitoiteh-")?
                } else {
                    name
                };
                name.parse::<SessionId>().ok()
            }) else {
                continue;
            };
            let result = (|| -> Result<bool, SessionError> {
                let metadata = fs::symlink_metadata(entry.path())?;
                if !private_metadata(&metadata, true) {
                    return Ok(false);
                }
                if !temporary {
                    let directory = open_directory_at(&sessions, id.as_str())?;
                    if !expired(&directory, retention, now)? {
                        return Ok(false);
                    }
                }
                let lock = inactive_lock(&sessions, &id)?;
                if matches!(lock, Inactive::Busy) {
                    return Ok(false);
                }
                if temporary {
                    remove::directory(&entry.path())?;
                } else if let Inactive::Locked(_guard) = &lock {
                    let directory = open_directory_at(&sessions, id.as_str())?;
                    // 候補の判定後に再開され更新された記録は消さない（設計書 01-05「記録を残す期間」）。
                    if !expired(&directory, retention, now)? {
                        return Ok(false);
                    }
                    remove::directory_at(&sessions, name.as_os_str(), true)?;
                } else {
                    return Ok(false);
                }
                // 一時領域も削除の間ずっと錠を保持する。
                drop(lock);
                Ok(true)
            })();
            match result {
                Ok(true) if temporary => {
                    report.temporary_removed = report.temporary_removed.saturating_add(1)
                }
                Ok(true) => report.sessions_removed = report.sessions_removed.saturating_add(1),
                Ok(false) => {}
                Err(error) => report.failures.push(CleanupFailure {
                    id: Some(id),
                    temporary,
                    reason: match error {
                        SessionError::Io(error) => error.to_string(),
                        other @ (SessionError::SafeFs(_)
                        | SessionError::Json(_)
                        | SessionError::Random(_)
                        | SessionError::Invalid(_)
                        | SessionError::Locked) => other.to_string(),
                    },
                }),
            }
        }
    }
    Ok(report)
}

fn expired(directory: &File, retention: Duration, now: SystemTime) -> Result<bool, SessionError> {
    let stat = match rustix::fs::statat(
        directory,
        "session.jsonl",
        rustix::fs::AtFlags::SYMLINK_NOFOLLOW,
    ) {
        Ok(stat) => stat,
        Err(rustix::io::Errno::NOENT) => rustix::fs::fstat(directory)?,
        Err(error) => return Err(error.into()),
    };
    let modified = u64::try_from(stat.st_mtime)
        .ok()
        .and_then(|seconds| SystemTime::UNIX_EPOCH.checked_add(Duration::from_secs(seconds)));
    Ok(modified
        .and_then(|time| now.duration_since(time).ok())
        .is_some_and(|age| age > retention))
}

enum Inactive {
    Missing,
    Busy,
    Locked(File),
}

fn inactive_lock(sessions: &File, id: &SessionId) -> Result<Inactive, SessionError> {
    let directory = match open_directory_at(sessions, id.as_str()) {
        Ok(directory) => directory,
        Err(SessionError::Io(error)) if error.kind() == io::ErrorKind::NotFound => {
            return Ok(Inactive::Missing);
        }
        Err(SessionError::Invalid(_)) => return Ok(Inactive::Busy),
        Err(error) => return Err(error),
    };
    let lock = match openat(
        &directory,
        "lock",
        OFlags::RDWR | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
        Mode::empty(),
    ) {
        Ok(lock) => File::from(lock),
        // 掃除は未完成のセッションや差し替わった要素を信用しない。
        Err(rustix::io::Errno::NOENT | rustix::io::Errno::LOOP) => return Ok(Inactive::Busy),
        Err(error) => return Err(error.into()),
    };
    if !private_metadata(&lock.metadata()?, false) {
        return Ok(Inactive::Busy);
    }
    match try_lock(&lock) {
        Ok(()) if lock.metadata()?.nlink() == 0 => Ok(Inactive::Missing),
        Ok(()) => Ok(Inactive::Locked(lock)),
        Err(SessionError::Locked) => Ok(Inactive::Busy),
        Err(error) => Err(error),
    }
}

fn try_lock(file: &File) -> Result<(), SessionError> {
    match flock(file, FlockOperation::NonBlockingLockExclusive) {
        Ok(()) => Ok(()),
        Err(error) if error == rustix::io::Errno::WOULDBLOCK => Err(SessionError::Locked),
        Err(error) => Err(error.into()),
    }
}

fn sessions_root(path: &Path) -> Result<File, SessionError> {
    DirBuilder::new().recursive(true).mode(0o700).create(path)?;
    let directory = File::from(open(
        path,
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::empty(),
    )?);
    if !private_metadata(&directory.metadata()?, true) {
        return Err(SessionError::Invalid(text::UNSAFE_PATH));
    }
    Ok(directory)
}

fn open_directory_at(parent: &File, name: &str) -> Result<File, SessionError> {
    let directory = File::from(openat(
        parent,
        name,
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::empty(),
    )?);
    if !private_metadata(&directory.metadata()?, true) {
        return Err(SessionError::Invalid(text::UNSAFE_PATH));
    }
    Ok(directory)
}

fn create_file(parent: &File, name: &str, flags: OFlags) -> Result<File, SessionError> {
    let file = File::from(openat(
        parent,
        name,
        flags | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::RUSR | Mode::WUSR,
    )?);
    if !private_metadata(&file.metadata()?, false) {
        return Err(SessionError::Invalid(text::UNSAFE_PATH));
    }
    Ok(file)
}

fn open_existing_file(parent: &File, name: &str, flags: OFlags) -> Result<File, SessionError> {
    let file = File::from(openat(
        parent,
        name,
        flags | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
        Mode::empty(),
    )?);
    if !private_metadata(&file.metadata()?, false) {
        return Err(SessionError::Invalid(text::UNSAFE_PATH));
    }
    Ok(file)
}

fn private_metadata(metadata: &fs::Metadata, directory: bool) -> bool {
    let (kind, mode) = if directory {
        (metadata.is_dir(), 0o700)
    } else {
        (metadata.is_file() && metadata.nlink() == 1, 0o600)
    };
    kind && metadata.uid() == rustix::process::geteuid().as_raw()
        && metadata.mode() & 0o7777 == mode
}

fn spill_path(path: &Path) -> Result<ScopeRelPath, SessionError> {
    let relative = ScopeRelPath::new(path)?;
    if path.components().count() != 2 || !path.starts_with("spill") {
        return Err(SessionError::Invalid(text::INVALID_SPILL));
    }
    Ok(relative)
}

fn read_spill(root: &ScopeRoot, path: &Path) -> Result<File, SessionError> {
    let file = root.open_read(&spill_path(path)?)?;
    if !private_metadata(&file.metadata()?, false) {
        return Err(SessionError::Invalid(text::UNSAFE_PATH));
    }
    Ok(file)
}
