//! 診断の行を検証し、読めるソースだけを添えて返す（設計書 04-01「終了状態と診断の受け渡し」）。

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, PoisonError};

use serde::{Deserialize, Deserializer};

use super::{display, text};

/// 診断の解決と実行中の保持に共通の上限。
pub(crate) const MAX_RESOLVED_DIAGNOSTICS: usize = 1000;
pub(super) const MAX_EXCLUSION_WORK: usize = 100_000;

/// JSON で与えられる、1 から数える行・桁とバイト位置。
#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd)]
pub struct Position {
    pub line: u64,
    pub column: u64,
    pub offset: u64,
}

/// ファイル中の診断の範囲。
#[derive(Clone, Debug, Deserialize)]
pub struct Span {
    pub file: String,
    pub start: Position,
    pub end: Position,
    pub label: String,
}

/// 処理系が提案した置き換え。表示するだけで適用しない。
#[derive(Clone, Debug, Deserialize)]
pub struct Edit {
    pub file: String,
    pub start: Position,
    pub end: Position,
    pub replacement: String,
}

/// 修正の案内と置き換えの一覧。
#[derive(Clone, Debug, Deserialize)]
pub struct Help {
    pub message: String,
    pub edits: DiagnosticItems<Edit>,
}

/// 必須の欄と型を検証した一件の診断。
#[derive(Clone, Debug, Deserialize)]
pub struct Diagnostic {
    pub kind: String,
    pub severity: String,
    #[serde(deserialize_with = "nullable")]
    pub code: Option<String>,
    pub message: String,
    #[serde(deserialize_with = "nullable")]
    pub primary: Option<Span>,
    pub secondary: DiagnosticItems<Span>,
    pub notes: DiagnosticItems<String>,
    pub helps: DiagnosticItems<Help>,
}

/// 診断の配列は先頭二十件だけを保持し、残りを構築せずに読み捨てる。
#[derive(Clone, Debug)]
pub struct DiagnosticItems<T> {
    values: Vec<T>,
    /// 保持せずに読み捨てた要素の件数。
    pub omitted: usize,
}
impl<T> std::ops::Deref for DiagnosticItems<T> {
    type Target = [T];
    fn deref(&self) -> &Self::Target {
        &self.values
    }
}
impl<'de, T: Deserialize<'de>> Deserialize<'de> for DiagnosticItems<T> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Items<T>(std::marker::PhantomData<T>);
        impl<'de, T: Deserialize<'de>> serde::de::Visitor<'de> for Items<T> {
            type Value = DiagnosticItems<T>;
            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str(text::DIAGNOSTIC_ARRAY)
            }
            fn visit_seq<A: serde::de::SeqAccess<'de>>(
                self,
                mut seq: A,
            ) -> Result<Self::Value, A::Error> {
                let mut values = Vec::new();
                while values.len() < 20 {
                    let Some(value) = seq.next_element::<T>()? else {
                        return Ok(DiagnosticItems { values, omitted: 0 });
                    };
                    values.push(value);
                }
                let mut omitted = 0usize;
                while seq.next_element::<serde::de::IgnoredAny>()?.is_some() {
                    omitted = omitted.saturating_add(1);
                }
                Ok(DiagnosticItems { values, omitted })
            }
        }
        deserializer.deserialize_seq(Items(std::marker::PhantomData))
    }
}

/// 位置の文字列を解決する前の固定の上限。
pub(crate) fn bounded_path(file: &str) -> bool {
    file.len() <= 4096 && Path::new(file).components().take(257).count() <= 256
}

/// パスを一度だけ正規化し、後ろの部分は文字列の借用だけで比べる。
pub(crate) fn relative_text(path: &Path) -> Option<String> {
    path.components()
        .map(|component| component.as_os_str().to_str())
        .collect::<Option<Vec<_>>>()
        .map(|parts| parts.join("/"))
}

/// 要素の境界を指すバイト位置から、後ろの部分を借用する。
pub(crate) fn suffixes(file: &str) -> impl Iterator<Item = &str> {
    file.match_indices('/')
        .filter_map(|(at, _)| file.get(at.saturating_add(1)..))
        .filter(|part| !part.is_empty())
}

