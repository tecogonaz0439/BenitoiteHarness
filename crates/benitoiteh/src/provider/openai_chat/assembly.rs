//! Chat Completions の差分を index ごとに結び、元の発言を保つ（設計書 02-01「共通の形」「OpenAI の Chat Completions API」）。

use std::collections::BTreeMap;

use serde_json::{Map, Value, json};

use crate::provider::ids::remap_tool_call_id;
use crate::provider::sse::SseEvent;
use crate::provider::{
    AdapterKind, AssistantPart, AssistantTurn, Destination, FinishReason, ProviderError, RawItems,
    Reasoning, StreamEvent, ToolCall, Usage,
};

use super::{OpenAiChatError, api_error, provider_error};

const MAX_EXTRA_CONTENT_DEPTH: usize = 16;
const MAX_EXTRA_CONTENT_BYTES: usize = 64 * 1024;

/// 一回の要求に対応する組み立て器。Debug で受け取った本文を表示しない。
pub struct OpenAiChatAssembler {
    destination: Destination,
    content: Option<String>,
    reasoning: Option<String>,
    parts: Vec<AssistantPart>,
    calls: BTreeMap<usize, Call>,
    indexed_calls: Option<bool>,
    extra_content: Option<Map<String, Value>>,
    extra_content_bytes: usize,
    usage: Value,
    reason: Option<FinishReason>,
    finished: bool,
    failed: bool,
}

#[derive(Default)]
struct Call {
    id: String,
    name: String,
    arguments: String,
    deltas: Vec<String>,
    extra_content: Option<Map<String, Value>>,
}

impl std::fmt::Debug for OpenAiChatAssembler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OpenAiChatAssembler")
            .field("finished", &self.finished)
            .finish_non_exhaustive()
    }
}

impl OpenAiChatAssembler {
    /// 送り先を RawItems に記録する組み立て器を作る。
    pub fn new(destination: Destination) -> Self {
        Self {
            destination,
            content: None,
            reasoning: None,
            parts: Vec::new(),
            calls: BTreeMap::new(),
            indexed_calls: None,
            extra_content: None,
            extra_content_bytes: 0,
            usage: Value::Null,
            reason: None,
            finished: false,
            failed: false,
        }
    }

    /// 一つの SSE イベントを読み、共通のイベントを返す。不正な応答の後は再開しない。
    pub fn read_event(&mut self, event: &SseEvent) -> Result<Vec<StreamEvent>, OpenAiChatError> {
        if self.failed || self.finished {
            return Err(OpenAiChatError::InvalidResponse);
        }
        let result = self.read_inner(event);
        if result.is_err() {
            self.failed = true;
        }
        result
    }

    /// [DONE] まで確認したかを返す。finish_reason だけでは完了としない。
    pub fn is_finished(&self) -> bool {
        self.finished
    }

    /// 正常に受け取り終えた発言と共通の応答を取り出す。途中の内容は返さない。
    pub fn finish(self) -> Result<AssistantTurn, OpenAiChatError> {
        if self.failed {
            return Err(OpenAiChatError::InvalidResponse);
        }
        if !self.finished {
            return Err(provider_error(ProviderError::StreamDisconnected));
        }
        let mut parts = self.parts;
        let mut raw_calls = Vec::new();
        for call in self.calls.into_values() {
            let mut raw_call = json!({
                "id": call.id, "type": "function",
                "function": {"name": call.name, "arguments": call.arguments}
            });
            if let Some(extra) = call.extra_content {
                raw_call
                    .as_object_mut()
                    .ok_or(OpenAiChatError::InvalidResponse)?
                    .insert("extra_content".into(), Value::Object(extra));
            }
            raw_calls.push(raw_call);
            parts.push(AssistantPart::ToolCall(ToolCall {
                id: format!("openai-chat-{}", remap_tool_call_id(&call.id)),
                provider_id: Some(call.id),
                provider_item_id: None,
                name: call.name,
                arguments: serde_json::from_str(&call.arguments).unwrap_or(Value::Null),
                raw_arguments: call.arguments,
            }));
        }
        if parts.is_empty() && !matches!(self.reason, Some(FinishReason::Refusal(_))) {
            return Err(provider_error(ProviderError::EmptyResponse));
        }
        let mut raw = serde_json::Map::new();
        raw.insert("role".into(), json!("assistant"));
        raw.insert("content".into(), json!(self.content));
        if let Some(extra) = self.extra_content {
            raw.insert("extra_content".into(), Value::Object(extra));
        }
        if let Some(reasoning) = self.reasoning {
            raw.insert("reasoning_content".into(), json!(reasoning));
        }
        if !raw_calls.is_empty() {
            raw.insert("tool_calls".into(), json!(raw_calls));
        }
        Ok(AssistantTurn {
            parts,
            raw_items: RawItems {
                destination: self.destination,
                adapter: AdapterKind::OpenAiChat,
                items: vec![Value::Object(raw)],
            },
            finish_reason: self.reason.ok_or(OpenAiChatError::InvalidResponse)?,
            usage: normalize_usage(&self.usage),
        })
    }

