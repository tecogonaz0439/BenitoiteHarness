//! 診断の行を検証し、読めるソースだけを添えて返す（設計書 04-01「終了状態と診断の受け渡し」）。

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};

use serde::{Deserialize, Deserializer};

use super::text;

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
    pub edits: Vec<Edit>,
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
    pub secondary: Vec<Span>,
    pub notes: Vec<String>,
    pub helps: Vec<Help>,
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
        let diagnostic = serde_json::from_slice::<serde_json::Value>(line)
            .ok()
            .filter(serde_json::Value::is_object)
            .and_then(|value| serde_json::from_value::<Diagnostic>(value).ok());
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

/// read と同じ判定を通した実のパス。開けない場合も存在の判定を保持する。
#[derive(Clone, Debug)]
pub struct DiagnosticFile {
    pub path: PathBuf,
    pub display: Option<PathBuf>,
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
    /// 相対のパスを処理系の作業ディレクトリから解き、read の可否を判定する。
    fn resolve(&self, file: &str) -> DiagnosticFile;
    /// 記述子を安全に開き直して一行だけを読む。読んだ版を記録しない。
    fn line(&self, file: &Path, line: u64) -> Option<SourceLine>;
}

struct ResolvedDiagnostic {
    diagnostic: Diagnostic,
    file: Option<DiagnosticFile>,
}

/// 同じ診断と、BH-029 の存在しない表示パスの重複を除いた診断。
pub struct PreparedDiagnostics {
    diagnostics: Vec<ResolvedDiagnostic>,
    files: Mutex<BTreeMap<String, DiagnosticFile>>,
}

