//! 利用者とプロジェクトの Skill を範囲内で読み、索引と読める根を固定する（設計書 01-04「利用者の Agent Skill」）。

mod text;

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::fs::File;
use std::io::Read;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::benitoite::skill::frontmatter;
use crate::config::UserSkillsConfig;
use crate::safe_fs::{self, SafeFsError, ScopeRelPath, ScopeRoot};
use crate::session::{RecordedSkill, SkillSource, SkippedInstruction};
use crate::tools::exclude::ExcludeRules;

const FILE_LIMIT: usize = 256 * 1024;
const ENTRY_LIMIT: usize = 1_000;
const SKILL_LIMIT: usize = 100;
const INDEX_LIMIT: usize = 16 * 1024;

/// 全文を保持せず、索引と読み取り範囲の識別情報だけを保つ。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LoadedSkill {
    pub name: String,
    pub description: String,
    pub skill_md: PathBuf,
    pub directory: PathBuf,
    pub source: SkillSource,
    pub sha256: String,
}

impl LoadedSkill {
    /// セッションのヘッダと開始の通知に共通の識別情報。
    pub fn recorded(&self) -> RecordedSkill {
        RecordedSkill {
            name: self.name.clone(),
            path: self.skill_md.clone(),
            sha256: self.sha256.clone(),
            source: self.source,
        }
    }

    fn render(&self) -> String {
        format!(
            "<skill name=\"{}\" path=\"{}\">{}</skill>",
            escape(&self.name),
            escape(&self.skill_md.to_string_lossy()),
            escape(&self.description),
        )
    }
}

/// 環境の情報と画面に渡す、セッションの間変えない一覧。
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct UserSkills {
    pub loaded: Vec<LoadedSkill>,
    pub skipped: Vec<SkippedInstruction>,
}

impl UserSkills {
    /// 置き換え後の索引。上限には案内の行を数えない。
    pub fn render(&self) -> Option<String> {
        if self.loaded.is_empty() {
            return None;
        }
        Some(format!(
            "{}\n{}",
            include_str!("../../prompts/user-skills.md").trim_end(),
            render_index(self.loaded.iter()),
        ))
    }

    /// 読んだファイルのハッシュを含む、記録用の一覧。
    pub fn recorded(&self) -> Vec<RecordedSkill> {
        self.loaded.iter().map(LoadedSkill::recorded).collect()
    }
}

fn escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn render_index<'a>(skills: impl Iterator<Item = &'a LoadedSkill>) -> String {
    format!(
        "<skills>\n{}\n</skills>",
        skills
            .map(LoadedSkill::render)
            .collect::<Vec<_>>()
            .join("\n")
    )
}

