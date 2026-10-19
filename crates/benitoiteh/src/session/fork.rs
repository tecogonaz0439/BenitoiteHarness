//! 選んだ道と参照する出力だけを独立した記録へ写す（設計書 01-05「セッションの木」）。

use std::collections::{BTreeMap, BTreeSet};
use std::fs::File;
use std::io::{Read, Write};
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use rustix::fs::OFlags;

use super::{ForkedFrom, Record, Session, SessionError, SessionId, SessionTree, SpillInfo, text};
use crate::config::paths::DataPaths;
use crate::provider::{AssistantPart, Item, ToolResult};

/// 画面で選んだ、写す道の終わり。節点番号は元のセッションのもの。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ForkTarget {
    /// 今の先端まで。
    Current,
    /// どの枝でも、依頼の直前まで。
    BeforeRequest(u64),
}

/// 記録を読み直さず、錠を保ったまま起動処理へ渡す写しの結果。
pub struct ForkedSession {
    pub session: Session,
    pub from: ForkedFrom,
    pub request: Option<String>,
}

impl Session {
    /// 元の錠を保持し、働き手を止めた呼び出し側が、道を新しい ID へ写す。
    /// 空の道と上限超過は、ディレクトリを作る前に断る。
    pub fn fork(
        &self,
        paths: &DataPaths,
        project: &Path,
        target: ForkTarget,
    ) -> Result<ForkedSession, SessionError> {
        let plan = ForkPlan::read(self, paths, project, target)?;
        let session = Self::create(paths, plan.id.clone())?;
        plan.finish(self, session)
    }
}

// 検査と実際の書き込みを分け、元の全記録とファイルの記述子をコピーの間に保たない。
struct ForkPlan {
    id: SessionId,
    from: ForkedFrom,
    request: Option<String>,
    records: Vec<Record>,
    assets: Vec<Asset>,
    usage: super::StorageUsage,
}

impl ForkPlan {
    fn read(
        source: &Session,
        paths: &DataPaths,
        project: &Path,
        target: ForkTarget,
    ) -> Result<Self, SessionError> {
        let records = super::read_records(source.directory())?;
        let tree = SessionTree::from_records(&records)?;
        let mut request = None;
        let tip = match target {
            ForkTarget::Current => tree.tip,
            ForkTarget::BeforeRequest(node) => {
                let (parent, index) = tree
                    .entry(node)
                    .ok_or(SessionError::Invalid(text::INVALID_FORMAT))?;
                request = records.get(index).and_then(super::request_text);
                if request.is_none() {
                    return Err(SessionError::Invalid(text::INVALID_FORMAT));
                }
                parent
            }
        }
        .ok_or(SessionError::Invalid(text::EMPTY_FORK))?;
        let id = SessionId::generate(SystemTime::now())?;
        let destination = paths.sessions_dir.join(id.as_str());
        let mut selected = select(&records, &tree, tip, &id, project, source.id())?;
        let spills = spill_table(&records);
        for record in &mut selected {
            if let Record::ToolResult {
                result,
                spill,
                stderr_spill,
                ..
            } = record
            {
                rewrite_spills(
                    result,
                    [spill.as_ref(), stderr_spill.as_ref()],
                    source.directory(),
                    &destination,
                );
            }
            if let Record::Compaction { conversation, .. } = record {
                for item in &mut conversation.items {
                    if let Item::ToolResult(result) = item {
                        rewrite_spills(
                            result,
                            spills
                                .get(result.id.as_str())
                                .copied()
                                .unwrap_or([None, None]),
                            source.directory(),
                            &destination,
                        );
                    }
                }
            }
        }
        let assets = assets(source, &selected, &spills)?;
        let size = selected
            .iter()
            .try_fold(0_u64, |total, record| {
                Ok::<_, SessionError>(total.saturating_add(stored_size(record)?))
            })?
            .saturating_add(assets.iter().fold(0_u64, |total, asset| {
                total.saturating_add(asset.identity.size)
            }));
        let usage = super::storage_usage(paths, None)?;
        if size > super::quota::SESSION_LIMIT.min(super::TOTAL_LIMIT.saturating_sub(usage.total)) {
            return Err(SessionError::Invalid(text::STORAGE_LIMIT));
        }
        Ok(Self {
            id,
            from: ForkedFrom {
                session: source.id().to_string(),
                node: tip,
            },
            request,
            records: selected,
            assets,
            usage,
        })
    }

