//! 会話を Responses の入力に変え、同じ送り先の項目を保つ（設計書 02-01「OpenAI の Responses API」）。

use std::collections::HashMap;
use std::fmt;

use serde_json::{Value, json};

use crate::http::{HeaderMap, HeaderValue, Uri};
use crate::provider::ids::remap_tool_call_id;
use crate::provider::{
    AdapterKind, AssistantPart, Conversation, Item, ReasoningReplay, SchemaKind, ToolSpec,
};

use super::{OpenAiResponsesAdapter, OpenAiResponsesError};

/// 文脈と設定から決める要求ごとの指定。
#[derive(Clone, Copy, Debug, Default)]
pub struct RequestOptions<'a> {
    /// 設定された推論の強さ。未指定なら medium。
    pub effort: Option<&'a str>,
    /// 前の要求の最後の項目の位置。Responses では区切りを付けない。
    pub previous_last_item: Option<usize>,
    /// 圧縮のため道具の呼び出しを禁じるか。
    pub disable_tools: bool,
    /// キャッシュの振り分けに使うセッションの ID。
    pub prompt_cache_key: Option<&'a str>,
}

/// 認証と本文を診断へ出さない送信専用の要求。
pub struct OpenAiResponsesRequest {
    pub(super) uri: Uri,
    pub(super) headers: HeaderMap,
    pub(super) body: Vec<u8>,
}

impl fmt::Debug for OpenAiResponsesRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OpenAiResponsesRequest")
            .finish_non_exhaustive()
    }
}

impl OpenAiResponsesAdapter {
    /// 共通の会話から、まだ送っていない要求を作る。
    /// 推論を戻せるハッシュの一致は文脈の管理が確かめる（設計書 01-04「推論の内容の扱い」）。
    pub fn build_request(
        &self,
        conversation: &Conversation,
        system: &str,
        tools: &[ToolSpec],
        options: RequestOptions<'_>,
    ) -> Result<OpenAiResponsesRequest, OpenAiResponsesError> {
        if options
            .previous_last_item
            .is_some_and(|i| i >= conversation.items.len())
            || (!tools.is_empty() && options.disable_tools && !self.capabilities.tool_choice_none)
        {
            return Err(OpenAiResponsesError::InvalidRequest);
        }
        let uri = format!(
            "{}/responses",
            self.destination.base_url.trim_end_matches('/')
        )
        .parse::<Uri>()
        .map_err(|_| OpenAiResponsesError::InvalidRequest)?;
        if !matches!(uri.scheme_str(), Some("http" | "https"))
            || uri.authority().is_none_or(|a| a.as_str().contains('@'))
        {
            return Err(OpenAiResponsesError::InvalidRequest);
        }
        let mut headers = HeaderMap::new();
        if !self.api_key.is_empty() {
            let mut key = HeaderValue::from_str(&format!("Bearer {}", self.api_key))
                .map_err(|_| OpenAiResponsesError::InvalidRequest)?;
            key.set_sensitive(true);
            headers.insert("authorization", key);
        }
        headers.insert("content-type", HeaderValue::from_static("application/json"));
        let mut body = json!({
            "model": self.destination.model,
            "instructions": system,
            "input": self.input(conversation)?,
            "tools": tools.iter().map(|tool| self.tool_definition(tool)).collect::<Vec<_>>(),
            "tool_choice": if options.disable_tools { "none" } else { "auto" },
            "parallel_tool_calls": self.capabilities.parallel_tool_calls,
            "store": false,
            "stream": true,
            "include": ["reasoning.encrypted_content"],
            "reasoning": {"effort": options.effort.unwrap_or("medium"), "summary": "auto"}
        });
        let fields = body
            .as_object_mut()
            .ok_or(OpenAiResponsesError::InvalidRequest)?;
        if let Some(limit) = self.capabilities.max_output_tokens {
            fields.insert("max_output_tokens".into(), json!(limit));
        }
        if let Some(key) = options.prompt_cache_key {
            fields.insert("prompt_cache_key".into(), json!(key));
        }
        if tools.is_empty() {
            let fields = body
                .as_object_mut()
                .ok_or(OpenAiResponsesError::InvalidRequest)?;
            fields.remove("tools");
            fields.remove("tool_choice");
            fields.remove("parallel_tool_calls");
        }
        Ok(OpenAiResponsesRequest {
            uri,
            headers,
            body: serde_json::to_vec(&body).map_err(|_| OpenAiResponsesError::InvalidRequest)?,
        })
    }

