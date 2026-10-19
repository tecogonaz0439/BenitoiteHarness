//! 模擬 HTTP サーバーを通した Messages API の契約（設計書 02-01、07-01「持ち主の境界」）。

mod support;

use std::error::Error;
use std::io;
use std::time::{Duration, Instant};

use benitoiteh::http::{HttpClient, HttpTimeouts};
use benitoiteh::provider::anthropic::{
    AnthropicAdapter, AnthropicError, AnthropicStream, RequestOptions,
};
use benitoiteh::provider::{
    AdapterKind, ArgumentSpec, ArgumentType, AssistantPart, AssistantTurn, Capabilities,
    Conversation, Destination, FinishReason, Item, NonRetryableKind, ProviderError,
    ReasoningReplay, StreamEvent, ToolResult, ToolSpec, UserMessage, UserPart, UserPartKind,
};
use serde_json::{Value, json};
use support::mock_provider::{Delivery, MockProvider, ResponseScript};

const KEY: &str = "synthetic-test-key";
type TestResult = Result<(), Box<dyn Error>>;

fn destination(server: &MockProvider) -> Destination {
    Destination {
        provider: "mock".into(),
        base_url: server.base_url(),
        model: "mock-model".into(),
    }
}

fn capabilities() -> Result<Capabilities, Box<dyn Error>> {
    AdapterKind::AnthropicMessages
        .official_capabilities()
        .ok_or_else(|| io::Error::from(io::ErrorKind::InvalidData).into())
}

fn conversation() -> Conversation {
    Conversation {
        items: vec![Item::UserMessage(UserMessage {
            parts: vec![
                UserPart {
                    kind: UserPartKind::Environment,
                    text: "Environment".into(),
                },
                UserPart {
                    kind: UserPartKind::Request,
                    text: "Request".into(),
                },
            ],
        })],
    }
}

fn tools() -> Vec<ToolSpec> {
    vec![ToolSpec {
        name: "read".into(),
        description: "Read a file.".into(),
        arguments: vec![ArgumentSpec {
            name: "path".into(),
            kind: ArgumentType::String,
            required: true,
            description: "File path.".into(),
        }],
    }]
}

fn client() -> HttpClient {
    HttpClient::new(HttpTimeouts {
        connect: Duration::from_secs(1),
        headers: Duration::from_secs(2),
        idle: Duration::from_secs(2),
        total: Duration::from_secs(5),
    })
}

// 台本は公式の SSE の例と同じ形で手書きし、アダプタで生成しない（設計書 07-01「模擬の提供者」）。
fn sse(data: &[Value]) -> Result<Vec<u8>, Box<dyn Error>> {
    let mut body = String::new();
    for value in data {
        let event = value
            .get("type")
            .and_then(Value::as_str)
            .ok_or(io::Error::from(io::ErrorKind::InvalidData))?;
        body.push_str(&format!("event: {event}\ndata: {value}\n\n"));
    }
    Ok(body.into_bytes())
}

fn simple(reason: &str, usage: Option<Value>) -> Result<Vec<u8>, Box<dyn Error>> {
    sse(&[
        json!({"type":"message_start", "message":{"usage":usage}}),
        json!({"type":"content_block_start", "index":0, "content_block":{"type":"text", "text":""}}),
        json!({"type":"content_block_delta", "index":0, "delta":{"type":"text_delta", "text":"Hello"}}),
        json!({"type":"content_block_stop", "index":0}),
        json!({"type":"message_delta", "delta":{"stop_reason":reason, "stop_details":{"category":"test-category"}}}),
        json!({"type":"message_stop"}),
    ])
}