// 同じ表示パスを何度も開かず、補助位置と置き換えにも同じ判定を使う。
// キャッシュは一回の検査の結果だけが持ち、次の書き込みへ持ち越さない。
struct CachedSources<'a, S> {
    source: &'a S,
    files: &'a Mutex<BTreeMap<String, DiagnosticFile>>,
}
impl<S: DiagnosticSources> DiagnosticSources for CachedSources<'_, S> {
    fn resolve(&self, file: &str) -> DiagnosticFile {
        let mut files = self.files.lock().unwrap_or_else(PoisonError::into_inner);
        files
            .entry(file.into())
            .or_insert_with(|| self.source.resolve(file))
            .clone()
    }
    fn line(&self, file: &Path, line: u64) -> Option<SourceLine> {
        self.source.line(file, line)
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
    /// 全体を解決してから重複を除く。存在する位置が後で出ても同じ扱いにする。
    pub fn new(diagnostics: Vec<Diagnostic>, sources: &impl DiagnosticSources) -> Self {
        let files = Mutex::new(BTreeMap::new());
        let cached = CachedSources {
            source: sources,
            files: &files,
        };
        let resolved: Vec<_> = diagnostics
            .into_iter()
            .map(|diagnostic| {
                let file = diagnostic
                    .primary
                    .as_ref()
                    .map(|span| cached.resolve(&span.file));
                ResolvedDiagnostic { diagnostic, file }
            })
            .collect();
        let mut relative_paths: BTreeMap<LocationKey, BTreeSet<PathBuf>> = BTreeMap::new();
        for entry in &resolved {
            if let Some(relative) = entry.file.as_ref().and_then(|file| file.relative.as_ref()) {
                relative_paths
                    .entry(location_key(&entry.diagnostic))
                    .or_default()
                    .insert(relative.clone());
            }
        }
        // 実在する偽の表示先も重複になる。先頭を落とした側へ寄せてから、
        // 存在しない表示先の回避を当てる（BH-029、設計書 04-01）。
        let resolved: Vec<_> = resolved
            .into_iter()
            .filter(|entry| {
                let Some(relative) = entry.file.as_ref().and_then(|file| file.relative.as_ref())
                else {
                    return true;
                };
                let Some(paths) = relative_paths.get(&location_key(&entry.diagnostic)) else {
                    return true;
                };
                !(1..relative.components().count()).any(|skip| {
                    paths.contains(&relative.components().skip(skip).collect::<PathBuf>())
                })
            })
            .collect();
        let existing: BTreeSet<_> = resolved
            .iter()
            .filter(|d| d.file.as_ref().is_some_and(|f| f.exists))
            .map(|d| location_key(&d.diagnostic))
            .collect();
        let mut seen = BTreeSet::new();
        let mut diagnostics = Vec::new();
        for resolved in resolved {
            let d = &resolved.diagnostic;
            if resolved.file.as_ref().is_some_and(|f| !f.exists)
                && existing.contains(&location_key(d))
            {
                continue;
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
        Self { diagnostics, files }
    }

    /// only があるときは、そのファイルと位置のない診断だけを本文にする。
    pub fn render(
        &self,
        sources: &impl DiagnosticSources,
        only: Option<&Path>,
    ) -> RenderedDiagnostics {
        let cached = CachedSources {
            source: sources,
            files: &self.files,
        };
        let mut result = RenderedDiagnostics {
            text: String::new(),
            other: 0,
            unread: 0,
            selected: 0,
            failed: false,
        };
        let mut shown = 0usize;
        let mut visible = 0usize;
        for entry in &self.diagnostics {
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
                visible = visible.saturating_add(1);
                if shown < 20 {
                    push_line(
                        &mut result.text,
                        &render_one(&entry.diagnostic, entry.file.as_ref(), &cached),
                    );
                    shown = shown.saturating_add(1);
                }
            }
        }
        if visible > shown {
            push_line(
                &mut result.text,
                &text::more_diagnostics(visible.saturating_sub(shown)),
            );
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

fn render_one(
    d: &Diagnostic,
    file: Option<&DiagnosticFile>,
    sources: &impl DiagnosticSources,
) -> String {
    let location = d.primary.as_ref().zip(file).and_then(|(span, file)| {
        file.display
            .as_ref()
            .map(|path| text::diagnostic_location(path, &span.start))
    });
    let mut output = text::diagnostic_header(
        &d.severity,
        d.code.as_deref(),
        location.as_deref(),
        &d.message,
    );
    if let Some((span, file)) = d.primary.as_ref().zip(file)
        && let Some(line) = sources.line(&file.path, span.start.line)
    {
        let truncated = line.truncated;
        let line = line.text;
        push_line(
            &mut output,
            &text::diagnostic_source(span.start.line, &line),
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
            let prefix: String = line
                .chars()
                .take(start)
                .map(|ch| if ch == '\t' { '\t' } else { ' ' })
                .collect();
            let width = end.saturating_sub(start).max(1);
            push_line(
                &mut output,
                &text::diagnostic_marker(span.start.line, &prefix, width, &span.label),
            );
        }
    }
    for span in d.secondary.iter().take(20) {
        if let Some(path) = sources.resolve(&span.file).display {
            push_line(
                &mut output,
                &text::diagnostic_secondary(&span.label, &path, &span.start),
            );
        }
    }
    if d.secondary.len() > 20 {
        push_line(
            &mut output,
            &text::omitted_secondary(d.secondary.len().saturating_sub(20)),
        );
    }
    for note in &d.notes {
        push_line(&mut output, &text::diagnostic_note(note));
    }
    let mut edits = 0usize;
    let mut omitted = 0usize;
    for help in &d.helps {
        push_line(&mut output, &text::diagnostic_help(&help.message));
        for edit in &help.edits {
            if edits >= 20 {
                omitted = omitted.saturating_add(1);
                continue;
            }
            edits = edits.saturating_add(1);
            if let Some(path) = sources.resolve(&edit.file).display {
                let same = file.and_then(|f| f.display.as_ref()) == Some(&path);
                let replacement = serde_json::Value::String(edit.replacement.clone()).to_string();
                push_line(
                    &mut output,
                    &text::diagnostic_edit(
                        (!same).then_some(path.as_path()),
                        &edit.start,
                        &edit.end,
                        &replacement,
                    ),
                );
            }
        }
    }
    if omitted > 0 {
        push_line(&mut output, &text::omitted_edits(omitted));
    }
    output
}

#[cfg(test)]
mod tests;