fn unread_file(file: &str) -> DiagnosticFile {
    DiagnosticFile {
        path: file.into(),
        display: None,
        exists: false,
        relative: None,
    }
}

fn bounded_diagnostic(diagnostic: &Diagnostic) -> bool {
    diagnostic
        .primary
        .iter()
        .chain(diagnostic.secondary.iter())
        .all(|span| bounded_path(&span.file))
        && diagnostic
            .helps
            .iter()
            .flat_map(|help| help.edits.iter())
            .all(|edit| bounded_path(&edit.file))
}

/// 一行がオブジェクトであることを確かめ、Value の木を構築せずに読む。
pub fn parse_line(line: &[u8]) -> Option<Diagnostic> {
    if line
        .iter()
        .copied()
        .find(|byte| !byte.is_ascii_whitespace())
        != Some(b'{')
    {
        return None;
    }
    serde_json::from_slice::<Diagnostic>(line).ok()
}

// Option の通常の読み方では欠けた欄まで null と同じになるため、必須として読む。
fn nullable<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(deserializer)
}

/// 診断の行と、それ以外の出力の行。
#[derive(Debug, Default)]
pub struct DiagnosticOutput {
    pub diagnostics: Vec<Diagnostic>,
    pub other_lines: Vec<String>,
}

/// 標準エラーの各行を独立して読む。壊れた一行を診断と取り違えない。
pub fn parse(bytes: &[u8]) -> DiagnosticOutput {
    let mut output = DiagnosticOutput::default();
    for line in bytes.split(|byte| *byte == b'\n') {
        let diagnostic = parse_line(line);
        match diagnostic {
            Some(diagnostic) => output.diagnostics.push(diagnostic),
            None => output
                .other_lines
                .push(String::from_utf8_lossy(line).into_owned()),
        }
    }
    output
}

/// 検査の終了状態と診断の整合を確かめた分類。
#[derive(Debug, Eq, PartialEq)]
pub enum CheckClass {
    Ok,
    Warnings,
    Failed,
    Usage(String),
    Processor(String),
}

/// 終了状態 2 を、検査の誤りと使い方の誤りに分ける。
pub fn classify_check(status: i32, output: &DiagnosticOutput) -> CheckClass {
    let diagnostics = &output.diagnostics;
    if status == 0 {
        if diagnostics.is_empty() {
            return CheckClass::Ok;
        }
        if diagnostics.iter().all(|d| d.severity == "warning") {
            return CheckClass::Warnings;
        }
    }
    if status == 2 {
        if diagnostics
            .iter()
            .any(|d| d.severity == "error" && (d.code.is_some() || d.primary.is_some()))
        {
            return CheckClass::Failed;
        }
        if let Some(error) = diagnostics.iter().find(|d| d.severity == "error") {
            return CheckClass::Usage(one_line(&error.message));
        }
        if diagnostics.is_empty() {
            let reason = output
                .other_lines
                .iter()
                .find(|line| !line.trim().is_empty());
            return CheckClass::Usage(
                reason.map_or_else(|| text::skill_exit(Some(status)), |s| one_line(s)),
            );
        }
    }
    CheckClass::Processor(text::skill_exit(Some(status)))
}

/// 誤りの理由を一行に限り、末尾の句点を結果の側で付ける。
pub fn one_line(reason: &str) -> String {
    reason
        .lines()
        .next()
        .unwrap_or_default()
        .trim_end_matches('.')
        .to_owned()
}

/// read と同じ判定を通した実のパス。開けない場合は存在を調べない。
#[derive(Clone, Debug)]
pub struct DiagnosticFile {
    pub path: PathBuf,
    pub display: Option<PathBuf>,
    /// 読めるファイルの存在。重複の判定には使わない。
    pub exists: bool,
    /// 処理系が示したパスの、プロジェクトからの相対の形。
    pub relative: Option<PathBuf>,
}

/// 上限まで保持したソースの一行と、残りを捨てた印。
#[derive(Debug)]
pub struct SourceLine {
    pub text: String,
    pub truncated: bool,
}