#[derive(Debug)]
enum SkillError {
    Invalid(&'static str),
    Excluded(String),
    Io,
    Scope(SafeFsError),
}

impl SkillError {
    fn reason(&self) -> String {
        match self {
            Self::Invalid(reason) => (*reason).into(),
            Self::Excluded(reason) => text::excluded(reason),
            Self::Io => text::UNREADABLE.into(),
            Self::Scope(error) => match error {
                SafeFsError::OutsideScope
                | SafeFsError::IntermediateSymlink(_)
                | SafeFsError::FinalSymlink => text::OUTSIDE_SCOPE.into(),
                SafeFsError::InvalidPath
                | SafeFsError::MultipleHardLinks
                | SafeFsError::NotRegularFile
                | SafeFsError::AlreadyExists(_)
                | SafeFsError::NotFound(_)
                | SafeFsError::Io(_) => text::UNREADABLE.into(),
            },
        }
    }
}
impl From<std::io::Error> for SkillError {
    fn from(_error: std::io::Error) -> Self {
        Self::Io
    }
}
impl From<SafeFsError> for SkillError {
    fn from(value: SafeFsError) -> Self {
        Self::Scope(value)
    }
}

/// 両方のモードで探索し、採用したディレクトリの記述子を道具へ移す。
/// home は設定で指定されたパスを受け取り、この関数で実のパスを確かめる。
pub fn collect(
    project: &Path,
    home: &Path,
    config: &UserSkillsConfig,
    exclude: &ExcludeRules,
) -> (UserSkills, Vec<ScopeRoot>) {
    let mut collector = Collector {
        project,
        home: std::fs::canonicalize(home).ok(),
        exclude,
        identities: HashSet::new(),
        seen_names: HashSet::new(),
        cutoffs: BTreeMap::new(),
        entries: BTreeMap::new(),
        skipped: Vec::new(),
    };
    if config.user {
        if let Some(home) = collector.home.clone() {
            collector.search(&home, &home.join(".agents/skills"), SkillSource::User);
        } else {
            collector.skip(home.join(".agents/skills"), text::HOME_UNAVAILABLE);
        }
    }
    if config.project {
        let scope =
            super::instructions::search_root(project, collector.home.as_deref().unwrap_or(home));
        let mut directories = project
            .ancestors()
            .take_while(|path| path.starts_with(scope))
            .collect::<Vec<_>>();
        directories.reverse();
        for directory in directories {
            collector.search(
                scope,
                &directory.join(".agents/skills"),
                SkillSource::Project,
            );
        }
    }
    let mut loaded = Vec::new();
    let mut roots = Vec::new();
    // 枠の配分は利用者を先に数えるが、索引の表示順は名前順を保つ（設計書 01-04）。
    let mut entries = collector.entries.into_values().collect::<Vec<_>>();
    entries.sort_by(|left, right| left.0.name.cmp(&right.0.name));
    for (skill, root, _candidate) in entries {
        loaded.push(skill);
        roots.push(root);
    }
    (
        UserSkills {
            loaded,
            skipped: collector.skipped,
        },
        roots,
    )
}

struct Collector<'a> {
    project: &'a Path,
    home: Option<PathBuf>,
    exclude: &'a ExcludeRules,
    identities: HashSet<(u64, u64)>,
    seen_names: HashSet<String>,
    cutoffs: BTreeMap<bool, (String, &'static str)>,
    // false（利用者）を先に、各側では名前順に数える。保持する記述子は最大 100 個。
    entries: BTreeMap<(bool, String), (LoadedSkill, ScopeRoot, PathBuf)>,
    skipped: Vec<SkippedInstruction>,
}

impl Collector<'_> {
    fn skip(&mut self, path: PathBuf, reason: impl Into<String>) {
        self.skipped.push(SkippedInstruction {
            // Linux では探索パス自体に不正な UTF-8 がありうる。除外の通知で
            // 記録全体を直列化できなくしない（設計書 01-04「利用者の Agent Skill」）。
            path: PathBuf::from(path.to_string_lossy().into_owned()),
            reason: reason.into(),
        });
    }

    fn consider(&mut self, skill: LoadedSkill, root: ScopeRoot, candidate: PathBuf) {
        if !self.seen_names.insert(skill.name.clone()) {
            self.skip(candidate, text::DUPLICATE_NAME);
            return;
        }
        let key = (skill.source == SkillSource::Project, skill.name.clone());
        if let Some((name, reason)) = self.cutoffs.get(&key.0)
            && skill.name >= *name
        {
            self.skip(candidate, *reason);
            return;
        }
        if self.entries.len() == SKILL_LIMIT {
            if self
                .entries
                .last_key_value()
                .is_some_and(|(last, _)| key > *last)
            {
                self.reject(skill, candidate, text::SKILL_LIMIT);
                return;
            }
            if let Some((_key, (skill, _root, path))) = self.entries.pop_last() {
                self.reject(skill, path, text::SKILL_LIMIT);
            }
        }
        self.entries.insert(key, (skill, root, candidate));
        let mut bytes = "<skills>\n</skills>".len();
        let cutoff = self.entries.iter().find_map(|(key, (skill, _, _))| {
            bytes = bytes.saturating_add(skill.render().len()).saturating_add(1);
            (bytes > INDEX_LIMIT).then(|| key.clone())
        });
        if let Some(cutoff) = cutoff {
            // 後の階層で名前の若い候補が現れたら入れ替える。落とした名前は
            // 記述子なしで覚え、後の同名で優先順位を覆させない（設計書 01-04）。
            for (_key, (skill, _root, path)) in self.entries.split_off(&cutoff) {
                self.reject(skill, path, text::INDEX_LIMIT);
            }
        }
    }

