//! 会話と道具を Messages API の要求に変える（設計書 02-01「共通の形」「Anthropic の Messages API」）。

use std::collections::HashMap;
use std::fmt;

use serde_json::{Value, json};

use crate::http::{HeaderMap, HeaderValue, Uri};
use crate::provider::ids::remap_tool_call_id;
use crate::provider::{
    AdapterKind, AssistantPart, Conversation, Item, ReasoningReplay, SchemaKind, ToolSpec,
};

use super::{AnthropicAdapter, AnthropicError};

/// 呼び出す側が文脈と設定から決める要求ごとの指定。
#[derive(Clone, Copy, Debug, Default)]
pub struct RequestOptions<'a> {
    /// 設定された effort。省略したときは high。
    pub effort: Option<&'a str>,
    /// 前の要求の最後の項目の Conversation.items 内の位置（0 から始まる）。
    pub previous_last_item: Option<usize>,
    /// 圧縮のため tool_choice: none を使うか。未対応の口では誤りにする。
    pub disable_tools: bool,
}

/// 認証ヘッダを含む送信専用の要求。Debug は内容を表示しない。
pub struct AnthropicRequest {
    pub(super) uri: Uri,
    pub(super) headers: HeaderMap,
    pub(super) body: Vec<u8>,
}

impl fmt::Debug for AnthropicRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AnthropicRequest").finish_non_exhaustive()
    }
}

impl AnthropicAdapter {
    /// 共通の会話から、まだ送っていない要求を作る。
    ///
    /// システムプロンプトと道具のハッシュの一致、圧縮後の推論の除去は文脈の
    /// 管理の責任とする（設計書 01-04「推論の内容の扱い」）。
    pub fn build_request(
        &self,
        conversation: &Conversation,
        system: &str,
        tools: &[ToolSpec],
        options: RequestOptions<'_>,
    ) -> Result<AnthropicRequest, AnthropicError> {
        if options
            .previous_last_item
            .is_some_and(|i| i >= conversation.items.len())
            || (options.disable_tools && !self.capabilities.tool_choice_none)
        {
            return Err(AnthropicError::InvalidRequest);
        }
        let uri = format!(
            "{}/v1/messages",
            self.destination.base_url.trim_end_matches('/')
        )
        .parse::<Uri>()
        .map_err(|_| AnthropicError::InvalidRequest)?;
        if !matches!(uri.scheme_str(), Some("http" | "https"))
            || uri.authority().is_none_or(|a| a.as_str().contains('@'))
        {
            return Err(AnthropicError::InvalidRequest);
        }
        let mut headers = HeaderMap::new();
        let mut key =
            HeaderValue::from_str(&self.api_key).map_err(|_| AnthropicError::InvalidRequest)?;
        key.set_sensitive(true);
        headers.insert("x-api-key", key);
        headers.insert("anthropic-version", HeaderValue::from_static("2023-06-01"));
        headers.insert("content-type", HeaderValue::from_static("application/json"));
        let mut system = json!([{"type": "text", "text": system}]);
        if self.capabilities.cache_control {
            mark_cache(system.as_array_mut().and_then(|s| s.last_mut()))?;
        }
        let mut body = json!({
            "model": self.destination.model,
            "max_tokens": self.capabilities.max_output_tokens.unwrap_or(32_000),
            "system": system,
            "tools": tools.iter().map(|t| self.tool_definition(t)).collect::<Vec<_>>(),
            "messages": self.messages(conversation, options)?,
            "stream": true,
            "thinking": {"type": "adaptive"},
            "output_config": {"effort": options.effort.unwrap_or("high")}
        });
        if options.disable_tools {
            insert(&mut body, "tool_choice", json!({"type": "none"}))?;
        }
        Ok(AnthropicRequest {
            uri,
            headers,
            body: serde_json::to_vec(&body).map_err(|_| AnthropicError::InvalidRequest)?,
        })
    }

