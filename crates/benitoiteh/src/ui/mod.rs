//! exec のイベントと端末に安全な文字列を出力する（設計書 05-01「exec の出力」、05-02「出力の示し方」）。

mod text;
pub mod tui;

use std::io::{self, Write};

use crate::agent::AgentEvent;
use crate::session::Record;

/// 信頼しない制御文字を可視化する。multiline が偽なら改行とタブも空白にする。
pub fn terminal_text(input: &str, multiline: bool) -> String {
    let mut output = String::with_capacity(input.len());
    for ch in input.chars() {
        match ch {
            '\n' | '\t' if !multiline => output.push(' '),
            '\n' | '\t' => output.push(ch),
            '\u{00}'..='\u{1f}' | '\u{7f}'..='\u{9f}' => {
                output.push_str(&format!("\\x{:02x}", u32::from(ch)));
            }
            '\u{202a}'..='\u{202e}'
            | '\u{2066}'..='\u{2069}'
            | '\u{200e}'
            | '\u{200f}'
            | '\u{61c}' => {
                output.push_str(&format!("\\u{{{:x}}}", u32::from(ch)));
            }
            _ => output.push(ch),
        }
    }
    output
}

/// 一覧の一件を、端末の制御文字を含まない一行にする。
pub fn session_entry(entry: &crate::session::SessionEntry) -> String {
    if let Some(error) = &entry.error {
        return text::session_error(&entry.id, &terminal_text(error, false));
    }
    let mode = text::session_mode(entry.mode);
    let project = entry
        .project_dir
        .as_ref()
        .map(|path| terminal_text(&path.to_string_lossy(), false))
        .unwrap_or_default();
    text::session_entry(
        &entry.id,
        entry.started_at.as_deref().unwrap_or_default(),
        mode,
        &project,
        &terminal_text(entry.first_request.as_deref().unwrap_or_default(), false),
        entry.active,
    )
}

/// 非対話の出力先。JSON と端末用の整形をここで分ける。
pub struct ExecOutput {
    json: bool,
}

impl ExecOutput {
    /// 一連の処理の出力形式を固定する。
    pub fn new(json: bool) -> Self {
        Self { json }
    }

    /// 起動の通知と診断を一行で標準エラー出力に示す。
    pub fn diagnostic(message: &str) -> io::Result<()> {
        writeln!(io::stderr().lock(), "{}", terminal_text(message, false))
    }

    /// JSON では全イベントを、端末では道具・確認・診断だけを示す。
    pub fn event(&mut self, event: &AgentEvent) -> io::Result<()> {
        if let AgentEvent::SessionStart {
            instruction_files,
            skipped_instructions,
            skills,
            skipped_skills,
            ..
        } = event
        {
            for skill in skills {
                Self::diagnostic(&text::skill_loaded(&skill.name, &skill.path))?;
            }
            for skipped in skipped_skills {
                Self::diagnostic(&text::skill_skipped(&skipped.path, &skipped.reason))?;
            }
            for path in instruction_files {
                Self::diagnostic(&text::instruction_loaded(path))?;
            }
            for skipped in skipped_instructions {
                Self::diagnostic(&text::instruction_skipped(&skipped.path, &skipped.reason))?;
            }
        }
        if let AgentEvent::Error { message, .. } = event {
            // 診断の行と印を保ち、制御文字は画面の命令にしない（設計書 01-01「上限」）。
            writeln!(
                io::stderr().lock(),
                "{}",
                terminal_text(&text::error(message), true)
            )?;
        }
        if self.json {
            // 差分は画面専用であり、大きい exec の JSON に含めない（実装プラン P4-06）。
            let mut event = event.clone();
            if let AgentEvent::ToolResult { display, .. } = &mut event {
                *display = None;
            }
            return json_line(&event);
        }
        let message = match event {
            AgentEvent::SessionStart { notice, .. } => notice.clone(),
            AgentEvent::ToolCall {
                name, arguments, ..
            } => Some(text::tool_call(name, arguments)),
            AgentEvent::ToolResult {
                name,
                is_error,
                summary,
                ..
            } => Some(text::tool_result(name, *is_error, summary)),
            AgentEvent::ApprovalResult {
                summary, decision, ..
            } => Some(text::approval(summary, *decision)),
            AgentEvent::Compaction {
                reason,
                summarized_items,
                kept_items,
                ..
            } => Some(text::compaction(*reason, *summarized_items, *kept_items)),
            AgentEvent::ProjectDirChanged { project_dir } => {
                Some(text::project_changed(project_dir))
            }
            AgentEvent::ImagesRemoved { .. } => Some(text::IMAGES_REMOVED.into()),
            AgentEvent::Checkpoint { checkpoint } => Some(text::checkpoint(checkpoint)),
            AgentEvent::HardLinks { paths } => Some(text::hard_links(paths)),
            AgentEvent::Error { .. } => None,
            AgentEvent::SessionEnd { outcome, reason }
                if *outcome == crate::agent::Outcome::Cancelled =>
            {
                Some(reason.clone())
            }
            AgentEvent::Steered { .. }
            | AgentEvent::TurnStart { .. }
            | AgentEvent::TextDelta { .. }
            | AgentEvent::ReasoningDelta { .. }
            | AgentEvent::ApprovalRequest { .. }
            | AgentEvent::TurnEnd { .. }
            | AgentEvent::Retry { .. }
            | AgentEvent::SessionEnd { .. } => None,
        };
        if let Some(message) = message {
            Self::diagnostic(&message)?;
        }
        Ok(())
    }

    /// 最後に集計の JSON、または最後の応答の文を出力する。
    pub fn finish(&mut self, summary: &Record, last_message: &str) -> io::Result<()> {
        if self.json {
            json_line(summary)
        } else {
            let mut stdout = io::stdout().lock();
            stdout.write_all(terminal_text(last_message, true).as_bytes())?;
            if !last_message.is_empty() && !last_message.ends_with('\n') {
                stdout.write_all(b"\n")?;
            }
            stdout.flush()
        }
    }
}

fn json_line(value: &impl serde::Serialize) -> io::Result<()> {
    let mut bytes = serde_json::to_vec(value).map_err(io::Error::other)?;
    bytes.push(b'\n');
    let mut stdout = io::stdout().lock();
    stdout.write_all(&bytes)?;
    stdout.flush()
}
