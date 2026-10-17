//! gitignore と除外を守って列挙と検索を行う（設計書 01-03「glob」「grep」）。

use super::search_notices::Notices;
use super::{ToolContext, ToolError, read, text, truncate};
use crate::safe_fs::{ScopeRelPath, ScopeRoot, gitignore};
use globset::{Glob, GlobMatcher};
use ignore::gitignore::Gitignore;
use regex::{Regex, RegexBuilder};
use rustix::fs::FileType;
use serde_json::Value;
use std::fs::File;
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

struct Pending {
    relative: PathBuf,
    ignores: Arc<Vec<Arc<Gitignore>>>,
}

struct WalkFile {
    directory: Arc<ScopeRoot>,
    local: ScopeRelPath,
    path: PathBuf,
    stat: rustix::fs::Stat,
}

struct Search<'a> {
    context: &'a ToolContext,
    root: ScopeRoot,
    parent: Option<(PathBuf, Arc<ScopeRoot>)>,
    last_path: PathBuf,
    filter: Option<GlobMatcher>,
    pending: Vec<Pending>,
    cancelled: &'a AtomicBool,
    started: Instant,
    visited: usize,
    notices: Notices,
    stopped: Option<&'static str>,
    entry_limited: bool,
}

impl<'a> Search<'a> {
    fn new(
        context: &'a ToolContext,
        arguments: &Value,
        pattern: Option<&str>,
        cancelled: &'a AtomicBool,
    ) -> Result<Self, ToolError> {
        let started = Instant::now();
        let requested = arguments.get("path").and_then(Value::as_str).unwrap_or(".");
        let path = context.search_path(Path::new(requested))?;
        let is_directory = std::fs::metadata(&path)?.is_dir();
        let directory = if is_directory {
            path.as_path()
        } else {
            path.parent().ok_or(ToolError::OutsideScope)?
        };
        // 起点だけはプロジェクト内のリンクを解決できる口で開く。その後は
        // 根の記述子から一要素ずつ、途中のリンクも拒む（設計書 01-03「glob」）。
        let root = if directory == context.project.real_path() {
            context.project.no_follow()?
        } else if let Ok(relative) = directory.strip_prefix(context.project.real_path()) {
            context
                .project
                .open_dir(&ScopeRelPath::new(relative)?)?
                .no_follow()?
        } else {
            ScopeRoot::open(directory)?.no_follow()?
        };
        let mut search = Self {
            context,
            last_path: root.real_path().to_path_buf(),
            root,
            parent: None,
            filter: pattern.map(compile_glob).transpose()?,
            pending: Vec::new(),
            cancelled,
            started,
            visited: 0,
            notices: Notices::default(),
            stopped: None,
            entry_limited: false,
        };
        let mut ignores = Vec::new();
        if is_directory {
            // 起点より上でも、プロジェクトの根より上の規則は読まない。
            if let Ok(relative) = search
                .root
                .real_path()
                .strip_prefix(context.project.real_path())
            {
                let relative = relative.to_path_buf();
                for prefix in relative.ancestors().collect::<Vec<_>>().into_iter().rev() {
                    if search.stop() {
                        break;
                    }
                    let ancestor = if prefix == relative {
                        search.root.no_follow()
                    } else if prefix.as_os_str().is_empty() {
                        context.project.no_follow()
                    } else {
                        context
                            .project
                            .no_follow()
                            .and_then(|root| root.open_dir(&ScopeRelPath::new(prefix)?))
                    };
                    match ancestor {
                        Ok(ancestor) => search.load_ignore(&ancestor, &mut ignores),
                        Err(_) => search
                            .notices
                            .directory(&context.project.real_path().join(prefix)),
                    }
                }
            } else {
                let directory = search.root.no_follow()?;
                search.load_ignore(&directory, &mut ignores);
            }
            let directory = search.root.no_follow()?;
            search.children(&directory, Path::new(""), Arc::new(ignores));
        } else {
            search.pending.push(Pending {
                relative: PathBuf::from(path.file_name().ok_or(ToolError::OutsideScope)?),
                ignores: Arc::new(Vec::new()),
            });
            if context.search_limits.max_entries == 0 {
                search.pending.clear();
                search.entry_limit(&path);
            }
        }
        Ok(search)
    }

