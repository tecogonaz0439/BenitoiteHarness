//! spill を読まずに一覧と継続の候補を走査する（設計書 05-01「sessions」、01-05「再開」）。

use std::fs::{self, File};
use std::io::{BufRead, BufReader, Read};
use std::path::PathBuf;

use rustix::fs::{Mode, OFlags, openat};
use serde::Serialize;
use serde_json::Value;

use super::{
    MAX_BYTES, RecordedMode, Session, SessionError, SessionId, open_directory_at, private_metadata,
    sessions_root, text, try_lock,
};
use crate::config::paths::DataPaths;

/// 一覧の一行。読めない記録では ID と誤りだけを持つ。
#[derive(Debug, Serialize)]
pub struct SessionEntry {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub started_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mode: Option<RecordedMode>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project_dir: Option<PathBuf>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub first_request: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub forked_from: Option<super::ForkedFrom>,
    pub active: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl SessionEntry {
    fn new(id: &SessionId) -> Self {
        Self {
            id: id.to_string(),
            started_at: None,
            mode: None,
            project_dir: None,
            first_request: None,
            active: false,
            forked_from: None,
            error: None,
        }
    }
}

pub(super) fn ids(paths: &DataPaths) -> Result<Vec<SessionId>, SessionError> {
    let _root = sessions_root(&paths.sessions_dir)?;
    let mut ids: Vec<SessionId> = Vec::new();
    for entry in fs::read_dir(&paths.sessions_dir)? {
        if let Some(id) = entry?
            .file_name()
            .to_str()
            .and_then(|name| name.parse().ok())
        {
            ids.push(id);
        }
    }
    ids.sort_unstable_by(|left, right| right.cmp(left));
    Ok(ids)
}

/// ID の降順に、記録の最後の起動値と最初の依頼を読む。
pub fn list_sessions(paths: &DataPaths) -> Result<Vec<SessionEntry>, SessionError> {
    let root = sessions_root(&paths.sessions_dir)?;
    ids(paths)?
        .into_iter()
        .map(|id| {
            let mut entry = SessionEntry::new(&id);
            if let Err(error) = scan(&root, &mut entry).and_then(|()| {
                let directory = open_directory_at(&root, &entry.id)?;
                let lock = super::open_existing_file(&directory, "lock", OFlags::RDWR)?;
                entry.active = match try_lock(&lock) {
                    Ok(()) => false,
                    Err(SessionError::Locked) => true,
                    Err(error) => return Err(error),
                };
                Ok(())
            }) {
                entry.forked_from = None;
                entry.started_at = None;
                entry.mode = None;
                entry.project_dir = None;
                entry.first_request = None;
                entry.error = Some(error.to_string());
            }
            Ok(entry)
        })
        .collect()
}

fn scan(root: &File, entry: &mut SessionEntry) -> Result<(), SessionError> {
    let directory = open_directory_at(root, entry.id.as_str())?;
    let file = File::from(openat(
        &directory,
        "session.jsonl",
        OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
        Mode::empty(),
    )?);
    if !private_metadata(&file.metadata()?, false) {
        return Err(SessionError::Invalid(text::UNSAFE_PATH));
    }
    let mut reader = BufReader::new(file);
    let mut first = true;
    loop {
        let mut bytes = Vec::new();
        if reader
            .by_ref()
            .take(
                u64::try_from(MAX_BYTES)
                    .unwrap_or(u64::MAX)
                    .saturating_add(1),
            )
            .read_until(b'\n', &mut bytes)?
            == 0
        {
            break;
        }
        if bytes.len() > MAX_BYTES {
            return Err(SessionError::Invalid(text::RECORD_TOO_LARGE));
        }
        if bytes.last() != Some(&b'\n') {
            break;
        }
        let value: Value = serde_json::from_slice(&bytes)?;
        if first {
            super::replay::check_format(&value)?;
        }
        match string(&value, "type")? {
            "header" if first => {
                entry.forked_from = value
                    .get("forked_from")
                    .filter(|v| !v.is_null())
                    .cloned()
                    .map(serde_json::from_value::<super::ForkedFrom>)
                    .transpose()?;
                entry.started_at = Some(string(&value, "started_at")?.into());
                entry.mode = Some(mode(&value)?);
                entry.project_dir = Some(PathBuf::from(string(&value, "project_dir")?));
            }
            "header" => return Err(SessionError::Invalid(text::INVALID_FORMAT)),
            "session_start" => entry.mode = Some(mode(&value)?),
            "project_dir_changed" => {
                entry.project_dir = Some(PathBuf::from(string(&value, "project_dir")?))
            }
            "user" if entry.first_request.is_none() => {
                let parts = value
                    .pointer("/message/parts")
                    .and_then(Value::as_array)
                    .ok_or(SessionError::Invalid(text::INVALID_FORMAT))?;
                for part in parts {
                    if string(part, "kind")? == "request" {
                        entry.first_request =
                            Some(string(part, "text")?.chars().take(60).collect());
                        break;
                    }
                }
            }
            "spilled" if entry.first_request.is_none() => {
                entry.first_request = Some(text::NOT_AVAILABLE.into())
            }
            _ => {}
        }
        first = false;
    }
    if first {
        return Err(SessionError::Invalid(text::INVALID_FORMAT));
    }
    if entry.first_request.is_none() {
        entry.first_request = Some(text::NOT_AVAILABLE.into());
    }
    Ok(())
}

/// 同じ実のプロジェクトに属し、錠を取れる最新のセッションを開く。
pub fn continue_session(
    paths: &DataPaths,
    project: &std::path::Path,
) -> Result<Session, SessionError> {
    let project = fs::canonicalize(project)?;
    let root = sessions_root(&paths.sessions_dir)?;
    // プロジェクトの違うものには錠を掛けない。候補を見つけたら以降の記録を読まない
    // （設計書 01-05「再開」、P4-02 の点検の指示 2）。
    for id in ids(paths)? {
        let mut entry = SessionEntry::new(&id);
        if scan(&root, &mut entry).is_err()
            || !entry
                .project_dir
                .as_ref()
                .and_then(|path| fs::canonicalize(path).ok())
                .is_some_and(|path| path == project)
        {
            continue;
        }
        match Session::open(paths, &id) {
            Ok(session) => {
                let state = session
                    .inner
                    .resume
                    .lock()
                    .map_err(|_| SessionError::Invalid(text::INVALID_FORMAT))?;
                let matches = state
                    .as_ref()
                    .and_then(|state| fs::canonicalize(&state.project_dir).ok())
                    .is_some_and(|path| path == project);
                drop(state);
                if matches {
                    return Ok(session);
                }
            }
            Err(_) => continue,
        }
    }
    Err(SessionError::Invalid(text::NO_SESSION))
}

fn string<'a>(value: &'a Value, key: &str) -> Result<&'a str, SessionError> {
    value
        .get(key)
        .and_then(Value::as_str)
        .ok_or(SessionError::Invalid(text::INVALID_FORMAT))
}

