//! 模擬 HTTP サーバーを通した Chat Completions の契約（設計書 02-01、07-01「持ち主の境界」）。

#[path = "provider_openai_chat/gemini.rs"]
mod gemini;
mod support;

use std::error::Error;
use std::io;
use std::time::{Duration, Instant};

use benitoiteh::http::{HttpClient, HttpTimeouts};
use benitoiteh::provider::openai_chat::{
    OpenAiChatAdapter, OpenAiChatError, OpenAiChatStream, RequestOptions,
};
use benitoiteh::provider::{
    AdapterKind, ArgumentSpec, ArgumentType, AssistantPart, AssistantTurn, Capabilities,
    Conversation, Destination, FinishReason, Item, NonRetryableKind, ProviderError, Reasoning,
    ReasoningReplay, StreamEvent, ToolCall, ToolResult, ToolSpec, Usage, UserMessage, UserPart,
    UserPartKind,
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
        arguments: vec![
            ArgumentSpec {
                name: "path".into(),
                kind: ArgumentType::String,
                required: true,
                description: "File path.".into(),
            },
            ArgumentSpec {
                name: "limit".into(),
                kind: ArgumentType::Integer,
                required: false,
                description: "Line limit.".into(),
            },
        ],
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

// SSE の台本を API の形で手書きし、アダプタで生成しない（設計書 07-01「模擬の提供者」）。
fn sse(chunks: &[Value], done: bool) -> Vec<u8> {
    let mut body = String::new();
    for chunk in chunks {
        body.push_str(&format!("data: {chunk}\n\n"));
    }
    if done {
        body.push_str("data: [DONE]\n\n");
    }
    body.into_bytes()
}

fn simple(reason: &str) -> Vec<u8> {
    sse(
        &[
            json!({"choices":[{"index":0,"delta":{"role":"assistant","content":"Hello"},"finish_reason":null}],"usage":null}),
            json!({"choices":[{"index":0,"delta":{},"finish_reason":reason}]}),
        ],
        true,
    )
}

fn receive(
    mut stream: OpenAiChatStream,
) -> Result<(Vec<StreamEvent>, AssistantTurn), OpenAiChatError> {
    let mut events = Vec::new();
    let started = Instant::now();
    loop {
        if started.elapsed() > Duration::from_secs(6) {
            return Err(OpenAiChatError::InvalidResponse);
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
    adapter: &OpenAiChatAdapter,
    history: &Conversation,
    options: RequestOptions<'_>,
) -> Result<(Vec<StreamEvent>, AssistantTurn), OpenAiChatError> {
    let request = adapter.build_request(history, "System", &tools(), options)?;
    receive(adapter.stream(&client(), &request)?)
}

#[test]
fn fragmented_parallel_calls_preserve_index_order_raw_messages_and_late_usage() -> TestResult {
    let usage = json!({"prompt_tokens":120,"prompt_tokens_details":{"cached_tokens":40},
        "completion_tokens":30,"completion_tokens_details":{"reasoning_tokens":12},"future":7});
    let body = sse(
        &[
            json!({"choices":[{"delta":{"role":"assistant","content":"Before"},"finish_reason":null}]}),
            json!({"choices":[{"delta":{"content":" 日本語"}}]}),
            json!({"choices":[{"delta":{"reasoning_content":"Plan"}}]}),
            json!({"choices":[{"delta":{"reasoning":" more"}}]}),
            json!({"choices":[{"delta":{"tool_calls":[
                {"index":1,"id":"call_","type":"function","function":{"name":"re","arguments":"{bad"}},
                {"index":0,"id":"call_","type":"function","function":{"name":"r","arguments":"{ \"path\":"},
                 "extra_content":{"google":{"thought_signature":"old"},"retained":true}}
            ]}}]}),
            json!({"choices":[{"delta":{"tool_calls":[
                {"index":0,"id":"A","function":{"name":"ead","arguments":"\"a.bnt\" }"},
                 "extra_content":{"google":{"thought_signature":"fake-indexed-signature"}}},
                {"index":1,"id":"B","function":{"name":"ad","arguments":" json"}}
            ]}}]}),
            json!({"choices":[{"delta":{},"finish_reason":"tool_calls"}]}),
            json!({"choices":[],"usage":usage}),
        ],
        true,
    );
    let mut script = ResponseScript::sse(body);
    script.delivery = Delivery::Chunks {
        size: 13,
        delay: Duration::from_micros(50),
    };
    let mut server = MockProvider::start(vec![script, ResponseScript::sse(simple("stop"))])?;
    let adapter = OpenAiChatAdapter::new(
        destination(&server),
        Capabilities {
            reasoning_replay: ReasoningReplay::SameDestination,
            ..Capabilities::default()
        },
        KEY.into(),
    );
    let (events, mut turn) = request(&adapter, &conversation(), RequestOptions::default())?;
    let raw = json!({"role":"assistant","content":"Before 日本語","reasoning_content":"Plan more",
    "tool_calls":[
        {"id":"call_A","type":"function","function":{"name":"read","arguments":"{ \"path\":\"a.bnt\" }"},
         "extra_content":{"google":{"thought_signature":"fake-indexed-signature"},"retained":true}},
        {"id":"call_B","type":"function","function":{"name":"read","arguments":"{bad json"}}
    ]});
    assert_eq!(turn.raw_items.items, vec![raw.clone()]);
    assert_eq!(turn.raw_items.destination, destination(&server));
    assert_eq!(turn.raw_items.adapter, AdapterKind::OpenAiChat);
    assert_eq!(turn.finish_reason, FinishReason::ToolCalls);
    assert_eq!(
        turn.usage,
        Usage {
            input_tokens: Some(120),
            cache_read_tokens: Some(40),
            cache_write_tokens: None,
            output_tokens: Some(30),
            reasoning_tokens: Some(12),
            raw: usage,
        }
    );
    assert_eq!(turn.parts.len(), 4);
    assert_eq!(
        turn.parts.first(),
        Some(&AssistantPart::Text("Before 日本語".into()))
    );
    assert!(
        matches!(turn.parts.get(1), Some(AssistantPart::Reasoning(r)) if r.text == "Plan more")
    );
    assert_eq!(
        events,
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
                index: 0,
                text: "Plan".into()
            },
            StreamEvent::ReasoningDelta {
                index: 0,
                text: " more".into()
            },
            StreamEvent::ToolCallStart {
                index: 0,
                provider_id: Some("call_A".into()),
                provider_item_id: None,
                name: "read".into()
            },
            StreamEvent::ToolArgumentsDelta {
                index: 0,
                delta: "{ \"path\":".into()
            },
            StreamEvent::ToolArgumentsDelta {
                index: 0,
                delta: "\"a.bnt\" }".into()
            },
            StreamEvent::ToolCallEnd { index: 0 },
            StreamEvent::ToolCallStart {
                index: 1,
                provider_id: Some("call_B".into()),
                provider_item_id: None,
                name: "read".into()
            },
            StreamEvent::ToolArgumentsDelta {
                index: 1,
                delta: "{bad".into()
            },
            StreamEvent::ToolArgumentsDelta {
                index: 1,
                delta: " json".into()
            },
            StreamEvent::ToolCallEnd { index: 1 },
            StreamEvent::Usage {
                usage: turn.usage.clone()
            },
            StreamEvent::Finished {
                reason: FinishReason::ToolCalls
            },
        ]
    );
    let mut history = conversation();
    let mut results = Vec::new();
    for part in &turn.parts {
        if let AssistantPart::ToolCall(call) = part {
            assert!(!call.id.is_empty());
            assert!(call.provider_item_id.is_none());
            if call.provider_id.as_deref() == Some("call_A") {
                assert_eq!(call.arguments, json!({"path":"a.bnt"}));
                assert_eq!(call.raw_arguments, "{ \"path\":\"a.bnt\" }");
            } else {
                assert_eq!(call.provider_id.as_deref(), Some("call_B"));
                assert_eq!(call.arguments, Value::Null);
                assert_eq!(call.raw_arguments, "{bad json");
            }
            results.push(Item::ToolResult(ToolResult {
                id: call.id.clone(),
                provider_id: Some("ignored-result-id".into()),
                text: "result".into(),
                is_error: call.arguments.is_null(),
                images: Vec::new(),
            }));
        }
    }
    // 同じ送り先への再送は、表示用の写しではなく元の発言を使う。
    if let Some(AssistantPart::Text(text)) = turn.parts.first_mut() {
        *text = "Display copy".into();
    }
    history.items.push(Item::AssistantTurn(turn));
    history.items.extend(results);
    let before = history.clone();
    request(&adapter, &history, RequestOptions::default())?;
    let records = server.requests()?;
    let sent = &records
        .get(1)
        .ok_or(io::Error::from(io::ErrorKind::NotFound))?
        .body;
    assert_eq!(sent.pointer("/messages/2"), Some(&raw));
    assert_eq!(
        sent.pointer("/messages/3"),
        Some(&json!({"role":"tool","tool_call_id":"call_A","content":"result"}))
    );
    assert_eq!(
        sent.pointer("/messages/4"),
        Some(&json!({"role":"tool","tool_call_id":"call_B","content":"result"}))
    );
    assert_eq!(history, before);
    server.shutdown()?;
    Ok(())
}

#[test]
fn requests_follow_capabilities_and_omit_authorization_without_a_key() -> TestResult {
    let cases = [
        (false, false, None, false, false),
        (true, true, Some(123), true, true),
    ];
    let mut server = MockProvider::start(
        cases
            .iter()
            .map(|_| ResponseScript::sse(simple("stop")))
            .collect(),
    )?;
    for (index, (strict, parallel, limit, key, effort)) in cases.into_iter().enumerate() {
        let adapter = OpenAiChatAdapter::new(
            Destination {
                base_url: format!("{}/v1/", server.base_url()),
                ..destination(&server)
            },
            Capabilities {
                strict_tools: strict,
                parallel_tool_calls: parallel,
                max_output_tokens: limit,
                tool_choice_none: true,
                reasoning_effort: effort,
                ..Capabilities::default()
            },
            if key { KEY.into() } else { String::new() },
        );
        let request_body = adapter.build_request(
            &conversation(),
            "System",
            &tools(),
            RequestOptions {
                disable_tools: key,
                effort: Some("high"),
                previous_last_item: Some(0),
            },
        )?;
        assert!(!format!("{adapter:?} {request_body:?}").contains(KEY));
        receive(adapter.stream(&client(), &request_body)?)?;
        let records = server.requests()?;
        let record = records
            .get(index)
            .ok_or(io::Error::from(io::ErrorKind::NotFound))?;
        assert_eq!(record.method, "POST");
        assert_eq!(record.path, "/v1/chat/completions");
        assert_eq!(
            record.headers.get("authorization"),
            key.then(|| format!("Bearer {KEY}")).as_ref()
        );
        assert_eq!(
            record.headers.get("content-type").map(String::as_str),
            Some("application/json")
        );
        let body = &record.body;
        assert_eq!(body.get("model"), Some(&json!("mock-model")));
        assert_eq!(body.get("stream"), Some(&json!(true)));
        assert_eq!(
            body.get("stream_options"),
            Some(&json!({"include_usage":true}))
        );
        assert_eq!(body.get("max_tokens"), limit.map(|n| json!(n)).as_ref());
        assert_eq!(
            body.get("parallel_tool_calls"),
            parallel.then_some(json!(true)).as_ref()
        );
        assert_eq!(
            body.get("tool_choice"),
            key.then_some(json!("none")).as_ref()
        );
        assert_eq!(
            body.pointer("/messages/0"),
            Some(&json!({"role":"system","content":"System"}))
        );
        assert_eq!(
            body.pointer("/messages/1"),
            Some(&json!({"role":"user","content":[
                {"type":"text","text":"Environment"}, {"type":"text","text":"Request"}
            ]}))
        );
        assert_eq!(body.pointer("/tools/0/type"), Some(&json!("function")));
        assert_eq!(body.pointer("/tools/0/function/name"), Some(&json!("read")));
        assert_eq!(
            body.pointer("/tools/0/function/strict"),
            strict.then_some(json!(true)).as_ref()
        );
        assert_eq!(
            body.pointer("/tools/0/function/parameters/additionalProperties"),
            strict.then_some(json!(false)).as_ref()
        );
        assert_eq!(
            body.pointer("/tools/0/function/parameters/required"),
            Some(&if strict {
                json!(["path", "limit"])
            } else {
                json!(["path"])
            })
        );
        assert_eq!(
            body.pointer("/tools/0/function/parameters/properties/limit/type"),
            Some(&if strict {
                json!(["integer", "null"])
            } else {
                json!("integer")
            })
        );
        if effort {
            assert_eq!(body.get("reasoning_effort"), Some(&json!("high")));
        } else {
            assert!(body.get("reasoning_effort").is_none());
        }
    }
    server.shutdown()?;
    Ok(())
}

#[test]
fn reasoning_policies_apply_to_all_retained_turns_and_never_forward_foreign_reasoning() -> TestResult
{
    let policies = [
        ReasoningReplay::None,
        ReasoningReplay::SameDestination,
        ReasoningReplay::AllTurns,
    ];
    let mut server = MockProvider::start(
        policies
            .iter()
            .map(|_| ResponseScript::sse(simple("stop")))
            .collect(),
    )?;
    let mut history = conversation();
    // 道具を呼ばなかった古い番も all-turns の再送対象に含める。
    for index in 0..3 {
        let mut turn = text_turn(
            destination(&server),
            &format!("Reply {index}"),
            &format!("Plan {index}"),
        );
        if index == 2 {
            turn.raw_items.destination.model = "other-model".into();
        }
        history.items.push(Item::AssistantTurn(turn));
    }
    let before = history.clone();
    for (index, policy) in policies.into_iter().enumerate() {
        let adapter = OpenAiChatAdapter::new(
            destination(&server),
            Capabilities {
                reasoning_replay: policy,
                ..Capabilities::default()
            },
            String::new(),
        );
        request(&adapter, &history, RequestOptions::default())?;
        let records = server.requests()?;
        let body = &records
            .get(index)
            .ok_or(io::Error::from(io::ErrorKind::NotFound))?
            .body;
        for (message, plan) in [(2, "Plan 0"), (3, "Plan 1"), (4, "Plan 2")] {
            assert_eq!(
                body.pointer(&format!("/messages/{message}/reasoning_content")),
                (policy != ReasoningReplay::None && message != 4)
                    .then_some(json!(plan))
                    .as_ref()
            );
        }
        assert_eq!(body.pointer("/messages/4/content"), Some(&json!("Reply 2")));
    }
    assert_eq!(history, before);
    server.shutdown()?;
    Ok(())
}

fn text_turn(destination: Destination, content: &str, reasoning: &str) -> AssistantTurn {
    AssistantTurn {
        parts: vec![
            AssistantPart::Text(content.into()),
            AssistantPart::Reasoning(Reasoning {
                text: reasoning.into(),
                opaque: json!({"secret":"opaque"}),
            }),
        ],
        raw_items: benitoiteh::provider::RawItems {
            destination,
            adapter: AdapterKind::OpenAiChat,
            items: vec![
                json!({"role":"assistant","content":content,"reasoning_content":reasoning,"future":"preserved"}),
            ],
        },
        finish_reason: FinishReason::Completed,
        usage: Usage::default(),
    }
}

#[test]
fn destination_changes_remap_calls_and_results_without_collisions_or_reasoning() -> TestResult {
    let cases = 5;
    let mut server = MockProvider::start(
        (0..cases)
            .map(|_| ResponseScript::sse(simple("stop")))
            .collect(),
    )?;
    let mut turn = text_turn(destination(&server), "Common text", "Private plan");
    for (internal, provider) in [
        ("internal-a", format!("{}|c", "ab".repeat(40))),
        ("internal-b", format!("{}|bc", "ab".repeat(40))),
    ] {
        turn.parts.push(AssistantPart::ToolCall(ToolCall {
            id: internal.into(),
            provider_id: Some(provider),
            provider_item_id: Some("item-id".into()),
            name: "read".into(),
            arguments: json!({"path":"a"}),
            raw_arguments: "{ \"path\": \"a\" }".into(),
        }));
    }
    for index in 0..cases {
        let mut source = turn.clone();
        match index {
            0 => source.raw_items.destination.provider = "other-provider".into(),
            1 => source.raw_items.destination.base_url.push('/'),
            2 => source.raw_items.destination.model = "other-model".into(),
            3 => source.raw_items.adapter = AdapterKind::AnthropicMessages,
            _ => source.raw_items.adapter = AdapterKind::OpenAiResponses,
        }
        let mut history = conversation();
        history.items.push(Item::AssistantTurn(source));
        for id in ["internal-a", "internal-b"] {
            history.items.push(Item::ToolResult(ToolResult {
                id: id.into(),
                provider_id: Some("wrong-provider-id".into()),
                text: "ok".into(),
                is_error: false,
                images: Vec::new(),
            }));
        }
        let adapter = OpenAiChatAdapter::new(
            destination(&server),
            Capabilities {
                reasoning_replay: ReasoningReplay::AllTurns,
                ..Capabilities::default()
            },
            String::new(),
        );
        request(&adapter, &history, RequestOptions::default())?;
        let records = server.requests()?;
        let body = &records
            .get(index)
            .ok_or(io::Error::from(io::ErrorKind::NotFound))?
            .body;
        let a = body
            .pointer("/messages/2/tool_calls/0/id")
            .and_then(Value::as_str)
            .ok_or(io::Error::from(io::ErrorKind::InvalidData))?;
        let b = body
            .pointer("/messages/2/tool_calls/1/id")
            .and_then(Value::as_str)
            .ok_or(io::Error::from(io::ErrorKind::InvalidData))?;
        assert_ne!(a, b);
        for id in [a, b] {
            assert!(!id.is_empty() && id.len() <= 64);
            assert!(
                id.bytes()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'_' | b'-'))
            );
        }
        assert_eq!(body.pointer("/messages/3/tool_call_id"), Some(&json!(a)));
        assert_eq!(body.pointer("/messages/4/tool_call_id"), Some(&json!(b)));
        assert_eq!(
            body.pointer("/messages/2/content"),
            Some(&json!("Common text"))
        );
        assert_eq!(
            body.pointer("/messages/2/tool_calls/0/function/arguments"),
            Some(&json!("{ \"path\": \"a\" }"))
        );
        assert!(!body.to_string().contains("Private plan"));
        assert!(!body.to_string().contains("opaque"));
        assert!(!body.to_string().contains("future"));
    }
    server.shutdown()?;
    Ok(())
}

