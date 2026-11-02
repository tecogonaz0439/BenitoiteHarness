//! `@` に続けてプロジェクトのファイルのパスを補う（設計書 05-02「入力の操作」）。
//!
//! 補うのはパスの文字だけで、ファイルの中身は読まない。中身は LLM が道具で読むので、
//! 読まないファイルなどの規則がそのまま効く。

use std::collections::VecDeque;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crate::safe_fs::{EntryKind, ScopeRelPath, ScopeRoot};

/// 集めるファイルの数の上限。大きなリポジトリでも入力を止めない。
const FILE_LIMIT: usize = 20_000;
/// 集める時間の上限。
const TIME_LIMIT: Duration = Duration::from_secs(2);
/// 候補として保つ数。
const MATCH_LIMIT: usize = 50;

/// 補っている途中の状態。続けて Tab を押すと次の候補に替える。
#[derive(Clone, Debug)]
pub(super) struct Mention {
    start: usize,
    end: usize,
    matches: Vec<String>,
    index: usize,
}

/// プロジェクトの下のファイルを、相対のパスで集める。リンクを辿らず、`.` で始まる名前と、
/// 根の `.gitignore` が除くものを飛ばす。
pub(super) fn project_files(project: &Path) -> Vec<String> {
    let Ok(root) = ScopeRoot::open(project).and_then(|root| root.no_follow()) else {
        return Vec::new();
    };
    let ignore =
        crate::safe_fs::gitignore::load_gitignore(&root, root.real_path(), 1024 * 1024, || false)
            .ok()
            .flatten();
    let started = Instant::now();
    let mut files = Vec::new();
    let mut pending = VecDeque::from([PathBuf::new()]);
    while let Some(directory) = pending.pop_front() {
        if files.len() >= FILE_LIMIT || started.elapsed() > TIME_LIMIT {
            break;
        }
        let entries = if directory.as_os_str().is_empty() {
            root.read_dir(None)
        } else {
            match ScopeRelPath::new(&directory) {
                Ok(relative) => root.read_dir(Some(&relative)),
                Err(error) => Err(error),
            }
        };
        let Ok(entries) = entries else {
            continue;
        };
        for entry in entries {
            if hidden(&entry.name) {
                continue;
            }
            let relative = directory.join(&entry.name);
            let is_directory = entry.kind == EntryKind::Directory;
            if ignore.as_ref().is_some_and(|rules| {
                crate::safe_fs::gitignore::decision(
                    rules,
                    &root.real_path().join(&relative),
                    is_directory,
                ) == Some(true)
            }) {
                continue;
            }
            match entry.kind {
                EntryKind::Directory => pending.push_back(relative),
                EntryKind::File => {
                    if let Some(path) = relative.to_str() {
                        files.push(path.to_owned());
                    }
                }
                EntryKind::Symlink | EntryKind::Other => {}
            }
        }
    }
    files.sort();
    files
}

fn hidden(name: &OsString) -> bool {
    name.as_encoded_bytes().first() == Some(&b'.')
}

/// 問いの文字を順に含むパスを、よく合う順に並べる。大文字と小文字を区別しない。
/// 名前の部分に問いを含むもの、間の飛びが少ないもの、短いものを先にする。
pub(super) fn rank(files: &[String], query: &str) -> Vec<String> {
    let query = query.to_lowercase();
    let mut scored = files
        .iter()
        .filter_map(|path| {
            let lower = path.to_lowercase();
            let gaps = subsequence_gaps(&lower, &query)?;
            let name = lower.rsplit('/').next().unwrap_or(&lower);
            Some(((!name.contains(&query), gaps, path.len()), path))
        })
        .collect::<Vec<_>>();
    scored.sort_by(|left, right| left.0.cmp(&right.0).then_with(|| left.1.cmp(right.1)));
    scored
        .into_iter()
        .take(MATCH_LIMIT)
        .map(|(_, path)| path.clone())
        .collect()
}

/// `query` の文字が `text` に順に現れるなら、間に挟まる文字の数を返す。
fn subsequence_gaps(text: &str, query: &str) -> Option<usize> {
    let mut gaps = 0_usize;
    let mut started = false;
    let mut wanted = query.chars().peekable();
    for character in text.chars() {
        match wanted.peek() {
            None => break,
            Some(next) if *next == character => {
                started = true;
                wanted.next();
            }
            Some(_) if started => gaps = gaps.saturating_add(1),
            Some(_) => {}
        }
    }
    wanted.peek().is_none().then_some(gaps)
}

impl super::Screen {
    /// カーソルの直前の `@` で始まる語を、候補のパスに替える。替えたら真を返す。
    pub(super) fn complete_mention(&mut self) -> bool {
        let cursor = self.input.cursor;
        if let Some(mention) = &mut self.mention
            && mention.end == cursor
            && mention.matches.len() > 1
            && self
                .input
                .text
                .get(mention.start..mention.end)
                .zip(mention.matches.get(mention.index))
                .is_some_and(|(current, path)| current.strip_prefix('@') == Some(path.as_str()))
        {
            mention.index = mention
                .index
                .saturating_add(1)
                .checked_rem(mention.matches.len())
                .unwrap_or(0);
            if let Some(path) = mention.matches.get(mention.index).cloned() {
                let replacement = format!("@{path}");
                self.input
                    .text
                    .replace_range(mention.start..mention.end, &replacement);
                mention.end = mention.start.saturating_add(replacement.len());
                self.input.cursor = mention.end;
            }
            return true;
        }
        let before = self.input.text.get(..cursor).unwrap_or_default();
        let start = before.rfind(char::is_whitespace).map_or(0, |index| {
            index.saturating_add(
                before
                    .get(index..)
                    .and_then(|rest| rest.chars().next())
                    .map_or(1, char::len_utf8),
            )
        });
        let Some(query) = before
            .get(start..)
            .and_then(|token| token.strip_prefix('@'))
        else {
            return false;
        };
        let Some(project) = self.project_dir.clone() else {
            return false;
        };
        let files = self.files.get_or_insert_with(|| project_files(&project));
        let matches = rank(files, query);
        let Some(first) = matches.first() else {
            return false;
        };
        let replacement = format!("@{first}");
        self.input.text.replace_range(start..cursor, &replacement);
        let end = start.saturating_add(replacement.len());
        self.input.cursor = end;
        self.mention = Some(Mention {
            start,
            end,
            matches,
            index: 0,
        });
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // 作成時の関門: 並べ方（名前に含む、飛びが少ない、短い）と、順に含まないものを除くことを表で確かめる。
    #[test]
    fn ranking_prefers_names_then_tight_then_short_matches() {
        let files = [
            "docs/main-notes.md",
            "src/main.rs",
            "src/domain/mapping.rs",
            "README.md",
            "src/cli/mod.rs",
        ]
        .map(str::to_owned);
        assert_eq!(
            rank(&files, "main"),
            ["src/main.rs", "docs/main-notes.md", "src/domain/mapping.rs"]
        );
        assert_eq!(rank(&files, "SRCMOD"), ["src/cli/mod.rs"]);
        assert!(rank(&files, "zzz").is_empty());
        assert_eq!(rank(&files, "").len(), files.len());
    }
}
