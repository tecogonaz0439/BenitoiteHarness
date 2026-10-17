//! 検索の通知を重複なく集計し、本文を圧迫しない量で示す（設計書 01-03「glob」「grep」「出力の切り詰め」）。

use super::{text, truncate};
use std::collections::{BTreeSet, HashSet};
use std::path::{Path, PathBuf};

const MAX_NOTICE_BYTES: usize = 4096;
const SAMPLE_BYTES: usize = 128;

#[derive(Default)]
struct Group {
    seen: HashSet<PathBuf>,
    first: Vec<(PathBuf, Option<&'static str>)>,
}

impl Group {
    fn add(&mut self, path: &Path, reason: Option<&'static str>) {
        if self.seen.insert(path.to_path_buf()) && self.first.len() < 5 {
            self.first.push((path.to_path_buf(), reason));
        }
    }

    fn paths(&self, display: &impl Fn(&Path) -> String) -> String {
        self.first
            .iter()
            .map(|(path, reason)| {
                let path = display(path);
                let path = if path.len() > SAMPLE_BYTES {
                    format!(
                        "{}...",
                        truncate::prefix(&path, SAMPLE_BYTES.saturating_sub(3))
                    )
                } else {
                    path
                };
                match reason {
                    Some(reason) => format!("{path} ({reason})"),
                    None => path,
                }
            })
            .collect::<Vec<_>>()
            .join(", ")
    }
}

#[derive(Default)]
pub(super) struct Notices {
    ignores: Group,
    directories: Group,
    files: Group,
    stops: Group,
    reasons: BTreeSet<&'static str>,
}

impl Notices {
    pub(super) fn ignore(&mut self, path: &Path) {
        self.ignores.add(path, None);
    }
    pub(super) fn directory(&mut self, path: &Path) {
        self.directories.add(path, None);
    }
    pub(super) fn file(&mut self, path: &Path, reason: &'static str) {
        self.files.add(path, Some(reason));
    }
    pub(super) fn stop(&mut self, path: &Path, reason: &'static str) {
        self.stops.add(path, None);
        self.reasons.insert(reason);
    }

    pub(super) fn render(&self, display: impl Fn(&Path) -> String) -> String {
        let mut lines = Vec::new();
        if !self.ignores.seen.is_empty() {
            lines.push(text::skipped_ignores(
                self.ignores.seen.len(),
                &self.ignores.paths(&display),
            ));
        }
        if !self.directories.seen.is_empty() {
            lines.push(text::skipped_directories(
                self.directories.seen.len(),
                &self.directories.paths(&display),
            ));
        }
        if !self.files.seen.is_empty() {
            lines.push(text::skipped_files(
                self.files.seen.len(),
                &self.files.paths(&display),
            ));
        }
        if !self.stops.seen.is_empty() {
            lines.push(text::stopped_search(
                &self.reasons.iter().copied().collect::<Vec<_>>().join(", "),
                self.stops.seen.len(),
                &self.stops.paths(&display),
            ));
        }
        // 四種類それぞれ五つの名前だけを示す。名前の可視化後にも長さを限り、
        // 共通の cap_middle が照合行を消す原因にしない（設計書 01-03「出力の切り詰め」）。
        truncate::prefix(&lines.join("\n"), MAX_NOTICE_BYTES).to_owned()
    }
}
