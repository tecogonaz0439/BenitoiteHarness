//! プロジェクトの時点を記録し、信頼しない一覧を検証して戻す（設計書 01-05「巻き戻し」）。

mod capture;
mod restore;
mod text;
mod types;
pub use types::*;

use crate::safe_fs::{SafeFsError, ScopeRoot};
use crate::session::{Record, Session, SessionError};
use crate::tools::exclude::ExcludeRules;
use std::collections::BTreeSet;
use std::fmt;
use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

/// 一覧の破損、会話の制約、保存の誤り。
#[derive(Debug)]
pub enum RewindError {
    Session(SessionError),
    SafeFs(SafeFsError),
    Invalid(&'static str),
    IncompleteBefore {
        checkpoint: String,
        incomplete: Vec<String>,
    },
    Cancelled,
}
impl fmt::Display for RewindError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Session(error) => error.fmt(f),
            Self::SafeFs(error) => error.fmt(f),
            Self::Invalid(message) => f.write_str(message),
            Self::IncompleteBefore { .. } => f.write_str(text::INCOMPLETE_BEFORE),
            Self::Cancelled => f.write_str(text::CANCELLED),
        }
    }
}
impl std::error::Error for RewindError {}
impl From<SessionError> for RewindError {
    fn from(error: SessionError) -> Self {
        Self::Session(error)
    }
}
impl From<SafeFsError> for RewindError {
    fn from(error: SafeFsError) -> Self {
        Self::SafeFs(error)
    }
}

/// 道具が実行する前に書いた時点と、新しく見つけたハードリンク。
#[derive(Clone, Debug)]
pub struct CheckpointNotice {
    pub checkpoint: Checkpoint,
    pub hard_links: Vec<std::path::PathBuf>,
}

#[derive(Debug)]
struct State {
    next: u64,
    turn: u64,
    node: Option<u64>,
    limits: CheckpointLimits,
    checkpoints: Vec<Checkpoint>,
    previous: Option<Manifest>,
    previous_started_at: Option<Timestamp>,
    hard_links: BTreeSet<std::path::PathBuf>,
    notices: Vec<CheckpointNotice>,
    stats: CheckpointStats,
}

/// エージェントと書く道具が共有する、実行の直前の記録の口。
#[derive(Clone, Debug)]
pub struct CheckpointRecorder {
    session: Session,
    project: Arc<ScopeRoot>,
    exclude: ExcludeRules,
    state: Arc<Mutex<State>>,
}

impl CheckpointRecorder {
    /// 記録の最大の連番と節点の続きから始める。渡された ID はパスに使わない。
    pub fn new(
        session: Session,
        project: &ScopeRoot,
        exclude: ExcludeRules,
        records: &[Record],
    ) -> Result<Self, RewindError> {
        let mut checkpoints = Vec::new();
        let mut maximum = 0_u64;
        let tree = crate::session::SessionTree::from_records(records)?;
        for record in records {
            match record {
                Record::Checkpoint(checkpoint) => {
                    let number = checkpoint_number(&checkpoint.id)
                        .ok_or(RewindError::Invalid(text::INVALID_CHECKPOINT))?;
                    maximum = maximum.max(number);
                    checkpoints.push(checkpoint.clone());
                }
                Record::ProjectDirChanged { .. } => checkpoints.clear(),
                Record::Branch { .. }
                | Record::Compaction { .. }
                | Record::Header(_)
                | Record::SessionStart { .. }
                | Record::TurnStart { .. }
                | Record::User { .. }
                | Record::Assistant { .. }
                | Record::ToolCall { .. }
                | Record::ToolStart { .. }
                | Record::ToolResult { .. }
                | Record::Approval { .. }
                | Record::ImagesRemoved { .. }
                | Record::Retry { .. }
                | Record::Error { .. }
                | Record::TurnEnd { .. }
                | Record::SessionEnd { .. }
                | Record::Summary { .. }
                | Record::WriteTarget { .. }
                | Record::Spilled { .. } => {}
            }
        }
        Ok(Self {
            session,
            project: Arc::new(project.no_follow()?),
            exclude,
            state: Arc::new(Mutex::new(State {
                next: maximum
                    .checked_add(1)
                    .ok_or(RewindError::Invalid(text::INVALID_CHECKPOINT))?,
                turn: 0,
                node: tree.tip,
                limits: CheckpointLimits::default(),
                checkpoints,
                previous: None,
                previous_started_at: None,
                hard_links: BTreeSet::new(),
                notices: Vec::new(),
                stats: CheckpointStats::default(),
            })),
        })
    }