    fn read_inner(&mut self, event: &SseEvent) -> Result<Vec<StreamEvent>, OpenAiChatError> {
        if !matches!(event.event.as_str(), "message" | "error") {
            return Ok(Vec::new());
        }
        if event.data == "[DONE]" {
            // 互換の口は道具を呼んでも stop を返す。完了の印まで届いてから
            // 道具の番にする（設計書 02-01「OpenAI の Chat Completions API」）。
            if self.reason == Some(FinishReason::Completed) && !self.calls.is_empty() {
                self.reason = Some(FinishReason::ToolCalls);
            }
            let reason = self
                .reason
                .clone()
                .ok_or(OpenAiChatError::InvalidResponse)?;
            self.finished = true;
            return Ok(vec![StreamEvent::Finished { reason }]);
        }
        let data: Value =
            serde_json::from_str(&event.data).map_err(|_| OpenAiChatError::InvalidResponse)?;
        if data.get("error").is_some() {
            return Err(api_error(&data, None, None));
        }
        let choices = data
            .get("choices")
            .and_then(Value::as_array)
            .ok_or(OpenAiChatError::InvalidResponse)?;
        let mut events = Vec::new();
        if let Some(choice) = choices.first() {
            if self.reason.is_some() {
                return Err(OpenAiChatError::InvalidResponse);
            }
            let delta = choice
                .get("delta")
                .filter(|d| d.is_object())
                .ok_or(OpenAiChatError::InvalidResponse)?;
            merge_extra_content(
                &mut self.extra_content,
                delta,
                &mut self.extra_content_bytes,
            )?;
            if let Some(text) = optional_string(delta, "content")? {
                self.content.get_or_insert_default().push_str(text);
                if !text.is_empty() {
                    match self.parts.last_mut() {
                        Some(AssistantPart::Text(value)) => value.push_str(text),
                        _ => self.parts.push(AssistantPart::Text(text.into())),
                    }
                    events.push(StreamEvent::TextDelta {
                        index: 0,
                        text: text.into(),
                    });
                }
            }
            for field in ["reasoning_content", "reasoning"] {
                if let Some(text) = optional_string(delta, field)? {
                    self.reasoning.get_or_insert_default().push_str(text);
                    if !text.is_empty() {
                        match self.parts.last_mut() {
                            Some(AssistantPart::Reasoning(value)) => value.text.push_str(text),
                            _ => self.parts.push(AssistantPart::Reasoning(Reasoning {
                                text: text.into(),
                                opaque: Value::Null,
                            })),
                        }
                        events.push(StreamEvent::ReasoningDelta {
                            index: 0,
                            text: text.into(),
                        });
                    }
                }
            }
            if let Some(calls) = delta.get("tool_calls").filter(|v| !v.is_null()) {
                for delta in calls.as_array().ok_or(OpenAiChatError::InvalidResponse)? {
                    self.read_call(delta)?;
                }
            }
            if let Some(reason) = optional_string(choice, "finish_reason")? {
                self.reason = Some(match reason {
                    "stop" => FinishReason::Completed,
                    "tool_calls" => FinishReason::ToolCalls,
                    "length" => FinishReason::OutputLimit,
                    "content_filter" => FinishReason::Refusal("content_filter".into()),
                    _ => FinishReason::Error,
                });
                self.end_calls(&mut events)?;
            }
        }
        if let Some(usage) = data.get("usage").filter(|v| !v.is_null()) {
            let usage = usage.as_object().ok_or(OpenAiChatError::InvalidResponse)?;
            if self.usage.is_null() {
                self.usage = json!({});
            }
            self.usage
                .as_object_mut()
                .ok_or(OpenAiChatError::InvalidResponse)?
                .extend(usage.clone());
            events.push(StreamEvent::Usage {
                usage: normalize_usage(&self.usage),
            });
        }
        Ok(events)
    }