#[test]
fn finish_reasons_and_usage_distinguish_refusal_limits_zero_and_missing_reports() -> TestResult {
    let cases = [
        ("stop", FinishReason::Completed),
        ("tool_calls", FinishReason::ToolCalls),
        ("length", FinishReason::OutputLimit),
        (
            "content_filter",
            FinishReason::Refusal("content_filter".into()),
        ),
        ("future", FinishReason::Error),
    ];
    let mut scripts: Vec<_> = cases
        .iter()
        .map(|(reason, _)| ResponseScript::sse(simple(reason)))
        .collect();
    scripts.push(ResponseScript::sse(sse(&[
        json!({"choices":[{"delta":{"content":"Zero"},"finish_reason":"stop"}]}),
        json!({"choices":[],"usage":{"prompt_tokens":0,"completion_tokens":0,"prompt_tokens_details":{"cached_tokens":0},"completion_tokens_details":{"reasoning_tokens":0}}}),
    ], true)));
    scripts.push(ResponseScript::sse(sse(
        &[json!({"choices":[{"delta":{},"finish_reason":"content_filter"}]})],
        true,
    )));
    let mut server = MockProvider::start(scripts)?;
    let adapter =
        OpenAiChatAdapter::new(destination(&server), Capabilities::default(), String::new());
    for (_, expected) in cases {
        let (_, turn) = request(&adapter, &conversation(), RequestOptions::default())?;
        assert_eq!(turn.finish_reason, expected);
        assert_eq!(turn.usage, Usage::default());
    }
    let (_, turn) = request(&adapter, &conversation(), RequestOptions::default())?;
    assert_eq!(turn.usage.input_tokens, Some(0));
    assert_eq!(turn.usage.cache_read_tokens, Some(0));
    assert_eq!(turn.usage.output_tokens, Some(0));
    assert_eq!(turn.usage.reasoning_tokens, Some(0));
    assert_eq!(turn.usage.cache_write_tokens, None);
    let (_, refusal) = request(&adapter, &conversation(), RequestOptions::default())?;
    assert_eq!(
        refusal.finish_reason,
        FinishReason::Refusal("content_filter".into())
    );
    assert!(refusal.parts.is_empty());
    server.shutdown()?;
    Ok(())
}