/// tools へ依存せずに、モードの読み取りの規則を受け取る境界。
pub trait DiagnosticSources {
    /// 照合に要る量。None は後ろの部分を調べる上限を超えたパスを表す。
    fn exclusion_cost(&self, _file: &str) -> Option<usize> {
        Some(0)
    }
    /// 相対のパスを処理系の作業ディレクトリから解き、read の可否を判定する。
    fn resolve(&self, file: &str) -> DiagnosticFile;
    /// 記述子を安全に開き直して一行だけを読む。読んだ版を記録しない。
    fn line(&self, file: &Path, line: u64) -> Option<SourceLine>;
    /// read と同じ開き方と上限で元のバイトを読み、SHA-256 を小文字の十六進で返す。
    /// 内容を読めない境界は、取り込みの確認を省略しない。
    fn content_hash(&self, _file: &Path, _cancelled: &AtomicBool) -> Option<String> {
        None
    }
}

struct ResolvedDiagnostic {
    diagnostic: Diagnostic,
    file: Option<DiagnosticFile>,
}

/// 同じ診断と、BH-029 の後ろの部分が重なる表示パスの重複を除いた診断。
pub struct PreparedDiagnostics {
    diagnostics: Vec<ResolvedDiagnostic>,
    files: Mutex<ResolutionState>,
    unread: usize,
    unresolved: usize,
    unresolved_failed: bool,
}

pub(super) struct ResolutionState {
    pub(super) files: BTreeMap<String, DiagnosticFile>,
    pub(super) limited: BTreeSet<String>,
    // 深い表示パスを大量に与えても、道具の時間上限の外で照合を続けない。
    pub(super) remaining: usize,
}

// 同じ表示パスを何度も開かず、補助位置と置き換えにも同じ判定を使う。
// キャッシュは一回の検査の結果だけが持ち、次の書き込みへ持ち越さない。
pub(super) struct CachedSources<'a, S> {
    pub(super) source: &'a S,
    pub(super) files: &'a Mutex<ResolutionState>,
    pub(super) cancelled: &'a AtomicBool,
    pub(super) refused: std::cell::Cell<bool>,
}
impl<S: DiagnosticSources> DiagnosticSources for CachedSources<'_, S> {
    fn resolve(&self, file: &str) -> DiagnosticFile {
        if self.cancelled.load(Ordering::Acquire) || !bounded_path(file) {
            return unread_file(file);
        }
        let mut state = self.files.lock().unwrap_or_else(PoisonError::into_inner);
        if state.limited.contains(file) {
            self.refused.set(true);
        }
        if let Some(cached) = state.files.get(file) {
            return cached.clone();
        }
        let cost = self.source.exclusion_cost(file);
        let resolved = if let Some(cost) = cost.filter(|cost| *cost <= state.remaining) {
            state.remaining = state.remaining.saturating_sub(cost);
            self.source.resolve(file)
        } else {
            self.refused.set(true);
            state.limited.insert(file.into());
            unread_file(file)
        };
        state.files.insert(file.into(), resolved.clone());
        resolved
    }
    fn line(&self, file: &Path, line: u64) -> Option<SourceLine> {
        if self.cancelled.load(Ordering::Acquire) {
            None
        } else {
            self.source.line(file, line)
        }
    }
}

/// 表示する診断と、別のファイル・読まないファイルの件数。
#[derive(Debug)]
pub struct RenderedDiagnostics {
    pub text: String,
    pub other: usize,
    pub unread: usize,
    pub selected: usize,
    pub failed: bool,
}

