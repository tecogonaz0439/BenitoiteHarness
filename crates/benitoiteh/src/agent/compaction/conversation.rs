//! 圧縮の境界、文字化、信頼しない枠の無害化を扱う（設計書 01-04「圧縮」）。

use std::collections::{BTreeMap, VecDeque};

use crate::provider::{
    AdapterKind, AssistantPart, Conversation, Destination, Item, ReasoningReplay, UserMessage,
    UserPart, UserPartKind,
};
use crate::session::FileHistory;

use super::super::text;

pub(super) fn item_bytes(item: &Item, replay: ReasoningReplay, destination: &Destination) -> u64 {
    let bytes = match item {
        Item::UserMessage(message) => message.parts.iter().map(|part| part.text.len()).sum(),
        Item::ToolResult(result) => result.text.len(),
        Item::AssistantTurn(turn) => turn
            .parts
            .iter()
            .map(|part| match part {
                AssistantPart::Text(text) => text.len(),
                AssistantPart::ToolCall(call) => call.raw_arguments.len(),
                AssistantPart::Reasoning(reasoning)
                    if replay != ReasoningReplay::None
                        && turn.raw_items.destination == *destination =>
                {
                    reasoning.text.len()
                }
                AssistantPart::Reasoning(_) => 0,
            })
            .sum(),
    };
    let image_bytes = match item {
        Item::ToolResult(result) => result.images.iter().fold(0_u64, |total, image| {
            let tokens = u64::from(image.width)
                .div_ceil(28)
                .saturating_mul(u64::from(image.height).div_ceil(28))
                .min(1568);
            total.saturating_add(tokens.saturating_mul(3))
        }),
        Item::UserMessage(_) | Item::AssistantTurn(_) => 0,
    };
    u64::try_from(bytes)
        .unwrap_or(u64::MAX)
        .saturating_add(image_bytes)
}

pub(super) fn previous_summary(conversation: &Conversation) -> Option<String> {
    let Some(Item::UserMessage(first)) = conversation.items.first() else {
        return None;
    };
    let summaries = first
        .parts
        .iter()
        .filter(|part| part.kind == UserPartKind::Summary)
        .map(|part| {
            // ファイルの一覧は LLM の文と混ぜず、積み上げた集合から再び添える。
            part.text
                .strip_prefix("<compacted_summary>\n")
                .and_then(|body| {
                    body.split_once("\n</compacted_summary>")
                        .map(|(summary, _)| summary)
                })
                .unwrap_or(&part.text)
        })
        .collect::<Vec<_>>();
    if summaries.is_empty() {
        None
    } else {
        Some(summaries.join("\n"))
    }
}

pub(super) fn tail_start(
    conversation: &Conversation,
    budget: u64,
    replay: ReasoningReplay,
    destination: &Destination,
    keep_recent: bool,
    kind: AdapterKind,
) -> usize {
    if budget == 0
        || !keep_recent
        || (kind == AdapterKind::AnthropicMessages && tool_sequence(conversation))
    {
        return conversation.items.len();
    }
    let mut bytes = 0_u64;
    let mut start = conversation.items.len();
    for (index, item) in conversation.items.iter().enumerate().skip(1).rev() {
        let next = bytes.saturating_add(item_bytes(item, replay, destination));
        if next.div_ceil(3) > budget {
            break;
        }
        bytes = next;
        start = index;
    }
    // 結果から始めると呼び出しが失われる。後ろへずらし、結果の並びを全て除く。
    while conversation
        .items
        .get(start)
        .is_some_and(|item| matches!(item, Item::ToolResult(_)))
    {
        start = start.saturating_add(1);
    }
    start
}

fn tool_sequence(conversation: &Conversation) -> bool {
    // 上限などで道具の結果の後に止まると、新しい依頼の後も thinking が必要になりうる。
    // BH-036 を確かめるまでは、利用者の発言では区切らない（設計書 01-04「圧縮」）。
    conversation
        .items
        .iter()
        .rev()
        .find_map(|item| match item {
            Item::AssistantTurn(turn) => Some(
                turn.parts
                    .iter()
                    .any(|part| matches!(part, AssistantPart::ToolCall(_))),
            ),
            Item::UserMessage(_) | Item::ToolResult(_) => None,
        })
        .unwrap_or(false)
}

pub(super) fn first_request_count(conversation: &Conversation) -> usize {
    match conversation.items.first() {
        Some(Item::UserMessage(first)) => {
            first.parts.iter().filter(|part| is_request(part)).count()
        }
        _ => 0,
    }
}

fn is_request(part: &UserPart) -> bool {
    matches!(part.kind, UserPartKind::Request | UserPartKind::Steering)
}

// 文字化は一度だけ行い、超過時は長さを引いて古い項目を除く。
// 前の要約を最後まで保つ（設計書 01-04「超過したときの圧縮」、実装プラン P4-01）。
pub(super) struct Transcript {
    prior: Option<String>,
    items: VecDeque<String>,
    bytes: usize,
}