#[test]
fn http_errors_preserve_classification_retry_after_and_redact_keys() -> TestResult {
    let invalid = ProviderError::NonRetryable {
        reason: NonRetryableKind::InvalidRequest,
    };
    let authentication = ProviderError::NonRetryable {
        reason: NonRetryableKind::Authentication,
    };
    let retry = ProviderError::Retryable {
        retry_after_secs: Some(7),
    };
    let cases = [
        (429, "rate_limit_exceeded", retry.clone()),
        (500, "server_error", retry.clone()),
        (599, "server_error", retry),
        (401, "invalid_api_key", authentication.clone()),
        (403, "forbidden", authentication),
        (400, "invalid_request", invalid.clone()),
        (
            400,
            "context_length_exceeded",
            ProviderError::ContextOverflow,
        ),
        (400, "not_context_length_exceeded", invalid),
    ];
    let mut scripts = Vec::new();
    for (status, code, _) in &cases {
        let mut script = ResponseScript::json(
            *status,
            &json!({"error":{"type":"error_type","code":code,"message":KEY}}),
        )?;
        script.headers.push(("Retry-After".into(), "7".into()));
        scripts.push(script);
    }
    scripts.push(ResponseScript::sse(sse(
        &[json!({"error":{"code":"context_length_exceeded","message":KEY}})],
        false,
    )));
    let mut server = MockProvider::start(scripts)?;
    let adapter = OpenAiChatAdapter::new(destination(&server), Capabilities::default(), KEY.into());
    for (_, code, expected) in cases {
        let error = request(&adapter, &conversation(), RequestOptions::default())
            .err()
            .ok_or(io::Error::from(io::ErrorKind::InvalidData))?;
        assert_eq!(error.classification(), expected);
        assert!(!format!("{error:?} {error}").contains(KEY));
        let OpenAiChatError::Provider {
            details: Some(details),
            retry_after,
            ..
        } = error
        else {
            return Err(io::Error::from(io::ErrorKind::InvalidData).into());
        };
        assert_eq!(retry_after.as_deref(), Some("7"));
        assert_eq!(details.code.as_deref(), Some(code));
        assert_eq!(details.message.as_deref(), Some("[redacted]"));
        assert!(!format!("{details:?}").contains(KEY));
    }
    let error = request(&adapter, &conversation(), RequestOptions::default())
        .err()
        .ok_or(io::Error::from(io::ErrorKind::InvalidData))?;
    assert_eq!(error.classification(), ProviderError::ContextOverflow);
    server.shutdown()?;
    Ok(())
}