    fn stop(&mut self) -> bool {
        if self.stopped.is_none() {
            if self.cancelled.load(Ordering::Acquire) {
                self.stopped = Some(text::STOP_CANCELLED);
                self.notices.stop(&self.last_path, text::STOP_CANCELLED);
            } else if self.started.elapsed() >= self.context.search_limits.max_duration {
                self.stopped = Some(text::STOP_TIME);
                self.notices.stop(&self.last_path, text::STOP_TIME);
            }
        }
        self.stopped.is_some()
    }

    fn entry_limit(&mut self, path: &Path) {
        self.entry_limited = true;
        self.notices.stop(path, text::STOP_ENTRY);
    }

    fn load_ignore(&mut self, directory: &ScopeRoot, ignores: &mut Vec<Arc<Gitignore>>) {
        self.last_path = directory.real_path().join(".gitignore");
        match gitignore::load_gitignore(
            directory,
            directory.real_path(),
            gitignore::MAX_IGNORE_BYTES,
            || self.stop(),
        ) {
            Ok(Some(ignore)) => ignores.push(Arc::new(ignore)),
            Ok(None) | Err(gitignore::IgnoreError::Stopped) => {}
            Err(_) => self
                .notices
                .ignore(&directory.real_path().join(".gitignore")),
        }
    }

    fn children(
        &mut self,
        directory: &ScopeRoot,
        relative: &Path,
        ignores: Arc<Vec<Arc<Gitignore>>>,
    ) {
        let mut names = Vec::new();
        let result = directory.visit_names(|name| {
            self.last_path = directory.real_path().join(&name);
            if self.stop() {
                return false;
            }
            if names.len()
                >= self
                    .context
                    .search_limits
                    .max_entries
                    .saturating_sub(self.visited)
            {
                self.entry_limit(&directory.real_path().join(&name));
                return false;
            }
            names.push(name);
            true
        });
        if result.is_err() {
            self.notices.directory(directory.real_path());
        }
        names.sort();
        // 子を先に処理する予算を残し、後回しの兄弟で上限を使わない。
        // 待つ名前の数も上限内に保つ（設計書 01-03「glob」）。
        let keep = self
            .context
            .search_limits
            .max_entries
            .saturating_sub(self.visited)
            .saturating_sub(names.len());
        let discard = self.pending.len().saturating_sub(keep);
        if discard > 0 {
            self.pending.drain(..discard);
            self.entry_limit(directory.real_path());
        }
        self.pending
            .extend(names.into_iter().rev().map(|name| Pending {
                relative: relative.join(name),
                ignores: Arc::clone(&ignores),
            }));
    }

    fn directory(&mut self, relative: &Path) -> Result<Arc<ScopeRoot>, ToolError> {
        if let Some((cached_path, directory)) = &self.parent
            && cached_path == relative
        {
            return Ok(Arc::clone(directory));
        }
        // 同じ親の兄弟と、その親から降りる間は直前の記述子を使う。
        // 祖先へ戻るときだけ根から開き直す（設計書 01-03「glob」）。
        let (mut directory, remainder) = match &self.parent {
            Some((cached_path, cached)) if relative.starts_with(cached_path) => (
                cached.no_follow()?,
                relative.strip_prefix(cached_path).unwrap_or(relative),
            ),
            Some(_) | None => (self.root.no_follow()?, relative),
        };
        for component in remainder.components() {
            directory =
                directory.open_dir(&ScopeRelPath::new(Path::new(component.as_os_str()))?)?;
        }
        let directory = Arc::new(directory);
        self.parent = Some((relative.to_path_buf(), Arc::clone(&directory)));
        Ok(directory)
    }

