//! 共通の会話から Chat Completions の要求を作る（設計書 02-01「共通の形」「OpenAI の Chat Completions API」）。

use std::collections::HashMap;
use std::fmt;

use serde_json::{Value, json};

use crate::http::{HeaderMap, HeaderValue, Uri};
use crate::provider::ids::remap_tool_call_id;
use crate::provider::{
    AdapterKind, AssistantPart, Conversation, Item, ReasoningReplay, SchemaKind, ToolSpec,
};

use super::{OpenAiChatAdapter, OpenAiChatError};

/// 呼び出す側が文脈と設定から決める要求ごとの指定。Messages の入口と形を揃える。
#[derive(Clone, Copy, Debug, Default)]
pub struct RequestOptions<'a> {
    /// 解決した推論の深さ。reasoning_effort の機能が真のときだけ要求に付ける。
    pub effort: Option<&'a str>,
    /// 前の要求の最後の項目の位置。Chat Completions はキャッシュの区切りを付けない。
    pub previous_last_item: Option<usize>,
    /// 圧縮のため tool_choice: none を使うか。未対応の口では誤りにする。
    pub disable_tools: bool,
}

/// 認証ヘッダを含む送信専用の要求。Debug は内容を表示しない。
pub struct OpenAiChatRequest {
    pub(super) uri: Uri,
    pub(super) headers: HeaderMap,
    pub(super) body: Vec<u8>,
}

impl fmt::Debug for OpenAiChatRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OpenAiChatRequest").finish_non_exhaustive()
    }
}

impl OpenAiChatAdapter {
    /// 共通の会話から、まだ送っていない要求を作る。
    ///
    /// ハッシュの一致と圧縮後の推論の除去は、Messages と同じく文脈の管理が
    /// 確かめる（設計書 01-04「推論の内容の扱い」）。
    pub fn build_request(
        &self,
        conversation: &Conversation,
        system: &str,
        tools: &[ToolSpec],
        options: RequestOptions<'_>,
    ) -> Result<OpenAiChatRequest, OpenAiChatError> {
        if options
            .previous_last_item
            .is_some_and(|i| i >= conversation.items.len())
            || (!tools.is_empty() && options.disable_tools && !self.capabilities.tool_choice_none)
        {
            return Err(OpenAiChatError::InvalidRequest);
        }
        let uri = format!(
            "{}/chat/completions",
            self.destination.base_url.trim_end_matches('/')
        )
        .parse::<Uri>()
        .map_err(|_| OpenAiChatError::InvalidRequest)?;
        if !matches!(uri.scheme_str(), Some("http" | "https"))
            || uri.authority().is_none_or(|a| a.as_str().contains('@'))
        {
            return Err(OpenAiChatError::InvalidRequest);
        }
        let mut headers = HeaderMap::new();
        if !self.api_key.is_empty() {
            let mut key = HeaderValue::from_str(&format!("Bearer {}", self.api_key))
                .map_err(|_| OpenAiChatError::InvalidRequest)?;
            key.set_sensitive(true);
            headers.insert("authorization", key);
        }
        headers.insert("content-type", HeaderValue::from_static("application/json"));
        let mut body = json!({
            "model": self.destination.model,
            "messages": self.messages(conversation, system)?,
            "tools": tools.iter().map(|t| self.tool_definition(t)).collect::<Vec<_>>(),
            "stream": true,
            "stream_options": {"include_usage": true}
        });
        if self.capabilities.reasoning_effort
            && let Some(effort) = options.effort
        {
            insert(&mut body, "reasoning_effort", json!(effort))?;
        }
        if let Some(max_tokens) = self.capabilities.max_output_tokens {
            insert(&mut body, "max_tokens", json!(max_tokens))?;
        }
        if !tools.is_empty() && self.capabilities.parallel_tool_calls {
            insert(&mut body, "parallel_tool_calls", json!(true))?;
        }
        if !tools.is_empty() && options.disable_tools {
            insert(&mut body, "tool_choice", json!("none"))?;
        }
        if tools.is_empty() {
            let fields = body
                .as_object_mut()
                .ok_or(OpenAiChatError::InvalidRequest)?;
            fields.remove("tools");
            fields.remove("tool_choice");
            fields.remove("parallel_tool_calls");
        }
        Ok(OpenAiChatRequest {
            uri,
            headers,
            body: serde_json::to_vec(&body).map_err(|_| OpenAiChatError::InvalidRequest)?,
        })
    }