    fn finish(self, source: &Session, session: Session) -> Result<ForkedSession, SessionError> {
        session.configure_budget(&self.usage);
        let result = self.write(source, &session);
        if let Err(error) = result {
            // 元の誤りを保ち、削除の失敗も利用者へ知らせる。記録は最後まで確定しない。
            let cleanup = super::remove::directory(session.directory());
            let id = session.id().clone();
            drop(session);
            return Err(match cleanup {
                Ok(()) => error,
                Err(cleanup) => SessionError::ForkCleanup {
                    error: Box::new(error),
                    cleanup,
                    session: id,
                },
            });
        }
        Ok(ForkedSession {
            session,
            from: self.from,
            request: self.request,
        })
    }

    fn write(&self, source: &Session, session: &Session) -> Result<(), SessionError> {
        for asset in &self.assets {
            asset.copy(source, session)?;
        }
        // 全出力を写してから記録を書き、一時の記録が完成した時点でだけ確定する。
        // 記録の書き込みや後始末が失敗しても、session.jsonl は空のままになる。
        let parent = session
            .directory()
            .parent()
            .ok_or(SessionError::Invalid(text::UNSAFE_PATH))?;
        let directory =
            super::open_directory_at(&super::sessions_root(parent)?, session.id().as_str())?;
        let staged = super::create_file(&directory, "fork.jsonl", OFlags::WRONLY | OFlags::APPEND)?;
        *session
            .inner
            .log
            .lock()
            .map_err(|_| SessionError::Invalid(text::WRITER_FAILED))? =
            super::storage::LogState::new(staged.try_clone()?);
        let mut writer = session.writer()?;
        for record in &self.records {
            writer.append(record)?;
        }
        // 再開の状態は既に検査した記録から一度だけ組み立て、起動側へ渡す。
        let state = super::ResumeState::from_records(&self.records)?;
        // 名前の確定より先に内容を同期し、不完全な記録を再開可能な名前で残さない。
        staged.sync_all()?;
        rustix::fs::renameat(&directory, "fork.jsonl", &directory, "session.jsonl")?;
        *session
            .inner
            .resume
            .lock()
            .map_err(|_| SessionError::Invalid(text::INVALID_FORMAT))? = Some(state);
        Ok(())
    }
}

type Spills<'a> = BTreeMap<&'a str, [Option<&'a SpillInfo>; 2]>;
fn spill_table(records: &[Record]) -> Spills<'_> {
    records
        .iter()
        .filter_map(|record| {
            if let Record::ToolResult {
                result,
                spill,
                stderr_spill,
                ..
            } = record
            {
                Some((result.id.as_str(), [spill.as_ref(), stderr_spill.as_ref()]))
            } else {
                None
            }
        })
        .collect()
}

