//! 記録を信頼せず、削除を先に行ってから浅い順に戻す（設計書 01-05「書き戻しの仕方」）。

use super::{CheckpointRecorder, RewindError, text, types::*};
use crate::safe_fs::{SafeFsError, ScopeRelPath, ScopeRoot};
use rustix::fs::FileType;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::File;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

impl CheckpointRecorder {
    /// ファイルを戻す前に時点を記録し、一覧と中身を検証してから操作する。
    pub fn restore_files(
        &self,
        checkpoint: &str,
        allow_incomplete: bool,
    ) -> Result<RewindReport, RewindError> {
        let selected = self.manifest(checkpoint)?;
        let before = self.capture(
            CheckpointReason::BeforeRewind,
            None,
            None,
            &AtomicBool::new(false),
        )?;
        if !before.incomplete.is_empty() && !allow_incomplete {
            return Err(RewindError::Invalid(text::INCOMPLETE_BEFORE));
        }
        // 保存量の不足でも、利用者が不完全な before を許した場合は、今作った一覧を使う。
        // 読み直す対象は選んだ時点だけであり、この値は主プロセスが組み立てたもの
        // （設計書 01-05「不完全な時点」）。
        let current = self
            .lock()?
            .previous
            .clone()
            .ok_or(RewindError::Invalid(text::NO_MANIFEST))?;
        let mut restore = Restore {
            root: &self.project,
            session: &self.session,
            limit: self.lock()?.limits.max_file_bytes,
            selected: &selected,
            target: indexed(&selected)?,
            current: indexed(&current)?,
            removed: BTreeSet::new(),
            report: RewindReport {
                before_rewind: Some(before.id),
                ..RewindReport::default()
            },
        };
        restore.run();
        Ok(restore.report)
    }
}
fn indexed(manifest: &Manifest) -> Result<BTreeMap<PathBuf, ManifestEntry>, RewindError> {
    manifest
        .entries
        .iter()
        .map(|entry| Ok((entry.raw_path()?, entry.clone())))
        .collect()
}
struct Restore<'a> {
    root: &'a ScopeRoot,
    session: &'a crate::session::Session,
    limit: u64,
    selected: &'a Manifest,
    target: BTreeMap<PathBuf, ManifestEntry>,
    current: BTreeMap<PathBuf, ManifestEntry>,
    removed: BTreeSet<PathBuf>,
    report: RewindReport,
}
impl Restore<'_> {
    fn failure(&mut self, path: &Path, reason: impl ToString) {
        self.report.failures.push(RewindFailure {
            path: path.into(),
            reason: reason.to_string(),
        });
    }
    fn changed(&mut self, path: &Path) {
        self.report.changed_paths.push(path.into());
        // 自分で子を作成・削除した親の属性だけを更新する。別の inode は受け入れない。
        if let Some(parent) = path.parent()
            && let Some(entry) = self.current.get_mut(parent)
            && let Ok(relative) = ScopeRelPath::new(parent)
            && let Ok(stat) = self.root.stat(&relative)
        {
            let identity = Identity::from_stat(&stat);
            if identity.dev == entry.identity.dev && identity.ino == entry.identity.ino {
                entry.identity = identity;
            }
        }
    }
    fn current_matches(&self, path: &Path) -> Result<(), RewindError> {
        let relative = ScopeRelPath::new(path)?;
        let actual = self.root.stat(&relative);
        let expected = if self.removed.contains(path) {
            None
        } else {
            self.current.get(path)
        };
        match (actual, expected) {
            (Err(SafeFsError::NotFound(_)), None) => Ok(()),
            (Ok(stat), Some(entry))
                if Identity::from_stat(&stat) == entry.identity
                    && kind(&stat) == entry.kind
                    && (stat_mode(&stat) & 0o111) == entry.executable =>
            {
                Ok(())
            }
            (Err(error), _) => Err(error.into()),
            _ => Err(RewindError::Invalid(text::CHANGED)),
        }
    }
    fn excluded(&self, path: &Path) -> bool {
        if protected(path) {
            return true;
        }
        path.ancestors().any(|ancestor| {
            self.current
                .get(ancestor)
                .is_some_and(|entry| entry.excluded.is_some() || entry.error.is_some())
                || self
                    .target
                    .get(ancestor)
                    .is_some_and(|entry| entry.excluded.is_some() || entry.error.is_some())
        })
    }
    fn deletable(&self, path: &Path) -> bool {
        if !self.selected.complete
            || self.excluded(path)
            || path.ancestors().any(|ancestor| {
                self.current.get(ancestor).is_some_and(contentless)
                    || self.target.get(ancestor).is_some_and(contentless)
            })
        {
            return false;
        }
        if let Some(entry) = self.target.get(path) {
            return entry.excluded.is_none() && entry.error.is_none();
        }
        for ancestor in path.ancestors().skip(1) {
            if let Some(entry) = self.target.get(ancestor)
                && entry.kind == ManifestKind::Directory
            {
                return entry.enumerated && entry.error.is_none() && entry.excluded.is_none();
            }
        }
        self.selected.root_enumerated
    }
    fn run(&mut self) {
        let mut deletions = self
            .current
            .iter()
            .filter(|(path, entry)| {
                self.target
                    .get(*path)
                    .is_none_or(|target| target.kind != entry.kind)
            })
            .map(|(path, _)| path.clone())
            .collect::<Vec<_>>();
        deletions.sort_by_key(|path| std::cmp::Reverse(path.components().count()));
        for path in deletions {
            if !self.deletable(&path) {
                self.failure(&path, text::NOT_DELETABLE);
                continue;
            }
            if let Err(error) = self.current_matches(&path) {
                self.failure(&path, error);
                continue;
            }
            let result = (|| -> Result<(), RewindError> {
                let relative = ScopeRelPath::new(&path)?;
                if self
                    .current
                    .get(&path)
                    .is_some_and(|entry| entry.kind == ManifestKind::Directory)
                {
                    self.root.remove_dir(&relative)?;
                } else {
                    self.root.remove_file(&relative)?;
                }
                Ok(())
            })();
            match result {
                Ok(()) => {
                    self.removed.insert(path.clone());
                    self.report.removed = self.report.removed.saturating_add(1);
                    self.changed(&path);
                }
                Err(error) => self.failure(&path, error),
            }
        }
        let mut targets = self.target.keys().cloned().collect::<Vec<_>>();
        targets.sort_by_key(|path| path.components().count());
        for path in targets {
            let Some(entry) = self.target.get(&path).cloned() else {
                continue;
            };
            if self.excluded(&path) {
                if entry.excluded.is_none() {
                    self.failure(&path, text::CURRENT_EXCLUSION);
                }
                continue;
            }
            let result = self.restore(&path, &entry);
            if let Err(error) = result {
                self.failure(&path, error);
            }
        }
    }
    fn restore(&mut self, path: &Path, entry: &ManifestEntry) -> Result<(), RewindError> {
        let relative = entry.relative_path()?;
        self.current_matches(path)?;
        let exists = !self.removed.contains(path) && self.current.contains_key(path);
        let current_kind = if exists {
            self.current.get(path).map(|entry| entry.kind)
        } else {
            None
        };
        if current_kind.is_some_and(|kind| kind != entry.kind) {
            return Err(RewindError::Invalid(text::CHANGED));
        }
        if entry.kind == ManifestKind::File && contentless(entry) {
            return Err(RewindError::Invalid(text::MISSING_CONTENT));
        }
        if exists
            && self
                .current
                .get(path)
                .is_some_and(|current| unchanged(entry, current))
        {
            return Ok(());
        }
        match entry.kind {
            ManifestKind::Directory => {
                if !exists {
                    self.root.create_dir(&relative)?;
                    self.report.created = self.report.created.saturating_add(1);
                    self.changed(path);
                    let stat = self.root.stat(&relative)?;
                    let mut updated = entry.clone();
                    updated.identity = Identity::from_stat(&stat);
                    updated.executable = stat_mode(&stat) & 0o111;
                    self.current.insert(path.into(), updated);
                    self.removed.remove(path);
                }
            }
            ManifestKind::File => {
                if exists
                    && self
                        .current
                        .get(path)
                        .is_some_and(|entry| entry.identity.nlink >= 2)
                {
                    return Err(RewindError::Invalid(text::HARD_LINK));
                }
                let hash = entry
                    .sha256
                    .as_ref()
                    .filter(|_| entry.stored)
                    .ok_or(RewindError::Invalid(text::MISSING_CONTENT))?;
                let source = self.session.open_object(hash, self.limit)?;
                let mut source = VerifiedReader {
                    source,
                    hasher: Sha256::new(),
                    expected: hash,
                    remaining: self.limit,
                    finished: false,
                };
                let mode = if exists {
                    (stat_mode(&self.root.stat(&relative)?) & 0o666) | (entry.executable & 0o111)
                } else if entry.executable != 0 {
                    0o755
                } else {
                    0o644
                };
                self.root
                    .write_from_with_mode(&relative, &mut source, mode)?;
                self.report.restored = self.report.restored.saturating_add(1);
                if !exists {
                    self.report.created = self.report.created.saturating_add(1);
                }
                self.changed(path);
            }
            ManifestKind::Symlink => {
                let target = entry.target()?;
                if exists {
                    self.root.remove_file(&relative)?;
                }
                self.root.symlink(&target, &relative)?;
                self.report.created = self.report.created.saturating_add(1);
                self.changed(path);
            }
            ManifestKind::Other => return Err(RewindError::Invalid(text::MISSING_CONTENT)),
        }
        Ok(())
    }
}
fn contentless(entry: &ManifestEntry) -> bool {
    entry.kind == ManifestKind::File && (!entry.stored || entry.sha256.is_none())
}
fn unchanged(selected: &ManifestEntry, current: &ManifestEntry) -> bool {
    selected.kind == current.kind
        && selected.executable == current.executable
        && match selected.kind {
            ManifestKind::File => {
                selected.stored && current.stored && selected.sha256 == current.sha256
            }
            ManifestKind::Symlink => selected
                .target()
                .ok()
                .zip(current.target().ok())
                .is_some_and(|(a, b)| a == b),
            ManifestKind::Directory => true,
            ManifestKind::Other => false,
        }
}
fn kind(stat: &rustix::fs::Stat) -> ManifestKind {
    match FileType::from_raw_mode(stat.st_mode) {
        FileType::RegularFile => ManifestKind::File,
        FileType::Directory => ManifestKind::Directory,
        FileType::Symlink => ManifestKind::Symlink,
        FileType::Fifo
        | FileType::Socket
        | FileType::CharacterDevice
        | FileType::BlockDevice
        | FileType::Unknown => ManifestKind::Other,
    }
}
// io::copy が EOF を得る前にハッシュを検証し、不一致なら一時ファイルを付け替えさせない。
// （設計書 01-05「書き戻しの仕方」）。
struct VerifiedReader<'a> {
    source: File,
    hasher: Sha256,
    expected: &'a str,
    remaining: u64,
    finished: bool,
}
impl Read for VerifiedReader<'_> {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        if self.finished || bytes.is_empty() {
            return Ok(0);
        }
        let count = self.source.read(bytes)?;
        if count == 0 {
            if hex(&self.hasher.clone().finalize()) != self.expected {
                return Err(io::Error::other(text::HASH_MISMATCH));
            }
            self.finished = true;
            return Ok(0);
        }
        self.remaining = self
            .remaining
            .checked_sub(u64::try_from(count).unwrap_or(u64::MAX))
            .ok_or_else(|| io::Error::other(text::FILE_LIMIT))?;
        let part = bytes
            .get(..count)
            .ok_or_else(|| io::Error::other(text::HASH_MISMATCH))?;
        self.hasher.update(part);
        Ok(count)
    }
}