    /// 番号と節点の続きは保ち、新しい根の時点だけを記録し始める。
    pub fn restart(
        &mut self,
        project: &ScopeRoot,
        exclude: ExcludeRules,
    ) -> Result<(), RewindError> {
        *self = self.prepare_restart(project, exclude)?;
        Ok(())
    }

    // 現在の時点を消す前に、記述子の複製と状態の取得を済ませる。
    // 共有していた元の記録にも触れない（設計書 01-03「create_project（制限モード）」）。
    pub(crate) fn prepare_restart(
        &self,
        project: &ScopeRoot,
        exclude: ExcludeRules,
    ) -> Result<Self, RewindError> {
        let root = project.no_follow()?;
        let state = self.lock()?;
        Ok(Self {
            session: self.session.clone(),
            project: Arc::new(root),
            exclude,
            state: Arc::new(Mutex::new(State {
                next: state.next,
                turn: state.turn,
                node: state.node,
                limits: state.limits.clone(),
                checkpoints: Vec::new(),
                previous: None,
                previous_started_at: None,
                hard_links: BTreeSet::new(),
                notices: Vec::new(),
                stats: CheckpointStats::default(),
            })),
        })
    }

    /// 一覧の画面に必要な情報をセッションの行の情報だけから返す。
    pub fn checkpoints(&self) -> Result<Vec<CheckpointInfo>, RewindError> {
        let records = crate::session::read_records(self.session.directory())?;
        let tree = crate::session::SessionTree::from_records(&records)?;
        self.checkpoints_from_tree(&tree)
    }

    /// 同じ操作が作った木の索引で、会話を戻せる時点を求める。
    pub(crate) fn checkpoints_from_tree(
        &self,
        tree: &crate::session::SessionTree,
    ) -> Result<Vec<CheckpointInfo>, RewindError> {
        let state = self.lock()?;
        Ok(state
            .checkpoints
            .iter()
            .map(|checkpoint| CheckpointInfo {
                conversation_available: tree
                    .checkpoint_request(&checkpoint.id)
                    .is_some_and(|node| tree.path_nodes().contains(&node)),
                checkpoint: checkpoint.clone(),
                request: None,
                tool: None,
            })
            .collect())
    }
    /// 後の道具が記録する番を更新する。
    pub fn set_turn(&self, turn: u64) -> Result<(), RewindError> {
        self.lock()?.turn = turn;
        Ok(())
    }
    /// 会話の節点を進めた後、次の時点の結び付け先を更新する。
    pub fn set_node(&self, node: Option<u64>) -> Result<(), RewindError> {
        self.lock()?.node = node;
        Ok(())
    }
    // 追記の後に錠の取得で失敗すると、会話と記録の先端がずれるため、先に取る
    // （設計書 01-05「セッションの木」）。
    pub(crate) fn commit_node(
        &self,
        node: Option<u64>,
        append: impl FnOnce() -> Result<(), SessionError>,
    ) -> Result<(), RewindError> {
        let mut state = self.lock()?;
        append()?;
        state.node = node;
        Ok(())
    }