impl PreparedDiagnostics {
    /// 先頭千件までを解決してから重複を除く。読まないパスの存在には依存しない。
    pub fn new(
        diagnostics: Vec<Diagnostic>,
        sources: &impl DiagnosticSources,
        cancelled: &AtomicBool,
    ) -> Self {
        let total = diagnostics.len();
        let files = Mutex::new(ResolutionState {
            files: BTreeMap::new(),
            limited: BTreeSet::new(),
            remaining: MAX_EXCLUSION_WORK,
        });
        let cached = CachedSources {
            source: sources,
            files: &files,
            cancelled,
            refused: std::cell::Cell::new(false),
        };
        let mut resolved = Vec::new();
        let mut unread = 0usize;
        let mut inspected = 0usize;
        let mut unresolved_failed = false;
        for diagnostic in diagnostics {
            if inspected >= MAX_RESOLVED_DIAGNOSTICS || cancelled.load(Ordering::Acquire) {
                unresolved_failed |= diagnostic.severity == "error";
                continue;
            }
            inspected = inspected.saturating_add(1);
            if !bounded_diagnostic(&diagnostic) {
                unread = unread.saturating_add(1);
                continue;
            }
            let file = diagnostic
                .primary
                .as_ref()
                .map(|span| cached.resolve(&span.file));
            resolved.push(ResolvedDiagnostic { diagnostic, file });
        }
        let mut relative_paths: BTreeMap<LocationKey, BTreeSet<String>> = BTreeMap::new();
        for entry in &resolved {
            if cancelled.load(Ordering::Acquire) {
                break;
            }
            if let Some(relative) = entry
                .file
                .as_ref()
                .and_then(|file| file.relative.as_ref())
                .and_then(|path| relative_text(path))
            {
                relative_paths
                    .entry(location_key(&entry.diagnostic))
                    .or_default()
                    .insert(relative);
            }
        }
        // ファイルの存在には依存せず、先頭を落とした側へ寄せる。
        // 読まないパスの有無を件数から推測させない（BH-029、設計書 04-01）。
        let resolved: Vec<_> = resolved
            .into_iter()
            .filter(|entry| {
                if cancelled.load(Ordering::Acquire) {
                    return false;
                }
                let Some(relative) = entry.file.as_ref().and_then(|file| file.relative.as_ref())
                else {
                    return true;
                };
                let Some(paths) = relative_paths.get(&location_key(&entry.diagnostic)) else {
                    return true;
                };
                relative_text(relative).is_none_or(|relative| {
                    !suffixes(&relative).any(|suffix| paths.contains(suffix))
                })
            })
            .collect();
        let mut seen = BTreeSet::new();
        let mut diagnostics = Vec::new();
        for resolved in resolved {
            let d = &resolved.diagnostic;
            if cancelled.load(Ordering::Acquire) {
                break;
            }
            let key = (
                d.severity.clone(),
                d.code.clone(),
                d.message.clone(),
                resolved
                    .file
                    .as_ref()
                    .map(|f| f.display.as_ref().unwrap_or(&f.path).clone()),
                d.primary.as_ref().map(|s| (s.start.clone(), s.end.clone())),
            );
            if seen.insert(key) {
                diagnostics.push(resolved);
            }
        }
        // sort_by_key は安定なので、同じ重さの出力順を保つ。
        diagnostics.sort_by_key(|d| d.diagnostic.severity != "error");
        Self {
            diagnostics,
            files,
            unread,
            unresolved: total.saturating_sub(inspected),
            unresolved_failed,
        }
    }

    /// only があるときは、そのファイルと位置のない診断だけを本文にする。
    pub fn render(
        &self,
        sources: &impl DiagnosticSources,
        only: Option<&Path>,
        cancelled: &AtomicBool,
    ) -> RenderedDiagnostics {
        let cached = CachedSources {
            source: sources,
            files: &self.files,
            cancelled,
            refused: std::cell::Cell::new(false),
        };
        let mut result = RenderedDiagnostics {
            text: String::new(),
            other: 0,
            unread: self.unread,
            selected: 0,
            failed: self.unresolved_failed,
        };
        let mut shown = 0usize;
        let mut visible = 0usize;
        for entry in &self.diagnostics {
            if cancelled.load(Ordering::Acquire) {
                break;
            }
            let selected =
                only.is_none_or(|path| entry.file.as_ref().is_none_or(|f| f.path == path));
            if selected {
                result.selected = result.selected.saturating_add(1);
                result.failed |= entry.diagnostic.severity == "error";
            }
            if entry
                .file
                .as_ref()
                .is_some_and(|file| file.display.is_none())
            {
                result.unread = result.unread.saturating_add(1);
            } else if !selected {
                result.other = result.other.saturating_add(1);
            } else {
                if shown < 20 {
                    cached.refused.set(false);
                    let rendered =
                        render_one_line(&entry.diagnostic, entry.file.as_ref(), &cached, cancelled);
                    if cached.refused.get() {
                        result.unread = result.unread.saturating_add(1);
                        continue;
                    }
                    push_line(&mut result.text, &rendered);
                    shown = shown.saturating_add(1);
                }
                visible = visible.saturating_add(1);
            }
        }
        if visible > shown {
            push_line(
                &mut result.text,
                &text::more_diagnostics(visible.saturating_sub(shown)),
            );
        }
        if self.unresolved > 0 {
            push_line(
                &mut result.text,
                &text::unresolved_diagnostics(self.unresolved),
            );
        }
        if cancelled.load(Ordering::Acquire) {
            push_line(&mut result.text, text::DIAGNOSTICS_CANCELLED);
        }
        result
    }
}