    fn reject(&mut self, skill: LoadedSkill, candidate: PathBuf, reason: &'static str) {
        let cutoff = self
            .cutoffs
            .entry(skill.source == SkillSource::Project)
            .or_insert_with(|| (skill.name.clone(), reason));
        if skill.name < cutoff.0 {
            *cutoff = (skill.name, reason);
        }
        // 各側で一度上限を超えた名前より後ろは採らない。利用者の側で
        // 余ったバイト数はプロジェクトの側に配れるよう、境界を分けて持つ。
        self.skip(candidate, reason);
    }

    fn search(&mut self, scope: &Path, directory: &Path, source: SkillSource) {
        let result = (|| -> Result<_, SkillError> {
            let root = ScopeRoot::open(scope)?;
            let relative = ScopeRelPath::new(
                directory
                    .strip_prefix(scope)
                    .map_err(|_| SkillError::Invalid(text::OUTSIDE_SCOPE))?,
            )?;
            if matches!(root.metadata(&relative), Err(SafeFsError::NotFound(_))) {
                return Ok(None);
            }
            let listing = root.open_dir(&relative)?;
            self.check_scope(&listing, scope)?;
            let mut names = BTreeSet::new();
            let mut count = 0usize;
            listing.visit_names(|name| {
                count = count.saturating_add(1);
                names.insert(name);
                if names.len() > ENTRY_LIMIT {
                    names.pop_last();
                }
                true
            })?;
            Ok(Some((root, listing, names, count)))
        })();
        let (root, listing, names, count) = match result {
            Ok(Some(value)) => value,
            Ok(None) => return,
            Err(error) => {
                self.skip(directory.into(), error.reason());
                return;
            }
        };
        if count > ENTRY_LIMIT {
            self.skip(
                directory.into(),
                text::entry_limit(count.saturating_sub(ENTRY_LIMIT)),
            );
        }
        for name in names {
            if name.to_str().is_none() {
                continue;
            }
            let candidate = directory.join(&name);
            let metadata = ScopeRelPath::new(Path::new(&name))
                .and_then(|relative| listing.metadata(&relative));
            match metadata {
                Ok(metadata) if !metadata.is_dir() && !metadata.file_type().is_symlink() => {
                    continue;
                }
                Err(_) => {
                    self.skip(candidate, text::UNREADABLE);
                    continue;
                }
                Ok(_) => {}
            }
            let result = self.read(&root, scope, &candidate, source);
            match result {
                Ok(Some((skill, directory))) => {
                    self.consider(skill, directory, candidate.join("SKILL.md"));
                }
                Ok(None) => {}
                Err(error) => self.skip(candidate.join("SKILL.md"), error.reason()),
            }
        }
    }

    fn check_scope(&self, root: &ScopeRoot, scope: &Path) -> Result<(), SkillError> {
        if !root.real_path().starts_with(scope) {
            return Err(SkillError::Invalid(text::OUTSIDE_SCOPE));
        }
        Ok(())
    }

