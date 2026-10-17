//! 変わる情報をシステムプロンプトから分け、差分だけを追記する（設計書 01-04「環境の情報」）。

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::config::Mode;

use super::instructions::{InstructionFile, ProjectInstructions};
use super::{ContextError, text};
use crate::session::SkippedInstruction;

/// 最初の依頼の前、または変化の後に置く環境の情報。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Environment {
    project: String,
    os: &'static str,
    date_utc: String,
    mode: Mode,
    writable_roots: Vec<String>,
    network_allowed: bool,
    benitoite_version: &'static str,
    instructions: ProjectInstructions,
}

impl Environment {
    /// 方針を決めた呼び出し元から、書ける場所と子プロセスの IP 通信の可否を受け取る。
    ///
    /// now は UTC の日付に変換する。P1 の処理系の版は常に not configured とする。
    pub fn capture(
        project: &Path,
        mode: Mode,
        writable_roots: &[PathBuf],
        network_allowed: bool,
        now: SystemTime,
    ) -> Result<Self, ContextError> {
        Ok(Self {
            project: absolute_path(project)?,
            os: std::env::consts::OS,
            date_utc: utc_date(now)?,
            mode,
            writable_roots: writable_roots
                .iter()
                .map(|path| absolute_path(path))
                .collect::<Result<_, _>>()?,
            network_allowed,
            benitoite_version: text::NOT_CONFIGURED,
            instructions: ProjectInstructions::default(),
        })
    }

    /// 起動時に集めた指示と除外理由を環境の情報に加える。
    pub fn with_project_instructions(mut self, instructions: ProjectInstructions) -> Self {
        self.instructions = instructions;
        self
    }

    /// 開始の通知に使う、読んだ指示の一覧。
    pub fn project_instructions(&self) -> &[InstructionFile] {
        &self.instructions.files
    }

    /// 開始の通知に使う、読まなかった指示とその理由。
    pub fn skipped_instructions(&self) -> &[SkippedInstruction] {
        &self.instructions.skipped
    }

    /// セッションの最初の利用者の発言に含める全文。
    pub fn render(&self) -> String {
        self.render_fields(None).unwrap_or_default()
    }

    /// 前回から変わった欄だけを同じ形で返す。変化がなければ何も追加しない。
    pub fn changes_since(&self, previous: &Self) -> Option<String> {
        self.render_fields(Some(previous))
    }

    fn render_fields(&self, previous: Option<&Self>) -> Option<String> {
        let fields = self.fields();
        let previous_fields = previous.map(Self::fields);
        let mut lines = Vec::new();
        for (name, value) in fields {
            if previous_fields.as_ref().is_some_and(|old| {
                old.iter()
                    .any(|(key, previous)| *key == name && previous == &value)
            }) {
                continue;
            }
            if name == "project_instructions" {
                lines.push(value);
            } else {
                lines.push(format!("<{name}>{}</{name}>", escape(&value)));
            }
        }
        if lines.is_empty() {
            None
        } else {
            Some(format!(
                "<environment>\n{}\n</environment>\n",
                lines.join("\n")
            ))
        }
    }

    fn fields(&self) -> Vec<(&'static str, String)> {
        let mut fields = vec![
            ("project_directory", self.project.clone()),
            ("os", self.os.into()),
            ("date_utc", self.date_utc.clone()),
            ("mode", text::mode(self.mode).into()),
            // JSON の配列で、空の一覧による読み取り専用への変化も表せる。
            (
                "writable_roots",
                serde_json::json!(self.writable_roots).to_string(),
            ),
            ("network", text::network(self.network_allowed).into()),
            ("benitoite_version", self.benitoite_version.into()),
        ];
        fields.extend(self.instructions.files.iter().map(|file| {
            (
                "project_instructions",
                format!(
                    "<project_instructions path=\"{}\">{}</project_instructions>",
                    escape_attribute(&file.path.to_string_lossy()),
                    escape(&file.contents),
                ),
            )
        }));
        fields
    }
}

fn absolute_path(path: &Path) -> Result<String, ContextError> {
    if !path.is_absolute() {
        return Err(ContextError::InvalidPath);
    }
    path.to_str()
        .map(str::to_owned)
        .ok_or(ContextError::InvalidPath)
}

fn escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn escape_attribute(value: &str) -> String {
    escape(value).replace('"', "&quot;")
}

fn utc_date(now: SystemTime) -> Result<String, ContextError> {
    let mut days = now
        .duration_since(UNIX_EPOCH)
        .map_err(|_| ContextError::InvalidDate)?
        .as_secs()
        / 86400;
    let mut year = 1970_u64;
    loop {
        let year_days = if leap_year(year) { 366 } else { 365 };
        if days < year_days {
            break;
        }
        days = days.saturating_sub(year_days);
        year = year.saturating_add(1);
        if year > 9999 {
            return Err(ContextError::InvalidDate);
        }
    }
    let mut month = 1_u64;
    loop {
        let month_days = match month {
            4 | 6 | 9 | 11 => 30,
            2 if leap_year(year) => 29,
            2 => 28,
            _ => 31,
        };
        if days < month_days {
            break;
        }
        days = days.saturating_sub(month_days);
        month = month.saturating_add(1);
    }
    Ok(format!(
        "{year:04}-{month:02}-{:02}",
        days.saturating_add(1)
    ))
}

fn leap_year(year: u64) -> bool {
    year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400))
}
