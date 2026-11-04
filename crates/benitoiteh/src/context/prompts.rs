//! 利用者の定義するコマンド（プロンプトの雛形）を読む（設計書 05-02「利用者の定義するコマンド」）。
//!
//! 利用者の雛形は設定のディレクトリの `prompts/` に、プロジェクトの雛形は探す範囲の
//! `.agents/prompts/` に置く。プロジェクトの雛形は、信頼の確認の後にだけ読む
//! （設計書 03-01「プロジェクトの信頼」）。

use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::fs::File;
use std::io::Read;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

use crate::safe_fs::{EntryKind, ScopeRelPath, ScopeRoot};
use crate::tools::exclude::ExcludeRules;

/// 一つの雛形の大きさの上限。
const FILE_LIMIT: u64 = 64 * 1024;
/// 読む雛形の数の上限。画面の一覧と入力の補完を重くしない。
const COUNT_LIMIT: usize = 200;
/// 名前の長さの上限。
const NAME_LIMIT: usize = 64;

/// 組み込みのコマンドの名前。雛形はこの名前を使えない。
pub const RESERVED: &[&str] = &[
    "help", "status", "compact", "rewind", "tree", "new", "fork", "quit", "model", "export", "init",
];

/// 雛形を読んだ場所。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TemplateSource {
    User,
    Project,
}

/// 一つの雛形。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PromptTemplate {
    pub name: String,
    pub description: Option<String>,
    pub body: String,
    pub source: TemplateSource,
    pub path: PathBuf,
}

/// 読んだ雛形と、読まなかった理由の知らせ。
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PromptTemplates {
    templates: BTreeMap<String, PromptTemplate>,
    pub notices: Vec<String>,
}

impl PromptTemplates {
    /// 名前で雛形を引く。
    pub fn get(&self, name: &str) -> Option<&PromptTemplate> {
        self.templates.get(name)
    }

    /// 名前の順に並べた雛形。
    pub fn iter(&self) -> impl Iterator<Item = &PromptTemplate> {
        self.templates.values()
    }

    pub fn is_empty(&self) -> bool {
        self.templates.is_empty()
    }
}

/// コマンドの名前として使えるか。英小文字で始まり、英小文字・数字・`-` だけを含む。
pub fn valid_name(name: &str) -> bool {
    name.len() <= NAME_LIMIT
        && name
            .bytes()
            .next()
            .is_some_and(|byte| byte.is_ascii_lowercase())
        && name
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}

/// 探す範囲の中に `.agents/prompts` があるか。中身は開かない。
pub fn project_has_prompts(project: &Path, home: &Path) -> bool {
    project_directories(project, home)
        .iter()
        .any(|directory| std::fs::symlink_metadata(directory.join(".agents/prompts")).is_ok())
}

/// 利用者の雛形と、`include_project` のときはプロジェクトの雛形を読む。
///
/// 同じ名前は先に読んだもの（利用者、範囲の根に近いディレクトリの順）を使う。
/// 理由: プロジェクトの雛形で、利用者の決めた同じ名前のコマンドを置き換えさせない。
pub fn collect(
    user_directory: &Path,
    project: &Path,
    home: &Path,
    include_project: bool,
    exclude: &ExcludeRules,
) -> PromptTemplates {
    let mut result = PromptTemplates::default();
    read_directory(user_directory, TemplateSource::User, None, &mut result);
    if include_project {
        for directory in project_directories(project, home) {
            read_directory(
                &directory.join(".agents/prompts"),
                TemplateSource::Project,
                Some(exclude),
                &mut result,
            );
        }
    }
    result
}

/// 探す範囲の根からプロジェクトのディレクトリまで。ホームは利用者の段なので除く。
fn project_directories(project: &Path, home: &Path) -> Vec<PathBuf> {
    let home = std::fs::canonicalize(home).unwrap_or_else(|_| home.to_path_buf());
    let scope = super::instructions::search_root(project, &home);
    let mut directories = project
        .ancestors()
        .take_while(|path| path.starts_with(scope))
        .filter(|directory| *directory != home)
        .map(Path::to_path_buf)
        .collect::<Vec<_>>();
    directories.reverse();
    directories
}