    /// 次の時点から適用する、走査と保存の上限。
    pub fn set_limits(&self, limits: CheckpointLimits) -> Result<(), RewindError> {
        self.lock()?.limits = limits;
        Ok(())
    }
    /// 最後の時点で実際に読み直した量。
    pub fn stats(&self) -> Result<CheckpointStats, RewindError> {
        Ok(self.lock()?.stats.clone())
    }
    /// 記録を済ませた通知を取り出す。記録の行はここへ戻る前に flush 済みである。
    pub fn take_notices(&self) -> Result<Vec<CheckpointNotice>, RewindError> {
        Ok(std::mem::take(&mut self.lock()?.notices))
    }
    /// ファイル一覧と中身を保存し、時点の行を実行より先に flush する。
    pub fn capture(
        &self,
        reason: CheckpointReason,
        call_id: Option<&str>,
        cancelled: &AtomicBool,
    ) -> Result<Checkpoint, RewindError> {
        let mut state = self.lock()?;
        let started_at = Timestamp::now();
        let (manifest, mut incomplete, stats, hard_links) = capture::capture(
            &self.project,
            &self.exclude,
            &self.session,
            &state.limits,
            state.previous.as_ref().zip(state.previous_started_at),
            cancelled,
        );
        state.stats = stats;
        // 実行しない道具の時点を記録すると、bash を実行済みと誤認する。
        // 走査を終えて取り消しを確認した後でだけ行を書く（設計書 01-05「記録する時点」）。
        if reason != CheckpointReason::Request
            && cancelled.load(std::sync::atomic::Ordering::Acquire)
        {
            return Err(RewindError::Cancelled);
        }

        let bytes = serde_json::to_vec(&manifest).map_err(SessionError::from)?;
        let hash = if u64::try_from(bytes.len()).unwrap_or(u64::MAX)
            > state.limits.max_manifest_bytes
        {
            incomplete.push(text::MANIFEST_LIMIT.into());
            None
        } else {
            match self.session.store_object(&bytes) {
                Ok(Some((hash, written))) => {
                    if written {
                        state.stats.objects_written = state.stats.objects_written.saturating_add(1);
                    }
                    Some(hash)
                }
                Ok(None) => {
                    incomplete.push(text::STORAGE_LIMIT.into());
                    None
                }
                Err(error) => {
                    incomplete.push(error.to_string());
                    None
                }
            }
        };
        if reason != CheckpointReason::Request
            && cancelled.load(std::sync::atomic::Ordering::Acquire)
        {
            return Err(RewindError::Cancelled);
        }
        let mut checkpoint = Checkpoint {
            id: format!("cp-{}", state.next),
            reason,
            turn: state.turn,
            call_id: call_id.map(str::to_owned),
            created_at: started_at,
            node: state.node,
            manifest: hash,
            incomplete,
        };
        state.next = state
            .next
            .checked_add(1)
            .ok_or(RewindError::Invalid(text::INVALID_CHECKPOINT))?;
        if let Err(error) = self
            .session
            .writer()
            .and_then(|mut writer| writer.append(&Record::Checkpoint(checkpoint.clone())))
        {
            // 行を書けないときも、実行の直前の失敗を画面へ渡す。道具の実行は
            // 呼び出す側が続けられる（設計書 01-05「不完全な時点」）。
            checkpoint.incomplete.push(error.to_string());
            state.notices.push(CheckpointNotice {
                checkpoint,
                hard_links: Vec::new(),
            });
            return Err(error.into());
        }
        let hard_links = hard_links
            .into_iter()
            .filter(|path| state.hard_links.insert(path.clone()))
            .collect();
        state.notices.push(CheckpointNotice {
            checkpoint: checkpoint.clone(),
            hard_links,
        });
        state.previous = Some(manifest);
        state.previous_started_at = Some(started_at);
        state.checkpoints.push(checkpoint.clone());
        Ok(checkpoint)
    }
    /// 一覧のハッシュ、形式、プロジェクト、パスを検証して読む。
    pub fn manifest(&self, checkpoint: &str) -> Result<Manifest, RewindError> {
        let info = self.selected(checkpoint)?;
        let hash = info
            .manifest
            .ok_or(RewindError::Invalid(text::NO_MANIFEST))?;
        let limit = self.lock()?.limits.max_manifest_bytes;
        let bytes = self.session.read_object(&hash, limit)?;
        let manifest: Manifest = serde_json::from_slice(&bytes).map_err(SessionError::from)?;
        if manifest.format != 1 || manifest.project != self.project.real_path() {
            return Err(RewindError::Invalid(text::PROJECT_MISMATCH));
        }
        let mut paths = BTreeSet::new();
        for entry in &manifest.entries {
            let raw = entry.raw_path()?;
            // 守る名前の項目は除外の存在の印だけを許し、決して操作の対象にしない。
            if types::protected(&raw) && entry.excluded.as_deref() == Some("protected") {
                crate::safe_fs::ScopeRelPath::new(&raw)?;
            } else {
                entry.relative_path()?;
            }
            if !paths.insert(raw) {
                return Err(RewindError::Invalid(text::INVALID_MANIFEST));
            }
        }
        Ok(manifest)
    }
    pub(super) fn selected(&self, id: &str) -> Result<Checkpoint, RewindError> {
        checkpoint_number(id).ok_or(RewindError::Invalid(text::INVALID_CHECKPOINT))?;
        self.lock()?
            .checkpoints
            .iter()
            .find(|checkpoint| checkpoint.id == id)
            .cloned()
            .ok_or(RewindError::Invalid(text::INVALID_CHECKPOINT))
    }
    fn lock(&self) -> Result<std::sync::MutexGuard<'_, State>, RewindError> {
        self.state
            .lock()
            .map_err(|_| RewindError::Invalid(text::STATE))
    }
    /// 書く対象が記録する範囲にあるかを返す。
    pub fn contains(&self, path: &Path) -> bool {
        path.starts_with(self.project.real_path())
    }
}
fn checkpoint_number(id: &str) -> Option<u64> {
    static PATTERN: std::sync::OnceLock<Result<regex::Regex, regex::Error>> =
        std::sync::OnceLock::new();
    let pattern = PATTERN
        .get_or_init(|| regex::Regex::new(r"^cp-[0-9]+$"))
        .as_ref()
        .ok()?;
    if !pattern.is_match(id) {
        return None;
    }
    id.strip_prefix("cp-")?.parse::<u64>().ok()
}