    fn next(&mut self) -> Result<Option<WalkFile>, ToolError> {
        while !self.stop() {
            let Some(Pending { relative, ignores }) = self.pending.pop() else {
                return Ok(None);
            };
            self.last_path = self.root.real_path().join(&relative);
            if self.visited >= self.context.search_limits.max_entries {
                self.entry_limit(&self.root.real_path().join(&relative));
                self.pending.clear();
                return Ok(None);
            }
            self.visited = self.visited.saturating_add(1);
            let parent = relative.parent().unwrap_or(Path::new(""));
            let directory = match self.directory(parent) {
                Ok(directory) => directory,
                Err(_) => {
                    self.notices.directory(&self.root.real_path().join(parent));
                    continue;
                }
            };
            let local = ScopeRelPath::new(Path::new(
                relative.file_name().ok_or(ToolError::OutsideScope)?,
            ))?;
            let path = directory.real_path().join(local.as_path());
            let stat = match directory.stat(&local) {
                Ok(stat) => stat,
                Err(_) => {
                    if self.context.exclude.reason(&path).is_none() {
                        self.notices.file(&path, text::SKIP_UNREADABLE);
                    }
                    continue;
                }
            };
            let kind = FileType::from_raw_mode(stat.st_mode);
            if kind == FileType::Symlink {
                continue;
            }
            let is_directory = kind == FileType::Directory;
            if is_directory {
                if !self.context.exclude.descend(&path) {
                    continue;
                }
            } else if self.context.exclude.reason(&path).is_some() {
                continue;
            }
            if ignores
                .iter()
                .rev()
                .find_map(|ignore| gitignore::decision(ignore, &path, is_directory))
                .unwrap_or(false)
            {
                continue;
            }
            if is_directory {
                match directory.open_dir(&local) {
                    Ok(child) => {
                        if !child.real_path().starts_with(self.root.real_path())
                            || !self.context.exclude.descend(child.real_path())
                        {
                            continue;
                        }
                        let mut ignores = ignores.as_ref().clone();
                        self.load_ignore(&child, &mut ignores);
                        let child = Arc::new(child);
                        self.parent = Some((relative.clone(), Arc::clone(&child)));
                        self.children(&child, &relative, Arc::new(ignores));
                    }
                    Err(_) => self.notices.directory(&path),
                }
            } else if self.matches(&path) {
                return Ok(Some(WalkFile {
                    directory,
                    local,
                    path,
                    stat,
                }));
            }
        }
        Ok(None)
    }

    fn matches(&self, path: &Path) -> bool {
        let relative = path.strip_prefix(self.root.real_path()).unwrap_or(path);
        self.filter.as_ref().is_none_or(|filter| {
            filter.is_match(relative)
                || path
                    .file_name()
                    .is_some_and(|name| filter.is_match(Path::new(name)))
        })
    }

    fn partial(&self) -> bool {
        self.entry_limited || self.stopped.is_some()
    }

    fn finish(&self, output: &mut String) {
        let notices = self.notices.render(|path| display_path(self.context, path));
        if notices.is_empty() {
            return;
        }
        let budget = truncate::MAX_OUTPUT_BYTES
            .saturating_sub(notices.len())
            .saturating_sub(1);
        if output.len() > budget {
            *output = truncate::truncate(
                output,
                truncate::Side::Head,
                usize::MAX,
                budget,
                text::SEARCH_TRUNCATED,
            )
            .text;
        }
        footer(output, &notices);
    }
}

pub(super) fn glob(
    context: &ToolContext,
    arguments: &Value,
    cancelled: &AtomicBool,
) -> Result<String, ToolError> {
    let pattern = read::string(arguments, "pattern")?;
    let mut search = Search::new(context, arguments, Some(pattern), cancelled)?;
    let mut files = Vec::new();
    while let Some(entry) = search.next()? {
        if FileType::from_raw_mode(entry.stat.st_mode) != FileType::RegularFile {
            continue;
        }
        // fstatat の結果を使い、列挙のためにファイルを開かない（設計書 01-03「glob」）。
        files.push(((entry.stat.st_mtime, entry.stat.st_mtime_nsec), entry.path));
    }
    files.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
    let mut output = files
        .iter()
        .take(500)
        .map(|(_, path)| display_path(context, path))
        .collect::<Vec<_>>()
        .join("\n");
    if files.len() > 500 {
        footer(
            &mut output,
            &if search.partial() {
                text::glob_partial(files.len())
            } else {
                text::glob_truncated(files.len())
            },
        );
    }
    if files.is_empty() {
        output.push_str(text::NO_FILES);
    }
    search.finish(&mut output);
    Ok(output)
}