    fn read(
        &mut self,
        root: &ScopeRoot,
        scope: &Path,
        candidate: &Path,
        source: SkillSource,
    ) -> Result<Option<(LoadedSkill, ScopeRoot)>, SkillError> {
        let relative = ScopeRelPath::new(
            candidate
                .strip_prefix(scope)
                .map_err(|_| SkillError::Invalid(text::OUTSIDE_SCOPE))?,
        )?;
        let directory = match root.open_dir(&relative) {
            Err(SafeFsError::Io(error)) if error.kind() == std::io::ErrorKind::NotADirectory => {
                return Ok(None);
            }
            other => other?,
        };
        self.check_scope(&directory, scope)?;
        let real = directory.real_path();
        if real.to_str().is_none() {
            return Err(SkillError::Invalid(text::PATH_NOT_UTF8));
        }
        if [Some(self.project), Some(scope), self.home.as_deref()]
            .into_iter()
            .flatten()
            .any(|path| path.starts_with(real))
        {
            return Err(SkillError::Invalid(text::BROAD_DIRECTORY));
        }
        let skill_path = ScopeRelPath::new("SKILL.md")?;
        if matches!(
            directory.metadata(&skill_path),
            Err(SafeFsError::NotFound(_))
        ) {
            return Ok(None);
        }
        if candidate.file_name() != real.file_name() {
            return Err(SkillError::Invalid(text::DIRECTORY_NAME));
        }
        let file = directory.open_read(&skill_path)?;
        let path = safe_fs::resolved_path(&file)?;
        if path.to_str().is_none() {
            return Err(SkillError::Invalid(text::PATH_NOT_UTF8));
        }
        if !path.starts_with(real) {
            return Err(SkillError::Invalid(text::OUTSIDE_SCOPE));
        }
        if let Some(reason) = self.exclude.reason(&path) {
            return Err(SkillError::Excluded(reason));
        }
        let contents = self.contents(&file)?;
        let (name, description) = attributes(&contents)?;
        if name == "benitoite" {
            return Err(SkillError::Invalid(text::RESERVED_NAME));
        }
        if candidate.file_name().and_then(|part| part.to_str()) != Some(name)
            || real.file_name().and_then(|part| part.to_str()) != Some(name)
        {
            return Err(SkillError::Invalid(text::DIRECTORY_NAME));
        }
        let sha256 = Sha256::digest(contents.as_bytes())
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        let skill = LoadedSkill {
            name: name.into(),
            description: description.into(),
            skill_md: path,
            directory: real.into(),
            source,
            sha256,
        };
        Ok(Some((skill, directory)))
    }

    fn contents(&mut self, file: &File) -> Result<String, SkillError> {
        let metadata = file.metadata()?;
        if !metadata.is_file() {
            return Err(SkillError::Invalid(text::NOT_REGULAR));
        }
        if metadata.nlink() != 1 {
            return Err(SkillError::Invalid(text::HARD_LINKS));
        }
        if !self.identities.insert((metadata.dev(), metadata.ino())) {
            return Err(SkillError::Invalid(text::DUPLICATE_FILE));
        }
        if metadata.len() > 262_144 {
            return Err(SkillError::Invalid(text::FILE_LIMIT));
        }
        let mut bytes = Vec::new();
        file.take(262_145).read_to_end(&mut bytes)?;
        if bytes.len() > FILE_LIMIT {
            return Err(SkillError::Invalid(text::FILE_LIMIT));
        }
        String::from_utf8(bytes).map_err(|_| SkillError::Invalid(text::NOT_UTF8))
    }
}

fn attributes(contents: &str) -> Result<(&str, &str), SkillError> {
    let parts = frontmatter(contents).ok_or(SkillError::Invalid(text::FRONTMATTER))?;
    if contents.contains("\r\n") {
        return Err(SkillError::Invalid(text::FRONTMATTER));
    }
    let attributes = contents
        .strip_prefix("---\n")
        .and_then(|value| value.split_once("\n---\n"))
        .map(|(attributes, _)| attributes)
        .ok_or(SkillError::Invalid(text::FRONTMATTER))?;
    let mut lines = attributes.lines().peekable();
    let mut seen = HashSet::new();
    while let Some(line) = lines.next() {
        if let Some(key) = ["name:", "description:"]
            .into_iter()
            .find(|key| line.starts_with(key))
            && seen.insert(key)
            && lines
                .peek()
                .is_some_and(|next| next.starts_with(char::is_whitespace))
        {
            return Err(SkillError::Invalid(text::MULTILINE));
        }
    }
    let name = single_line(parts.name)?;
    let description = single_line(parts.description)?;
    if name.is_empty()
        || name.len() > 64
        || !name
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
    {
        return Err(SkillError::Invalid(text::NAME));
    }
    if description.is_empty()
        || description.chars().count() > 1024
        || description.contains(['\r', '\n'])
    {
        return Err(SkillError::Invalid(text::DESCRIPTION));
    }
    Ok((name, description))
}

fn single_line(value: &str) -> Result<&str, SkillError> {
    if value.starts_with(['>', '|']) {
        return Err(SkillError::Invalid(text::MULTILINE));
    }
    if let Some(quote) = value.chars().next().filter(|ch| matches!(ch, '\'' | '"')) {
        return value
            .strip_prefix(quote)
            .and_then(|value| value.strip_suffix(quote))
            .ok_or(SkillError::Invalid(text::MULTILINE));
    }
    Ok(value)
}
