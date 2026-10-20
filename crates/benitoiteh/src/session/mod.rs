//! セッションの記録、ロック、一時領域、保持期間の掃除（設計書 01-05、05-01「データの置き場所」）。

mod files;
mod fork;
mod id;
mod objects;
mod quota;
mod record;
pub(crate) mod remove;
mod replay;
mod resume;
mod scan;
mod storage;
mod temporary;
pub(crate) mod text;
mod tree;

pub use files::FileHistory;
pub use fork::{ForkTarget, ForkedSession};
pub use id::SessionId;
pub use quota::{StorageUsage, TOTAL_LIMIT, storage_usage};
pub use record::*;
pub use replay::{SessionTree, read_records, replay, request_text};
pub use resume::{ReasoningContext, ResumeState};
pub use scan::{
    SessionEntry, continue_session, list_sessions, previous_benitoite, recorded_benitoite,
};
pub use storage::{SessionWriter, SpillWriter};
pub(crate) use tree::request_heading;
pub use tree::{TreeRow, tree_rows};

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
    /// 写す処理の元の誤りと、作りかけを削除できなかった誤り。
    ForkCleanup {
        error: Box<SessionError>,
        cleanup: io::Error,
        session: SessionId,
    },
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
            Self::ForkCleanup {
                error,
                cleanup,
                session,
            } => f.write_str(&text::fork_cleanup(error, cleanup, session)),
        }
    }
}

impl std::error::Error for SessionError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::SafeFs(error) => Some(error),
            Self::Json(error) => Some(error),
            Self::ForkCleanup { error, .. } => Some(error.as_ref()),
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
    notices: Vec<String>,
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
        let resume = ResumeState::from_records(records)?;
        let spill = open_directory_at(&directory, "spill")?;
        match rustix::fs::mkdirat(&directory, "objects", Mode::RWXU) {
            Ok(()) | Err(rustix::io::Errno::EXIST) => {}
            Err(error) => return Err(error.into()),
        }
        let objects = open_directory_at(&directory, "objects")?;
        // 子プロセスが権限や印を変えた残りでも再開を止めない（設計書 01-05「一時ディレクトリ」）。
        let (temporary, notices) = temporary::prepare(&paths.temporary_root, id)?;
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
                notices,
            }),
        })
    }

    /// 開いたときの知らせ（残った一時ディレクトリを消せなかった、など）。
    pub fn startup_notices(&self) -> &[String] {
        &self.inner.notices
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
                    notices: Vec::new(),
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
            let _cleanup = remove::temporary(&self.temporary);
        }
    }
}

/// 起動時の掃除の件数と、削除できなかった項目。
#[derive(Debug, Default, Eq, PartialEq)]
pub struct CleanupReport {
    pub temporary_removed: u64,
    pub sessions_removed: u64,
    /// 0700 でなくなっていたのを消した一時ディレクトリ（設計書 01-05「一時ディレクトリ」）。
    pub temporary_unsafe_removed: Vec<PathBuf>,
    pub failures: Vec<CleanupFailure>,
}

impl CleanupReport {
    /// 起動のときに示す知らせ（消した権限の変わった一時ディレクトリと、失敗）。
    pub fn diagnostics(&self) -> Vec<String> {
        self.temporary_unsafe_removed
            .iter()
            .map(|path| text::temporary_unsafe_removed(path))
            .chain(self.failures.iter().map(CleanupFailure::diagnostic))
            .collect()
    }
}