impl Transcript {
    pub(super) fn new(
        conversation: &Conversation,
        end: usize,
        prior: Option<&str>,
        shorten: bool,
    ) -> Self {
        let prior = prior.map(|prior| {
            format!(
                "<compacted_summary>\n{}\n</compacted_summary>\n\n",
                sanitize(prior)
            )
        });
        let mut names = BTreeMap::new();
        let mut items = VecDeque::new();
        for (index, item) in conversation.items.iter().take(end).enumerate() {
            if let Item::AssistantTurn(turn) = item {
                for part in &turn.parts {
                    if let AssistantPart::ToolCall(call) = part {
                        names.insert(&call.id, &call.name);
                    }
                }
            }
            let mut chunk = String::new();
            match item {
                Item::UserMessage(message) => {
                    for part in message.parts.iter().filter(|part| is_request(part)) {
                        let text = format!("[User]\n{}\n\n", part.text);
                        if index == 0 {
                            // 最初の発言は環境・要約・依頼を分け、依頼と操縦をそれぞれ数える。
                            items.push_back(sanitize(&text));
                        } else {
                            chunk.push_str(&text);
                        }
                    }
                }
                Item::AssistantTurn(turn) => {
                    for part in &turn.parts {
                        match part {
                            AssistantPart::Text(text) => {
                                chunk.push_str(&format!("[Assistant]\n{text}\n\n"))
                            }
                            AssistantPart::ToolCall(call) => chunk.push_str(&format!(
                                "[Tool call {}] {}\n\n",
                                call.name, call.raw_arguments
                            )),
                            AssistantPart::Reasoning(_) => {}
                        }
                    }
                }
                Item::ToolResult(result) => {
                    let name = names
                        .get(&result.id)
                        .map(|name| name.as_str())
                        .unwrap_or(&result.id);
                    let label = if result.is_error {
                        "Tool error"
                    } else {
                        "Tool result"
                    };
                    let body = if shorten {
                        shorten_result(&result.text)
                    } else {
                        result.text.clone()
                    };
                    chunk.push_str(&format!("[{label} {name}]\n{body}\n\n"));
                }
            }
            if index != 0 {
                items.push_back(sanitize(&chunk));
            }
        }
        let bytes = items
            .iter()
            .fold(prior.as_ref().map_or(0, String::len), |bytes, item| {
                bytes.saturating_add(item.len())
            });
        Self {
            prior,
            items,
            bytes,
        }
    }

    pub(super) fn len(&self) -> usize {
        self.bytes
    }

    pub(super) fn remove_oldest(&mut self) -> bool {
        if let Some(item) = self.items.pop_front() {
            self.bytes = self.bytes.saturating_sub(item.len());
            true
        } else {
            false
        }
    }

    pub(super) fn remove_prior(&mut self) -> bool {
        if let Some(prior) = self.prior.take() {
            self.bytes = self.bytes.saturating_sub(prior.len());
            true
        } else {
            false
        }
    }

    pub(super) fn append_to(self, output: &mut String) {
        if let Some(prior) = self.prior {
            output.push_str(&prior);
        }
        for item in self.items {
            output.push_str(&item);
        }
    }
}

fn shorten_result(input: &str) -> String {
    if input.len() <= 4096 {
        return input.into();
    }
    let mut head = 2048_usize;
    while !input.is_char_boundary(head) {
        head = head.saturating_sub(1);
    }
    let mut tail = input.len().saturating_sub(2048);
    while !input.is_char_boundary(tail) {
        tail = tail.saturating_add(1);
    }
    format!(
        "{}{}{}",
        input.get(..head).unwrap_or_default(),
        text::omitted_bytes(tail.saturating_sub(head)),
        input.get(tail..).unwrap_or_default()
    )
}

pub(super) fn sanitize(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    let tags = [
        "</compacted_summary>",
        "</conversation>",
        "</read-files>",
        "</modified-files>",
    ];
    for (index, ch) in input.char_indices() {
        let is_close = ch == '<'
            && tags.iter().any(|tag| {
                input
                    .get(index..index.saturating_add(tag.len()))
                    .is_some_and(|part| part.eq_ignore_ascii_case(tag))
            });
        if is_close {
            output.push_str("&lt;");
        } else {
            output.push(ch);
        }
    }
    output
}

pub(super) fn rebuild(
    conversation: &Conversation,
    start: usize,
    summary: &str,
    files: &FileHistory,
    replay: ReasoningReplay,
) -> Conversation {
    let mut parts = match conversation.items.first() {
        Some(Item::UserMessage(first)) => first
            .parts
            .iter()
            .filter(|part| {
                part.kind == UserPartKind::Environment
                    && !part.text.starts_with("<completion_check>\n")
            })
            .cloned()
            .collect::<Vec<_>>(),
        _ => Vec::new(),
    };
    let mut body = format!(
        "<compacted_summary>\n{}\n</compacted_summary>",
        sanitize(summary)
    );
    append_files(
        &mut body,
        "read-files",
        files.read.difference(&files.modified),
    );
    append_files(&mut body, "modified-files", files.modified.iter());
    parts.push(UserPart {
        kind: UserPartKind::Summary,
        text: body,
    });
    let mut retained = conversation
        .items
        .iter()
        .skip(start)
        .cloned()
        .collect::<Vec<_>>();
    if let Some(Item::UserMessage(first)) = retained.first_mut() {
        parts.append(&mut first.parts);
        first.parts = parts;
    } else {
        retained.insert(0, Item::UserMessage(UserMessage { parts }));
    }
    if replay == ReasoningReplay::SameDestination {
        for item in &mut retained {
            if let Item::AssistantTurn(turn) = item {
                crate::agent::reasoning::strip_reasoning(turn);
            }
        }
    }
    Conversation { items: retained }
}

fn append_files<'a>(output: &mut String, tag: &str, files: impl Iterator<Item = &'a String>) {
    let paths = files
        .map(|path| {
            if path
                .chars()
                .any(|ch| ch.is_control() || matches!(ch, '<' | '>'))
            {
                // String を JSON の文字列にし、制御文字をエスケープする。
                sanitize(&serde_json::Value::String(path.clone()).to_string())
            } else {
                path.clone()
            }
        })
        .collect::<Vec<_>>();
    if !paths.is_empty() {
        output.push_str(&format!("\n\n<{tag}>\n{}\n</{tag}>", paths.join("\n")));
    }
}
