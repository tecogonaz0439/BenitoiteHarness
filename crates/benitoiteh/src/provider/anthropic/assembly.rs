//! SSE の差分から元の content block と共通の応答を作る（設計書 02-01「共通の形」）。

use serde_json::{Value, json};

use crate::provider::ids::remap_tool_call_id;
use crate::provider::sse::SseEvent;
use crate::provider::{
    AdapterKind, AssistantPart, AssistantTurn, Destination, FinishReason, ProviderError, RawItems,
    Reasoning, StreamEvent, ToolCall, Usage,
};

use super::{AnthropicError, api_error, provider_error};

/// 一回の要求に対応する組み立て器。Debug で受け取った本文を表示しない。
pub struct AnthropicAssembler {
    destination: Destination,
    blocks: Vec<Block>,
    usage: Value,
    reason: Option<FinishReason>,
    started: bool,
    finished: bool,
    failed: bool,
}

struct Block {
    index: usize,
    raw: Value,
    arguments: Option<String>,
    closed: bool,
}

impl std::fmt::Debug for AnthropicAssembler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AnthropicAssembler")
            .field("finished", &self.finished)
            .finish_non_exhaustive()
    }
}

impl AnthropicAssembler {
    /// 送り先を RawItems に記録する組み立て器を作る。
    pub fn new(destination: Destination) -> Self {
        Self {
            destination,
            blocks: Vec::new(),
            usage: Value::Null,
            reason: None,
            started: false,
            finished: false,
            failed: false,
        }
    }

    /// 一つの SSE イベントを読み、共通のイベントを受信順に返す。
    /// 未知のイベントは無視する。既知のイベントの不正な形では再開しない。
    pub fn read_event(&mut self, event: &SseEvent) -> Result<Vec<StreamEvent>, AnthropicError> {
        if self.failed || self.finished {
            return Err(AnthropicError::InvalidResponse);
        }
        let result = self.read_inner(event);
        if result.is_err() {
            self.failed = true;
        }
        result
    }

    /// message_stop まで確認したかを返す。
    pub fn is_finished(&self) -> bool {
        self.finished
    }

    /// 正常に受け取り終えた content block と共通の応答を取り出す。
    /// 未完了のストリームでは部分的な内容を返さない。
    pub fn finish(self) -> Result<AssistantTurn, AnthropicError> {
        if self.failed || !self.finished {
            return Err(provider_error(ProviderError::StreamDisconnected));
        }
        let mut parts = Vec::new();
        let mut items = Vec::new();
        for block in self.blocks {
            match string(&block.raw, "type")? {
                "text" => parts.push(AssistantPart::Text(string(&block.raw, "text")?.into())),
                "thinking" | "redacted_thinking" => {
                    parts.push(AssistantPart::Reasoning(Reasoning {
                        text: block
                            .raw
                            .get("thinking")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .into(),
                        opaque: block.raw.clone(),
                    }))
                }
                "tool_use" => {
                    let provider_id = string(&block.raw, "id")?.to_owned();
                    let raw_arguments = match block.arguments {
                        Some(raw) => raw,
                        None => serde_json::to_string(
                            block
                                .raw
                                .get("input")
                                .ok_or(AnthropicError::InvalidResponse)?,
                        )
                        .map_err(|_| AnthropicError::InvalidResponse)?,
                    };
                    let arguments = serde_json::from_str(&raw_arguments).unwrap_or(Value::Null);
                    parts.push(AssistantPart::ToolCall(ToolCall {
                        id: format!("anthropic-{}", remap_tool_call_id(&provider_id)),
                        provider_id: Some(provider_id),
                        provider_item_id: None,
                        name: string(&block.raw, "name")?.into(),
                        arguments,
                        raw_arguments,
                    }));
                }
                _ => {}
            }
            items.push(block.raw);
        }
        if parts.is_empty() && !matches!(self.reason, Some(FinishReason::Refusal(_))) {
            return Err(provider_error(ProviderError::EmptyResponse));
        }
        Ok(AssistantTurn {
            parts,
            raw_items: RawItems {
                destination: self.destination,
                adapter: AdapterKind::AnthropicMessages,
                items,
            },
            finish_reason: self.reason.ok_or(AnthropicError::InvalidResponse)?,
            usage: normalize_usage(&self.usage),
        })
    }

