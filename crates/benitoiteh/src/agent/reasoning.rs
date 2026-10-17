//! 圧縮と再開で推論の内容を除く（設計書 01-04「推論の内容の扱い」）。

use crate::provider::{AdapterKind, AssistantPart, AssistantTurn};

pub(super) fn strip_reasoning(turn: &mut AssistantTurn) {
    turn.parts
        .retain(|part| !matches!(part, AssistantPart::Reasoning(_)));
    match turn.raw_items.adapter {
        AdapterKind::AnthropicMessages => turn.raw_items.items.retain(|raw| {
            !matches!(
                raw.get("type").and_then(serde_json::Value::as_str),
                Some("thinking" | "redacted_thinking")
            )
        }),
        AdapterKind::OpenAiResponses => {
            let had_reasoning = turn.raw_items.items.iter().any(|raw| {
                raw.get("type").and_then(serde_json::Value::as_str) == Some("reasoning")
            });
            turn.raw_items.items.retain(|raw| {
                raw.get("type").and_then(serde_json::Value::as_str) != Some("reasoning")
            });
            if had_reasoning {
                for raw in &mut turn.raw_items.items {
                    if let Some(fields) = raw.as_object_mut() {
                        fields.remove("id");
                    }
                }
            }
        }
        AdapterKind::OpenAiChat => {
            for raw in &mut turn.raw_items.items {
                if let Some(fields) = raw.as_object_mut() {
                    fields.remove("reasoning_content");
                    fields.remove("reasoning");
                }
            }
        }
    }
}

pub(super) fn has_reasoning(turn: &AssistantTurn) -> bool {
    turn.parts
        .iter()
        .any(|part| matches!(part, AssistantPart::Reasoning(_)))
        || turn.raw_items.items.iter().any(|raw| {
            matches!(
                raw.get("type").and_then(serde_json::Value::as_str),
                Some("thinking" | "redacted_thinking" | "reasoning")
            ) || ["reasoning", "reasoning_content"].iter().any(|key| {
                raw.get(*key)
                    .is_some_and(|value| !value.is_null() && value.as_str() != Some(""))
            })
        })
}
