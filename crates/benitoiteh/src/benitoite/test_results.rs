//! テストの JSON Lines を上限付きで読み、読まないソースの情報を伏せる（設計書 04-01）。
use super::diagnostics::{
    self, CachedSources, Diagnostic, DiagnosticItems, DiagnosticSources, ResolutionState, Span,
};
use super::display::one_line;
use super::report::decode::{Fields, Object, Scalar};
use crate::tools::{process_output::Output, text};
use serde::{
    Deserialize, Deserializer,
    de::{IgnoredAny, MapAccess, Visitor},
};
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
    io::{self, Write},
    sync::{
        Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

const MAX_BYTES: usize = 16 * 1024 * 1024;
const MAX_FAILURES: usize = 1000;

struct Tail(Output);
impl Default for Tail {
    fn default() -> Self {
        Self(Output::memory(4096, 100))
    }
}
impl<'de> Deserialize<'de> for Tail {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct TailVisitor;
        impl Visitor<'_> for TailVisitor {
            type Value = Tail;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(super::text::REPORT_JSON)
            }
            fn visit_str<E: serde::de::Error>(self, s: &str) -> Result<Tail, E> {
                let mut tail = Tail::default();
                for chunk in s.as_bytes().chunks(4096) {
                    tail.0.write_all(chunk).map_err(E::custom)?;
                }
                Ok(tail)
            }
        }
        d.deserialize_str(TailVisitor)
    }
}

// 全欄を直接読む。runtime の診断も同じオブジェクトから取り出し、中間の木を作らない。
#[derive(Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct Failure {
    reason: Option<String>,
    message: Option<String>,
    primary: Option<Span>,
    left: Option<String>,
    right: Option<String>,
    die: Option<Diagnostic>,
    kind: Option<String>,
    severity: Option<String>,
    code: Option<String>,
    secondary: Option<DiagnosticItems<Span>>,
    notes: Option<DiagnosticItems<String>>,
    helps: Option<DiagnosticItems<diagnostics::Help>>,
}
impl Failure {
    fn diagnostic(self) -> Option<Diagnostic> {
        Some(Diagnostic {
            kind: self.kind?,
            severity: self.severity?,
            code: self.code,
            message: self.message?,
            primary: self.primary,
            secondary: self.secondary?,
            notes: self.notes?,
            helps: self.helps?,
        })
    }
}
#[derive(Default)]
struct Row {
    kind: Scalar,
    file: Scalar,
    name: Scalar,
    outcome: Scalar,
    location: Option<Span>,
    failure: Option<Failure>,
    stdout: Option<Tail>,
    stderr: Option<Tail>,
    passed: Scalar,
    failed: Scalar,
    files_not_run: Scalar,
    interrupted: Scalar,
}
impl Fields for Row {
    fn field<'de, A: MapAccess<'de>>(&mut self, key: &str, map: &mut A) -> Result<(), A::Error> {
        match key {
            "kind" => self.kind = map.next_value()?,
            "file" => self.file = map.next_value()?,
            "name" => self.name = map.next_value()?,
            "outcome" => self.outcome = map.next_value()?,
            "location" => self.location = map.next_value()?,
            "failure" => self.failure = map.next_value()?,
            "stdout" => self.stdout = map.next_value()?,
            "stderr" => self.stderr = map.next_value()?,
            "passed" => self.passed = map.next_value()?,
            "failed" => self.failed = map.next_value()?,
            "filesNotRun" => self.files_not_run = map.next_value()?,
            "interrupted" => self.interrupted = map.next_value()?,
            _ => {
                let _ignored = map.next_value::<IgnoredAny>()?;
            }
        }
        Ok(())
    }
}
struct Summary {
    passed: u64,
    failed: u64,
    files_not_run: u64,
    interrupted: bool,
}
impl Summary {
    fn from_row(row: &Row) -> Option<Self> {
        Some(Self {
            passed: row.passed.unsigned()?,
            failed: row.failed.unsigned()?,
            files_not_run: row.files_not_run.unsigned()?,
            interrupted: row.interrupted.boolean()?,
        })
    }
}

