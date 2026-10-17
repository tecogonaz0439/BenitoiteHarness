//! Responses の確定した項目を順序どおりに保つ（設計書 02-01「OpenAI の Responses API」）。

use std::collections::BTreeMap;

use serde_json::{Value, json};

use crate::provider::ids::remap_tool_call_id;
use crate::provider::sse::SseEvent;
use crate::provider::{
    AdapterKind, AssistantPart, AssistantTurn, Destination, FinishReason, ProviderError, RawItems,
    Reasoning, StreamEvent, ToolCall, Usage,
};

use super::{OpenAiResponsesError, api_error, provider_error};

/// 差分を表示用のイベントにし、done の項目から応答を組み立てる。
pub struct OpenAiResponsesAssembler {
    destination: Destination,
    items: BTreeMap<usize, OutputItem>,
    parts: Vec<AssistantPart>,
    usage: Usage,
    reason: Option<FinishReason>,
    refusal: bool,
    failed: bool,
}

struct OutputItem {
    added: Value,
    done: Option<Value>,
}

impl std::fmt::Debug for OpenAiResponsesAssembler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OpenAiResponsesAssembler")
            .field("finished", &self.is_finished())
            .finish_non_exhaustive()
    }
}

impl OpenAiResponsesAssembler {
    /// 送り先を元の項目とともに記録する組み立て器を作る。
    pub fn new(destination: Destination) -> Self {
        Self {
            destination,
            items: BTreeMap::new(),
            parts: Vec::new(),
            usage: Usage::default(),
            reason: None,
            refusal: false,
            failed: false,
        }
    }

    /// SSE の data の type で振り分ける。不正な応答の後は再開しない。
    pub fn read_event(
        &mut self,
        event: &SseEvent,
    ) -> Result<Vec<StreamEvent>, OpenAiResponsesError> {
        if self.failed || self.is_finished() {
            return Err(OpenAiResponsesError::InvalidResponse);
        }
        let result = self.read_inner(event);
        if result.is_err() {
            self.failed = true;
        }
        result
    }

    /// 応答全体の終端イベントを確かめたかを返す。
    pub fn is_finished(&self) -> bool {
        self.reason.is_some()
    }

    /// 正常に受け取った応答だけを取り出す。
    pub fn finish(self) -> Result<AssistantTurn, OpenAiResponsesError> {
        if self.failed || !self.is_finished() {
            return Err(provider_error(ProviderError::StreamDisconnected));
        }
        if self.parts.is_empty() && !matches!(self.reason, Some(FinishReason::Refusal(_))) {
            return Err(provider_error(ProviderError::EmptyResponse));
        }
        Ok(AssistantTurn {
            parts: self.parts,
            raw_items: RawItems {
                destination: self.destination,
                adapter: AdapterKind::OpenAiResponses,
                items: self
                    .items
                    .into_values()
                    .map(|i| i.done.ok_or(OpenAiResponsesError::InvalidResponse))
                    .collect::<Result<_, _>>()?,
            },
            finish_reason: self.reason.ok_or(OpenAiResponsesError::InvalidResponse)?,
            usage: self.usage,
        })
    }

