//! gitignore と除外を守って列挙と検索を行う（設計書 01-03「glob」「grep」）。

use super::{ToolContext, ToolError, read, text};
use crate::safe_fs::ScopeRoot;
use globset::{Glob, GlobMatcher};
use ignore::{Walk, WalkBuilder};
use regex::RegexBuilder;
use serde_json::Value;
use std::collections::BTreeSet;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

enum SearchRoot<'a> {
    Project(&'a ScopeRoot),
    External(ScopeRoot),
}

impl SearchRoot<'_> {
    fn scope(&self) -> &ScopeRoot {
        match self {
            Self::Project(root) => root,
            Self::External(root) => root,
        }
    }
}

struct Search<'a> {
    path: PathBuf,
    root: SearchRoot<'a>,
    walk: Walk,
    filter: Option<GlobMatcher>,
}

impl<'a> Search<'a> {
    fn new(
        context: &'a ToolContext,
        arguments: &Value,
        pattern: Option<&str>,
    ) -> Result<Self, ToolError> {
        let requested = arguments.get("path").and_then(Value::as_str).unwrap_or(".");
        let path = context.search_path(Path::new(requested))?;
        let directory = if path.is_dir() {
            path.as_path()
        } else {
            path.parent().ok_or(ToolError::OutsideScope)?
        };
        let root = if directory.starts_with(context.project.real_path()) {
            // 検査後に走査の根が差し替わっても、開く範囲をプロジェクトから広げない。
            SearchRoot::Project(&context.project)
        } else {
            SearchRoot::External(ScopeRoot::open(directory)?)
        };
        let mut builder = WalkBuilder::new(&path);
        let rules = context.exclude.clone();
        let project = context.project.real_path().to_path_buf();
        let restricted = context.mode == crate::config::Mode::Restricted;
        builder
            .hidden(false)
            .require_git(false)
            .git_global(false)
            .git_exclude(false)
            .parents(false)
            .follow_links(false)
            .sort_by_file_path(|a, b| a.cmp(b))
            .filter_entry(move |entry| {
                std::fs::canonicalize(entry.path()).is_ok_and(|real| {
                    (!restricted || real.starts_with(&project)) && rules.descend(&real)
                })
            });
        Ok(Self {
            path,
            root,
            walk: builder.build(),
            filter: pattern.map(compile_glob).transpose()?,
        })
    }

    fn matches(&self, path: &Path) -> bool {
        let relative = path.strip_prefix(&self.path).unwrap_or(path);
        self.filter.as_ref().is_none_or(|filter| {
            filter.is_match(relative)
                || path
                    .file_name()
                    .is_some_and(|name| filter.is_match(Path::new(name)))
        })
    }
}