fn interleaved() -> Result<Vec<u8>, Box<dyn Error>> {
    sse(&[
        json!({"type":"message_start", "message":{"id":"msg_test", "usage":{"input_tokens":100, "cache_read_input_tokens":200, "cache_creation_input_tokens":30, "output_tokens":1, "future":{"count":9}}}}),
        json!({"type":"ping"}),
        json!({"type":"content_block_start", "index":0, "content_block":{"type":"text", "text":"Before"}}),
        json!({"type":"content_block_delta", "index":0, "delta":{"type":"text_delta", "text":" 日本語"}}),
        json!({"type":"content_block_stop", "index":0}),
        json!({"type":"content_block_start", "index":1, "content_block":{"type":"thinking", "thinking":"", "signature":""}}),
        json!({"type":"content_block_delta", "index":1, "delta":{"type":"thinking_delta", "thinking":"Plan"}}),
        json!({"type":"content_block_delta", "index":1, "delta":{"type":"signature_delta", "signature":"opaque+/"}}),
        json!({"type":"content_block_delta", "index":1, "delta":{"type":"signature_delta", "signature":"="}}),
        json!({"type":"content_block_stop", "index":1}),
        json!({"type":"content_block_start", "index":2, "content_block":{"type":"tool_use", "id":"toolu_A", "name":"read", "input":{}}}),
        json!({"type":"content_block_delta", "index":2, "delta":{"type":"input_json_delta", "partial_json":"{ \"path\":"}}),
        json!({"type":"content_block_delta", "index":2, "delta":{"type":"input_json_delta", "partial_json":"\"a.bnt\" }"}}),
        json!({"type":"content_block_stop", "index":2}),
        json!({"type":"content_block_start", "index":3, "content_block":{"type":"redacted_thinking", "data":"encrypted+/="}}),
        json!({"type":"content_block_stop", "index":3}),
        json!({"type":"content_block_start", "index":4, "content_block":{"type":"text", "text":"After"}}),
        json!({"type":"content_block_stop", "index":4}),
        json!({"type":"content_block_start", "index":5, "content_block":{"type":"tool_use", "id":"toolu_B", "name":"read", "input":{}}}),
        json!({"type":"content_block_delta", "index":5, "delta":{"type":"input_json_delta", "partial_json":"{broken"}}),
        json!({"type":"content_block_stop", "index":5}),
        json!({"type":"message_delta", "delta":{"stop_reason":null}, "usage":{"output_tokens":8}}),
        json!({"type":"message_delta", "delta":{"stop_reason":"tool_use"}, "usage":{"output_tokens":11}}),
        json!({"type":"message_stop"}),
    ])
}

fn receive(
    mut stream: AnthropicStream,
) -> Result<(Vec<StreamEvent>, AssistantTurn), AnthropicError> {
    let mut events = Vec::new();
    let started = Instant::now();
    loop {
        if started.elapsed() > Duration::from_secs(6) {
            return Err(AnthropicError::InvalidResponse);
        }
        let next = stream.poll(Duration::from_millis(20))?;
        let ended = next
            .iter()
            .any(|e| matches!(e, StreamEvent::Finished { .. }));
        events.extend(next);
        if ended {
            return Ok((events, stream.finish()?));
        }
    }
}

fn request(
    adapter: &AnthropicAdapter,
    conversation: &Conversation,
    options: RequestOptions<'_>,
) -> Result<(Vec<StreamEvent>, AssistantTurn), AnthropicError> {
    let request = adapter.build_request(conversation, "System", &tools(), options)?;
    receive(adapter.stream(&client(), &request)?)
}