    fn tool_definition(&self, tool: &ToolSpec) -> Value {
        let kind = if self.capabilities.strict_tools {
            SchemaKind::OpenAiStrict
        } else {
            SchemaKind::Plain
        };
        json!({"type": "function", "name": tool.name, "description": tool.description,
            "parameters": tool.input_schema(kind), "strict": self.capabilities.strict_tools})
    }

    fn input(&self, conversation: &Conversation) -> Result<Vec<Value>, OpenAiResponsesError> {
        let mut input = Vec::new();
        let mut ids = HashMap::new();
        for item in &conversation.items {
            match item {
                Item::UserMessage(user) => input.push(json!({"type": "message", "role": "user",
                    "content": user.parts.iter().map(|p| json!({"type": "input_text", "text": p.text})).collect::<Vec<_>>()})),
                Item::ToolResult(result) => {
                    let id = ids.get(&result.id).ok_or(OpenAiResponsesError::InvalidRequest)?;
                    let (body, images) = crate::provider::images::prepare(result, self.capabilities.image_input);
                    let output = if images.is_empty() {
                        json!(body)
                    } else {
                        let mut parts = vec![json!({"type": "input_text", "text": body})];
                        for image in images {
                            parts.push(json!({"type": "input_image", "image_url": crate::provider::images::data_url(image), "detail": "auto"}));
                        }
                        json!(parts)
                    };
                    input.push(json!({"type": "function_call_output", "call_id": id, "output": output}));
                }
                Item::AssistantTurn(turn) => {
                    let same = turn.raw_items.destination == self.destination
                        && turn.raw_items.adapter == AdapterKind::OpenAiResponses;
                    // 内部の ID はループが付け替えるため、結果の対応だけは写しから取る。
                    // 応答の内容と順序は元の項目から戻す（設計書 02-01「共通の形」）。
                    for part in &turn.parts {
                        if let AssistantPart::ToolCall(call) = part {
                            let original = call.provider_id.as_deref().unwrap_or(&call.id);
                            ids.insert(call.id.clone(), if same { original.to_owned() } else { remap_tool_call_id(original) });
                        }
                    }
                    if same {
                        for raw in &turn.raw_items.items {
                            if raw.get("type").and_then(Value::as_str) == Some("reasoning")
                                && self.capabilities.reasoning_replay == ReasoningReplay::None
                            {
                                continue;
                            }
                            let mut raw = raw.clone();
                            let fields = raw.as_object_mut().ok_or(OpenAiResponsesError::InvalidRequest)?;
                            if !fields.get("id").and_then(Value::as_str).is_some_and(is_prefixed) {
                                fields.remove("id");
                            }
                            input.push(raw);
                        }
                    } else {
                        for part in &turn.parts {
                            match part {
                                AssistantPart::Text(text) => input.push(json!({"type": "message", "role": "assistant",
                                    "content": [{"type": "output_text", "text": text, "annotations": []}]})),
                                AssistantPart::Reasoning(_) => {},
                                AssistantPart::ToolCall(call) => input.push(json!({"type": "function_call",
                                    "call_id": ids.get(&call.id).ok_or(OpenAiResponsesError::InvalidRequest)?,
                                    "name": call.name, "arguments": call.raw_arguments})),
                            }
                        }
                    }
                }
            }
        }
        Ok(input)
    }
}

// Codex CLI a45b8cc の response_item_id::is_prefixed に揃える（設計書 02-01「OpenAI の Responses API」）。
fn is_prefixed(id: &str) -> bool {
    id.split_once('_')
        .is_some_and(|(prefix, suffix)| !prefix.is_empty() && !suffix.is_empty())
}

#[cfg(test)]
mod tests {
    //! 提供者が認める項目の ID の境界値を固定する（設計書 02-01「OpenAI の Responses API」）。

    #[test]
    fn item_id_requires_both_prefix_and_suffix() {
        for (id, expected) in [
            ("rs_1", true),
            ("a_b_c", true),
            ("_x", false),
            ("rs_", false),
            ("abc", false),
            ("", false),
        ] {
            assert_eq!(super::is_prefixed(id), expected, "{id}");
        }
    }
}