type LocationKey = (String, Option<String>, String, Option<(u64, u64, u64, u64)>);

fn location_key(d: &Diagnostic) -> LocationKey {
    (
        d.severity.clone(),
        d.code.clone(),
        d.message.clone(),
        d.primary
            .as_ref()
            .map(|s| (s.start.line, s.start.column, s.end.line, s.end.column)),
    )
}

/// 空でない行を区切りながら追加する。
pub fn push_line(output: &mut String, line: &str) {
    if !output.is_empty() {
        output.push('\n');
    }
    output.push_str(line);
}

pub(super) fn render_one_line(
    d: &Diagnostic,
    file: Option<&DiagnosticFile>,
    sources: &impl DiagnosticSources,
    cancelled: &AtomicBool,
) -> String {
    let location = d.primary.as_ref().zip(file).and_then(|(span, file)| {
        file.display.as_ref().map(|path| {
            text::diagnostic_location(
                Path::new(&display::one_line(&path.to_string_lossy())),
                &span.start,
            )
        })
    });
    let mut output = text::diagnostic_header(
        &display::one_line(&d.severity),
        d.code.as_deref().map(display::one_line).as_deref(),
        location.as_deref(),
        &display::one_line(&d.message),
    );
    if let Some((span, file)) = d.primary.as_ref().zip(file)
        && let Some(line) = sources.line(&file.path, span.start.line)
    {
        let truncated = line.truncated;
        let line = line.text;
        push_line(
            &mut output,
            &text::diagnostic_source(span.start.line, &display::source_line(&line)),
        );
        let start = usize::try_from(span.start.column.saturating_sub(1)).unwrap_or(usize::MAX);
        let len = line.chars().count();
        if start <= len && (start < 2000 || !truncated) {
            let end = if span.end.line > span.start.line {
                len
            } else {
                usize::try_from(span.end.column.saturating_sub(1))
                    .unwrap_or(usize::MAX)
                    .min(len)
            };
            let before = display::source_line(&line.chars().take(start).collect::<String>());
            let through = display::source_line(&line.chars().take(end).collect::<String>());
            let prefix: String = before
                .chars()
                .map(|ch| if ch == '\t' { '\t' } else { ' ' })
                .collect();
            let width = through
                .chars()
                .count()
                .saturating_sub(before.chars().count())
                .max(1);
            push_line(
                &mut output,
                &text::diagnostic_marker(
                    span.start.line,
                    &prefix,
                    width,
                    &display::one_line(&span.label),
                ),
            );
        }
    }
    for span in d.secondary.iter().take(20) {
        if cancelled.load(Ordering::Acquire) {
            break;
        }
        if let Some(path) = sources.resolve(&span.file).display {
            push_line(
                &mut output,
                &text::diagnostic_secondary(
                    &display::one_line(&span.label),
                    Path::new(&display::one_line(&path.to_string_lossy())),
                    &span.start,
                ),
            );
        }
    }
    if d.secondary.omitted > 0 {
        push_line(&mut output, &text::omitted_secondary(d.secondary.omitted));
    }
    for note in d.notes.iter() {
        if cancelled.load(Ordering::Acquire) {
            break;
        }
        push_line(
            &mut output,
            &text::diagnostic_note(&display::one_line(note)),
        );
    }
    if d.notes.omitted > 0 {
        push_line(&mut output, &text::omitted_notes(d.notes.omitted));
    }
    let mut edits = 0usize;
    let mut omitted = 0usize;
    for help in d.helps.iter() {
        if cancelled.load(Ordering::Acquire) {
            break;
        }
        omitted = omitted.saturating_add(help.edits.omitted);
        push_line(
            &mut output,
            &text::diagnostic_help(&display::one_line(&help.message)),
        );
        for edit in help.edits.iter() {
            if cancelled.load(Ordering::Acquire) {
                break;
            }
            if edits >= 20 {
                omitted = omitted.saturating_add(1);
                continue;
            }
            edits = edits.saturating_add(1);
            if let Some(path) = sources.resolve(&edit.file).display {
                let same = file.and_then(|f| f.display.as_ref()) == Some(&path);
                let replacement = display::quoted(&edit.replacement);
                push_line(
                    &mut output,
                    &text::diagnostic_edit(
                        (!same)
                            .then(|| display::one_line(&path.to_string_lossy()))
                            .as_deref()
                            .map(Path::new),
                        &edit.start,
                        &edit.end,
                        &replacement,
                    ),
                );
            }
        }
    }
    if d.helps.omitted > 0 {
        push_line(&mut output, &text::omitted_helps(d.helps.omitted));
    }
    if omitted > 0 {
        push_line(&mut output, &text::omitted_edits(omitted));
    }
    output
}