#[test]
fn interleaved_blocks_survive_http_assembly_and_same_destination_replay() -> TestResult {
    let mut response = ResponseScript::sse(interleaved()?);
    response.delivery = Delivery::Chunks {
        size: 13,
        delay: Duration::from_micros(50),
    };
    let mut server = MockProvider::start(vec![
        response,
        ResponseScript::sse(simple("end_turn", None)?),
    ])?;
    let adapter = AnthropicAdapter::new(destination(&server), capabilities()?, KEY.into());
    let (events, turn) = request(&adapter, &conversation(), RequestOptions::default())?;
    let expected = vec![
        json!({"type":"text", "text":"Before 日本語"}),
        json!({"type":"thinking", "thinking":"Plan", "signature":"opaque+/="}),
        json!({"type":"tool_use", "id":"toolu_A", "name":"read", "input":{"path":"a.bnt"}}),
        json!({"type":"redacted_thinking", "data":"encrypted+/="}),
        json!({"type":"text", "text":"After"}),
        json!({"type":"tool_use", "id":"toolu_B", "name":"read", "input":{}}),
    ];
    assert_eq!(turn.raw_items.items, expected);
    assert_eq!(turn.raw_items.destination, destination(&server));
    assert_eq!(turn.raw_items.adapter, AdapterKind::AnthropicMessages);
    assert_eq!(turn.finish_reason, FinishReason::ToolCalls);
    assert_eq!(turn.usage.input_tokens, Some(330));
    assert_eq!(turn.usage.cache_read_tokens, Some(200));
    assert_eq!(turn.usage.cache_write_tokens, Some(30));
    assert_eq!(turn.usage.output_tokens, Some(11));
    assert_eq!(turn.usage.reasoning_tokens, None);
    assert_eq!(
        turn.usage.raw,
        json!({"input_tokens":100, "cache_read_input_tokens":200, "cache_creation_input_tokens":30, "output_tokens":11, "future":{"count":9}})
    );
    assert!(matches!(turn.parts.first(), Some(AssistantPart::Text(t)) if t == "Before 日本語"));
    assert!(
        matches!(turn.parts.get(1), Some(AssistantPart::Reasoning(r)) if r.text == "Plan" && r.opaque == json!({"type":"thinking","thinking":"Plan","signature":"opaque+/="}))
    );
    assert!(
        matches!(turn.parts.get(3), Some(AssistantPart::Reasoning(r)) if r.text.is_empty() && r.opaque == json!({"type":"redacted_thinking","data":"encrypted+/="}))
    );
    assert!(matches!(turn.parts.get(4), Some(AssistantPart::Text(t)) if t == "After"));
    assert_eq!(turn.parts.len(), 6);
    let visible: Vec<_> = events
        .into_iter()
        .filter(|e| !matches!(e, StreamEvent::Usage { .. }))
        .collect();
    assert_eq!(
        visible,
        vec![
            StreamEvent::TextDelta {
                index: 0,
                text: "Before".into()
            },
            StreamEvent::TextDelta {
                index: 0,
                text: " 日本語".into()
            },
            StreamEvent::ReasoningDelta {
                index: 1,
                text: "Plan".into()
            },
            StreamEvent::ToolCallStart {
                index: 2,
                provider_id: Some("toolu_A".into()),
                provider_item_id: None,
                name: "read".into()
            },
            StreamEvent::ToolArgumentsDelta {
                index: 2,
                delta: "{ \"path\":".into()
            },
            StreamEvent::ToolArgumentsDelta {
                index: 2,
                delta: "\"a.bnt\" }".into()
            },
            StreamEvent::ToolCallEnd { index: 2 },
            StreamEvent::TextDelta {
                index: 4,
                text: "After".into()
            },
            StreamEvent::ToolCallStart {
                index: 5,
                provider_id: Some("toolu_B".into()),
                provider_item_id: None,
                name: "read".into()
            },
            StreamEvent::ToolArgumentsDelta {
                index: 5,
                delta: "{broken".into()
            },
            StreamEvent::ToolCallEnd { index: 5 },
            StreamEvent::Finished {
                reason: FinishReason::ToolCalls
            },
        ]
    );
    let mut history = conversation();
    history.items.push(Item::AssistantTurn(turn.clone()));
    for part in &turn.parts {
        if let AssistantPart::ToolCall(call) = part {
            assert!(call.provider_item_id.is_none());
            assert!(!call.id.is_empty());
            if call.provider_id.as_deref() == Some("toolu_A") {
                assert_eq!(call.arguments, json!({"path":"a.bnt"}));
                assert_eq!(call.raw_arguments, "{ \"path\":\"a.bnt\" }");
            } else {
                assert_eq!(call.provider_id.as_deref(), Some("toolu_B"));
                assert_eq!(call.arguments, Value::Null);
                assert_eq!(call.raw_arguments, "{broken");
            }
            history.items.push(Item::ToolResult(ToolResult {
                id: call.id.clone(),
                provider_id: call.provider_id.clone(),
                text: "result".into(),
                is_error: call.arguments.is_null(),
                images: Vec::new(),
            }));
        }
    }
    request(
        &adapter,
        &history,
        RequestOptions {
            previous_last_item: Some(0),
            ..RequestOptions::default()
        },
    )?;
    let records = server.requests()?;
    let sent = records
        .get(1)
        .ok_or(io::Error::from(io::ErrorKind::NotFound))?;
    assert_eq!(
        sent.body.pointer("/messages/1/content"),
        Some(&json!(expected))
    );
    assert_eq!(
        sent.body.pointer("/messages/2/content"),
        Some(&json!([
            {"type":"tool_result","tool_use_id":"toolu_A","content":"result","is_error":false},
            {"type":"tool_result","tool_use_id":"toolu_B","content":"result","is_error":true,"cache_control":{"type":"ephemeral"}},
        ]))
    );
    assert_eq!(
        sent.body.pointer("/system/0/cache_control"),
        Some(&json!({"type":"ephemeral"}))
    );
    assert_eq!(
        sent.body.pointer("/messages/0/content/1/cache_control"),
        Some(&json!({"type":"ephemeral"}))
    );
    assert_eq!(cache_markers(&sent.body), 3);
    assert_eq!(history.items.get(1), Some(&Item::AssistantTurn(turn)));
    server.shutdown()?;
    Ok(())
}