    fn tool_definition(&self, tool: &ToolSpec) -> Value {
        let kind = if self.capabilities.strict_tools {
            SchemaKind::OpenAiStrict
        } else {
            SchemaKind::Plain
        };
        let mut function = json!({
            "name": tool.name,
            "description": tool.description,
            "parameters": tool.input_schema(kind)
        });
        if let Some(fields) = function.as_object_mut()
            && self.capabilities.strict_tools
        {
            fields.insert("strict".into(), json!(true));
        }
        json!({"type": "function", "function": function})
    }

    fn messages(
        &self,
        conversation: &Conversation,
        system: &str,
    ) -> Result<Vec<Value>, OpenAiChatError> {
        let mut messages = vec![json!({"role": "system", "content": system})];
        let mut ids = HashMap::new();
        for item in &conversation.items {
            let message = match item {
                Item::UserMessage(user) => json!({
                    "role": "user",
                    "content": user.parts.iter().map(|p| json!({"type": "text", "text": p.text})).collect::<Vec<_>>()
                }),
                Item::AssistantTurn(turn) => {
                    let same = turn.raw_items.destination == self.destination
                        && turn.raw_items.adapter == AdapterKind::OpenAiChat;
                    let mut calls = Vec::new();
                    let mut content = String::new();
                    for part in &turn.parts {
                        match part {
                            AssistantPart::Text(text) => content.push_str(text),
                            AssistantPart::Reasoning(_) => {}
                            AssistantPart::ToolCall(call) => {
                                let original = call.provider_id.as_deref().unwrap_or(&call.id);
                                let id = if same {
                                    original.to_owned()
                                } else {
                                    remap_tool_call_id(original)
                                };
                                ids.insert(call.id.clone(), id.clone());
                                calls.push(json!({
                                    "id": id, "type": "function",
                                    "function": {"name": call.name, "arguments": call.raw_arguments}
                                }));
                            }
                        }
                    }
                    if same {
                        let [raw] = turn.raw_items.items.as_slice() else {
                            return Err(OpenAiChatError::InvalidRequest);
                        };
                        let mut raw = raw.clone();
                        let fields = raw.as_object_mut().ok_or(OpenAiChatError::InvalidRequest)?;
                        if fields.get("role").and_then(Value::as_str) != Some("assistant") {
                            return Err(OpenAiChatError::InvalidRequest);
                        }
                        if self.capabilities.reasoning_replay == ReasoningReplay::None {
                            fields.remove("reasoning_content");
                            fields.remove("reasoning");
                        }
                        // Gemini が求める署名を保つため、extra_content は reasoning_replay によらず
                        // 同じ送り先へ送り返す（設計書 02-01「OpenAI の Chat Completions API」）。
                        raw
                    } else {
                        // 別の送り先の推論は all-turns でも送らない（実装プラン P1-03「会話の変換」）。
                        let mut message = json!({"role": "assistant", "content": content});
                        if !calls.is_empty() {
                            insert(&mut message, "tool_calls", json!(calls))?;
                        }
                        message
                    }
                }
                Item::ToolResult(result) => {
                    let id = ids.get(&result.id).ok_or(OpenAiChatError::InvalidRequest)?;
                    json!({"role": "tool", "tool_call_id": id, "content": result.text})
                }
            };
            messages.push(message);
        }
        Ok(messages)
    }
}

fn insert(value: &mut Value, name: &str, content: Value) -> Result<(), OpenAiChatError> {
    value
        .as_object_mut()
        .ok_or(OpenAiChatError::InvalidRequest)?
        .insert(name.into(), content);
    Ok(())
}