fn mode(value: &Value) -> Result<RecordedMode, SessionError> {
    match string(value, "mode")? {
        "restricted" => Ok(RecordedMode::Restricted),
        "normal" => Ok(RecordedMode::Normal),
        _ => Err(SessionError::Invalid(text::INVALID_FORMAT)),
    }
}

/// 錠を取らずにヘッダの処理系だけを読む。再開では元のヘッダと比べる。
pub fn recorded_benitoite(
    paths: &DataPaths,
    id: &SessionId,
) -> Result<Option<super::RecordedBenitoite>, SessionError> {
    let root = sessions_root(&paths.sessions_dir)?;
    let directory = open_directory_at(&root, id.as_str())?;
    let file = super::open_existing_file(&directory, "session.jsonl", OFlags::RDONLY)?;
    let mut bytes = Vec::new();
    BufReader::new(file)
        .take(
            u64::try_from(MAX_BYTES)
                .unwrap_or(u64::MAX)
                .saturating_add(1),
        )
        .read_until(b'\n', &mut bytes)?;
    if bytes.len() > MAX_BYTES || bytes.last() != Some(&b'\n') {
        return Err(SessionError::Invalid(text::INVALID_FORMAT));
    }
    let value: Value = serde_json::from_slice(&bytes)?;
    super::replay::check_format(&value)?;
    if string(&value, "type")? != "header" {
        return Err(SessionError::Invalid(text::INVALID_FORMAT));
    }
    value
        .get("benitoite")
        .cloned()
        .map(serde_json::from_value)
        .transpose()
        .map(Option::flatten)
        .map_err(Into::into)
}

/// 今の ID を除き、同じプロジェクトの直前のヘッダを読む。候補の錠は取らない。
pub fn previous_benitoite(
    paths: &DataPaths,
    project: &std::path::Path,
    current: &SessionId,
) -> Result<Option<super::RecordedBenitoite>, SessionError> {
    let project = fs::canonicalize(project)?;
    let root = sessions_root(&paths.sessions_dir)?;
    // ID の乱数部分は同じ秒の生成順を表さないので、最後に使った記録の時刻で選ぶ。
    let mut candidates = Vec::new();
    for id in ids(paths)? {
        if &id == current {
            continue;
        }
        let modified = open_directory_at(&root, id.as_str())
            .and_then(|directory| {
                super::open_existing_file(&directory, "session.jsonl", OFlags::RDONLY)
            })
            .and_then(|file| Ok(file.metadata()?.modified()?));
        if let Ok(modified) = modified {
            candidates.push((modified, id));
        }
    }
    candidates.sort_unstable_by(|left, right| right.cmp(left));
    for (_, id) in candidates {
        if &id == current {
            continue;
        }
        let mut entry = SessionEntry::new(&id);
        if scan(&root, &mut entry).is_ok()
            && entry
                .project_dir
                .as_ref()
                .and_then(|path| fs::canonicalize(path).ok())
                .is_some_and(|path| path == project)
        {
            return recorded_benitoite(paths, &id);
        }
    }
    Ok(None)
}