fn cache_markers(value: &Value) -> usize {
    match value {
        Value::Object(map) => usize::from(map.contains_key("cache_control"))
            .saturating_add(map.values().map(cache_markers).sum()),
        Value::Array(array) => array.iter().map(cache_markers).sum(),
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => 0,
    }
}

#[test]
fn request_flags_follow_capabilities_and_effort_is_always_explicit() -> TestResult {
    let cases = [
        (true, true, true),
        (false, true, true),
        (true, false, true),
        (true, true, false),
        (false, false, false),
    ];
    let mut server = MockProvider::start(
        cases
            .iter()
            .map(|_| simple("end_turn", None).map(ResponseScript::sse))
            .collect::<Result<_, _>>()?,
    )?;
    for (index, (cache, strict, eager)) in cases.into_iter().enumerate() {
        let adapter = AnthropicAdapter::new(
            Destination {
                base_url: format!("{}/gateway/", server.base_url()),
                ..destination(&server)
            },
            Capabilities {
                cache_control: cache,
                strict_tools: strict,
                eager_input_streaming: eager,
                tool_choice_none: true,
                max_output_tokens: if index == 0 { Some(12345) } else { None },
                ..Capabilities::default()
            },
            KEY.into(),
        );
        request(
            &adapter,
            &conversation(),
            RequestOptions {
                effort: if index == 0 { Some("medium") } else { None },
                disable_tools: index == 0,
                ..RequestOptions::default()
            },
        )?;
        let records = server.requests()?;
        let record = records
            .get(index)
            .ok_or(io::Error::from(io::ErrorKind::NotFound))?;
        assert_eq!(record.method, "POST");
        assert_eq!(record.path, "/gateway/v1/messages");
        assert!(
            record
                .headers
                .get("x-api-key")
                .is_some_and(|key| key == KEY)
        );
        assert_eq!(
            record.headers.get("anthropic-version").map(String::as_str),
            Some("2023-06-01")
        );
        assert_eq!(
            record.headers.get("content-type").map(String::as_str),
            Some("application/json")
        );
        let body = &record.body;
        assert_eq!(body.get("model"), Some(&json!("mock-model")));
        assert_eq!(body.get("stream"), Some(&json!(true)));
        assert_eq!(body.get("thinking"), Some(&json!({"type":"adaptive"})));
        assert_eq!(
            body.pointer("/output_config/effort"),
            Some(&json!(if index == 0 { "medium" } else { "low" }))
        );
        assert_eq!(
            body.get("max_tokens"),
            Some(&json!(if index == 0 { 12345 } else { 32000 }))
        );
        assert_eq!(
            body.get("tool_choice"),
            if index == 0 {
                Some(json!({"type":"none"}))
            } else {
                None
            }
            .as_ref()
        );
        assert_eq!(
            body.pointer("/tools/0/strict"),
            strict.then_some(json!(true)).as_ref()
        );
        assert_eq!(
            body.pointer("/tools/0/eager_input_streaming"),
            eager.then_some(json!(true)).as_ref()
        );
        assert_eq!(body.pointer("/tools/0/name"), Some(&json!("read")));
        assert_eq!(
            body.pointer("/tools/0/input_schema/required"),
            Some(&json!(["path"]))
        );
        assert_eq!(
            body.pointer("/tools/0/input_schema/additionalProperties"),
            strict.then_some(json!(false)).as_ref()
        );
        assert_eq!(cache_markers(body), if cache { 2 } else { 0 });
        assert_eq!(
            body.pointer("/messages/0/content/0/text"),
            Some(&json!("Environment"))
        );
        assert_eq!(
            body.pointer("/messages/0/content/1/text"),
            Some(&json!("Request"))
        );
    }
    server.shutdown()?;
    Ok(())
}

