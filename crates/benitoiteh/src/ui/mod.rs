//! exec のイベントと端末に安全な文字列を出力する（設計書 05-01「exec の出力」、05-02「出力の示し方」）。

mod text;

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
            '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}' => {
                output.push_str(&format!("\\u{{{:x}}}", u32::from(ch)));
            }
            _ => output.push(ch),
        }
    }
    output
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
            ..
        } = event
        {
            for path in instruction_files {
                Self::diagnostic(&text::instruction_loaded(path))?;
            }
            for skipped in skipped_instructions {
                Self::diagnostic(&text::instruction_skipped(&skipped.path, &skipped.reason))?;
            }
        }
        if self.json {
            return json_line(event);
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
            AgentEvent::Error { message, .. } => Some(text::error(message)),
            AgentEvent::SessionEnd { outcome, reason }
                if *outcome == crate::agent::Outcome::Cancelled =>
            {
                Some(reason.clone())
            }
            AgentEvent::TurnStart { .. }
            | AgentEvent::TextDelta { .. }
            | AgentEvent::ReasoningDelta { .. }
            | AgentEvent::ApprovalRequest { .. }
            | AgentEvent::TurnEnd { .. }
            | AgentEvent::Compaction
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