    fn read_inner(&mut self, event: &SseEvent) -> Result<Vec<StreamEvent>, AnthropicError> {
        // 新しいイベントを追加できる API のため、知らない event の本文は解釈しない。
        if !matches!(
            event.event.as_str(),
            "message_start"
                | "content_block_start"
                | "content_block_delta"
                | "content_block_stop"
                | "message_delta"
                | "message_stop"
                | "error"
        ) {
            return Ok(Vec::new());
        }
        let data: Value =
            serde_json::from_str(&event.data).map_err(|_| AnthropicError::InvalidResponse)?;
        if string(&data, "type")? != event.event {
            return Err(AnthropicError::InvalidResponse);
        }
        let mut events = Vec::new();
        if event.event == "error" {
            return Err(api_error(&data, None, None));
        }
        if event.event == "message_start" {
            if self.started {
                return Err(AnthropicError::InvalidResponse);
            }
            self.started = true;
            self.update_usage(data.pointer("/message/usage"), &mut events)?;
            return Ok(events);
        }
        if !self.started {
            return Err(AnthropicError::InvalidResponse);
        }
        match event.event.as_str() {
            "content_block_start" => {
                let index = index(&data)?;
                if self.blocks.iter().any(|b| b.index == index) {
                    return Err(AnthropicError::InvalidResponse);
                }
                let raw = data
                    .get("content_block")
                    .filter(|v| v.is_object())
                    .ok_or(AnthropicError::InvalidResponse)?
                    .clone();
                match string(&raw, "type")? {
                    "tool_use" => events.push(StreamEvent::ToolCallStart {
                        index,
                        provider_id: Some(string(&raw, "id")?.into()),
                        provider_item_id: None,
                        name: string(&raw, "name")?.into(),
                    }),
                    "text" => {
                        let text = string(&raw, "text")?;
                        if !text.is_empty() {
                            events.push(StreamEvent::TextDelta {
                                index,
                                text: text.into(),
                            });
                        }
                    }
                    "thinking" => {
                        let text = string(&raw, "thinking")?;
                        if !text.is_empty() {
                            events.push(StreamEvent::ReasoningDelta {
                                index,
                                text: text.into(),
                            });
                        }
                    }
                    _ => {}
                }
                self.blocks.push(Block {
                    index,
                    raw,
                    arguments: None,
                    closed: false,
                });
            }
            "content_block_delta" => {
                let index = index(&data)?;
                let block = self
                    .blocks
                    .iter_mut()
                    .find(|b| b.index == index && !b.closed)
                    .ok_or(AnthropicError::InvalidResponse)?;
                let delta = data.get("delta").ok_or(AnthropicError::InvalidResponse)?;
                match string(delta, "type")? {
                    "text_delta" => {
                        let text = string(delta, "text")?;
                        block.append("text", text)?;
                        events.push(StreamEvent::TextDelta {
                            index,
                            text: text.into(),
                        });
                    }
                    "thinking_delta" => {
                        let text = string(delta, "thinking")?;
                        block.append("thinking", text)?;
                        events.push(StreamEvent::ReasoningDelta {
                            index,
                            text: text.into(),
                        });
                    }
                    "signature_delta" => block.append("signature", string(delta, "signature")?)?,
                    "input_json_delta" => {
                        if string(&block.raw, "type")? != "tool_use" {
                            return Err(AnthropicError::InvalidResponse);
                        }
                        let delta = string(delta, "partial_json")?;
                        block.arguments.get_or_insert_default().push_str(delta);
                        events.push(StreamEvent::ToolArgumentsDelta {
                            index,
                            delta: delta.into(),
                        });
                    }
                    _ => {}
                }
            }
            "content_block_stop" => {
                let index = index(&data)?;
                let block = self
                    .blocks
                    .iter_mut()
                    .find(|b| b.index == index && !b.closed)
                    .ok_or(AnthropicError::InvalidResponse)?;
                block.closed = true;
                if string(&block.raw, "type")? == "tool_use" {
                    if let Some(raw) = &block.arguments {
                        // 不正な JSON を再送すると 400 になる。元の断片は共通の呼び出しに残す（実装プラン P1-02）。
                        let input = serde_json::from_str::<Value>(raw).unwrap_or(json!({}));
                        block
                            .raw
                            .as_object_mut()
                            .ok_or(AnthropicError::InvalidResponse)?
                            .insert("input".into(), input);
                    }
                    events.push(StreamEvent::ToolCallEnd { index });
                }
            }
            "message_delta" => {
                let delta = data.get("delta").ok_or(AnthropicError::InvalidResponse)?;
                if let Some(reason) = delta.get("stop_reason").and_then(Value::as_str) {
                    self.reason = Some(match reason {
                        "end_turn" | "stop_sequence" | "pause_turn" => FinishReason::Completed,
                        "tool_use" => FinishReason::ToolCalls,
                        "max_tokens" => FinishReason::OutputLimit,
                        "refusal" => FinishReason::Refusal(
                            delta
                                .pointer("/stop_details/category")
                                .or_else(|| data.pointer("/stop_details/category"))
                                .and_then(Value::as_str)
                                .unwrap_or_default()
                                .into(),
                        ),
                        _ => FinishReason::Error,
                    });
                }
                self.update_usage(data.get("usage"), &mut events)?;
            }
            "message_stop" => {
                if self.blocks.iter().any(|b| !b.closed) {
                    return Err(AnthropicError::InvalidResponse);
                }
                let reason = self.reason.clone().ok_or(AnthropicError::InvalidResponse)?;
                self.finished = true;
                events.push(StreamEvent::Finished { reason });
            }
            _ => {}
        }
        Ok(events)
    }