#[test]
fn changed_destinations_remap_call_and_result_ids_and_never_replay_reasoning() -> TestResult {
    let scripts = (0..5)
        .map(|_| simple("end_turn", None).map(ResponseScript::sse))
        .collect::<Result<_, _>>()?;
    let mut server = MockProvider::start(scripts)?;
    let original = destination(&server);
    // Responses の長い ID を使い、別の二つの ID の区切りを削るだけの変換を検出する。
    let foreign: AssistantTurn = serde_json::from_value(json!({
        "parts":[
            {"kind":"text","content":"Before"},
            {"kind":"reasoning","content":{"text":"Private plan","opaque":{"signature":"signed"}}},
            {"kind":"tool_call","content":{"id":"internal-a","provider_id":"ab|c","provider_item_id":"fc_a","name":"read","arguments":{"path":"a"},"raw_arguments":"{\"path\":\"a\"}"}},
            {"kind":"tool_call","content":{"id":"internal-b","provider_id":"a|bc","provider_item_id":"fc_b","name":"read","arguments":{"path":"b"},"raw_arguments":"{\"path\":\"b\"}"}}
        ],
        "raw_items":{"destination":original,"adapter":"openai-responses","items":[{"type":"reasoning","encrypted_content":"private"}]},
        "finish_reason":{"kind":"tool_calls"},
        "usage":{"input_tokens":null,"cache_read_tokens":null,"cache_write_tokens":null,"output_tokens":null,"reasoning_tokens":null,"raw":null}
    }))?;
    let mut history = conversation();
    history.items.push(Item::AssistantTurn(foreign));
    for (internal, provider) in [("internal-a", "ab|c"), ("internal-b", "a|bc")] {
        history.items.push(Item::ToolResult(ToolResult {
            id: internal.into(),
            provider_id: Some(provider.into()),
            text: "ok".into(),
            is_error: false,
            images: Vec::new(),
        }));
    }
    for index in 0..5 {
        let mut target = original.clone();
        match index {
            0 => {}
            1 => target.provider = "changed".into(),
            2 => target.base_url.push('/'),
            3 => target.model = "changed".into(),
            _ => {}
        }
        // 0 は別アダプタ、1〜3 は送り先の各要素、4 は推論を送らない機能の設定。
        if index > 0
            && let Some(Item::AssistantTurn(turn)) = history.items.get_mut(1)
        {
            turn.raw_items.adapter = AdapterKind::AnthropicMessages;
            turn.raw_items.items = vec![
                json!({"type":"text","text":"Before"}),
                json!({"type":"thinking","thinking":"Private plan","signature":"signed"}),
                json!({"type":"tool_use","id":"ab|c","name":"read","input":{"path":"a"}}),
                json!({"type":"tool_use","id":"a|bc","name":"read","input":{"path":"b"}}),
            ];
        }
        let caps = Capabilities {
            reasoning_replay: if index == 4 {
                ReasoningReplay::None
            } else {
                ReasoningReplay::SameDestination
            },
            ..Capabilities::default()
        };
        let adapter = AnthropicAdapter::new(target, caps, KEY.into());
        request(&adapter, &history, RequestOptions::default())?;
        let records = server.requests()?;
        let sent = &records
            .get(index)
            .ok_or(io::Error::from(io::ErrorKind::NotFound))?
            .body;
        let blocks = sent
            .pointer("/messages/1/content")
            .and_then(Value::as_array)
            .ok_or(io::Error::from(io::ErrorKind::InvalidData))?;
        assert_eq!(blocks.len(), 3);
        assert_eq!(
            blocks.first(),
            Some(&json!({"type":"text","text":"Before"}))
        );
        let a = blocks
            .get(1)
            .and_then(|b| b.get("id"))
            .and_then(Value::as_str)
            .ok_or(io::Error::from(io::ErrorKind::InvalidData))?;
        let b = blocks
            .get(2)
            .and_then(|b| b.get("id"))
            .and_then(Value::as_str)
            .ok_or(io::Error::from(io::ErrorKind::InvalidData))?;
        assert_ne!(a, b);
        if index < 4 {
            assert!(
                a.bytes()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'_' | b'-'))
            );
            assert!(
                b.bytes()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'_' | b'-'))
            );
        } else {
            assert_eq!((a, b), ("ab|c", "a|bc"));
        }
        assert_eq!(
            sent.pointer("/messages/2/content/0/tool_use_id"),
            Some(&json!(a))
        );
        assert_eq!(
            sent.pointer("/messages/2/content/1/tool_use_id"),
            Some(&json!(b))
        );
        assert!(!sent.to_string().contains("Private plan"));
        assert!(!sent.to_string().contains("signed"));
    }
    server.shutdown()?;
    Ok(())
}

