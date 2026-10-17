//! リンクと特殊ファイルを開かずに、上限付きで木を辿る（設計書 01-05「辿り方」）。

use super::{text, types::*};
use crate::safe_fs::{ScopeRelPath, ScopeRoot, gitignore};
use crate::session::Session;
use crate::tools::exclude::ExcludeRules;
use ignore::gitignore::Gitignore;
use rustix::fs::FileType;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashSet};
use std::io::Read;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

pub(super) fn capture(
    root: &ScopeRoot,
    exclude: &ExcludeRules,
    session: &Session,
    limits: &CheckpointLimits,
    previous: Option<(&Manifest, Timestamp)>,
    cancelled: &AtomicBool,
) -> (Manifest, Vec<String>, CheckpointStats, Vec<PathBuf>) {
    let mut walk = Walk {
        root,
        session,
        limits,
        previous_started_at: previous.map(|(_, time)| time),
        cancelled,
        started: Instant::now(),
        manifest: Manifest {
            format: 1,
            project: root.real_path().into(),
            complete: true,
            root_enumerated: false,
            entries: Vec::new(),
        },
        incomplete: Vec::new(),
        incomplete_seen: HashSet::new(),
        stats: CheckpointStats::default(),
        hard_links: Vec::new(),
        content_bytes: 0,
        previous_entries: previous
            .map(|(m, _)| {
                m.entries
                    .iter()
                    .filter_map(|e| e.raw_path().ok().map(|p| (p, e)))
                    .collect()
            })
            .unwrap_or_default(),
    };
    let root_directory = match root.no_follow() {
        Ok(directory) => Arc::new(directory),
        Err(error) => {
            walk.failed(Path::new(""), None, error.to_string());
            walk.manifest.complete = false;
            return (walk.manifest, walk.incomplete, walk.stats, walk.hard_links);
        }
    };
    let root_exclusion = exclude.reason(root.real_path());
    let mut pending = vec![Pending {
        parent: root_directory,
        local: None,
        path: PathBuf::new(),
        index: None,
        ignores: Arc::new(Vec::new()),
    }];
    while let Some(Pending {
        parent,
        local,
        path,
        index,
        mut ignores,
    }) = pending.pop()
    {
        if walk.stopped() {
            break;
        }
        // 子は保持中の親から一要素だけで開く。祖先をパスから開き直さず、
        // 同じ親を待つ兄弟は記述子を共有する（設計書 01-05「辿り方」）。
        let directory = match local {
            None => parent,
            Some(local) => match parent.open_dir(&local) {
                Ok(directory) => Arc::new(directory),
                Err(error) => {
                    walk.failed(&path, index, error.to_string());
                    continue;
                }
            },
        };
        let ignore_error = if root_exclusion.is_none() {
            walk.load_ignore(&directory, &path, &mut ignores)
        } else {
            None
        };
        let mut names = Vec::new();
        let remaining = limits
            .max_entries
            .saturating_sub(walk.manifest.entries.len());
        let mut enumerated = true;
        let result = directory.visit_names(|name| {
            if names.len() >= remaining || walk.stopped() {
                enumerated = false;
                return false;
            }
            names.push(name);
            true
        });
        if let Err(error) = result {
            enumerated = false;
            walk.failed(&path, index, error.to_string());
        }
        names.sort();
        if !enumerated && !walk.stopped() {
            walk.mark(text::ENTRY_LIMIT);
        }
        let mut children = Vec::new();
        for name in names {
            if walk.stopped() || walk.manifest.entries.len() >= limits.max_entries {
                enumerated = false;
                walk.mark(text::ENTRY_LIMIT);
                break;
            }
            let relative = path.join(&name);
            let local = match ScopeRelPath::new(Path::new(&name)) {
                Ok(local) => local,
                Err(error) => {
                    walk.failed(&relative, None, error.to_string());
                    continue;
                }
            };
            let stat = match directory.stat(&local) {
                Ok(stat) => stat,
                Err(error) => {
                    walk.failed(&relative, None, error.to_string());
                    continue;
                }
            };
            let kind = match FileType::from_raw_mode(stat.st_mode) {
                FileType::RegularFile => ManifestKind::File,
                FileType::Directory => ManifestKind::Directory,
                FileType::Symlink => ManifestKind::Symlink,
                FileType::Fifo
                | FileType::Socket
                | FileType::CharacterDevice
                | FileType::BlockDevice
                | FileType::Unknown => ManifestKind::Other,
            };
            let mut entry = ManifestEntry::new(
                &relative,
                kind,
                Identity::from_stat(&stat),
                stat_mode(&stat) & 0o111,
            );
            let absolute = root.real_path().join(&relative);
            entry.excluded = root_exclusion.clone().or_else(|| {
                exclusion(
                    Path::new(&name),
                    kind,
                    &entry.identity,
                    &ignores,
                    exclude,
                    &absolute,
                    limits,
                )
            });
            if kind == ManifestKind::File && entry.identity.nlink >= 2 {
                walk.hard_links.push(relative.clone());
            }
            if entry.excluded.is_none() {
                match kind {
                    ManifestKind::Directory => children.push(Pending {
                        parent: Arc::clone(&directory),
                        local: Some(local),
                        path: relative.clone(),
                        index: Some(walk.manifest.entries.len()),
                        ignores: Arc::clone(&ignores),
                    }),
                    ManifestKind::Symlink => match directory.read_link(&local) {
                        Ok(target) => {
                            (entry.link_target, entry.link_target_hex) = encode_path(&target);
                        }
                        Err(error) => entry.error = Some(error.to_string()),
                    },
                    ManifestKind::File => {
                        if let Err(error) = walk.file(&directory, &local, &relative, &mut entry) {
                            entry.error = Some(error);
                        }
                    }
                    ManifestKind::Other => {}
                }
                if name == ".gitignore" && ignore_error.is_some() {
                    entry.error = ignore_error.clone();
                }
            } else {
                // 除外領域の活動は時点の内容を変えない。存在・種類・理由だけを残す。
                entry.identity = Identity::default();
                entry.executable = 0;
            }
            walk.manifest.entries.push(entry);
        }
        if let Some(entry) = index.and_then(|index| walk.manifest.entries.get_mut(index)) {
            entry.enumerated = enumerated;
        } else if index.is_none() {
            walk.manifest.root_enumerated = enumerated;
        }
        pending.extend(children.into_iter().rev());
    }
    walk.manifest.entries.sort_by_cached_key(|entry| {
        entry
            .raw_path()
            .unwrap_or_default()
            .as_os_str()
            .as_bytes()
            .to_vec()
    });
    walk.manifest.complete = walk.incomplete.is_empty();
    (walk.manifest, walk.incomplete, walk.stats, walk.hard_links)
}
struct Pending {
    parent: Arc<ScopeRoot>,
    local: Option<ScopeRelPath>,
    path: PathBuf,
    index: Option<usize>,
    ignores: Arc<Vec<Arc<Gitignore>>>,
}