    fn read_inner(&mut self, event: &SseEvent) -> Result<Vec<StreamEvent>, OpenAiResponsesError> {
        if event.data == "[DONE]" {
            return Ok(Vec::new());
        }
        let data: Value =
            serde_json::from_str(&event.data).map_err(|_| OpenAiResponsesError::InvalidResponse)?;
        let kind = string(&data, "type")?;
        let mut events = Vec::new();
        match kind {
            "error" | "response.failed" => {
                // ストリームの誤りは HTTP と包みが違う。符号と文を同じ分類へ渡す。
                let error = if kind == "error" {
                    &data
                } else {
                    data.pointer("/response/error")
                        .ok_or(OpenAiResponsesError::InvalidResponse)?
                };
                return Err(api_error(&json!({"error": error}), None, None));
            }
            "response.output_item.added" => {
                let index = output_index(&data)?;
                let item = data
                    .get("item")
                    .filter(|i| i.is_object())
                    .ok_or(OpenAiResponsesError::InvalidResponse)?;
                if self.items.contains_key(&index) {
                    return Err(OpenAiResponsesError::InvalidResponse);
                }
                if string(item, "type")? == "function_call" {
                    events.push(StreamEvent::ToolCallStart {
                        index,
                        provider_id: Some(string(item, "call_id")?.into()),
                        provider_item_id: optional_id(item)?,
                        name: string(item, "name")?.into(),
                    });
                }
                self.items.insert(
                    index,
                    OutputItem {
                        added: item.clone(),
                        done: None,
                    },
                );
            }
            "response.output_item.done" => {
                let index = output_index(&data)?;
                let item = data
                    .get("item")
                    .filter(|i| i.is_object())
                    .ok_or(OpenAiResponsesError::InvalidResponse)?;
                let state = self
                    .items
                    .get_mut(&index)
                    .ok_or(OpenAiResponsesError::InvalidResponse)?;
                let kind = string(item, "type")?;
                if state.done.is_some() || string(&state.added, "type")? != kind {
                    return Err(OpenAiResponsesError::InvalidResponse);
                }
                if kind == "function_call" {
                    for field in ["call_id", "name"] {
                        if string(&state.added, field)? != string(item, field)? {
                            return Err(OpenAiResponsesError::InvalidResponse);
                        }
                    }
                    string(item, "arguments")?;
                    events.push(StreamEvent::ToolCallEnd { index });
                }
                state.done = Some(item.clone());
            }
            "response.output_text.delta"
            | "response.reasoning_summary_text.delta"
            | "response.function_call_arguments.delta" => {
                let index = output_index(&data)?;
                let item = self
                    .items
                    .get(&index)
                    .ok_or(OpenAiResponsesError::InvalidResponse)?;
                let expected = match kind {
                    "response.output_text.delta" => "message",
                    "response.reasoning_summary_text.delta" => "reasoning",
                    _ => "function_call",
                };
                if item.done.is_some() || string(&item.added, "type")? != expected {
                    return Err(OpenAiResponsesError::InvalidResponse);
                }
                let delta = string(&data, "delta")?.to_owned();
                events.push(match kind {
                    "response.output_text.delta" => StreamEvent::TextDelta { index, text: delta },
                    "response.reasoning_summary_text.delta" => {
                        StreamEvent::ReasoningDelta { index, text: delta }
                    }
                    _ => StreamEvent::ToolArgumentsDelta { index, delta },
                });
            }
            "response.refusal.delta" => self.refusal = true,
            "response.completed" | "response.incomplete" => {
                let response = data
                    .get("response")
                    .filter(|r| r.is_object())
                    .ok_or(OpenAiResponsesError::InvalidResponse)?;
                // added の不完全な暗号化内容や差分の引数を、確定した内容として使わない。
                // 終端までに done がない項目は応答の形の誤り（設計書 02-01「OpenAI の Responses API」）。
                for state in self.items.values() {
                    let item = state
                        .done
                        .as_ref()
                        .ok_or(OpenAiResponsesError::InvalidResponse)?;
                    append_parts(item, &mut self.parts, &mut self.refusal)?;
                }
                let reason = if self.refusal {
                    FinishReason::Refusal("refusal".into())
                } else if kind == "response.incomplete" {
                    match response
                        .pointer("/incomplete_details/reason")
                        .and_then(Value::as_str)
                    {
                        Some("max_output_tokens") => FinishReason::OutputLimit,
                        Some("content_filter") => FinishReason::Refusal("content_filter".into()),
                        _ => FinishReason::Error,
                    }
                } else if self
                    .parts
                    .iter()
                    .any(|p| matches!(p, AssistantPart::ToolCall(_)))
                {
                    FinishReason::ToolCalls
                } else {
                    FinishReason::Completed
                };
                if let Some(raw) = response.get("usage").filter(|v| !v.is_null()) {
                    if !raw.is_object() {
                        return Err(OpenAiResponsesError::InvalidResponse);
                    }
                    self.usage = normalize_usage(raw);
                    events.push(StreamEvent::Usage {
                        usage: self.usage.clone(),
                    });
                }
                self.reason = Some(reason.clone());
                events.push(StreamEvent::Finished { reason });
            }
            // 推論の生の文、注釈、将来追加されるイベントは表示にも記録にも使わない。
            _ => {}
        }
        Ok(events)
    }
}