#[test]
fn stop_reasons_and_usage_distinguish_limits_refusals_and_missing_reports() -> TestResult {
    let cases = [
        ("end_turn", FinishReason::Completed),
        ("stop_sequence", FinishReason::Completed),
        ("tool_use", FinishReason::ToolCalls),
        ("max_tokens", FinishReason::OutputLimit),
        ("refusal", FinishReason::Refusal("test-category".into())),
        ("pause_turn", FinishReason::Completed),
    ];
    let mut scripts = Vec::new();
    for (reason, _) in &cases {
        scripts.push(ResponseScript::sse(simple(reason, None)?));
    }
    scripts.push(ResponseScript::sse(simple(
        "end_turn",
        Some(json!({"input_tokens":0,"cache_read_input_tokens":0,"output_tokens":0})),
    )?));
    let mut server = MockProvider::start(scripts)?;
    let adapter = AnthropicAdapter::new(destination(&server), capabilities()?, KEY.into());
    for (_, reason) in cases {
        let (_, turn) = request(&adapter, &conversation(), RequestOptions::default())?;
        assert_eq!(turn.finish_reason, reason);
        assert_eq!(turn.usage, benitoiteh::provider::Usage::default());
    }
    let (_, turn) = request(&adapter, &conversation(), RequestOptions::default())?;
    assert_eq!(turn.usage.input_tokens, Some(0));
    assert_eq!(turn.usage.cache_read_tokens, Some(0));
    assert_eq!(turn.usage.cache_write_tokens, None);
    assert_eq!(turn.usage.output_tokens, Some(0));
    server.shutdown()?;
    Ok(())
}

#[test]
fn http_and_stream_errors_keep_classification_and_retry_after_without_exposing_keys() -> TestResult
{
    let cases = [
        (
            429,
            "rate_limit_error",
            "limited",
            ProviderError::Retryable {
                retry_after_secs: Some(7),
            },
        ),
        (
            500,
            "api_error",
            "internal",
            ProviderError::Retryable {
                retry_after_secs: Some(7),
            },
        ),
        (
            503,
            "api_error",
            "unavailable",
            ProviderError::Retryable {
                retry_after_secs: Some(7),
            },
        ),
        (
            529,
            "overloaded_error",
            "overloaded",
            ProviderError::Retryable {
                retry_after_secs: Some(7),
            },
        ),
        (
            401,
            "authentication_error",
            KEY,
            ProviderError::NonRetryable {
                reason: NonRetryableKind::Authentication,
            },
        ),
        (
            403,
            "permission_error",
            "denied",
            ProviderError::NonRetryable {
                reason: NonRetryableKind::Authentication,
            },
        ),
        (
            400,
            "invalid_request_error",
            "wrong shape",
            ProviderError::NonRetryable {
                reason: NonRetryableKind::RequestRejected,
            },
        ),
        (
            413,
            "request_too_large",
            "too large",
            ProviderError::NonRetryable {
                reason: NonRetryableKind::RequestRejected,
            },
        ),
        (
            400,
            "authentication_error",
            "denied",
            ProviderError::NonRetryable {
                reason: NonRetryableKind::Authentication,
            },
        ),
        (
            422,
            "invalid_request_error",
            "wrong shape",
            ProviderError::NonRetryable {
                reason: NonRetryableKind::InvalidRequest,
            },
        ),
        (
            400,
            "invalid_request_error",
            "prompt is too long: 100 > 50",
            ProviderError::ContextOverflow,
        ),
    ];
    let mut scripts = Vec::new();
    for (status, kind, message, _) in &cases {
        let mut script = ResponseScript::json(
            *status,
            &json!({"type":"error","error":{"type":kind,"message":message}}),
        )?;
        script.headers.push(("Retry-After".into(), "7".into()));
        scripts.push(script);
    }
    scripts.push(ResponseScript::sse(sse(&[
        json!({"type":"error","error":{"type":"overloaded_error","message":"Busy"}}),
    ])?));
    scripts.push(ResponseScript::sse(sse(&[
        json!({"type":"error","error":{"type":"invalid_request_error","message":"bad"}}),
    ])?));
    let mut server = MockProvider::start(scripts)?;
    let adapter = AnthropicAdapter::new(destination(&server), capabilities()?, KEY.into());
    assert!(!format!("{adapter:?}").contains(KEY));
    for (_, kind, message, expected) in cases {
        let error = request(&adapter, &conversation(), RequestOptions::default())
            .err()
            .ok_or(io::Error::from(io::ErrorKind::InvalidData))?;
        assert_eq!(error.classification(), expected);
        assert!(!format!("{error:?} {error}").contains(KEY));
        let AnthropicError::Provider {
            details: Some(details),
            retry_after,
            ..
        } = error
        else {
            return Err(io::Error::from(io::ErrorKind::InvalidData).into());
        };
        assert_eq!(retry_after.as_deref(), Some("7"));
        assert_eq!(details.error_type.as_deref(), Some(kind));
        assert_eq!(
            details.message.as_deref(),
            Some(if message == KEY {
                "[redacted]"
            } else {
                message
            })
        );
        assert!(!format!("{details:?}").contains(KEY));
    }
    for expected in [
        ProviderError::Retryable {
            retry_after_secs: None,
        },
        ProviderError::NonRetryable {
            reason: NonRetryableKind::InvalidRequest,
        },
    ] {
        let error = request(&adapter, &conversation(), RequestOptions::default())
            .err()
            .ok_or(io::Error::from(io::ErrorKind::InvalidData))?;
        assert_eq!(error.classification(), expected);
    }
    server.shutdown()?;
    Ok(())
}