fn assets(
    source: &Session,
    selected: &[Record],
    spills: &Spills<'_>,
) -> Result<Vec<Asset>, SessionError> {
    let mut paths = BTreeSet::new();
    let mut hashes = BTreeSet::new();
    for record in selected {
        if let Record::ToolResult {
            result,
            spill,
            stderr_spill,
            ..
        } = record
        {
            paths.extend(
                spill
                    .iter()
                    .chain(stderr_spill.iter())
                    .map(|info| info.path.clone()),
            );
            hashes.extend(result.images.iter().map(|image| image.sha256.clone()));
        }
        if let Record::Compaction { conversation, .. } = record {
            for item in &conversation.items {
                if let Item::ToolResult(result) = item {
                    hashes.extend(result.images.iter().map(|image| image.sha256.clone()));
                    if let Some(infos) = spills.get(result.id.as_str()) {
                        paths.extend(infos.iter().flatten().map(|info| info.path.clone()));
                    }
                }
            }
        }
    }
    let mut assets = Vec::new();
    for kind in paths
        .into_iter()
        .map(AssetKind::Spill)
        .chain(hashes.into_iter().map(AssetKind::Object))
    {
        match kind.open(source) {
            Ok(file) => assets.push(Asset {
                identity: Identity::read(&file)?,
                kind,
            }),
            Err(SessionError::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
    }
    Ok(assets)
}

fn rewrite_spills(
    result: &mut ToolResult,
    infos: [Option<&SpillInfo>; 2],
    source: &Path,
    destination: &Path,
) {
    // 同じディレクトリの別のパスや、実在する名前を前置きとする本文は変えない。
    for info in infos.into_iter().flatten() {
        let old = source.join(&info.path).display().to_string();
        let new = destination.join(&info.path).display().to_string();
        let mut rewritten = String::new();
        let mut copied = 0;
        for (start, _) in result.text.match_indices(&old) {
            let end = start.saturating_add(old.len());
            let before = result
                .text
                .get(..start)
                .and_then(|value| value.chars().next_back());
            let after = result
                .text
                .get(end..)
                .and_then(|value| value.chars().next());
            let boundary = |value: Option<char>| {
                value.is_none_or(|ch| {
                    ch.is_whitespace() || matches!(ch, '[' | ']' | '(' | ')' | '"' | '\'' | '`')
                })
            };
            if boundary(before) && boundary(after) {
                rewritten.push_str(result.text.get(copied..start).unwrap_or_default());
                rewritten.push_str(&new);
                copied = end;
            }
        }
        if copied != 0 {
            rewritten.push_str(result.text.get(copied..).unwrap_or_default());
            result.text = rewritten;
        }
    }
}

fn select(
    records: &[Record],
    tree: &SessionTree,
    tip: u64,
    id: &SessionId,
    project: &Path,
    source: &SessionId,
) -> Result<Vec<Record>, SessionError> {
    let Some(Record::Header(header)) = records.first() else {
        return Err(SessionError::Invalid(text::INVALID_FORMAT));
    };
    let mut header = header.clone();
    header.session_id = id.to_string();
    header.started_at = id.started_at_utc();
    header.benitoiteh_version = env!("CARGO_PKG_VERSION").into();
    header.project_dir = project.to_path_buf();
    header.forked_from = Some(ForkedFrom {
        session: source.to_string(),
        node: tip,
    });
    let path: BTreeSet<_> = tree.path(Some(tip)).into_iter().collect();
    let last = tree
        .entry(tip)
        .ok_or(SessionError::Invalid(text::INVALID_FORMAT))?
        .1;
    let calls: BTreeSet<_> = records
        .iter()
        .filter(|record| record.node().is_some_and(|node| path.contains(&node)))
        .filter_map(|record| {
            if let Record::Assistant { response, .. } = record {
                Some(response)
            } else {
                None
            }
        })
        .flat_map(|response| response.parts.iter())
        .filter_map(|part| {
            if let AssistantPart::ToolCall(call) = part {
                Some(call.id.as_str())
            } else {
                None
            }
        })
        .collect();
    let mut result = vec![Record::Header(header)];
    let mut next = 1;
    for (index, record) in records.iter().enumerate().skip(1) {
        let keep = if matches!(record, Record::SessionStart { .. }) {
            index < last
        } else if let Record::ToolCall { call, .. } | Record::ToolStart { call } = record {
            calls.contains(call.id.as_str())
        } else {
            record.node().is_some_and(|node| path.contains(&node))
        };
        if !keep {
            continue;
        }
        let mut record = record.clone();
        if record.node().is_some() {
            record.set_node(next);
            next = next
                .checked_add(1)
                .ok_or(SessionError::Invalid(text::INVALID_FORMAT))?;
        }
        if let Record::SessionStart { session_id, .. } = &mut record {
            *session_id = id.to_string();
        }
        result.push(record);
    }
    Ok(result)
}

// 分割記録の名前は固定長の乱数なので、作成前にも実際の保存量を数えられる。
fn stored_size(record: &Record) -> Result<u64, SessionError> {
    let bytes = serde_json::to_vec(record)?.len();
    let size = if bytes >= super::MAX_BYTES {
        let parts = (0..bytes.div_ceil(super::MAX_BYTES))
            .map(|index| PathBuf::from(format!("spill/record-0000000000000000-{index}.json")))
            .collect();
        bytes
            .saturating_add(serde_json::to_vec(&Record::Spilled { parts })?.len())
            .saturating_add(1)
    } else {
        bytes.saturating_add(1)
    };
    Ok(u64::try_from(size).unwrap_or(u64::MAX))
}

#[derive(Debug, Eq, PartialEq)]
struct Identity {
    size: u64,
    dev: u64,
    ino: u64,
}
impl Identity {
    fn read(file: &File) -> Result<Self, SessionError> {
        let metadata = file.metadata()?;
        Ok(Self {
            size: metadata.len(),
            dev: metadata.dev(),
            ino: metadata.ino(),
        })
    }
}

struct Asset {
    identity: Identity,
    kind: AssetKind,
}
enum AssetKind {
    Spill(PathBuf),
    Object(String),
}
impl AssetKind {
    fn open(&self, source: &Session) -> Result<File, SessionError> {
        match self {
            Self::Spill(path) => {
                let _relative = super::spill_path(path)?;
                if !super::private_metadata(&source.inner.spill.metadata()?, true) {
                    return Err(SessionError::Invalid(text::UNSAFE_PATH));
                }
                let name = path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .ok_or(SessionError::Invalid(text::INVALID_SPILL))?;
                super::open_existing_file(&source.inner.spill, name, OFlags::RDONLY)
            }
            Self::Object(hash) => source.open_object(hash, crate::tools::MAX_IMAGE_BYTES),
        }
    }
}
impl Asset {
    fn copy(&self, source: &Session, session: &Session) -> Result<(), SessionError> {
        let mut file = self.kind.open(source)?;
        if Identity::read(&file)? != self.identity {
            return Err(SessionError::Invalid(text::FORK_ASSET_CHANGED));
        }
        match &self.kind {
            AssetKind::Spill(path) => {
                if !session.reserve(self.identity.size) {
                    return Err(SessionError::Invalid(text::STORAGE_LIMIT));
                }
                let name = path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .ok_or(SessionError::Invalid(text::INVALID_SPILL))?;
                let mut output = super::create_file(&session.inner.spill, name, OFlags::WRONLY)?;
                let copied = std::io::copy(
                    &mut Read::by_ref(&mut file).take(self.identity.size),
                    &mut output,
                )?;
                output.flush()?;
                if copied != self.identity.size || Identity::read(&file)? != self.identity {
                    return Err(SessionError::Invalid(text::FORK_ASSET_CHANGED));
                }
            }
            AssetKind::Object(hash) => {
                let mut bytes = Vec::new();
                Read::by_ref(&mut file)
                    .take(crate::tools::MAX_IMAGE_BYTES.saturating_add(1))
                    .read_to_end(&mut bytes)?;
                if Identity::read(&file)? != self.identity
                    || u64::try_from(bytes.len()).unwrap_or(u64::MAX)
                        > crate::tools::MAX_IMAGE_BYTES
                {
                    return Err(SessionError::Invalid(text::FORK_ASSET_CHANGED));
                }
                let (stored, _) = session
                    .store_object(&bytes)?
                    .ok_or(SessionError::Invalid(text::STORAGE_LIMIT))?;
                if stored != *hash {
                    return Err(SessionError::Invalid(text::INVALID_OBJECT));
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};

    type TestResult = Result<(), Box<dyn std::error::Error>>;
    struct Fixture {
        root: PathBuf,
        paths: DataPaths,
        source: Session,
        hash: String,
    }
    impl Fixture {
        fn new() -> Result<Self, Box<dyn std::error::Error>> {
            let root = std::env::temp_dir().join(format!(
                "bh-fork-phases-{}",
                SessionId::generate(SystemTime::now())?
            ));
            fs::create_dir(&root)?;
            let root = fs::canonicalize(root)?;
            fs::create_dir(root.join("tmp"))?;
            let paths = DataPaths::from_values(
                Some(&root.join("home")),
                Some(&root.join("config")),
                Some(&root.join("state")),
                Some(&root.join("tmp")),
            )?;
            let source = Session::create(&paths, SessionId::generate(SystemTime::now())?)?;
            let (hash, _) = source.store_object(b"original")?.ok_or("image")?;
            let header: Record = serde_json::from_value(serde_json::json!({
                "type":"header", "format":2, "session_id":source.id().as_str(), "started_at":source.id().started_at_utc(),
                "benitoiteh_version":"test", "mode":"normal", "project_dir":root, "benitoite":null,
                "provider":{"name":"mock","model":"mock","base_url":"https://mock.test","adapter":"openai-chat"},
                "system_prompt_sha256":"system", "tools_sha256":"tools"
            }))?;
            let mut writer = source.writer()?;
            writer.append(&header)?;
            let mut spill = source.spill_writer("output")?;
            spill.write_all(b"output")?;
            let spill = spill.finish()?;
            writer.append(&Record::ToolResult {
                node: 1,
                turn: 1,
                name: "read".into(),
                result: ToolResult {
                    id: "output".into(),
                    provider_id: None,
                    text: "image".into(),
                    is_error: false,
                    images: vec![crate::provider::ToolImage {
                        media_type: "image/gif".into(),
                        sha256: hash.clone(),
                        width: 1,
                        height: 1,
                        bytes: 8,
                        data: None,
                    }],
                },
                spill: Some(spill),
                stderr_spill: None,
            })?;
            Ok(Self {
                root,
                paths,
                source,
                hash,
            })
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _removed = fs::remove_dir_all(&self.root);
        }
    }

    // 関門: 本番の検査とコピーの間で実ファイルを変え、時間に頼らず TOCTOU を再現する。
    #[test]
    fn reopening_assets_rejects_replacements_size_changes_links_and_permissions() -> TestResult {
        for kind in ["spill", "object"] {
            for change in ["inode", "length", "link", "permissions", "missing"] {
                let f = Fixture::new()?;
                let plan = ForkPlan::read(&f.source, &f.paths, &f.root, ForkTarget::Current)?;
                let path = if kind == "object" {
                    f.source.directory().join("objects").join(&f.hash)
                } else {
                    let records = super::super::read_records(f.source.directory())?;
                    let spill = records
                        .iter()
                        .find_map(|r| {
                            if let Record::ToolResult { spill, .. } = r {
                                spill.as_ref()
                            } else {
                                None
                            }
                        })
                        .ok_or("spill")?;
                    f.source.directory().join(&spill.path)
                };
                let bytes = fs::read(&path)?;
                match change {
                    "inode" => {
                        let renamed = path.with_extension("retained");
                        fs::rename(&path, &renamed)?;
                        let file = fs::OpenOptions::new()
                            .write(true)
                            .create_new(true)
                            .mode(0o600)
                            .open(&path)?;
                        drop(file);
                        fs::write(&path, &bytes)?;
                    }
                    "length" => fs::write(&path, b"different length")?,
                    "link" => {
                        fs::remove_file(&path)?;
                        std::os::unix::fs::symlink(
                            f.source.directory().join("session.jsonl"),
                            &path,
                        )?;
                    }
                    "permissions" => fs::set_permissions(&path, fs::Permissions::from_mode(0o644))?,
                    "missing" => fs::remove_file(&path)?,
                    _ => return Err("case".into()),
                }
                let copied = Session::create(&f.paths, plan.id.clone())?;
                let error = plan
                    .finish(&f.source, copied)
                    .err()
                    .ok_or("changed asset was accepted")?;
                if matches!(change, "inode" | "length") {
                    assert!(
                        matches!(error, SessionError::Invalid(text::FORK_ASSET_CHANGED)),
                        "{kind}/{change}: {error}"
                    );
                }
                assert_eq!(
                    fs::read_dir(&f.paths.sessions_dir)?.count(),
                    1,
                    "{kind}/{change}"
                );
            }
        }
        Ok(())
    }

    // 関門: 後始末の削除順に依存せず、画像の失敗前に記録を公開しないことを守る。
    #[test]
    fn asset_failure_does_not_publish_any_session_records() -> TestResult {
        let f = Fixture::new()?;
        let plan = ForkPlan::read(&f.source, &f.paths, &f.root, ForkTarget::Current)?;
        let session = Session::create(&f.paths, plan.id.clone())?;
        fs::write(
            f.source.directory().join("objects").join(&f.hash),
            b"tampered",
        )?;
        let error = plan
            .write(&f.source, &session)
            .err()
            .ok_or("invalid hash was accepted")?;
        assert!(matches!(error, SessionError::Invalid(text::INVALID_OBJECT)));
        assert!(fs::read(session.directory().join("session.jsonl"))?.is_empty());
        assert!(super::super::ResumeState::read(session.directory()).is_err());
        Ok(())
    }

    // 関門: コピーか記録の書き込みの失敗に削除の失敗を重ね、元の原因と非再開性を守る。
    #[test]
    fn cleanup_failure_preserves_the_cause_and_leaves_no_committed_record() -> TestResult {
        if rustix::process::geteuid().is_root() {
            eprintln!(
                "Skipped fork cleanup permission check: root can remove mode 000 directories."
            );
            return Ok(());
        }
        for failure in ["object", "record"] {
            let f = Fixture::new()?;
            let mut plan = ForkPlan::read(&f.source, &f.paths, &f.root, ForkTarget::Current)?;
            let id = plan.id.clone();
            let session = Session::create(&f.paths, id.clone())?;
            let directory = session.directory().to_owned();
            let blocked = directory.join("blocked");
            fs::create_dir(&blocked)?;
            fs::set_permissions(&blocked, fs::Permissions::from_mode(0o000))?;
            if failure == "object" {
                fs::write(
                    f.source.directory().join("objects").join(&f.hash),
                    b"tampered",
                )?;
            } else {
                // ヘッダの後に不正な二つ目のヘッダを置き、途中まで書いてから失敗させる。
                let header = plan.records.first().cloned().ok_or("header")?;
                plan.records.push(header);
            }
            let outcome = plan.finish(&f.source, session);
            fs::set_permissions(&blocked, fs::Permissions::from_mode(0o700))?;
            let error = outcome.err().ok_or("fork should fail")?;
            let SessionError::ForkCleanup {
                error: cause,
                cleanup,
                session,
            } = &error
            else {
                return Err(format!("cleanup failure lost: {error}").into());
            };
            assert_eq!(session, &id);
            let expected = if failure == "object" {
                text::INVALID_OBJECT
            } else {
                text::INVALID_FORMAT
            };
            assert!(
                matches!(cause.as_ref(), SessionError::Invalid(message) if *message == expected)
            );
            assert_eq!(cleanup.kind(), std::io::ErrorKind::PermissionDenied);
            assert!(error.to_string().contains(expected));
            assert!(
                error
                    .to_string()
                    .contains("Could not remove incomplete fork")
            );
            assert!(Session::open(&f.paths, &id).is_err());
            assert!(
                super::super::list_sessions(&f.paths)?
                    .iter()
                    .any(|entry| entry.id == id.as_str() && entry.error.is_some())
            );
            if let Ok(record) = fs::read(directory.join("session.jsonl")) {
                assert!(record.is_empty());
            }
        }
        Ok(())
    }
}