/// 一項目の掃除の失敗。ID の分かる項目の文は ID で示し、一時の根・askpass の親・
/// 一覧の読み取りの失敗（ID の分からないもの）は、パスを含む文にする。
#[derive(Debug, Eq, PartialEq)]
pub struct CleanupFailure {
    /// 一時の根・askpass の親・一覧の読み取りの失敗では ID を得られない。
    pub id: Option<SessionId>,
    /// 一時ディレクトリの失敗か。
    pub temporary: bool,
    /// ID があれば OS の短い診断か固定の検証の誤り。ID がなければ、パスを含む
    /// `Skipped cleanup of <パス>: …` の文で、そのまま示す。
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
/// 記録の根を開けなければ失敗し、一時の根や項目ごとの失敗は報告して次へ進む。
pub fn cleanup(
    paths: &DataPaths,
    retention_days: u64,
    now: SystemTime,
) -> Result<CleanupReport, SessionError> {
    let sessions = sessions_root(&paths.sessions_dir)?;
    let session_entries = fs::read_dir(&paths.sessions_dir)?;
    let mut report = CleanupReport::default();
    let retention = Duration::from_secs(retention_days.saturating_mul(86400));
    let mut roots = Vec::new();
    // 開けなければ一覧も読めず、その失敗は下で報告する。
    let temporary_root = temporary::open_root(&paths.temporary_root).ok();
    match fs::read_dir(&paths.temporary_root) {
        Ok(entries) => roots.push((true, false, paths.temporary_root.clone(), entries)),
        Err(error) => report.failures.push(CleanupFailure {
            id: None,
            temporary: true,
            reason: text::cleanup_skipped(&paths.temporary_root, &error.to_string()),
        }),
    }
    // 短い補助の名前は自分の記録から一意に対応する ID だけを解決する。
    let mut helper_ids = std::collections::BTreeMap::new();
    for entry in fs::read_dir(&paths.sessions_dir)? {
        let Ok(entry) = entry else {
            continue;
        };
        let Some(id) = entry
            .file_name()
            .to_str()
            .and_then(|name| name.parse::<SessionId>().ok())
        else {
            continue;
        };
        helper_ids
            .entry(crate::askpass::helper_name(id.as_str()).to_owned())
            .and_modify(|value| *value = None)
            .or_insert(Some(id));
    }
    let mut visited = std::collections::BTreeSet::new();
    let mut helper_roots = vec![paths.temporary_root.clone(), PathBuf::from("/tmp")];
    helper_roots.extend(crate::askpass::runtime_directory());
    for root in &helper_roots {
        let root = match fs::canonicalize(root) {
            Ok(root) => root,
            Err(_) => continue,
        };
        if !visited.insert(root.clone()) {
            continue;
        }
        let parent = crate::askpass::parent_directory(&root);
        match fs::symlink_metadata(&parent) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            _ if !crate::askpass::valid_parent(&parent) => {
                report.failures.push(CleanupFailure {
                    id: None,
                    temporary: true,
                    reason: text::cleanup_skipped(&parent, text::NOT_PRIVATE_DIRECTORY),
                });
                continue;
            }
            _ => {}
        }
        match fs::read_dir(&parent) {
            Ok(entries) => roots.push((true, true, parent.clone(), entries)),
            Err(error) => report.failures.push(CleanupFailure {
                id: None,
                temporary: true,
                reason: text::cleanup_skipped(&parent, &error.to_string()),
            }),
        }
    }
    roots.push((false, false, paths.sessions_dir.clone(), session_entries));
    for (temporary, askpass, listed, entries) in roots {
        for entry in entries {
            let entry = match entry {
                Ok(entry) => entry,
                Err(error) => {
                    // ID がなくても、どの根の一覧の誤りかを示す。
                    report.failures.push(CleanupFailure {
                        id: None,
                        temporary,
                        reason: text::cleanup_skipped(&listed, &error.to_string()),
                    });
                    continue;
                }
            };
            let name = entry.file_name();
            let Some(name_text) = name.to_str() else {
                continue;
            };
            if !temporary && let Some(id) = temporary::parse_session_tombstone(name_text) {
                // 墓標は期間によらず、錠を取れれば消す（設計書 01-05「記録を残す期間」）。
                // 同じセッションを二度数えないよう、sessions_removed には数えない。
                match remove_session_tombstone(
                    paths,
                    &sessions,
                    temporary_root.as_ref(),
                    &id,
                    name_text,
                ) {
                    Ok(count) => {
                        report.temporary_removed = report.temporary_removed.saturating_add(count);
                    }
                    Err(error) => report
                        .failures
                        .push(cleanup_failure(Some(id), false, error)),
                }
                continue;
            }
            let ordinary = temporary && !askpass;
            let parsed = if ordinary {
                temporary::parse(name_text)
            } else if askpass {
                // 子の `id` を読めればその ID で錠と記録を確かめる。`id` のない子（前の版が
                // 作ったもの）だけ名前の対応表で扱い、確かめに通らない `id` の子は残す
                // （設計書 03-04「補助」）。
                match crate::askpass::read_id(&entry.path()) {
                    Ok(Some(found)) => found
                        .parse::<SessionId>()
                        .ok()
                        .filter(|id| crate::askpass::helper_name(id.as_str()) == name_text),
                    Ok(None) => name_text
                        .parse::<SessionId>()
                        .ok()
                        .or_else(|| helper_ids.get(name_text).cloned().flatten()),
                    Err(_) => None,
                }
                .map(|id| (id, temporary::Kind::Primary))
            } else {
                name_text
                    .parse::<SessionId>()
                    .ok()
                    .map(|id| (id, temporary::Kind::Primary))
            };
            let Some((id, kind)) = parsed else {
                continue;
            };
            let mut side_removed = 0_u64;
            let result = (|| -> Result<bool, SessionError> {
                let metadata = match fs::symlink_metadata(entry.path()) {
                    Ok(metadata) => metadata,
                    Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
                    Err(error) => return Err(error.into()),
                };
                if ordinary {
                    // 記録があり錠を取れたものだけを、権限によらず消す。ほかのデータの
                    // 置き場所のセッションの一時ディレクトリは記録がないので残る
                    // （設計書 01-05「一時ディレクトリ」）。
                    if !metadata.is_dir() || metadata.uid() != rustix::process::geteuid().as_raw() {
                        return Ok(false);
                    }
                    let lock = match inactive_lock(&sessions, &id)? {
                        Inactive::Locked(lock) => Some(lock),
                        Inactive::Busy => return Ok(false),
                        // 記録が墓標の名前に替わっていれば、その錠で確かめる。
                        Inactive::Missing => match lock_tombstone_of(paths, &sessions, &id)? {
                            Some(lock) => lock,
                            None => return Ok(false),
                        },
                    };
                    let root = temporary_root
                        .as_ref()
                        .ok_or_else(|| io::Error::from(io::ErrorKind::NotFound))?;
                    if kind == temporary::Kind::Tombstone {
                        remove::directory_at(root, &name, remove::Target::Temporary)?;
                    } else {
                        match temporary::discard(root, name_text, &id)? {
                            temporary::Discard::Absent => return Ok(false),
                            temporary::Discard::Removed => {}
                            temporary::Discard::Pinned(error)
                            | temporary::Discard::Failed { error, .. } => {
                                return Err(error.into());
                            }
                        }
                        if !private_metadata(&metadata, true) {
                            report.temporary_unsafe_removed.push(entry.path());
                        }
                    }
                    drop(lock);
                    return Ok(true);
                }
                if !private_metadata(&metadata, true) {
                    return Ok(false);
                }
                if !temporary {
                    // 同時に掃除するほかの起動が墓標に替えたものは、消すものがない。
                    let Some(directory) = open_if_present(&sessions, id.as_str())? else {
                        return Ok(false);
                    };
                    if !expired(&directory, retention, now)? {
                        return Ok(false);
                    }
                }
                let lock = inactive_lock(&sessions, &id)?;
                // 利用者専用の親は残し、自分の記録と錠で生死を確かめた子だけ消す。
                if matches!(lock, Inactive::Busy)
                    || (askpass && !matches!(lock, Inactive::Locked(_)))
                {
                    return Ok(false);
                }
                if temporary {
                    if askpass {
                        if !entry
                            .path()
                            .parent()
                            .is_some_and(crate::askpass::valid_parent)
                        {
                            return Err(io::Error::from(io::ErrorKind::PermissionDenied).into());
                        }
                        let record = paths.sessions_dir.join(id.as_str()).join("session.jsonl");
                        if !fs::symlink_metadata(record)
                            .is_ok_and(|metadata| private_metadata(&metadata, false))
                        {
                            return Ok(false);
                        }
                        match fs::remove_file(entry.path().join("sock")) {
                            Ok(()) => {}
                            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                            Err(error) => return Err(error.into()),
                        }
                    }
                    remove::directory(&entry.path())?;
                } else if let Inactive::Locked(_guard) = &lock {
                    let Some(directory) = open_if_present(&sessions, id.as_str())? else {
                        return Ok(false);
                    };
                    // 候補の判定後に再開され更新された記録は消さない（設計書 01-05「記録を残す期間」）。
                    if !expired(&directory, retention, now)? {
                        return Ok(false);
                    }
                    drop(directory);
                    // 錠を持ったまま墓標の名前へ替え、その後に一時ディレクトリ、墓標の順に
                    // 消す。途中で失敗しても半端なセッションを ID で開かせない
                    // （設計書 01-05「記録を残す期間」）。
                    let tombstone = temporary::session_tombstone(&id)?;
                    match rustix::fs::renameat(
                        &sessions,
                        name.as_os_str(),
                        &sessions,
                        tombstone.as_str(),
                    ) {
                        Ok(()) => {}
                        Err(rustix::io::Errno::NOENT) => return Ok(false),
                        Err(error) => return Err(error.into()),
                    }
                    side_removed = finish_session_tombstone(
                        paths,
                        &sessions,
                        temporary_root.as_ref(),
                        &id,
                        &tombstone,
                    )?;
                } else {
                    return Ok(false);
                }
                // 一時領域も削除の間ずっと錠を保持する。
                drop(lock);
                Ok(true)
            })();
            report.temporary_removed = report.temporary_removed.saturating_add(side_removed);
            match result {
                Ok(true) if temporary => {
                    report.temporary_removed = report.temporary_removed.saturating_add(1)
                }
                Ok(true) => report.sessions_removed = report.sessions_removed.saturating_add(1),
                Ok(false) => {}
                Err(error) => report
                    .failures
                    .push(cleanup_failure(Some(id), temporary, error)),
            }
        }
    }
    Ok(report)
}