    fn update_usage(
        &mut self,
        usage: Option<&Value>,
        events: &mut Vec<StreamEvent>,
    ) -> Result<(), AnthropicError> {
        if let Some(usage) = usage.filter(|v| !v.is_null()) {
            let usage = usage.as_object().ok_or(AnthropicError::InvalidResponse)?;
            if self.usage.is_null() {
                self.usage = json!({});
            }
            // message_delta の値は累計なので、差分として足さず、報告された欄だけ置き換える。
            self.usage
                .as_object_mut()
                .ok_or(AnthropicError::InvalidResponse)?
                .extend(usage.clone());
            events.push(StreamEvent::Usage {
                usage: normalize_usage(&self.usage),
            });
        }
        Ok(())
    }
}

impl Block {
    fn append(&mut self, field: &str, delta: &str) -> Result<(), AnthropicError> {
        let fields = self
            .raw
            .as_object_mut()
            .ok_or(AnthropicError::InvalidResponse)?;
        let value = fields.entry(field.to_owned()).or_insert_with(|| json!(""));
        let Value::String(text) = value else {
            return Err(AnthropicError::InvalidResponse);
        };
        text.push_str(delta);
        Ok(())
    }
}

fn string<'a>(value: &'a Value, field: &str) -> Result<&'a str, AnthropicError> {
    value
        .get(field)
        .and_then(Value::as_str)
        .ok_or(AnthropicError::InvalidResponse)
}

fn index(value: &Value) -> Result<usize, AnthropicError> {
    value
        .get("index")
        .and_then(Value::as_u64)
        .and_then(|n| usize::try_from(n).ok())
        .ok_or(AnthropicError::InvalidResponse)
}

fn normalize_usage(raw: &Value) -> Usage {
    let input = raw.get("input_tokens").and_then(Value::as_u64);
    let read = raw.get("cache_read_input_tokens").and_then(Value::as_u64);
    let write = raw
        .get("cache_creation_input_tokens")
        .and_then(Value::as_u64);
    Usage {
        input_tokens: input.map(|n| {
            n.saturating_add(read.unwrap_or_default())
                .saturating_add(write.unwrap_or_default())
        }),
        cache_read_tokens: read,
        cache_write_tokens: write,
        output_tokens: raw.get("output_tokens").and_then(Value::as_u64),
        reasoning_tokens: None,
        raw: raw.clone(),
    }
}