pub(super) fn grep(
    context: &ToolContext,
    arguments: &Value,
    cancelled: &AtomicBool,
) -> Result<String, ToolError> {
    let pattern = read::string(arguments, "pattern")?;
    let regex = RegexBuilder::new(pattern)
        .case_insensitive(flag(arguments, "ignore_case"))
        .build()
        .map_err(|error| ToolError::Pattern(text::pattern(&error)))?;
    let context_lines =
        usize::try_from(read::number(arguments, "context", 0, true)?).unwrap_or(usize::MAX);
    let files_only = flag(arguments, "files_only");
    let filter = arguments.get("glob").and_then(Value::as_str);
    let mut search = Search::new(context, arguments, filter, cancelled)?;
    let mut output = String::new();
    let mut hits = 0usize;
    let mut truncated = false;
    let options = GrepOptions {
        regex,
        context_lines,
        files_only,
    };
    while let Some(entry) = search.next()? {
        let kind = FileType::from_raw_mode(entry.stat.st_mode);
        if kind != FileType::RegularFile {
            if matches!(
                kind,
                FileType::Fifo | FileType::CharacterDevice | FileType::BlockDevice
            ) {
                search.notices.file(&entry.path, text::SKIP_SPECIAL);
            }
            continue;
        }
        let before = output.len();
        let previous_hits = hits;
        match grep_file(
            context,
            &entry,
            &options,
            &mut search,
            &mut output,
            &mut hits,
        ) {
            Ok(limited) => truncated = limited,
            Err(error) => {
                // ファイル単位の失敗は、その途中の結果と件数を戻して次へ進む。
                // F_GETPATH の長さの制限も全体の失敗にしない（設計書 01-03「grep」）。
                output.truncate(before);
                hits = previous_hits;
                if !matches!(error, ToolError::Excluded(_)) {
                    search.notices.file(&entry.path, text::SKIP_UNREADABLE);
                }
            }
        }
        if truncated || search.stop() {
            break;
        }
    }
    if truncated {
        footer(&mut output, text::SEARCH_TRUNCATED);
    }
    if output.is_empty() {
        output.push_str(text::NO_MATCHES);
    }
    search.finish(&mut output);
    Ok(output)
}

struct GrepOptions {
    regex: Regex,
    context_lines: usize,
    files_only: bool,
}

fn grep_file(
    context: &ToolContext,
    entry: &WalkFile,
    options: &GrepOptions,
    search: &mut Search<'_>,
    output: &mut String,
    hits: &mut usize,
) -> Result<bool, ToolError> {
    let path = display_path(context, &entry.path);
    let file = entry.directory.open_read(&entry.local)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() {
        search.notices.file(&entry.path, text::SKIP_SPECIAL);
        return Ok(false);
    }
    let opened = context.check_file(file, Some(&search.root))?;
    let max_bytes = context.search_limits.max_file_bytes;
    if metadata.len() > max_bytes {
        search.notices.file(&entry.path, text::SKIP_LARGE);
        return Ok(false);
    }
    let mut truncated = false;
    let mut lines = Lines::new(opened.file, max_bytes);
    if lines.is_binary(metadata.len(), search)? {
        return Ok(false);
    }
    let mut number = 0usize;
    let mut matched = Vec::new();
    while let Some(line) = lines.next(search)? {
        number = number.saturating_add(1);
        let Some(found) = options.regex.find(&line) else {
            continue;
        };
        if *hits >= 100 {
            truncated = true;
            break;
        }
        if options.files_only {
            if !append(output, &path) {
                truncated = true;
            } else {
                *hits = hits.saturating_add(1);
            }
            break;
        }
        *hits = hits.saturating_add(1);
        let record = text::matched(&path, number, &excerpt(&line, found.start()));
        if options.context_lines == 0 {
            if !append(output, &record) {
                truncated = true;
                break;
            }
        } else {
            matched.push((number, record));
        }
    }
    if options.context_lines > 0 && !options.files_only && !matched.is_empty() {
        if search.stop() {
            // 走査中の時間切れでも、すでに見つけた行は返す。
            for (_, record) in &matched {
                if !append(output, record) {
                    truncated = true;
                    break;
                }
            }
        } else if !show_context(
            &mut lines,
            &matched,
            options.context_lines,
            &options.regex,
            &path,
            search,
            output,
        )? {
            truncated = true;
        }
    }
    Ok(truncated)
}