pub(super) fn glob(context: &ToolContext, arguments: &Value) -> Result<String, ToolError> {
    let pattern = read::string(arguments, "pattern")?;
    let mut search = Search::new(context, arguments, Some(pattern))?;
    let mut files = Vec::new();
    while let Some(entry) = search.walk.next() {
        let entry = entry.map_err(|error| ToolError::Io(std::io::Error::other(error)))?;
        if !entry.file_type().is_some_and(|kind| kind.is_file()) || !search.matches(entry.path()) {
            continue;
        }
        let opened = match context.open_walk_file(search.root.scope(), entry.path()) {
            Ok(opened) => opened,
            Err(ToolError::Excluded(_)) => continue,
            Err(error) => return Err(error),
        };
        let modified = opened
            .file
            .metadata()?
            .modified()
            .unwrap_or(SystemTime::UNIX_EPOCH);
        files.push((modified, display_path(context, entry.path())));
    }
    files.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
    let mut output = files
        .iter()
        .take(500)
        .map(|(_, path)| path.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    if files.len() > 500 {
        output.push('\n');
        output.push_str(&text::glob_truncated(files.len()));
    }
    if files.is_empty() {
        output.push_str(text::NO_FILES);
    }
    Ok(output)
}

pub(super) fn grep(context: &ToolContext, arguments: &Value) -> Result<String, ToolError> {
    let pattern = read::string(arguments, "pattern")?;
    let regex = RegexBuilder::new(pattern)
        .case_insensitive(flag(arguments, "ignore_case"))
        .build()
        .map_err(|error| ToolError::Pattern(text::pattern(&error)))?;
    let context_lines =
        usize::try_from(read::number(arguments, "context", 0, true)?).unwrap_or(usize::MAX);
    let files_only = flag(arguments, "files_only");
    let filter = arguments.get("glob").and_then(Value::as_str);
    let mut search = Search::new(context, arguments, filter)?;
    let mut output = String::new();
    let mut hits = 0usize;
    let mut truncated = false;
    'files: while let Some(entry) = search.walk.next() {
        let entry = entry.map_err(|error| ToolError::Io(std::io::Error::other(error)))?;
        if !entry.file_type().is_some_and(|kind| kind.is_file()) || !search.matches(entry.path()) {
            continue;
        }
        let mut opened = match context.open_walk_file(search.root.scope(), entry.path()) {
            Ok(opened) => opened,
            Err(ToolError::Excluded(_)) => continue,
            Err(error) => return Err(error),
        };
        let mut bytes = Vec::new();
        opened.file.read_to_end(&mut bytes)?;
        if read::binary(&bytes) {
            continue;
        }
        let contents = String::from_utf8_lossy(&bytes);
        let lines: Vec<_> = contents.lines().collect();
        let path = display_path(context, entry.path());
        let mut shown = BTreeSet::new();
        for (index, line) in lines.iter().enumerate() {
            let Some(found) = regex.find(line) else {
                continue;
            };
            if hits >= 100 {
                truncated = true;
                break 'files;
            }
            if files_only {
                if !append(&mut output, &path) {
                    truncated = true;
                    break 'files;
                }
                hits = hits.saturating_add(1);
                continue 'files;
            }
            hits = hits.saturating_add(1);
            let first = index.saturating_sub(context_lines);
            let last = index
                .saturating_add(context_lines)
                .min(lines.len().saturating_sub(1));
            for (number, nearby) in lines
                .iter()
                .enumerate()
                .take(last.saturating_add(1))
                .skip(first)
            {
                if !shown.insert(number) {
                    continue;
                }
                let position = if number == index {
                    found.start()
                } else {
                    regex.find(nearby).map_or(0, |matched| matched.start())
                };
                let record =
                    text::matched(&path, number.saturating_add(1), &excerpt(nearby, position));
                if !append(&mut output, &record) {
                    truncated = true;
                    break 'files;
                }
            }
        }
    }
    if truncated {
        output.push_str(text::SEARCH_TRUNCATED);
    }
    if output.is_empty() {
        output.push_str(text::NO_MATCHES);
    }
    Ok(output)
}

fn append(output: &mut String, record: &str) -> bool {
    if output
        .len()
        .saturating_add(record.len())
        .saturating_add(1)
        .saturating_add(text::SEARCH_TRUNCATED.len())
        > read::MAX_BYTES
    {
        return false;
    }
    output.push_str(record);
    output.push('\n');
    true
}

fn excerpt(line: &str, matched_byte: usize) -> String {
    let total = line.chars().count();
    if total <= 500 {
        return line.to_owned();
    }
    let position = line.get(..matched_byte).unwrap_or_default().chars().count();
    let start = position.saturating_sub(500);
    let end = position.saturating_add(500).min(total);
    let contents: String = line
        .chars()
        .skip(start)
        .take(end.saturating_sub(start))
        .collect();
    format!(
        "{contents}{}",
        text::char_range(start.saturating_add(1), end, total)
    )
}

fn display_path(context: &ToolContext, path: &Path) -> String {
    path.strip_prefix(context.project.real_path())
        .unwrap_or(path)
        .to_string_lossy()
        .into_owned()
}

fn compile_glob(pattern: &str) -> Result<GlobMatcher, ToolError> {
    Glob::new(pattern)
        .map(|glob| glob.compile_matcher())
        .map_err(|error| ToolError::Pattern(text::pattern(&error)))
}

fn flag(arguments: &Value, name: &str) -> bool {
    arguments
        .get(name)
        .and_then(Value::as_bool)
        .unwrap_or(false)
}