    fn read_call(&mut self, delta: &Value) -> Result<(), OpenAiChatError> {
        let indexed = delta.get("index").is_some();
        if self
            .indexed_calls
            .is_some_and(|previous| previous != indexed)
        {
            return Err(OpenAiChatError::InvalidResponse);
        }
        self.indexed_calls = Some(indexed);
        let index = if indexed {
            delta
                .get("index")
                .and_then(Value::as_u64)
                .and_then(|i| usize::try_from(i).ok())
                .ok_or(OpenAiChatError::InvalidResponse)?
        } else {
            // 番号のない項目は完全な呼び出しであり、後続の断片とは結ばない
            // （設計書 02-01「OpenAI の Chat Completions API」）。
            if delta.get("type").and_then(Value::as_str) != Some("function")
                || optional_string(delta, "id")?.is_none_or(str::is_empty)
                || delta.get("function").is_none_or(|function| {
                    function
                        .get("name")
                        .and_then(Value::as_str)
                        .is_none_or(str::is_empty)
                        || function.get("arguments").and_then(Value::as_str).is_none()
                })
            {
                return Err(OpenAiChatError::InvalidResponse);
            }
            match self.calls.last_key_value() {
                Some((&last, _)) => last
                    .checked_add(1)
                    .ok_or(OpenAiChatError::InvalidResponse)?,
                None => 0,
            }
        };
        let call = self.calls.entry(index).or_default();
        merge_extra_content(
            &mut call.extra_content,
            delta,
            &mut self.extra_content_bytes,
        )?;
        if let Some(id) = optional_string(delta, "id")? {
            call.id.push_str(id);
        }
        if let Some(function) = delta.get("function").filter(|v| !v.is_null()) {
            if !function.is_object() {
                return Err(OpenAiChatError::InvalidResponse);
            }
            if let Some(name) = optional_string(function, "name")? {
                call.name.push_str(name);
            }
            if let Some(arguments) = optional_string(function, "arguments")? {
                call.arguments.push_str(arguments);
                call.deltas.push(arguments.into());
            }
        }
        Ok(())
    }

    fn end_calls(&self, events: &mut Vec<StreamEvent>) -> Result<(), OpenAiChatError> {
        for (&index, call) in &self.calls {
            if call.id.is_empty() || call.name.is_empty() {
                return Err(OpenAiChatError::InvalidResponse);
            }
            // id と名前も断片なので、finish_reason まで確定しない。未確定の名前で
            // Start を出さず、引数の差分も Start の後に渡す（実装プラン P1-03）。
            events.push(StreamEvent::ToolCallStart {
                index,
                provider_id: Some(call.id.clone()),
                provider_item_id: None,
                name: call.name.clone(),
            });
            events.extend(
                call.deltas
                    .iter()
                    .map(|delta| StreamEvent::ToolArgumentsDelta {
                        index,
                        delta: delta.clone(),
                    }),
            );
            events.push(StreamEvent::ToolCallEnd { index });
        }
        Ok(())
    }
}