// 前後の行は同じ記述子から二度目の走査で得る。巨大な context を指定されても
// ファイル全体や行の一覧を保持しない（設計書 01-03「grep」）。
fn show_context(
    lines: &mut Lines,
    matched: &[(usize, String)],
    context: usize,
    regex: &Regex,
    path: &str,
    search: &mut Search<'_>,
    output: &mut String,
) -> Result<bool, ToolError> {
    lines.rewind()?;
    let mut number = 0usize;
    let mut current = 0usize;
    let mut last_shown = 0usize;
    while let Some(line) = lines.next(search)? {
        number = number.saturating_add(1);
        while matched
            .get(current)
            .is_some_and(|(hit, _)| hit.saturating_add(context) < number)
        {
            current = current.saturating_add(1);
        }
        let Some((hit, _)) = matched.get(current) else {
            break;
        };
        if number < hit.saturating_sub(context) {
            continue;
        }
        let position = regex.find(&line).map_or(0, |found| found.start());
        if !append(
            output,
            &text::matched(path, number, &excerpt(&line, position)),
        ) {
            return Ok(false);
        }
        last_shown = number;
    }
    for (number, record) in matched {
        if *number > last_shown && !append(output, record) {
            return Ok(false);
        }
    }
    Ok(true)
}

struct Lines {
    reader: BufReader<File>,
    consumed: u64,
    max_bytes: u64,
}
impl Lines {
    fn new(file: File, max_bytes: u64) -> Self {
        Self {
            reader: BufReader::with_capacity(8192, file),
            consumed: 0,
            max_bytes,
        }
    }
    fn is_binary(&mut self, file_len: u64, search: &mut Search<'_>) -> Result<bool, ToolError> {
        if search.stop() {
            return Ok(false);
        }
        let buffer = self.reader.fill_buf()?;
        // 先頭の 8 KiB（小さいファイルは全体）が既にあれば消費せずに使う。
        // 同じ中身の read と seek を繰り返さない（設計書 01-03「grep」）。
        if buffer.is_empty()
            || buffer.len() == 8192
            || (file_len > 0 && u64::try_from(buffer.len()).unwrap_or(u64::MAX) >= file_len)
        {
            return Ok(read::binary(buffer));
        }
        // 通常のファイルでも短い read は許されるので、足りない場合は従来どおり
        // EOF か 8 KiB まで読み、同じ記述子を巻き戻す。
        let mut sample = Vec::new();
        let mut buffer = [0; 8192];
        while sample.len() < 8192 && !search.stop() {
            let remaining = 8192usize.saturating_sub(sample.len());
            let count = self
                .reader
                .read(buffer.get_mut(..remaining).unwrap_or_default())?;
            if count == 0 {
                break;
            }
            sample.extend_from_slice(buffer.get(..count).unwrap_or_default());
        }
        self.rewind()?;
        Ok(read::binary(&sample))
    }
    fn rewind(&mut self) -> Result<(), ToolError> {
        self.reader.seek(SeekFrom::Start(0))?;
        self.consumed = 0;
        Ok(())
    }
    fn next(&mut self, search: &mut Search<'_>) -> Result<Option<String>, ToolError> {
        let mut bytes = Vec::new();
        let mut newline = false;
        loop {
            if search.stop() {
                return Ok(None);
            }
            let buffer = self.reader.fill_buf()?;
            if buffer.is_empty() {
                break;
            }
            let count =
                buffer
                    .iter()
                    .position(|byte| *byte == b'\n')
                    .map_or(buffer.len(), |position| {
                        newline = true;
                        position.saturating_add(1)
                    });
            self.consumed = self
                .consumed
                .saturating_add(u64::try_from(count).unwrap_or(u64::MAX));
            if self.consumed > self.max_bytes {
                search.notices.file(&search.last_path, text::SKIP_LARGE);
                return Ok(None);
            }
            bytes.extend_from_slice(buffer.get(..count).unwrap_or_default());
            self.reader.consume(count);
            if newline {
                break;
            }
        }
        if bytes.is_empty() {
            return Ok(None);
        }
        if newline {
            bytes.pop();
            if bytes.last() == Some(&b'\r') {
                bytes.pop();
            }
        }
        Ok(Some(match String::from_utf8(bytes) {
            Ok(line) => line,
            Err(error) => String::from_utf8_lossy(error.as_bytes()).into_owned(),
        }))
    }
}

fn footer(output: &mut String, notice: &str) {
    if !output.is_empty() && !output.ends_with('\n') {
        output.push('\n');
    }
    output.push_str(notice);
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
    let path = path
        .strip_prefix(context.project.real_path())
        .unwrap_or(path);
    let path = if path.as_os_str().is_empty() {
        Path::new(".")
    } else {
        path
    };
    text::visible_path(&path.to_string_lossy())
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
