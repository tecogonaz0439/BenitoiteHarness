//! 利用者とプロジェクトの Skill とプラグインを読み、索引と読める根を固定する（設計書 01-04）。

mod text;

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::fs::File;
use std::io::Read;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use super::plugins;
use crate::benitoite::skill::frontmatter;
use crate::config::{UserPluginsConfig, UserSkillsConfig};
use crate::safe_fs::{self, SafeFsError, ScopeRelPath, ScopeRoot};
use crate::session::{RecordedPlugin, RecordedSkill, SkillSource, SkippedInstruction};
use crate::tools::exclude::ExcludeRules;

const FILE_LIMIT: usize = 256 * 1024;
const ENTRY_LIMIT: usize = 1_000;
const SKILL_LIMIT: usize = 100;
const INDEX_LIMIT: usize = 16 * 1024;
const PLUGIN_LIMIT: usize = 100;
// 段ごとに、すべてのプラグインを合わせて開く Skill の候補の数（設計書 01-04 の「長さと数の上限」）。
const PLUGIN_CANDIDATE_LIMIT: usize = 1_000;
// 段ごとに一件ずつ示して記録する、飛ばした Skill と断ったプラグインの数。
const SKIPPED_REPORT_LIMIT: usize = 100;
// 開かずに飛ばした Skill をこの数まで一件ずつ示し、超えたら一行にまとめる。
const SKIPPED_DETAIL_LIMIT: usize = 20;

/// 全文を保持せず、索引と読み取り範囲の識別情報だけを保つ。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LoadedSkill {
    pub name: String,
    pub description: String,
    pub skill_md: PathBuf,
    pub directory: PathBuf,
    pub source: SkillSource,
    pub sha256: String,
    pub plugin: Option<String>,
}