impl CheckpointRecorder {
    /// 実行の直前の一覧に残した .gitignore を使い、まだなかった書く先も分類する。
    pub(crate) fn excluded_in_manifest(
        &self,
        manifest: Option<&Manifest>,
        absolute: &Path,
    ) -> bool {
        if self.exclude.reason(absolute).is_some() {
            return true;
        }
        let Ok(relative) = absolute.strip_prefix(self.project.real_path()) else {
            return true;
        };
        if types::protected(relative) {
            return true;
        }
        let Some(manifest) = manifest else {
            return true;
        };
        let entries = manifest
            .entries
            .iter()
            .filter_map(|entry| entry.raw_path().ok().map(|path| (path, entry)))
            .collect::<std::collections::BTreeMap<_, _>>();
        if relative.ancestors().any(|path| {
            entries
                .get(path)
                .is_some_and(|entry| entry.excluded.is_some())
        }) {
            return true;
        }
        // 最も近い .gitignore の明示の規則が、祖先の規則より優先する。
        for directory in relative.ancestors().skip(1) {
            let Some(entry) = entries.get(&directory.join(".gitignore")) else {
                continue;
            };
            let Some(hash) = &entry.sha256 else {
                return true;
            };
            let Ok(bytes) = self.session.read_object(hash, 1024 * 1024) else {
                return true;
            };
            let Ok(ignore) = crate::safe_fs::gitignore::build_gitignore(
                &self.project.real_path().join(directory),
                &bytes,
                || false,
            ) else {
                return true;
            };
            if let Some(ignored) = crate::safe_fs::gitignore::decision(&ignore, absolute, false) {
                return ignored;
            }
        }
        false
    }
}
