//! 今の枝の会話を、読み返すための Markdown に書き出す（設計書 01-05「会話の書き出し」）。
//!
//! 圧縮で LLM に送らなくなった番も、記録にあるかぎり書く。理由: 読み返しの目的では、
//! 今の文脈ではなく、起きたことの全体が要る。

use std::fmt::Write as _;
use std::path::Path;

use crate::provider::{AssistantPart, UserMessage, UserPartKind};

use super::{Record, SessionError, SessionTree, read_records, text};

/// 記録のディレクトリから、今の枝を Markdown にする。
pub fn export_markdown(directory: &Path) -> Result<String, SessionError> {
    let records = read_records(directory)?;
    let tree = SessionTree::from_records(&records)?;
    let mut output = String::new();
    if let Some(Record::Header(header)) = records.first() {
        let _written = writeln!(output, "# benitoiteh session {}\n", header.session_id);
        let _written = writeln!(output, "- Project: `{}`", header.project_dir.display());
        let mode = match header.mode {
            super::RecordedMode::Restricted => "restricted",
            super::RecordedMode::Normal => "normal",
        };
        let _written = writeln!(output, "- Mode: {mode}");
        let _written = writeln!(
            output,
            "- Provider: {}; model: {}",
            header.provider.name, header.provider.model
        );
        let _written = writeln!(output, "- Started: {}", header.started_at);
    }
    for node in tree.path(tree.tip) {
        let Some((_, index)) = tree.entry(node) else {
            continue;
        };
        match records.get(index) {
            Some(Record::User { message, .. }) => user(&mut output, message),
            Some(Record::Assistant { response, .. }) => {
                for part in &response.parts {
                    match part {
                        AssistantPart::Text(value) if !value.trim().is_empty() => {
                            let _written =
                                write!(output, "\n## Assistant\n\n{}\n", value.trim_end());
                        }
                        AssistantPart::ToolCall(call) => {
                            let arguments = serde_json::to_string_pretty(&call.arguments)
                                .unwrap_or_else(|_| call.arguments.to_string());
                            let _written = write!(
                                output,
                                "\n### Tool call: {}\n\n{}",
                                inline(&call.name),
                                fenced("json", &arguments)
                            );
                        }
                        // 推論の内容は提供者が暗号にしたものを含み、読み返しには要らない。
                        AssistantPart::Text(_) | AssistantPart::Reasoning(_) => {}
                    }
                }
            }
            Some(Record::ToolResult { name, result, .. }) => {
                let title = if result.is_error {
                    text::EXPORT_TOOL_ERROR
                } else {
                    text::EXPORT_TOOL_RESULT
                };
                let _written = write!(
                    output,
                    "\n### {title}: {}\n\n{}",
                    inline(name),
                    fenced("text", &result.text)
                );
                if !result.images.is_empty() {
                    let _written =
                        writeln!(output, "\n{}", text::export_images(result.images.len()));
                }
            }
            Some(Record::Compaction { conversation, .. }) => {
                let _written = write!(output, "\n> {}\n", text::EXPORT_COMPACTED);
                // 以後の LLM が読んだ要約も示す。
                for item in &conversation.items {
                    if let crate::provider::Item::UserMessage(message) = item {
                        for part in &message.parts {
                            if part.kind == UserPartKind::Summary {
                                let _written = write!(
                                    output,
                                    "\n<details><summary>{}</summary>\n\n{}\n</details>\n",
                                    text::EXPORT_SUMMARY,
                                    fenced("text", &part.text)
                                );
                            }
                        }
                    }
                }
            }
            // 道の上のほかの記録（画像を外した印など）は、読み返しに要らない。
            Some(_) | None => {}
        }
    }
    Ok(output)
}

fn user(output: &mut String, message: &UserMessage) {
    for part in &message.parts {
        let heading = match part.kind {
            UserPartKind::Request => text::EXPORT_USER,
            UserPartKind::Steering => text::EXPORT_STEERING,
            UserPartKind::Summary => text::EXPORT_SUMMARY,
            // 環境の情報は毎回同じ形の長い文なので、書き出しでは畳む。
            UserPartKind::Environment => {
                let _written = write!(
                    output,
                    "\n<details><summary>{}</summary>\n\n{}\n</details>\n",
                    text::EXPORT_ENVIRONMENT,
                    fenced("text", &part.text)
                );
                continue;
            }
        };
        let _written = write!(output, "\n## {heading}\n\n{}\n", part.text.trim_end());
    }
}

/// 中身の最も長い連続した `` ` `` より長い囲みで包む。理由: 中身の囲みで、書き出しの囲みを閉じさせない。
fn fenced(language: &str, value: &str) -> String {
    let mut longest = 0_usize;
    let mut current = 0_usize;
    for character in value.chars() {
        if character == '`' {
            current = current.saturating_add(1);
            longest = longest.max(current);
        } else {
            current = 0;
        }
    }
    let fence = "`".repeat(longest.saturating_add(1).max(3));
    let body = value.trim_end_matches('\n');
    format!("{fence}{language}\n{body}\n{fence}\n")
}

/// 見出しに入れる名前。改行と `` ` `` を除く。
fn inline(value: &str) -> String {
    let cleaned = value
        .chars()
        .filter(|character| !character.is_control() && *character != '`')
        .collect::<String>();
    format!("`{cleaned}`")
}