impl LoadedSkill {
    /// セッションのヘッダと開始の通知に共通の識別情報。
    pub fn recorded(&self) -> RecordedSkill {
        RecordedSkill {
            name: self.name.clone(),
            path: self.skill_md.clone(),
            sha256: self.sha256.clone(),
            source: self.source,
            plugin: self.plugin.clone(),
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
    pub plugins: Vec<RecordedPlugin>,
    pub skipped_plugins: Vec<SkippedInstruction>,
    pub plugin_notices: Vec<SkippedInstruction>,
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

/// 読めなかった理由。ファイルと範囲の誤りは、Skill とプラグインのどちらの
/// 文で示すかを呼ぶ側が選ぶ（設計書 01-04「Agent Plugins のプラグイン」）。
#[derive(Debug)]
enum SkillError {
    Invalid(&'static str),
    Message(String),
    Excluded(String),
    Io,
    Scope(SafeFsError),
    OutsideScope,
    Broad,
    PathNotUtf8,
    NotRegular,
    HardLinks,
    DuplicateFile,
    FileLimit,
    NotUtf8,
}

/// 理由の文の組。置換で作らず、組ごとに定数を選ぶ。
struct Reasons {
    unreadable: &'static str,
    outside_scope: &'static str,
    broad: &'static str,
    path_not_utf8: &'static str,
    not_regular: &'static str,
    hard_links: &'static str,
    duplicate_file: &'static str,
    file_limit: &'static str,
    not_utf8: &'static str,
    excluded: fn(&str) -> String,
}

const SKILL_REASONS: Reasons = Reasons {
    unreadable: text::UNREADABLE,
    outside_scope: text::OUTSIDE_SCOPE,
    broad: text::BROAD_DIRECTORY,
    path_not_utf8: text::PATH_NOT_UTF8,
    not_regular: text::NOT_REGULAR,
    hard_links: text::HARD_LINKS,
    duplicate_file: text::DUPLICATE_FILE,
    file_limit: text::FILE_LIMIT,
    not_utf8: text::NOT_UTF8,
    excluded: text::excluded,
};

const PLUGIN_REASONS: Reasons = Reasons {
    unreadable: plugins::text::UNREADABLE,
    outside_scope: plugins::text::OUTSIDE_SCOPE,
    broad: plugins::text::BROAD_DIRECTORY,
    path_not_utf8: plugins::text::PATH_NOT_UTF8,
    not_regular: plugins::text::NOT_REGULAR,
    hard_links: plugins::text::HARD_LINKS,
    duplicate_file: plugins::text::DUPLICATE_FILE,
    file_limit: plugins::text::FILE_LIMIT,
    not_utf8: plugins::text::NOT_UTF8,
    excluded: plugins::text::excluded,
};

impl SkillError {
    fn reason(&self) -> String {
        self.reason_with(&SKILL_REASONS)
    }

    fn plugin_reason(&self) -> String {
        self.reason_with(&PLUGIN_REASONS)
    }

    fn reason_with(&self, reasons: &Reasons) -> String {
        match self {
            Self::Invalid(reason) => (*reason).into(),
            Self::Message(reason) => reason.clone(),
            Self::Excluded(reason) => (reasons.excluded)(reason),
            Self::Io => reasons.unreadable.into(),
            Self::OutsideScope => reasons.outside_scope.into(),
            Self::Broad => reasons.broad.into(),
            Self::PathNotUtf8 => reasons.path_not_utf8.into(),
            Self::NotRegular => reasons.not_regular.into(),
            Self::HardLinks => reasons.hard_links.into(),
            Self::DuplicateFile => reasons.duplicate_file.into(),
            Self::FileLimit => reasons.file_limit.into(),
            Self::NotUtf8 => reasons.not_utf8.into(),
            Self::Scope(error) => match error {
                SafeFsError::OutsideScope
                | SafeFsError::IntermediateSymlink(_)
                | SafeFsError::FinalSymlink => reasons.outside_scope.into(),
                SafeFsError::InvalidPath
                | SafeFsError::MultipleHardLinks
                | SafeFsError::NotRegularFile
                | SafeFsError::AlreadyExists(_)
                | SafeFsError::NotFound(_)
                | SafeFsError::Io(_) => reasons.unreadable.into(),
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
    plugins_config: &UserPluginsConfig,
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
        plugins: Vec::new(),
        skipped_plugins: Vec::new(),
        plugin_notices: Vec::new(),
        seen_plugins: HashSet::new(),
        plugin_counts: BTreeMap::new(),
        plugin_candidates: BTreeMap::new(),
        reported: BTreeMap::new(),
    };
    if config.user {
        if let Some(home) = collector.home.clone() {
            collector.search(&home, &home.join(".agents/skills"), SkillSource::User);
        } else {
            collector.skip(
                SkillSource::User,
                home.join(".agents/skills"),
                text::HOME_UNAVAILABLE,
            );
        }
    }
    if plugins_config.user {
        if let Some(home) = collector.home.clone() {
            collector.search_plugins(&home, &home.join(".agents/plugins"), SkillSource::User);
        } else {
            collector.skip_plugin(
                SkillSource::User,
                home.join(".agents/plugins"),
                plugins::text::HOME_UNAVAILABLE,
            );
        }
    }
    let scope =
        super::instructions::search_root(project, collector.home.as_deref().unwrap_or(home));
    let mut directories = project
        .ancestors()
        .take_while(|path| path.starts_with(scope))
        .collect::<Vec<_>>();
    directories.reverse();
    if config.project {
        for directory in &directories {
            collector.search(
                scope,
                &directory.join(".agents/skills"),
                SkillSource::Project,
            );
        }
    }
    if plugins_config.project {
        for directory in directories {
            collector.search_plugins(
                scope,
                &directory.join(".agents/plugins"),
                SkillSource::Project,
            );
        }
    }
    collector.report_plugin_limits();
    collector.report_hidden();
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
            plugins: collector.plugins,
            skipped_plugins: collector.skipped_plugins,
            plugin_notices: collector.plugin_notices,
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
    cutoffs: BTreeMap<u8, ((String, String), &'static str)>,
    // 四段階の優先順位で枠を配る。保持する Skill の記述子は最大 100 個。
    entries: BTreeMap<(u8, String, String), (LoadedSkill, ScopeRoot, PathBuf)>,
    skipped: Vec<SkippedInstruction>,
    plugins: Vec<RecordedPlugin>,
    skipped_plugins: Vec<SkippedInstruction>,
    plugin_notices: Vec<SkippedInstruction>,
    seen_plugins: HashSet<String>,
    // 出所ごとの、plugin.json を読んだ数と、上限を越えて開かなかった項目の数と最初の場所。
    plugin_counts: BTreeMap<u8, (usize, usize, Option<PathBuf>)>,
    // 出所ごとの、開いたプラグインの Skill の候補の数と、開かなかった数と最初の場所。
    plugin_candidates: BTreeMap<u8, (usize, usize, Option<PathBuf>)>,
    // 出所ごとの、一件ずつ示した数と、示さず数えた数と最初の場所。
    reported: BTreeMap<u8, (usize, usize, Option<PathBuf>)>,
}

impl Collector<'_> {
    fn skip(&mut self, source: SkillSource, path: PathBuf, reason: impl Into<String>) {
        if self.report(source, &path) {
            self.skipped.push(skipped(path, reason));
        }
    }

    fn skip_plugin(&mut self, source: SkillSource, path: PathBuf, reason: impl Into<String>) {
        if self.report(source, &path) {
            self.skipped_plugins.push(skipped(path, reason));
        }
    }

    /// 信頼しないリポジトリが多数の誤りで画面と記録を膨らませないよう、段ごとに
    /// 100 件まで一件ずつ示し、残りは数える（設計書 01-04 の「長さと数の上限」）。
    fn report(&mut self, source: SkillSource, path: &Path) -> bool {
        let counts = self.reported.entry(stage(source, false)).or_default();
        if counts.0 < SKIPPED_REPORT_LIMIT {
            counts.0 = counts.0.saturating_add(1);
            return true;
        }
        counts.1 = counts.1.saturating_add(1);
        counts.2.get_or_insert_with(|| path.to_path_buf());
        false
    }

    fn report_hidden(&mut self) {
        let reported = std::mem::take(&mut self.reported);
        for (_source, (_shown, hidden, path)) in reported {
            if let Some(path) = path.filter(|_| hidden > 0) {
                self.skipped
                    .push(skipped(path, text::skipped_hidden(hidden)));
            }
        }
    }

    fn consider(&mut self, skill: LoadedSkill, root: ScopeRoot, candidate: PathBuf) {
        if !self.seen_names.insert(skill.name.clone()) {
            self.skip(skill.source, candidate, text::DUPLICATE_NAME);
            return;
        }
        let key = skill_key(&skill);
        if let Some((name, reason)) = self.cutoffs.get(&key.0)
            && (key.1.clone(), key.2.clone()) >= *name
        {
            let reason = *reason;
            self.skip(skill.source, candidate, reason);
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

    /// 項目の名前で鍵が分かるので、上限を越える候補を開かずに判定する
    /// （設計書 01-04「Agent Plugins のプラグイン」の「長さと数の上限」）。
    fn precut(&mut self, key: &(u8, String, String)) -> Option<&'static str> {
        if let Some((name, reason)) = self.cutoffs.get(&key.0)
            && (key.1.clone(), key.2.clone()) >= *name
        {
            return Some(reason);
        }
        if self.entries.len() == SKILL_LIMIT
            && self
                .entries
                .last_key_value()
                .is_some_and(|(last, _)| key > last)
        {
            let order = (key.1.clone(), key.2.clone());
            let cutoff = self
                .cutoffs
                .entry(key.0)
                .or_insert_with(|| (order.clone(), text::SKILL_LIMIT));
            if order < cutoff.0 {
                *cutoff = (order, text::SKILL_LIMIT);
            }
            return Some(text::SKILL_LIMIT);
        }
        None
    }

    fn report_plugin_limits(&mut self) {
        let counts = std::mem::take(&mut self.plugin_counts);
        // 数をまとめた行は段ごとに一行なので、100 件の枠に入れず必ず示す。
        for (_source, (_read, over, path)) in counts {
            if let Some(path) = path.filter(|_| over > 0) {
                self.skipped_plugins
                    .push(skipped(path, plugins::text::plugin_limit(over)));
            }
        }
        let candidates = std::mem::take(&mut self.plugin_candidates);
        for (_source, (_opened, over, path)) in candidates {
            if let Some(path) = path.filter(|_| over > 0) {
                self.skipped
                    .push(skipped(path, text::plugin_candidate_limit(over)));
            }
        }
    }

    fn reject(&mut self, skill: LoadedSkill, candidate: PathBuf, reason: &'static str) {
        let (stage, plugin, name) = skill_key(&skill);
        let order = (plugin, name);
        let cutoff = self
            .cutoffs
            .entry(stage)
            .or_insert_with(|| (order.clone(), reason));
        if order < cutoff.0 {
            *cutoff = (order, reason);
        }
        // 各段で一度上限を超えた名前より後ろは採らない。余ったバイト数は
        // 次の段で使えるよう、打ち切りの境界を段ごとに持つ（設計書 01-04）。
        self.skip(skill.source, candidate, reason);
    }

    fn search(&mut self, scope: &Path, directory: &Path, source: SkillSource) {
        match ScopeRoot::open(scope) {
            Ok(root) => self.search_skills(&root, scope, directory, source, None),
            Err(error) => self.skip(source, directory.into(), SkillError::from(error).reason()),
        }
    }

    fn listing(
        &self,
        root: &ScopeRoot,
        scope: &Path,
        directory: &Path,
    ) -> Result<Option<(ScopeRoot, BTreeSet<std::ffi::OsString>, usize)>, SkillError> {
        let relative = ScopeRelPath::new(
            directory
                .strip_prefix(root.real_path())
                .map_err(|_| SkillError::OutsideScope)?,
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
        Ok(Some((listing, names, count)))
    }

    fn search_skills(
        &mut self,
        root: &ScopeRoot,
        scope: &Path,
        directory: &Path,
        source: SkillSource,
        plugin: Option<&str>,
    ) {
        let (listing, names, count) = match self.listing(root, scope, directory) {
            Ok(Some(value)) => value,
            Ok(None) => return,
            Err(error) => {
                self.skip(source, directory.into(), error.reason());
                return;
            }
        };
        if count > ENTRY_LIMIT {
            self.skip(
                source,
                directory.into(),
                text::entry_limit(count.saturating_sub(ENTRY_LIMIT)),
            );
        }
        let mut precut = Vec::new();
        for name in names {
            let Some(entry) = name.to_str() else {
                continue;
            };
            let candidate = directory.join(&name);
            if plugin.is_some() {
                // 規則に合わない候補と同名で負けた候補も数え、多数の候補で起動を
                // 遅くさせない（設計書 01-04 の「長さと数の上限」）。
                let counts = self
                    .plugin_candidates
                    .entry(stage(source, false))
                    .or_default();
                if counts.0 >= PLUGIN_CANDIDATE_LIMIT {
                    counts.1 = counts.1.saturating_add(1);
                    counts.2.get_or_insert_with(|| directory.to_path_buf());
                    continue;
                }
                counts.0 = counts.0.saturating_add(1);
            }
            let key = (
                stage(source, plugin.is_some()),
                plugin.unwrap_or_default().to_owned(),
                entry.to_owned(),
            );
            let metadata = ScopeRelPath::new(Path::new(&name))
                .and_then(|relative| listing.metadata(&relative));
            match metadata {
                Ok(metadata) if !metadata.is_dir() && !metadata.file_type().is_symlink() => {
                    continue;
                }
                Err(_) => {
                    self.skip(source, candidate, text::UNREADABLE);
                    continue;
                }
                Ok(_) => {}
            }
            if let Some(reason) = self.precut(&key) {
                // 開かなくても名前は項目の名前と一致するはずなので、後の段の同名に
                // 優先順位を覆させないよう覚える（設計書 01-04）。
                self.seen_names.insert(entry.to_owned());
                precut.push((candidate.join("SKILL.md"), reason));
                continue;
            }
            match self.read(root, scope, &candidate, source, plugin) {
                Ok(Some((skill, directory))) => {
                    self.consider(skill, directory, candidate.join("SKILL.md"))
                }
                Ok(None) => {}
                Err(error) => self.skip(source, candidate.join("SKILL.md"), error.reason()),
            }
        }
        if precut.len() > SKIPPED_DETAIL_LIMIT {
            self.skip(
                source,
                directory.into(),
                text::skipped_beyond_limits(precut.len()),
            );
        } else {
            for (path, reason) in precut {
                self.skip(source, path, reason);
            }
        }
    }

    fn search_plugins(&mut self, scope: &Path, directory: &Path, source: SkillSource) {
        let result = (|| -> Result<_, SkillError> {
            let root = ScopeRoot::open(scope)?;
            Ok(self
                .listing(&root, scope, directory)?
                .map(|(listing, names, count)| (root, listing, names, count)))
        })();
        let (scope_root, listing, names, count) = match result {
            Ok(Some(value)) => value,
            Ok(None) => return,
            Err(error) => {
                self.skip_plugin(source, directory.into(), error.plugin_reason());
                return;
            }
        };
        if count > ENTRY_LIMIT {
            self.skip_plugin(
                source,
                directory.into(),
                text::entry_limit(count.saturating_sub(ENTRY_LIMIT)),
            );
        }
        for name in names {
            if name.to_str().is_none() {
                continue;
            }
            let candidate = directory.join(&name);
            let source_key = stage(source, false);
            let metadata = ScopeRelPath::new(Path::new(&name))
                .and_then(|relative| listing.metadata(&relative));
            if let Ok(metadata) = &metadata
                && !metadata.is_dir()
                && !metadata.file_type().is_symlink()
            {
                continue;
            }
            // 信頼しないリポジトリの多数のプラグインで記録が膨らまないよう、
            // 出所ごとに 100 個を超えた項目は開かず数だけを示す（設計書 01-04 の「長さと数の上限」）。
            let counts = self.plugin_counts.entry(source_key).or_default();
            if counts.0 >= PLUGIN_LIMIT {
                counts.1 = counts.1.saturating_add(1);
                counts.2.get_or_insert_with(|| directory.to_path_buf());
                continue;
            }
            // plugin.json を読む前に断った項目も数に入れる。plugin.json のない
            // ディレクトリだけは、プラグインでないので数えない（設計書 01-04 の「長さと数の上限」）。
            let mut counted = false;
            let result = (|| -> Result<(), SkillError> {
                metadata?;
                // Linux の BENEATH は開いた根を境界にするため、一覧の根ではなく
                // 探索範囲の記述子から開く。範囲内の別ディレクトリへのリンクを許す。
                let relative = ScopeRelPath::new(
                    candidate
                        .strip_prefix(scope_root.real_path())
                        .map_err(|_| SkillError::OutsideScope)?,
                )?;
                let root = match scope_root.open_dir(&relative) {
                    Err(SafeFsError::Io(error))
                        if error.kind() == std::io::ErrorKind::NotADirectory =>
                    {
                        return Ok(());
                    }
                    other => other?,
                };
                self.check_directory(&root, scope)?;
                let manifest_path = ScopeRelPath::new("plugin.json")?;
                if matches!(root.metadata(&manifest_path), Err(SafeFsError::NotFound(_))) {
                    return Ok(());
                }
                counted = true;
                let counts = self.plugin_counts.entry(source_key).or_default();
                counts.0 = counts.0.saturating_add(1);
                let (_, contents) = self.file_contents(&root, "plugin.json")?;
                let manifest = plugins::parse(
                    contents.as_bytes(),
                    name.to_str(),
                    root.real_path().file_name().and_then(|name| name.to_str()),
                )
                .map_err(|error| SkillError::Message(error.reason()))?;
                if !self.seen_plugins.insert(manifest.name.clone()) {
                    return Err(SkillError::Invalid(plugins::text::DUPLICATE_NAME));
                }
                for reason in manifest.notices {
                    self.plugin_notices
                        .push(skipped(root.real_path().into(), reason));
                }
                // MCP の内容や拡張のディレクトリは開かない（設計書 01-04、BH-116）。
                match root.metadata(&ScopeRelPath::new("mcp.json")?) {
                    Err(SafeFsError::NotFound(_)) => {}
                    _ => self
                        .plugin_notices
                        .push(skipped(root.real_path().into(), plugins::text::MCP)),
                }
                self.search_skills(
                    &root,
                    root.real_path(),
                    &root.real_path().join("skills"),
                    source,
                    Some(&manifest.name),
                );
                self.plugins.push(RecordedPlugin {
                    name: manifest.name,
                    version: manifest.version,
                    path: root.real_path().into(),
                    sha256: digest(contents.as_bytes()),
                    source,
                });
                Ok(())
            })();
            if let Err(error) = result {
                if !counted {
                    let counts = self.plugin_counts.entry(source_key).or_default();
                    counts.0 = counts.0.saturating_add(1);
                }
                self.skip_plugin(source, candidate, error.plugin_reason());
            }
        }
    }

    fn check_directory(&self, root: &ScopeRoot, scope: &Path) -> Result<(), SkillError> {
        self.check_scope(root, scope)?;
        let real = root.real_path();
        if real.to_str().is_none() {
            return Err(SkillError::PathNotUtf8);
        }
        if [Some(self.project), Some(scope), self.home.as_deref()]
            .into_iter()
            .flatten()
            .any(|path| path.starts_with(real))
        {
            return Err(SkillError::Broad);
        }
        Ok(())
    }

    fn check_scope(&self, root: &ScopeRoot, scope: &Path) -> Result<(), SkillError> {
        if !root.real_path().starts_with(scope) {
            return Err(SkillError::OutsideScope);
        }
        Ok(())
    }

    fn read(
        &mut self,
        root: &ScopeRoot,
        scope: &Path,
        candidate: &Path,
        source: SkillSource,
        plugin: Option<&str>,
    ) -> Result<Option<(LoadedSkill, ScopeRoot)>, SkillError> {
        let relative = ScopeRelPath::new(
            candidate
                .strip_prefix(root.real_path())
                .map_err(|_| SkillError::OutsideScope)?,
        )?;
        let directory = match root.open_dir(&relative) {
            Err(SafeFsError::Io(error)) if error.kind() == std::io::ErrorKind::NotADirectory => {
                return Ok(None);
            }
            other => other?,
        };
        self.check_directory(&directory, scope)?;
        let real = directory.real_path();
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
        let (path, contents) = self.file_contents(&directory, "SKILL.md")?;
        let (name, description) = attributes(&contents)?;
        if name == "benitoite" {
            return Err(SkillError::Invalid(text::RESERVED_NAME));
        }
        if candidate.file_name().and_then(|part| part.to_str()) != Some(name)
            || real.file_name().and_then(|part| part.to_str()) != Some(name)
        {
            return Err(SkillError::Invalid(text::DIRECTORY_NAME));
        }
        let sha256 = digest(contents.as_bytes());
        let skill = LoadedSkill {
            name: name.into(),
            description: description.into(),
            skill_md: path,
            directory: real.into(),
            source,
            sha256,
            plugin: plugin.map(str::to_owned),
        };
        Ok(Some((skill, directory)))
    }

    fn file_contents(
        &mut self,
        directory: &ScopeRoot,
        filename: &str,
    ) -> Result<(PathBuf, String), SkillError> {
        let file = directory.open_read(&ScopeRelPath::new(filename)?)?;
        let path = safe_fs::resolved_path(&file)?;
        if path.to_str().is_none() {
            return Err(SkillError::PathNotUtf8);
        }
        if !path.starts_with(directory.real_path()) {
            return Err(SkillError::OutsideScope);
        }
        if let Some(reason) = self.exclude.reason(&path) {
            return Err(SkillError::Excluded(reason));
        }
        Ok((path, self.contents(&file)?))
    }

    fn contents(&mut self, file: &File) -> Result<String, SkillError> {
        let metadata = file.metadata()?;
        if !metadata.is_file() {
            return Err(SkillError::NotRegular);
        }
        if metadata.nlink() != 1 {
            return Err(SkillError::HardLinks);
        }
        if !self.identities.insert((metadata.dev(), metadata.ino())) {
            return Err(SkillError::DuplicateFile);
        }
        if metadata.len() > 262_144 {
            return Err(SkillError::FileLimit);
        }
        let mut bytes = Vec::new();
        file.take(262_145).read_to_end(&mut bytes)?;
        if bytes.len() > FILE_LIMIT {
            return Err(SkillError::FileLimit);
        }
        String::from_utf8(bytes).map_err(|_| SkillError::NotUtf8)
    }
}

fn stage(source: SkillSource, plugin: bool) -> u8 {
    match (source, plugin) {
        (SkillSource::User, false) => 0,
        (SkillSource::User, true) => 1,
        (SkillSource::Project, false) => 2,
        (SkillSource::Project, true) => 3,
    }
}

fn skill_key(skill: &LoadedSkill) -> (u8, String, String) {
    (
        stage(skill.source, skill.plugin.is_some()),
        skill.plugin.clone().unwrap_or_default(),
        skill.name.clone(),
    )
}

fn skipped(path: PathBuf, reason: impl Into<String>) -> SkippedInstruction {
    // 不正な UTF-8 の探索パスでもセッションの記録を直列化できる形にする。
    SkippedInstruction {
        path: PathBuf::from(path.to_string_lossy().into_owned()),
        reason: reason.into(),
    }
}

fn digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
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