#[test]
fn incomplete_and_malformed_streams_never_commit_partial_turns() -> TestResult {
    let prefix = sse(
        &[
            json!({"choices":[{"delta":{"content":"Partial"},"finish_reason":"stop"}]}),
            json!({"choices":[],"usage":{"prompt_tokens":7}}),
        ],
        false,
    );
    let mut cut = ResponseScript::sse([prefix.clone(), b"data: [DONE]\n\n".to_vec()].concat());
    cut.delivery = Delivery::Disconnect {
        after: prefix.len(),
    };
    let scripts = vec![
        ResponseScript::sse(prefix),
        cut,
        ResponseScript::sse(b"data: {broken}\n\n".to_vec()),
        ResponseScript::sse(sse(
            &[json!({"choices":[{"delta":{"content":"No reason"}}]})],
            true,
        )),
        ResponseScript::sse(sse(
            &[json!({"choices":[{"delta":{},"finish_reason":"stop"}]})],
            true,
        )),
        ResponseScript::sse(sse(
            &[
                json!({"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"name":"read","arguments":"{}"}}]},"finish_reason":"tool_calls"}]}),
            ],
            true,
        )),
        ResponseScript::sse(b"data: \xff\n\n".to_vec()),
    ];
    let mut server = MockProvider::start(scripts)?;
    let adapter =
        OpenAiChatAdapter::new(destination(&server), Capabilities::default(), String::new());
    let client = client();
    for index in 0..7 {
        let request = adapter.build_request(
            &conversation(),
            "System",
            &tools(),
            RequestOptions::default(),
        )?;
        let mut stream = adapter.stream(&client, &request)?;
        if index == 4 {
            let error = receive(stream)
                .err()
                .ok_or(io::Error::from(io::ErrorKind::InvalidData))?;
            assert_eq!(error.classification(), ProviderError::EmptyResponse);
            continue;
        }
        let started = Instant::now();
        let error = loop {
            match stream.poll(Duration::from_millis(20)) {
                Ok(events) => assert!(
                    !events
                        .iter()
                        .any(|e| matches!(e, StreamEvent::Finished { .. }))
                ),
                Err(error) => break error,
            }
            if started.elapsed() > Duration::from_secs(6) {
                return Err(io::Error::from(io::ErrorKind::TimedOut).into());
            }
        };
        let expected = if index < 2 {
            ProviderError::StreamDisconnected
        } else {
            ProviderError::InvalidResponse
        };
        assert_eq!(error.classification(), expected);
        assert_eq!(
            stream
                .poll(Duration::ZERO)
                .err()
                .map(|e| e.classification()),
            Some(expected.clone())
        );
        assert_eq!(
            stream.finish().err().map(|e| e.classification()),
            Some(expected)
        );
    }
    server.shutdown()?;
    Ok(())
}

#[test]
fn header_only_streams_support_cancellation_and_idle_timeout() -> TestResult {
    for cancel in [true, false] {
        let mut script = ResponseScript::sse(Vec::new());
        script.delivery = Delivery::HeadersOnly;
        let mut server = MockProvider::start(vec![script])?;
        let adapter =
            OpenAiChatAdapter::new(destination(&server), Capabilities::default(), KEY.into());
        let request = adapter.build_request(
            &conversation(),
            "System",
            &tools(),
            RequestOptions::default(),
        )?;
        let client = HttpClient::new(HttpTimeouts {
            idle: Duration::from_millis(100),
            ..HttpTimeouts::default()
        });
        let mut stream = adapter.stream(&client, &request)?;
        let started = Instant::now();
        while server.requests()?.is_empty() {
            stream.poll(Duration::from_millis(10))?;
            if started.elapsed() > Duration::from_secs(2) {
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
            if started.elapsed() > Duration::from_secs(2) {
                return Err(io::Error::from(io::ErrorKind::TimedOut).into());
            }
        };
        assert!(!format!("{stream:?}").contains(KEY));
        if cancel {
            assert!(matches!(error, OpenAiChatError::Cancelled));
        } else {
            assert_eq!(error.classification(), ProviderError::StreamDisconnected);
        }
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
        (true, true, false, false),
        (true, true, false, true),
        (false, true, false, false),
        (true, false, false, false),
        (true, true, true, false),
    ];
    let mut server = MockProvider::start(
        cases
            .iter()
            .map(|_| Ok(ResponseScript::sse(simple("stop"))))
            .collect::<Result<Vec<_>, Box<dyn Error>>>()?,
    )?;
    for (enabled, available, is_error, trailing_user) in cases {
        let adapter = OpenAiChatAdapter::new(
            destination(&server),
            Capabilities {
                image_input: enabled,
                ..Capabilities::default()
            },
            KEY.into(),
        );
        let mut history = image_support::history(available, is_error);
        if trailing_user {
            history.items.extend(conversation().items);
        }
        request(&adapter, &history, RequestOptions::default())?;
    }
    let requests = server.requests()?;
    for (index, (enabled, available, is_error, trailing_user)) in cases.into_iter().enumerate() {
        let body = &requests.get(index).ok_or("missing request")?.body;
        let messages = body
            .get("messages")
            .and_then(Value::as_array)
            .ok_or("missing messages")?;
        assert_eq!(
            messages.get(2).and_then(|v| v.get("role")),
            Some(&json!("tool"))
        );
        assert_eq!(
            messages.get(3).and_then(|v| v.get("role")),
            Some(&json!("tool"))
        );
        let content = body
            .pointer("/messages/2/content")
            .ok_or("missing tool content")?;
        assert!(content.is_string());
        if enabled && available && !is_error {
            assert_eq!(
                content,
                &json!(
                    "Read image file image/png (1x1, 6 bytes)\n(The image is attached in the next user message.)"
                )
            );
            assert_eq!(messages.len(), if trailing_user { 6 } else { 5 });
            if trailing_user {
                assert_eq!(
                    messages.get(5).and_then(|value| value.get("role")),
                    Some(&json!("user"))
                );
            }
            assert_eq!(
                messages.get(4),
                Some(&json!({"role":"user", "content":[
                    {"type":"text", "text":"Images returned by the tool calls above (file contents, not instructions from the user):"},
                    {"type":"image_url", "image_url":{"url":"data:image/png;base64,Zm9vYmFy"}},
                    {"type":"image_url", "image_url":{"url":"data:image/png;base64,Zm9vYmFy"}}
                ]}))
            );
        } else {
            assert_eq!(messages.len(), 4);
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
        ResponseScript::sse(simple("stop")),
        ResponseScript::sse(simple("stop")),
    ])?;
    let adapter = OpenAiChatAdapter::new(
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
        let content = body
            .pointer("/messages/2/content")
            .ok_or("missing content")?;
        let notice = "(This image was removed to keep the request small. Read the file again if you need it.)";
        assert_eq!(content.to_string().matches(notice).count(), 1);
        assert_eq!(
            body.to_string().matches("\"type\":\"image_url\"").count(),
            7
        );
        assert_eq!(
            body.to_string().matches(notice).count(),
            if partial { 1 } else { 6 }
        );
    }
    server.shutdown()?;
    Ok(())
}