fn cleanup_failure(id: Option<SessionId>, temporary: bool, error: SessionError) -> CleanupFailure {
    CleanupFailure {
        id,
        temporary,
        reason: match error {
            SessionError::Io(error) => error.to_string(),
            other @ (SessionError::SafeFs(_)
            | SessionError::Json(_)
            | SessionError::Random(_)
            | SessionError::Invalid(_)
            | SessionError::Locked
            | SessionError::ForkCleanup { .. }) => other.to_string(),
        },
    }
}

/// 消すものがなければ `None` を返して開く（同時に掃除する起動との競合を失敗にしない）。
fn open_if_present(parent: &File, name: &str) -> Result<Option<File>, SessionError> {
    match open_directory_at(parent, name) {
        Ok(directory) => Ok(Some(directory)),
        Err(SessionError::Io(error)) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

/// セッションの墓標を、中の錠を取れれば片付け、消した一時ディレクトリの数を返す。
/// 錠のないものは動くセッションがないので片付ける。
fn remove_session_tombstone(
    paths: &DataPaths,
    sessions: &File,
    temporary_root: Option<&File>,
    id: &SessionId,
    name: &str,
) -> Result<u64, SessionError> {
    let Some(_lock) = lock_session_tombstone(sessions, name)? else {
        return Ok(0);
    };
    finish_session_tombstone(paths, sessions, temporary_root, id, name)
}

/// 墓標の錠を持ったまま、先にそのセッションの一時ディレクトリ（墓標の名前のものを含む）を
/// 消し、消せたら墓標を消す。消せなければ墓標を錠のファイルだけにして残し、失敗を返す。
/// 墓標を先に消すと、残った一時ディレクトリを記録と錠で確かめて消す者がいなくなる
/// （設計書 01-05「記録を残す期間」）。
fn finish_session_tombstone(
    paths: &DataPaths,
    sessions: &File,
    temporary_root: Option<&File>,
    id: &SessionId,
    name: &str,
) -> Result<u64, SessionError> {
    let removed = match temporary_root {
        Some(root) => temporary::remove_all(root, &paths.temporary_root, id),
        None => Ok(0),
    };
    let tombstone = std::ffi::OsStr::new(name);
    match removed {
        Ok(count) => {
            remove::directory_at(sessions, tombstone, remove::Target::Session)?;
            Ok(count)
        }
        Err(error) => {
            let reason = cleanup_failure(None, true, error).reason;
            let contents =
                remove::directory_at(sessions, tombstone, remove::Target::SessionContents)
                    .err()
                    .map(|error| error.to_string());
            Err(io::Error::other(text::session_kept(&reason, contents.as_deref())).into())
        }
    }
}

/// 同じ ID のセッションの墓標の錠を取る。一時ディレクトリを消す前のセッションの削除が
/// 止まったときに、残った一時ディレクトリの墓標を消すために使う。
fn lock_tombstone_of(
    paths: &DataPaths,
    sessions: &File,
    id: &SessionId,
) -> Result<Option<Option<File>>, SessionError> {
    for entry in fs::read_dir(&paths.sessions_dir)? {
        let Ok(entry) = entry else {
            continue;
        };
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        if temporary::parse_session_tombstone(name).as_ref() == Some(id)
            && let Some(lock) = lock_session_tombstone(sessions, name)?
        {
            return Ok(Some(lock));
        }
    }
    Ok(None)
}

/// 墓標の錠を取る。`None` は消してはならない（錠を取れない、不正な要素）。
/// `Some(None)` は錠のない墓標で、中で動くセッションはない。
fn lock_session_tombstone(
    sessions: &File,
    name: &str,
) -> Result<Option<Option<File>>, SessionError> {
    let directory = match open_directory_at(sessions, name) {
        Ok(directory) => directory,
        Err(SessionError::Io(error)) if error.kind() == io::ErrorKind::NotFound => {
            return Ok(None);
        }
        Err(SessionError::Invalid(_)) => return Ok(None),
        Err(error) => return Err(error),
    };
    Ok(Some(
        match openat(
            &directory,
            "lock",
            OFlags::RDWR | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
            Mode::empty(),
        ) {
            Ok(lock) => {
                let lock = File::from(lock);
                if !private_metadata(&lock.metadata()?, false) {
                    return Ok(None);
                }
                match try_lock(&lock) {
                    Ok(()) => Some(lock),
                    Err(SessionError::Locked) => return Ok(None),
                    Err(error) => return Err(error),
                }
            }
            Err(rustix::io::Errno::NOENT) => None,
            Err(error) => return Err(error.into()),
        },
    ))
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