fn append_parts(
    item: &Value,
    parts: &mut Vec<AssistantPart>,
    refusal: &mut bool,
) -> Result<(), OpenAiResponsesError> {
    match string(item, "type")? {
        "message" => {
            for content in item
                .get("content")
                .and_then(Value::as_array)
                .ok_or(OpenAiResponsesError::InvalidResponse)?
            {
                match string(content, "type")? {
                    "output_text" => {
                        parts.push(AssistantPart::Text(string(content, "text")?.into()))
                    }
                    "refusal" => *refusal = true,
                    _ => {}
                }
            }
        }
        "reasoning" => {
            let summary = item
                .get("summary")
                .and_then(Value::as_array)
                .ok_or(OpenAiResponsesError::InvalidResponse)?;
            let text = summary
                .iter()
                .map(|s| string(s, "text"))
                .collect::<Result<Vec<_>, _>>()?
                .join("\n\n");
            parts.push(AssistantPart::Reasoning(Reasoning {
                text,
                opaque: item
                    .get("encrypted_content")
                    .cloned()
                    .unwrap_or(Value::Null),
            }));
        }
        "function_call" => {
            let call_id = string(item, "call_id")?;
            let arguments = string(item, "arguments")?;
            parts.push(AssistantPart::ToolCall(ToolCall {
                id: format!("openai-responses-{}", remap_tool_call_id(call_id)),
                provider_id: Some(call_id.into()),
                provider_item_id: optional_id(item)?,
                name: string(item, "name")?.into(),
                arguments: serde_json::from_str(arguments).unwrap_or(Value::Null),
                raw_arguments: arguments.into(),
            }));
        }
        _ => {}
    }
    Ok(())
}

fn string<'a>(value: &'a Value, field: &str) -> Result<&'a str, OpenAiResponsesError> {
    value
        .get(field)
        .and_then(Value::as_str)
        .ok_or(OpenAiResponsesError::InvalidResponse)
}

fn output_index(data: &Value) -> Result<usize, OpenAiResponsesError> {
    data.get("output_index")
        .and_then(Value::as_u64)
        .and_then(|i| usize::try_from(i).ok())
        .ok_or(OpenAiResponsesError::InvalidResponse)
}

fn optional_id(item: &Value) -> Result<Option<String>, OpenAiResponsesError> {
    item.get("id")
        .filter(|v| !v.is_null())
        .map(|v| {
            v.as_str()
                .map(str::to_owned)
                .ok_or(OpenAiResponsesError::InvalidResponse)
        })
        .transpose()
}

fn normalize_usage(raw: &Value) -> Usage {
    Usage {
        input_tokens: raw.get("input_tokens").and_then(Value::as_u64),
        cache_read_tokens: raw
            .pointer("/input_tokens_details/cached_tokens")
            .and_then(Value::as_u64),
        cache_write_tokens: raw
            .pointer("/input_tokens_details/cache_write_tokens")
            .and_then(Value::as_u64),
        output_tokens: raw.get("output_tokens").and_then(Value::as_u64),
        reasoning_tokens: raw
            .pointer("/output_tokens_details/reasoning_tokens")
            .and_then(Value::as_u64),
        raw: raw.clone(),
    }
}