fn merge_extra_content(
    target: &mut Option<Map<String, Value>>,
    delta: &Value,
    total_bytes: &mut usize,
) -> Result<(), OpenAiChatError> {
    if let Some(extra) = delta.get("extra_content").filter(|value| !value.is_null()) {
        check_extra_content_depth(extra, 0)?;
        let fields = extra.as_object().ok_or(OpenAiChatError::InvalidResponse)?;
        // 署名の中身を解釈せず、後の差分で同じ欄だけを置き換える
        // （設計書 02-01「共通の形」「OpenAI の Chat Completions API」）。
        let previous_bytes = target
            .as_ref()
            .map(serde_json::to_vec)
            .transpose()
            .map_err(|_| OpenAiChatError::InvalidResponse)?
            .map_or(0, |bytes| bytes.len());
        let mut merged = target.clone().unwrap_or_default();
        merged.extend(fields.clone());
        let merged_bytes = serde_json::to_vec(&merged)
            .map_err(|_| OpenAiChatError::InvalidResponse)?
            .len();
        // 差分と全呼び出しの保持量を合算し、上書きで消えた値は差し引く。
        // 記録を読み直せる深さと再送する量を制限する（設計書 02-01「OpenAI の Chat Completions API」）。
        let updated_bytes = total_bytes
            .checked_sub(previous_bytes)
            .and_then(|bytes| bytes.checked_add(merged_bytes))
            .filter(|&bytes| bytes <= MAX_EXTRA_CONTENT_BYTES)
            .ok_or(OpenAiChatError::InvalidResponse)?;
        *target = Some(merged);
        *total_bytes = updated_bytes;
    }
    Ok(())
}

fn check_extra_content_depth(value: &Value, parent_depth: usize) -> Result<(), OpenAiChatError> {
    let depth = parent_depth.saturating_add(1);
    match value {
        Value::Object(fields) if depth <= MAX_EXTRA_CONTENT_DEPTH => fields
            .values()
            .try_for_each(|value| check_extra_content_depth(value, depth)),
        Value::Array(values) if depth <= MAX_EXTRA_CONTENT_DEPTH => values
            .iter()
            .try_for_each(|value| check_extra_content_depth(value, depth)),
        Value::Object(_) | Value::Array(_) => Err(OpenAiChatError::InvalidResponse),
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => Ok(()),
    }
}

fn optional_string<'a>(value: &'a Value, name: &str) -> Result<Option<&'a str>, OpenAiChatError> {
    value
        .get(name)
        .filter(|v| !v.is_null())
        .map(|v| v.as_str().ok_or(OpenAiChatError::InvalidResponse))
        .transpose()
}

fn normalize_usage(raw: &Value) -> Usage {
    let mut usage = Usage {
        input_tokens: raw.get("prompt_tokens").and_then(Value::as_u64),
        cache_read_tokens: raw
            .pointer("/prompt_tokens_details/cached_tokens")
            .and_then(Value::as_u64),
        cache_write_tokens: None,
        output_tokens: raw.get("completion_tokens").and_then(Value::as_u64),
        reasoning_tokens: raw
            .pointer("/completion_tokens_details/reasoning_tokens")
            .and_then(Value::as_u64),
        raw: raw.clone(),
    };
    // Gemini は推論を total にだけ含める。明示された内数を二重に足さず、
    // 和が溢れる報告も補わない（設計書 02-01「OpenAI の Chat Completions API」）。
    if usage.reasoning_tokens.is_none()
        && let (Some(prompt), Some(completion), Some(total)) = (
            usage.input_tokens,
            usage.output_tokens,
            raw.get("total_tokens").and_then(Value::as_u64),
        )
        && let Some(sum) = prompt.checked_add(completion)
        && total > sum
    {
        usage.reasoning_tokens = total.checked_sub(sum);
        usage.output_tokens = total.checked_sub(prompt);
    }
    usage
}