    fn tool_definition(&self, tool: &ToolSpec) -> Value {
        let kind = if self.capabilities.strict_tools {
            SchemaKind::AnthropicStrict
        } else {
            SchemaKind::Plain
        };
        let mut definition = serde_json::Map::new();
        definition.insert("name".into(), json!(tool.name));
        definition.insert("description".into(), json!(tool.description));
        definition.insert("input_schema".into(), tool.input_schema(kind));
        if self.capabilities.strict_tools {
            definition.insert("strict".into(), json!(true));
        }
        if self.capabilities.eager_input_streaming {
            definition.insert("eager_input_streaming".into(), json!(true));
        }
        Value::Object(definition)
    }

    fn messages(
        &self,
        conversation: &Conversation,
        options: RequestOptions<'_>,
    ) -> Result<Vec<Value>, AnthropicError> {
        let mut messages = Vec::new();
        let mut ids = HashMap::new();
        let mut collecting_results = false;
        for (index, item) in conversation.items.iter().enumerate() {
            let (role, mut content) = match item {
                Item::UserMessage(user) => (
                    "user",
                    user.parts
                        .iter()
                        .map(|p| json!({"type": "text", "text": p.text}))
                        .collect(),
                ),
                Item::AssistantTurn(turn) => {
                    let same = turn.raw_items.destination == self.destination
                        && turn.raw_items.adapter == AdapterKind::AnthropicMessages;
                    for part in &turn.parts {
                        if let AssistantPart::ToolCall(call) = part {
                            let original = call.provider_id.as_deref().unwrap_or(&call.id);
                            ids.insert(
                                call.id.clone(),
                                if same {
                                    original.to_owned()
                                } else {
                                    remap_tool_call_id(original)
                                },
                            );
                        }
                    }
                    let content: Vec<Value> = if same {
                        turn.raw_items
                            .items
                            .iter()
                            .filter(|block| {
                                self.capabilities.reasoning_replay != ReasoningReplay::None
                                    || !is_reasoning(block)
                            })
                            .cloned()
                            .collect()
                    } else {
                        turn.parts.iter().filter_map(|part| match part {
                            AssistantPart::Text(text) => Some(json!({"type": "text", "text": text})),
                            AssistantPart::Reasoning(_) => None,
                            AssistantPart::ToolCall(call) => Some(json!({
                                "type": "tool_use", "id": ids.get(&call.id), "name": call.name,
                                "input": if call.arguments.is_null() { json!({}) } else { call.arguments.clone() }
                            })),
                        }).collect()
                    };
                    ("assistant", content)
                }
                Item::ToolResult(result) => {
                    // 呼び出しと結果は内部 ID で結び、別の送り先でも同じ変換を使う。
                    let id = ids.get(&result.id).ok_or(AnthropicError::InvalidRequest)?;
                    (
                        "user",
                        vec![
                            json!({"type": "tool_result", "tool_use_id": id, "content": result.text, "is_error": result.is_error}),
                        ],
                    )
                }
            };
            if self.capabilities.cache_control
                && (options.previous_last_item == Some(index)
                    || Some(index) == conversation.items.len().checked_sub(1))
                && !content.is_empty()
            {
                mark_cache(content.last_mut())?;
            }
            let is_result = matches!(item, Item::ToolResult(_));
            if collecting_results && is_result {
                let previous: &mut Value =
                    messages.last_mut().ok_or(AnthropicError::InvalidRequest)?;
                previous
                    .get_mut("content")
                    .and_then(Value::as_array_mut)
                    .ok_or(AnthropicError::InvalidRequest)?
                    .extend(content);
            } else if !content.is_empty() {
                messages.push(json!({"role": role, "content": content}));
            }
            collecting_results = is_result;
        }
        Ok(messages)
    }
}

fn is_reasoning(block: &Value) -> bool {
    matches!(
        block.get("type").and_then(Value::as_str),
        Some("thinking" | "redacted_thinking")
    )
}

fn insert(value: &mut Value, name: &str, content: Value) -> Result<(), AnthropicError> {
    value
        .as_object_mut()
        .ok_or(AnthropicError::InvalidRequest)?
        .insert(name.into(), content);
    Ok(())
}

fn mark_cache(block: Option<&mut Value>) -> Result<(), AnthropicError> {
    insert(
        block.ok_or(AnthropicError::InvalidRequest)?,
        "cache_control",
        json!({"type": "ephemeral"}),
    )
}