#[test]
fn interrupted_and_malformed_streams_do_not_commit_partial_turns() -> TestResult {
    let prefix = sse(&[
        json!({"type":"message_start","message":{}}),
        json!({"type":"content_block_start","index":0,"content_block":{"type":"text","text":"Partial"}}),
        json!({"type":"content_block_stop","index":0}),
        json!({"type":"message_delta","delta":{"stop_reason":"end_turn"}}),
    ])?;
    let mut cut = ResponseScript::sse(
        [
            prefix.clone(),
            b"event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n".to_vec(),
        ]
        .concat(),
    );
    cut.delivery = Delivery::Disconnect {
        after: prefix.len(),
    };
    let scripts = vec![
        ResponseScript::sse(prefix),
        cut,
        ResponseScript::sse(b"event: message_start\ndata: {broken}\n\n".to_vec()),
        ResponseScript::sse(sse(&[
            json!({"type":"message_start","message":{}}),
            json!({"type":"message_delta","delta":{"stop_reason":"end_turn"}}),
            json!({"type":"message_stop"}),
        ])?),
    ];
    let mut server = MockProvider::start(scripts)?;
    let adapter = AnthropicAdapter::new(destination(&server), capabilities()?, KEY.into());
    for index in 0..4 {
        let error = request(&adapter, &conversation(), RequestOptions::default())
            .err()
            .ok_or(io::Error::from(io::ErrorKind::InvalidData))?;
        assert_eq!(
            error.classification(),
            if index == 3 {
                ProviderError::EmptyResponse
            } else {
                ProviderError::StreamDisconnected
            }
        );
    }
    server.shutdown()?;
    Ok(())
}

#[test]
fn header_only_responses_can_be_cancelled_and_time_out() -> TestResult {
    for cancel in [true, false] {
        let mut script = ResponseScript::sse(Vec::new());
        script.delivery = Delivery::HeadersOnly;
        let mut server = MockProvider::start(vec![script])?;
        let adapter = AnthropicAdapter::new(destination(&server), capabilities()?, KEY.into());
        let request = adapter.build_request(
            &conversation(),
            "System",
            &tools(),
            RequestOptions::default(),
        )?;
        assert!(!format!("{request:?}").contains(KEY));
        let client = HttpClient::new(HttpTimeouts {
            idle: Duration::from_millis(100),
            ..HttpTimeouts::default()
        });
        let mut stream = adapter.stream(&client, &request)?;
        let start = Instant::now();
        while server.requests()?.is_empty() {
            stream.poll(Duration::from_millis(10))?;
            if start.elapsed() > Duration::from_secs(2) {
                return Err(io::Error::from(io::ErrorKind::TimedOut).into());
            }
        }
        if cancel {
            stream.cancellation().cancel();
        }
        let error = loop {
            if let Err(error) = stream.poll(Duration::from_millis(20)) {
                break error;
            }
            if start.elapsed() > Duration::from_secs(2) {
                return Err(io::Error::from(io::ErrorKind::TimedOut).into());
            }
        };
        assert!(!format!("{stream:?}").contains(KEY));
        if cancel {
            assert!(matches!(error, AnthropicError::Cancelled));
        } else {
            assert_eq!(error.classification(), ProviderError::StreamDisconnected);
        }
        assert!(stream.poll(Duration::ZERO).is_err());
        assert!(stream.finish().is_err());
        server.shutdown()?;
    }
    Ok(())
}