fn exclusion(
    path: &Path,
    kind: ManifestKind,
    identity: &Identity,
    ignores: &[Arc<Gitignore>],
    exclude: &ExcludeRules,
    absolute: &Path,
    limits: &CheckpointLimits,
) -> Option<String> {
    if protected(path) {
        return Some("protected".into());
    }
    if let Some(reason) = exclude.reason_entry(absolute) {
        return Some(reason);
    }
    for ignore in ignores.iter().rev() {
        if let Some(ignored) =
            gitignore::decision(ignore, absolute, kind == ManifestKind::Directory)
        {
            if ignored {
                return Some("gitignore".into());
            }
            break;
        }
    }
    if kind == ManifestKind::File && identity.size > limits.max_file_bytes {
        return Some("file_size".into());
    }
    if kind == ManifestKind::Other {
        return Some("special".into());
    }
    None
}

struct Walk<'a> {
    root: &'a ScopeRoot,
    session: &'a Session,
    limits: &'a CheckpointLimits,
    previous_started_at: Option<Timestamp>,
    cancelled: &'a AtomicBool,
    started: Instant,
    manifest: Manifest,
    incomplete: Vec<String>,
    incomplete_seen: HashSet<String>,
    stats: CheckpointStats,
    hard_links: Vec<PathBuf>,
    content_bytes: u64,
    previous_entries: BTreeMap<PathBuf, &'a ManifestEntry>,
}
impl Walk<'_> {
    fn mark(&mut self, reason: &str) {
        // 多数の同じ上限の理由で記録の行を膨らませない（設計書 01-05「不完全な時点」）。
        if self.incomplete_seen.insert(reason.to_owned()) {
            self.incomplete.push(reason.into());
        }
    }
    fn stopped(&mut self) -> bool {
        if self.cancelled.load(Ordering::Acquire) {
            self.mark(text::CANCELLED);
            true
        } else if self.started.elapsed() >= self.limits.max_duration {
            self.mark(text::TIME_LIMIT);
            true
        } else {
            false
        }
    }
    fn failed(&mut self, path: &Path, index: Option<usize>, error: String) {
        self.mark(&format!("{}: {error}", path.display()));
        if let Some(entry) = index.and_then(|index| self.manifest.entries.get_mut(index)) {
            entry.error = Some(error);
        } else if !path.as_os_str().is_empty() {
            let mut entry = ManifestEntry::new(path, ManifestKind::Other, Identity::default(), 0);
            entry.error = Some(error);
            self.manifest.entries.push(entry);
        }
    }
    fn load_ignore(
        &mut self,
        directory: &ScopeRoot,
        relative: &Path,
        ignores: &mut Arc<Vec<Arc<Gitignore>>>,
    ) -> Option<String> {
        let max_bytes = self.limits.max_ignore_bytes;
        let result = gitignore::load_gitignore(
            directory,
            &self.root.real_path().join(relative),
            max_bytes,
            || self.stopped(),
        );
        match result {
            Ok(Some(ignore)) => {
                Arc::make_mut(ignores).push(Arc::new(ignore));
                None
            }
            Ok(None) => None,
            Err(error) => Some(error.to_string()),
        }
    }

    fn file(
        &mut self,
        directory: &ScopeRoot,
        local: &ScopeRelPath,
        path: &Path,
        entry: &mut ManifestEntry,
    ) -> Result<(), String> {
        if self.content_bytes.saturating_add(entry.identity.size) > self.limits.max_content_bytes {
            return Ok(());
        }
        if let (Some(previous), Some(old)) =
            (self.previous_started_at, self.previous_entries.get(path))
            && old.kind == ManifestKind::File
            && old.reusable
            && old.stored
            && old.identity == entry.identity
            && entry.identity.mtime.seconds < previous.seconds
            && entry.identity.ctime.seconds < previous.seconds
            && let Some(hash) = &old.sha256
            && (self.session.object_known(hash)
                || self
                    .session
                    .read_object(hash, self.limits.max_file_bytes)
                    .is_ok())
        {
            entry.sha256 = Some(hash.clone());
            entry.stored = true;
            entry.reusable = true;
            self.content_bytes = self.content_bytes.saturating_add(entry.identity.size);
            return Ok(());
        }
        let mut file = directory.open_read(local).map_err(|e| e.to_string())?;
        let before = file.metadata().map_err(|e| e.to_string())?;
        if !before.is_file()
            || before.len() > self.limits.max_file_bytes
            || Identity::from_metadata(&before) != entry.identity
        {
            return Err(text::CHANGED.into());
        }
        let mut bytes = Vec::new();
        let mut buffer = [0; 64 * 1024];
        loop {
            if self.stopped() {
                return Err(text::CANCELLED.into());
            }
            let read = file.read(&mut buffer).map_err(|e| e.to_string())?;
            if read == 0 {
                break;
            }
            let part = buffer.get(..read).ok_or_else(|| text::CHANGED.to_owned())?;
            bytes.extend_from_slice(part);
            self.stats.bytes_read = self
                .stats
                .bytes_read
                .saturating_add(u64::try_from(read).unwrap_or(u64::MAX));
            if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > self.limits.max_file_bytes {
                return Err(text::CHANGED.into());
            }
        }
        self.stats.files_hashed = self.stats.files_hashed.saturating_add(1);
        entry.sha256 = Some(hex(&Sha256::digest(&bytes)));
        let after = file.metadata().map_err(|e| e.to_string())?;
        entry.reusable = Identity::from_metadata(&after) == entry.identity;
        if !entry.reusable {
            entry.error = Some(text::CHANGED.into());
        }
        if let Some((hash, written)) = self
            .session
            .store_object(&bytes)
            .map_err(|e| e.to_string())?
        {
            entry.sha256 = Some(hash);
            entry.stored = true;
            self.content_bytes = self
                .content_bytes
                .saturating_add(u64::try_from(bytes.len()).unwrap_or(u64::MAX));
            if written {
                self.stats.objects_written = self.stats.objects_written.saturating_add(1);
            }
        }
        Ok(())
    }
}