#[cfg(test)]
mod tests;

/// 診断の構造体と、保持している文字列・配列の確保量。
pub(super) fn retained_bytes(d: &Diagnostic) -> usize {
    fn span(s: &Span) -> usize {
        s.file.capacity().saturating_add(s.label.capacity())
    }
    let mut bytes = std::mem::size_of::<Diagnostic>()
        .saturating_add(d.kind.capacity())
        .saturating_add(d.severity.capacity())
        .saturating_add(d.code.as_ref().map_or(0, String::capacity))
        .saturating_add(d.message.capacity());
    bytes = bytes
        .saturating_add(d.secondary.storage_bytes())
        .saturating_add(d.notes.storage_bytes())
        .saturating_add(d.helps.storage_bytes());
    for s in d.primary.iter().chain(d.secondary.iter()) {
        bytes = bytes.saturating_add(span(s));
    }
    for note in d.notes.iter() {
        bytes = bytes.saturating_add(note.capacity());
    }
    for help in d.helps.iter() {
        bytes = bytes
            .saturating_add(help.message.capacity())
            .saturating_add(help.edits.storage_bytes());
        for edit in help.edits.iter() {
            bytes = bytes
                .saturating_add(edit.file.capacity())
                .saturating_add(edit.replacement.capacity());
        }
    }
    bytes
}
impl<T> DiagnosticItems<T> {
    pub(super) fn storage_bytes(&self) -> usize {
        self.values
            .capacity()
            .saturating_mul(std::mem::size_of::<T>())
    }
}
/// 読まない利用者のソースを、診断と同じ予算・BH-029 の判定と内容の照合で数える。
pub(crate) fn unread_imports(
    sources: &[super::report::ApprovalSource],
    resolver: &impl DiagnosticSources,
    cancelled: &AtomicBool,
) -> usize {
    let state = Mutex::new(ResolutionState {
        files: BTreeMap::new(),
        limited: BTreeSet::new(),
        remaining: MAX_EXCLUSION_WORK,
    });
    let cached = CachedSources {
        source: resolver,
        files: &state,
        cancelled,
        refused: std::cell::Cell::new(false),
    };
    let mut seen = BTreeSet::new();
    sources
        .iter()
        .filter(|s| {
            seen.insert((
                s.display_path.as_str(),
                s.content_hash.algorithm.as_str(),
                s.content_hash.value.as_str(),
            ))
        })
        .enumerate()
        .filter(|(n, s)| {
            if *n >= MAX_RESOLVED_DIAGNOSTICS
                || s.content_hash.algorithm != "sha256"
                || s.content_hash.value.len() != 64
                || !s
                    .content_hash
                    .value
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            {
                return true;
            }
            let resolved = cached.resolve(&s.display_path);
            // BH-029 の表示パスが読める囮と一致しても、検査した元の内容とは
            // 区別する（設計書 04-01「実行の前の検査の報告」）。
            resolved.display.is_none()
                || resolver.content_hash(&resolved.path, cancelled).as_deref()
                    != Some(s.content_hash.value.as_str())
        })
        .count()
}