#[path = "support/images.rs"]
mod image_support;

// 作成時の関門: 既存のテキストの要求では、画像の API ごとの包みと、誤り・未対応・
// 欠落時の抑止を検出できない。公開の要求作成と HTTP 送信を通し、本文を確かめる。
#[test]
fn tool_images_use_api_blocks_and_omit_unavailable_or_unsupported_data() -> TestResult {
    let cases = [
        (true, true, false),
        (false, true, false),
        (true, false, false),
        (true, true, true),
    ];
    let mut server = MockProvider::start(
        cases
            .iter()
            .map(|_| Ok(ResponseScript::sse(simple("end_turn", None)?)))
            .collect::<Result<Vec<_>, Box<dyn Error>>>()?,
    )?;
    for (enabled, available, is_error) in cases {
        let adapter = AnthropicAdapter::new(
            destination(&server),
            Capabilities {
                image_input: enabled,
                ..Capabilities::default()
            },
            KEY.into(),
        );
        let history = image_support::history(available, is_error);
        request(&adapter, &history, RequestOptions::default())?;
    }
    let requests = server.requests()?;
    for (index, (enabled, available, is_error)) in cases.into_iter().enumerate() {
        let body = &requests.get(index).ok_or("missing request")?.body;
        let content = body
            .pointer("/messages/1/content/0/content")
            .ok_or("missing tool content")?;
        if enabled && available && !is_error {
            assert_eq!(
                content,
                &json!([
                    {"type":"text", "text":"Read image file image/png (1x1, 6 bytes)"},
                    {"type":"image", "source":{"type":"base64", "media_type":"image/png", "data":"Zm9vYmFy"}}
                ])
            );
            assert_eq!(body.pointer("/messages/1/content/1/content"), Some(content));
        } else {
            assert!(content.is_string());
            let expected = if !enabled {
                "Read image file image/png (1x1, 6 bytes)\n(Image omitted: the current provider does not accept images.)"
            } else if !available {
                "Read image file image/png (1x1, 6 bytes)\n(Image omitted: the image data is not available.)"
            } else {
                "Read image file image/png (1x1, 6 bytes)"
            };
            assert_eq!(content, &json!(expected));
            assert!(!body.to_string().contains("Zm9vYmFy"));
        }
    }
    server.shutdown()?;
    Ok(())
}

// 作成時の関門: 画像の選別結果が API の各形式に反映され、一部だけ除く結果でも
// 断りの文が一つになることを HTTP 本文で守る。規則そのものは共通層で確かめる。
#[test]
fn bounded_images_are_omitted_with_one_notice_per_result() -> TestResult {
    let mut server = MockProvider::start(vec![
        ResponseScript::sse(simple("end_turn", None)?),
        ResponseScript::sse(simple("end_turn", None)?),
    ])?;
    let adapter = AnthropicAdapter::new(
        destination(&server),
        Capabilities {
            image_input: true,
            ..Capabilities::default()
        },
        KEY.into(),
    );
    for partial in [false, true] {
        let mut history =
            image_support::sized_history(true, false, if partial { 1 } else { 13 }, 1_048_576);
        if partial && let Some(Item::ToolResult(result)) = history.items.last_mut() {
            result.images = (0..13)
                .flat_map(|_| result.images.iter().cloned())
                .collect();
        }
        request(&adapter, &history, RequestOptions::default())?;
    }
    let requests = server.requests()?;
    for (index, partial) in [false, true].into_iter().enumerate() {
        let body = &requests.get(index).ok_or("missing request")?.body;
        let results = body
            .pointer("/messages/1/content")
            .and_then(Value::as_array)
            .ok_or("missing results")?;
        let content = results
            .first()
            .and_then(|v| v.get("content"))
            .ok_or("missing content")?;
        let notice = "(This image was removed to keep the request small. Read the file again if you need it.)";
        assert_eq!(content.to_string().matches(notice).count(), 1);
        assert_eq!(body.to_string().matches("\"type\":\"image\"").count(), 7);
        assert_eq!(
            body.to_string().matches(notice).count(),
            if partial { 1 } else { 6 }
        );
    }
    server.shutdown()?;
    Ok(())
}