fn read_directory(
    directory: &Path,
    source: TemplateSource,
    exclude: Option<&ExcludeRules>,
    result: &mut PromptTemplates,
) {
    // ないディレクトリは知らせない。置いていないのが普通である。
    if std::fs::symlink_metadata(directory).is_err() {
        return;
    }
    // 途中の要素も含めてリンクを辿らない。リンクで範囲の外のファイルを依頼として送らせない。
    let root = match ScopeRoot::open(directory).and_then(|root| root.no_follow()) {
        Ok(root) => root,
        Err(error) => {
            result
                .notices
                .push(super::text::prompt_skipped(directory, &error.to_string()));
            return;
        }
    };
    // プロジェクトの `.agents` か `prompts` をリンクにして、範囲の外を読ませない。
    // 利用者の設定のディレクトリは利用者が置くので、リンクでもよい。
    if source == TemplateSource::Project && root.real_path() != directory {
        result.notices.push(super::text::prompt_skipped(
            directory,
            super::text::PROMPT_LINKED_DIRECTORY,
        ));
        return;
    }
    let entries = match root.read_dir(None) {
        Ok(entries) => entries,
        Err(error) => {
            result
                .notices
                .push(super::text::prompt_skipped(directory, &error.to_string()));
            return;
        }
    };
    for entry in entries {
        let path = directory.join(&entry.name);
        let Some(name) = Path::new(&entry.name)
            .extension()
            .filter(|extension| *extension == OsStr::new("md"))
            .and_then(|_| Path::new(&entry.name).file_stem())
            .and_then(OsStr::to_str)
        else {
            continue;
        };
        let reason = if entry.kind != EntryKind::File {
            Some(super::text::PROMPT_NOT_REGULAR.to_owned())
        } else if !valid_name(name) {
            Some(super::text::PROMPT_INVALID_NAME.to_owned())
        } else if RESERVED.contains(&name) {
            Some(super::text::PROMPT_RESERVED.to_owned())
        } else if result.templates.contains_key(name) {
            Some(super::text::PROMPT_DUPLICATE.to_owned())
        } else if result.templates.len() >= COUNT_LIMIT {
            Some(super::text::PROMPT_COUNT_LIMIT.to_owned())
        } else {
            exclude.and_then(|rules| rules.reason(&path))
        };
        if let Some(reason) = reason {
            result
                .notices
                .push(super::text::prompt_skipped(&path, &reason));
            continue;
        }
        match read_file(&root, &entry.name) {
            Ok(contents) => {
                let (description, body) = split_front_matter(&contents);
                result.templates.insert(
                    name.to_owned(),
                    PromptTemplate {
                        name: name.to_owned(),
                        description,
                        body,
                        source,
                        path,
                    },
                );
            }
            Err(reason) => result
                .notices
                .push(super::text::prompt_skipped(&path, &reason)),
        }
    }
}

fn read_file(root: &ScopeRoot, name: &std::ffi::OsString) -> Result<String, String> {
    let relative = ScopeRelPath::new(name).map_err(|error| error.to_string())?;
    let file: File = root
        .open_read(&relative)
        .map_err(|error| error.to_string())?;
    let metadata = file.metadata().map_err(|error| error.to_string())?;
    if !metadata.is_file() {
        return Err(super::text::PROMPT_NOT_REGULAR.into());
    }
    // 固いリンクで、読ませない場所のファイルを別の名前で見せる道筋を塞ぐ。
    if metadata.nlink() != 1 {
        return Err(super::text::PROMPT_HARD_LINKS.into());
    }
    if metadata.len() > FILE_LIMIT {
        return Err(super::text::PROMPT_FILE_LIMIT.into());
    }
    let mut bytes = Vec::new();
    file.take(FILE_LIMIT.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > FILE_LIMIT {
        return Err(super::text::PROMPT_FILE_LIMIT.into());
    }
    String::from_utf8(bytes).map_err(|_| super::text::PROMPT_NOT_UTF8.into())
}

/// 先頭の `---` の間から `description:` だけを読み、残りを本文とする。
fn split_front_matter(contents: &str) -> (Option<String>, String) {
    let Some(rest) = contents
        .strip_prefix("---\n")
        .or_else(|| contents.strip_prefix("---\r\n"))
    else {
        return (None, contents.to_owned());
    };
    let mut description = None;
    let mut offset = 0_usize;
    for line in rest.split_inclusive('\n') {
        offset = offset.saturating_add(line.len());
        let trimmed = line.trim_end_matches(['\r', '\n']);
        if trimmed == "---" {
            let body = rest.get(offset..).unwrap_or_default();
            return (description, body.to_owned());
        }
        if let Some(value) = trimmed.strip_prefix("description:") {
            let value = value.trim().trim_matches('"').trim();
            if !value.is_empty() {
                description = Some(value.to_owned());
            }
        }
    }
    // 閉じていなければ、全体を本文とする。
    (None, contents.to_owned())
}

impl PromptTemplate {
    /// 引数を埋めた依頼の文。`$ARGUMENTS` は引数の全体、`$1`〜`$9` は空白で分けた各引数、
    /// `$$` は `$` に替える。埋める場所がない雛形に引数があれば、本文の後に段落を分けて加える。
    /// 理由: 書いた引数を黙って捨てない。
    pub fn expand(&self, arguments: &str) -> String {
        let arguments = arguments.trim();
        let words = arguments.split_whitespace().collect::<Vec<_>>();
        let mut output = String::new();
        let mut used = false;
        let mut rest = self.body.as_str();
        while let Some(index) = rest.find('$') {
            output.push_str(rest.get(..index).unwrap_or_default());
            let after = rest.get(index.saturating_add(1)..).unwrap_or_default();
            if let Some(remaining) = after.strip_prefix('$') {
                output.push('$');
                rest = remaining;
            } else if let Some(remaining) = after.strip_prefix("ARGUMENTS") {
                output.push_str(arguments);
                used = true;
                rest = remaining;
            } else if let Some(digit) = after
                .bytes()
                .next()
                .filter(|byte| (b'1'..=b'9').contains(byte))
            {
                let position = usize::from(digit.saturating_sub(b'1'));
                output.push_str(words.get(position).copied().unwrap_or_default());
                used = true;
                rest = after.get(1..).unwrap_or_default();
            } else {
                output.push('$');
                rest = after;
            }
        }
        output.push_str(rest);
        if !used && !arguments.is_empty() {
            let trimmed = output.trim_end().len();
            output.truncate(trimmed);
            output.push_str("\n\n");
            output.push_str(arguments);
        }
        output
    }
}

#[cfg(test)]
mod tests;