pub(crate) struct Results {
    pub(crate) other: Output,
    line: Vec<u8>,
    oversized: bool,
    forwarding: bool,
    failures: Vec<Row>,
    retained_bytes: usize,
    failed_seen: usize,
    omitted: usize,
    too_long: usize,
    invalid: usize,
    summary: Option<Summary>,
    summaries: usize,
    pub(crate) closed: bool,
}
impl Results {
    pub(crate) fn new(other: Output) -> Self {
        Self {
            other,
            line: Vec::new(),
            oversized: false,
            forwarding: false,
            failures: Vec::new(),
            retained_bytes: 0,
            failed_seen: 0,
            omitted: 0,
            too_long: 0,
            invalid: 0,
            summary: None,
            summaries: 0,
            closed: false,
        }
    }
    pub(crate) fn end_line(&mut self) -> io::Result<()> {
        if self.oversized {
            self.too_long = self.too_long.saturating_add(1);
        } else if !self.forwarding && !self.line.is_empty() {
            let row = serde_json::from_slice::<Object<Row>>(&self.line)
                .ok()
                .and_then(|row| row.0);
            match row {
                Some(row) if row.kind.string() == Some("testSummary") => {
                    self.summaries = self.summaries.saturating_add(1);
                    self.summary = Summary::from_row(&row);
                }
                Some(row) if row.kind.string() == Some("test") => {
                    if row.outcome.string() == Some("failed") {
                        self.failed_seen = self.failed_seen.saturating_add(1);
                        let retained = self.retained_bytes.saturating_add(row.retained_bytes());
                        if retained <= MAX_BYTES && self.failed_seen <= MAX_FAILURES {
                            self.retained_bytes = retained;
                            self.failures.push(row);
                        } else {
                            self.omitted = self.omitted.saturating_add(1);
                        }
                    }
                }
                None if self.line.iter().copied().find(|b| !b.is_ascii_whitespace())
                    == Some(b'{') =>
                {
                    self.invalid = self.invalid.saturating_add(1);
                }
                _ => self.other.write_all(&self.line)?,
            }
        }
        self.line.clear();
        self.oversized = false;
        self.forwarding = false;
        Ok(())
    }
    pub(crate) fn summary(&self, code: i32, completed: bool) -> (String, bool) {
        let Some(s) = &self.summary else {
            return (text::TEST_NO_SUMMARY.into(), false);
        };
        let passed = self.summaries == 1
            && code == 0
            && s.failed == 0
            && s.files_not_run == 0
            && !s.interrupted
            && completed
            && self.invalid == 0
            && self.too_long == 0;
        let mut body = text::tests_summary(s.passed, s.failed);
        if passed {
            diagnostics::push_line(
                &mut body,
                if s.passed == 0 && s.failed == 0 {
                    text::TEST_EMPTY
                } else {
                    text::TEST_PASSED
                },
            );
        }
        if s.files_not_run != 0 {
            diagnostics::push_line(&mut body, &text::test_files_not_run(s.files_not_run));
        }
        if s.interrupted {
            diagnostics::push_line(&mut body, text::TEST_INTERRUPTED);
        }
        (body, passed)
    }
    pub(crate) fn render(
        &mut self,
        sources: &impl DiagnosticSources,
        cancelled: &AtomicBool,
    ) -> String {
        let files = Mutex::new(ResolutionState {
            files: BTreeMap::new(),
            limited: BTreeSet::new(),
            remaining: diagnostics::MAX_EXCLUSION_WORK,
        });
        let cached = CachedSources {
            source: sources,
            files: &files,
            cancelled,
            refused: std::cell::Cell::new(false),
        };
        let mut body = String::new();
        let (mut unread, mut more, mut shown) = (0usize, 0usize, 0usize);
        let mut pending = self.failures.len();
        // 複数の見出しには合わせて10 KiB、単独なら三つの欄に各8 KiBまで配る。
        // 欄と診断に4 KiB、出力に合わせて32 KiBを配る。
        // 道具全体の64 KiBの制限で、後のFAILED行を落とさない（設計書 01-03）。
        let headings = self.failures.len().clamp(1, 20);
        let heading_part_budget = if headings == 1 {
            8000
        } else {
            10240usize
                .checked_div(headings)
                .unwrap_or(512)
                .saturating_sub(128)
                / 3
        };
        let mut details_budget = 4 * 1024usize;
        let mut output_budget = 32 * 1024usize;
        for row in std::mem::take(&mut self.failures) {
            if cancelled.load(Ordering::Acquire) {
                break;
            }
            pending = pending.saturating_sub(1);
            let Some(failure) = row.failure else {
                unread = unread.saturating_add(1);
                continue;
            };
            let Some((file, location)) = row.file.string().zip(row.location.as_ref()) else {
                unread = unread.saturating_add(1);
                continue;
            };
            // 一つの予算を本文と診断で共有し、BH-029 の誤ったパスも read と同じく拒む。
            let paths = [
                Some(file),
                Some(location.file.as_str()),
                failure.primary.as_ref().map(|s| s.file.as_str()),
                failure
                    .die
                    .as_ref()
                    .and_then(|d| d.primary.as_ref())
                    .map(|s| s.file.as_str()),
            ];
            if paths
                .into_iter()
                .flatten()
                .any(|path| cached.resolve(path).display.is_none())
            {
                unread = unread.saturating_add(1);
                continue;
            }
            if shown >= 20 {
                more = more.saturating_add(1);
                continue;
            }
            if cancelled.load(Ordering::Acquire) {
                pending = pending.saturating_add(1);
                break;
            }
            shown = shown.saturating_add(1);
            let span = if failure.reason.as_deref() == Some("assert") {
                failure.primary.as_ref().unwrap_or(location)
            } else {
                location
            };
            let resolved = cached.resolve(
                if failure.reason.as_deref() == Some("assert") && failure.primary.is_some() {
                    &span.file
                } else {
                    file
                },
            );
            let Some(display) = &resolved.display else {
                unread = unread.saturating_add(1);
                continue;
            };
            let reason = failure
                .reason
                .as_deref()
                .filter(|r| ["assert", "exit", "error", "runtime"].contains(r))
                .unwrap_or("error");
            diagnostics::push_line(
                &mut body,
                &text::test_failed(
                    std::path::Path::new(&super::display::line_with_budget(
                        &display.to_string_lossy(),
                        heading_part_budget,
                    )),
                    span.start.line,
                    span.start.column,
                    &super::display::quoted_with_budget(
                        row.name.string().unwrap_or_default(),
                        heading_part_budget,
                    ),
                    reason,
                    &super::display::quoted_with_budget(
                        failure.message.as_deref().unwrap_or_default(),
                        heading_part_budget,
                    ),
                ),
            );
            let mut details = String::new();
            match reason {
                "assert" => {
                    for (side, value) in [
                        ("left", failure.left.as_deref()),
                        ("right", failure.right.as_deref()),
                    ] {
                        if let Some(value) = value {
                            diagnostics::push_line(
                                &mut details,
                                &text::test_value(side, &one_line(value)),
                            );
                        }
                    }
                }
                "exit" => {
                    if let Some(diagnostic) = &failure.die {
                        let file = diagnostic.primary.as_ref().map(|s| cached.resolve(&s.file));
                        diagnostics::push_line(
                            &mut details,
                            &diagnostics::render_one_line(
                                diagnostic,
                                file.as_ref(),
                                &cached,
                                cancelled,
                            ),
                        );
                    }
                }
                "runtime" => {
                    if let Some(diagnostic) = failure.diagnostic() {
                        let file = diagnostic.primary.as_ref().map(|s| cached.resolve(&s.file));
                        diagnostics::push_line(
                            &mut details,
                            &diagnostics::render_one_line(
                                &diagnostic,
                                file.as_ref(),
                                &cached,
                                cancelled,
                            ),
                        );
                    }
                }
                _ => {}
            }
            if !details.is_empty() && details_budget > 0 {
                let kept = crate::tools::truncate::truncate(
                    &details,
                    crate::tools::truncate::Side::Head,
                    usize::MAX,
                    details_budget.saturating_sub(1),
                    text::TEST_DETAILS_OMITTED,
                )
                .text;
                details_budget = details_budget.saturating_sub(kept.len().saturating_add(1));
                diagnostics::push_line(&mut body, &kept);
            }
            if !details.is_empty() && details_budget == 0 {
                diagnostics::push_line(&mut body, text::TEST_DETAILS_OMITTED);
            }
            for (label, tail) in [
                (text::RUN_STDOUT, row.stdout),
                (text::RUN_STDERR, row.stderr),
            ] {
                if let Some(tail) = tail {
                    let (value, first, total, truncated) = tail.0.tail();
                    if value.is_empty() && !truncated {
                        continue;
                    }
                    let mut section = label.to_owned();
                    diagnostics::push_line(&mut section, &value);
                    if truncated {
                        diagnostics::push_line(
                            &mut section,
                            &text::test_output_truncated(first, total),
                        );
                    }
                    if section.len().saturating_add(1) <= output_budget {
                        output_budget =
                            output_budget.saturating_sub(section.len().saturating_add(1));
                        diagnostics::push_line(&mut body, &section);
                    } else {
                        output_budget = 0;
                        diagnostics::push_line(&mut body, text::TEST_OUTPUT_OMITTED);
                    }
                }
            }
        }
        if unread > 0 {
            diagnostics::push_line(&mut body, &text::tests_unread(unread));
        }
        if more > 0 {
            diagnostics::push_line(&mut body, &text::tests_more(more));
        }
        if self.omitted > 0 {
            diagnostics::push_line(&mut body, &text::tests_omitted(self.omitted));
        }
        if self.too_long > 0 {
            diagnostics::push_line(&mut body, &text::tests_long(self.too_long));
        }
        if self.invalid > 0 {
            diagnostics::push_line(&mut body, &text::tests_invalid(self.invalid));
        }
        if pending > 0 {
            diagnostics::push_line(&mut body, &text::tests_uninspected(pending));
        }
        if cancelled.load(Ordering::Acquire) {
            diagnostics::push_line(&mut body, super::text::DIAGNOSTICS_CANCELLED);
        }
        body
    }
}
impl Write for Results {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if self.closed {
            return Ok(bytes.len());
        }
        for part in bytes.split_inclusive(|b| *b == b'\n') {
            if self.forwarding {
                self.other.write_all(part)?;
            } else if !self.oversized {
                if self.line.len().saturating_add(part.len()) <= MAX_BYTES {
                    self.line.extend_from_slice(part);
                } else if self
                    .line
                    .iter()
                    .chain(part.iter())
                    .copied()
                    .find(|b| !b.is_ascii_whitespace())
                    == Some(b'{')
                {
                    self.line.clear();
                    self.oversized = true;
                } else {
                    self.other.write_all(&self.line)?;
                    self.line.clear();
                    self.other.write_all(part)?;
                    self.forwarding = true;
                }
            }
            if part.ends_with(b"\n") {
                self.end_line()?;
            }
        }
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
#[cfg(test)]
mod tests;

impl Row {
    fn retained_bytes(&self) -> usize {
        fn span(s: &Span) -> usize {
            s.file.capacity().saturating_add(s.label.capacity())
        }
        let mut n = std::mem::size_of::<Self>();
        for value in [&self.kind, &self.file, &self.name, &self.outcome] {
            if let Scalar::String(value) = value {
                n = n.saturating_add(value.capacity());
            }
        }
        if let Some(s) = &self.location {
            n = n.saturating_add(span(s));
        }
        if let Some(f) = &self.failure {
            n = n.saturating_add(std::mem::size_of::<Failure>());
            for s in [
                &f.reason,
                &f.message,
                &f.left,
                &f.right,
                &f.kind,
                &f.severity,
                &f.code,
            ]
            .into_iter()
            .flatten()
            {
                n = n.saturating_add(s.capacity());
            }
            if let Some(s) = &f.primary {
                n = n.saturating_add(span(s));
            }
            if let Some(d) = &f.die {
                n = n.saturating_add(diagnostics::retained_bytes(d));
            }
            if let Some(spans) = &f.secondary {
                n = n.saturating_add(spans.storage_bytes());
                for s in spans.iter() {
                    n = n.saturating_add(span(s));
                }
            }
            if let Some(notes) = &f.notes {
                n = n.saturating_add(notes.storage_bytes());
                for s in notes.iter() {
                    n = n.saturating_add(s.capacity());
                }
            }
            if let Some(helps) = &f.helps {
                n = n.saturating_add(helps.storage_bytes());
                for h in helps.iter() {
                    n = n
                        .saturating_add(h.message.capacity())
                        .saturating_add(h.edits.storage_bytes());
                    for e in h.edits.iter() {
                        n = n
                            .saturating_add(e.file.capacity())
                            .saturating_add(e.replacement.capacity());
                    }
                }
            }
        }
        for tail in [&self.stdout, &self.stderr].into_iter().flatten() {
            n = n.saturating_add(tail.0.retained_bytes());
        }
        n
    }
}
