//! 模擬 HTTP サーバーを通した Responses の契約（設計書 02-01、07-01「持ち主の境界」）。

mod support;

use std::error::Error;
use std::io;
use std::time::{Duration, Instant};

use benitoiteh::http::{HttpClient, HttpTimeouts};
use benitoiteh::provider::openai_responses::{
    OpenAiResponsesAdapter, OpenAiResponsesError, OpenAiResponsesStream, RequestOptions,
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

// 作成時の関門: 要求・再送・終端・誤りはアダプタの公開の境界で確かめる。
// 既存の別 API のテストでは、Responses 固有の包みや call_id の取り違えを捕まえられない。
// 台本は手書きの API の項目から作り、本番にテスト専用の入口を加えない。
fn sse(chunks: &[Value]) -> Vec<u8> {
    let mut body = String::from(": keep-alive\n\n");
    for (sequence, chunk) in chunks.iter().enumerate() {
        let mut chunk = chunk.clone();
        if let Some(fields) = chunk.as_object_mut() {
            fields.insert("sequence_number".into(), json!(sequence));
        }
        body.push_str(&format!(
            "event: {}\ndata: {chunk}\n\n",
            chunk
                .get("type")
                .and_then(Value::as_str)
                .unwrap_or_default()
        ));
    }
    body.into_bytes()
}

fn message(text: &str) -> Value {
    json!({"type":"message","role":"assistant","id":"msg_1","status":"completed",
        "phase":"final_answer","content":[{"type":"output_text","text":text,"annotations":[]}]})
}

fn simple() -> Vec<u8> {
    sse(&[
        json!({"type":"response.output_item.added","output_index":0,"item":{"type":"message","id":"msg_1","role":"assistant","content":[],"status":"in_progress"}}),
        json!({"type":"response.output_text.delta","output_index":0,"content_index":0,"item_id":"msg_1","delta":"Hello"}),
        json!({"type":"response.output_item.done","output_index":0,"item":message("Hello")}),
        json!({"type":"response.completed","response":{"status":"completed"}}),
    ])
}

fn receive(
    mut stream: OpenAiResponsesStream,
) -> Result<(Vec<StreamEvent>, AssistantTurn), OpenAiResponsesError> {
    let mut events = Vec::new();
    let started = Instant::now();
    loop {
        if started.elapsed() > Duration::from_secs(6) {
            return Err(OpenAiResponsesError::InvalidResponse);
        }
        let next = match stream.poll(Duration::from_millis(20)) {
            Ok(next) => next,
            Err(error) => {
                stream.cancellation().cancel();
                stream
                    .join()
                    .map_err(|_| OpenAiResponsesError::InvalidResponse)?;
                return Err(error);
            }
        };
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
    adapter: &OpenAiResponsesAdapter,
    history: &Conversation,
    options: RequestOptions<'_>,
) -> Result<(Vec<StreamEvent>, AssistantTurn), OpenAiResponsesError> {
    let request = adapter.build_request(history, "System", &tools(), options)?;
    receive(adapter.stream(&client(), &request)?)
}

#[test]
fn requests_follow_capabilities_and_do_not_authenticate_without_a_key() -> TestResult {
    let cases = [(false, false, None, false), (true, true, Some(123), true)];
    let mut server = MockProvider::start(
        cases
            .iter()
            .map(|_| ResponseScript::sse(simple()))
            .collect(),
    )?;
    for (index, (strict, parallel, limit, key)) in cases.into_iter().enumerate() {
        let adapter = OpenAiResponsesAdapter::new(
            Destination {
                base_url: format!("{}/v1/", server.base_url()),
                ..destination(&server)
            },
            Capabilities {
                strict_tools: strict,
                parallel_tool_calls: parallel,
                max_output_tokens: limit,
                tool_choice_none: true,
                ..Capabilities::default()
            },
            if key { KEY.into() } else { String::new() },
        );
        let request = adapter.build_request(
            &conversation(),
            "System",
            &tools(),
            RequestOptions {
                effort: Some("high"),
                previous_last_item: Some(0),
                disable_tools: key,
                prompt_cache_key: Some("session-test"),
            },
        )?;
        assert!(!format!("{adapter:?} {request:?}").contains(KEY));
        receive(adapter.stream(&client(), &request)?)?;
        let records = server.requests()?;
        let record = records
            .get(index)
            .ok_or(io::Error::from(io::ErrorKind::NotFound))?;
        assert_eq!(record.method, "POST");
        assert_eq!(record.path, "/v1/responses");
        assert_eq!(
            record.headers.get("authorization"),
            key.then(|| format!("Bearer {KEY}")).as_ref()
        );
        assert_eq!(
            record.headers.get("content-type").map(String::as_str),
            Some("application/json")
        );
        let body = &record.body;
        for (name, expected) in [
            ("model", json!("mock-model")),
            ("instructions", json!("System")),
            ("stream", json!(true)),
            ("store", json!(false)),
            ("parallel_tool_calls", json!(parallel)),
            ("tool_choice", json!(if key { "none" } else { "auto" })),
            ("include", json!(["reasoning.encrypted_content"])),
            ("reasoning", json!({"effort":"high","summary":"auto"})),
            ("prompt_cache_key", json!("session-test")),
        ] {
            assert_eq!(body.get(name), Some(&expected), "{name}");
        }
        assert_eq!(
            body.get("max_output_tokens"),
            limit.map(|n| json!(n)).as_ref()
        );
        assert_eq!(
            body.pointer("/input/0"),
            Some(&json!({"type":"message","role":"user","content":[
            {"type":"input_text","text":"Environment"}, {"type":"input_text","text":"Request"}]}))
        );
        let tool = body
            .pointer("/tools/0")
            .ok_or(io::Error::from(io::ErrorKind::NotFound))?;
        assert_eq!(tool.get("type"), Some(&json!("function")));
        assert_eq!(tool.get("name"), Some(&json!("read")));
        assert_eq!(tool.get("description"), Some(&json!("Read a file.")));
        assert_eq!(tool.get("strict"), Some(&json!(strict)));
        assert!(tool.get("function").is_none());
        assert_eq!(
            tool.pointer("/parameters/additionalProperties"),
            strict.then_some(json!(false)).as_ref()
        );
        assert_eq!(
            tool.pointer("/parameters/required"),
            Some(&if strict {
                json!(["path", "limit"])
            } else {
                json!(["path"])
            })
        );
        assert_eq!(
            tool.pointer("/parameters/properties/limit/type"),
            Some(&if strict {
                json!(["integer", "null"])
            } else {
                json!("integer")
            })
        );
    }
    let adapter =
        OpenAiResponsesAdapter::new(destination(&server), Capabilities::default(), String::new());
    assert!(matches!(
        adapter.build_request(
            &conversation(),
            "System",
            &tools(),
            RequestOptions {
                disable_tools: true,
                ..RequestOptions::default()
            }
        ),
        Err(OpenAiResponsesError::InvalidRequest)
    ));
    server.shutdown()?;
    Ok(())
}

#[test]
fn interleaved_calls_use_done_items_and_replay_raw_items_before_call_outputs() -> TestResult {
    let reasoning = json!({"type":"reasoning","id":"rs_1","summary":[
        {"type":"summary_text","text":"First"},{"type":"summary_text","text":"Second"}],
        "encrypted_content":"final-encrypted","status":"completed","future":{"id":"nested"}});
    let a = json!({"type":"function_call","id":"fc_A","call_id":"call/A","name":"read","arguments":"{ \"path\":\"a\" }","status":"completed"});
    let b = json!({"type":"function_call","id":"fc_B","call_id":"call/B","name":"read","arguments":"{bad json","status":"incomplete"});
    let usage = json!({"input_tokens":120,"input_tokens_details":{"cached_tokens":40,"cache_write_tokens":7},
        "output_tokens":30,"output_tokens_details":{"reasoning_tokens":12},"future":9});
    let body = sse(&[
        json!({"type":"response.queued","response":{"status":"queued"}}),
        json!({"type":"response.output_item.added","output_index":0,"item":{"type":"reasoning","id":"rs_1","summary":[]}}),
        json!({"type":"response.reasoning_text.delta","output_index":0,"delta":"PRIVATE RAW REASONING"}),
        json!({"type":"response.reasoning_summary_text.delta","output_index":0,"item_id":"rs_1","summary_index":0,"delta":"Preview"}),
        json!({"type":"response.output_item.added","output_index":1,"item":{"type":"message","id":"msg_1","role":"assistant","content":[]}}),
        json!({"type":"response.output_text.delta","output_index":1,"item_id":"msg_1","content_index":0,"delta":"日本語"}),
        json!({"type":"response.output_text.annotation.added","output_index":1,"annotation":{"future":true}}),
        json!({"type":"response.output_item.added","output_index":3,"item":{"type":"function_call","id":"fc_B","call_id":"call/B","name":"read","arguments":""}}),
        json!({"type":"response.output_item.added","output_index":2,"item":{"type":"function_call","id":"fc_A","call_id":"call/A","name":"read","arguments":""}}),
        json!({"type":"response.function_call_arguments.delta","output_index":3,"item_id":"fc_B","delta":"{bad"}),
        json!({"type":"response.function_call_arguments.delta","output_index":2,"item_id":"fc_A","delta":"{\"path\":"}),
        json!({"type":"response.function_call_arguments.delta","output_index":3,"item_id":"fc_B","delta":" preview"}),
        json!({"type":"response.function_call_arguments.delta","output_index":2,"item_id":"fc_A","delta":"\"preview\"}"}),
        json!({"type":"response.output_item.done","output_index":3,"item":b}),
        json!({"type":"response.output_item.done","output_index":2,"item":a}),
        json!({"type":"response.output_item.done","output_index":1,"item":message("Final text")}),
        json!({"type":"response.output_item.done","output_index":0,"item":reasoning}),
        json!({"type":"response.completed","response":{"status":"completed","usage":usage}}),
    ]);
    let mut script = ResponseScript::sse(body);
    script.delivery = Delivery::Chunks {
        size: 17,
        delay: Duration::from_micros(20),
    };
    let mut server = MockProvider::start(vec![script, ResponseScript::sse(simple())])?;
    let adapter = OpenAiResponsesAdapter::new(
        destination(&server),
        Capabilities {
            reasoning_replay: ReasoningReplay::SameDestination,
            ..Capabilities::default()
        },
        KEY.into(),
    );
    let (events, mut turn) = request(&adapter, &conversation(), RequestOptions::default())?;
    assert_eq!(
        turn.raw_items.items,
        vec![
            reasoning.clone(),
            message("Final text"),
            a.clone(),
            b.clone()
        ]
    );
    assert_eq!(turn.raw_items.destination, destination(&server));
    assert_eq!(turn.raw_items.adapter, AdapterKind::OpenAiResponses);
    assert_eq!(turn.finish_reason, FinishReason::ToolCalls);
    assert_eq!(
        turn.usage,
        Usage {
            input_tokens: Some(120),
            cache_read_tokens: Some(40),
            cache_write_tokens: Some(7),
            output_tokens: Some(30),
            reasoning_tokens: Some(12),
            raw: usage
        }
    );
    assert_eq!(
        turn.parts.first(),
        Some(&AssistantPart::Reasoning(Reasoning {
            text: "First\n\nSecond".into(),
            opaque: json!("final-encrypted")
        }))
    );
    assert_eq!(
        turn.parts.get(1),
        Some(&AssistantPart::Text("Final text".into()))
    );
    let mut history = conversation();
    let mut results = Vec::new();
    for (index, part) in turn.parts.iter_mut().enumerate() {
        match part {
            AssistantPart::ToolCall(call) => {
                let first = index == 2;
                assert_eq!(
                    call.provider_id.as_deref(),
                    Some(if first { "call/A" } else { "call/B" })
                );
                assert_eq!(
                    call.provider_item_id.as_deref(),
                    Some(if first { "fc_A" } else { "fc_B" })
                );
                assert_eq!(
                    call.raw_arguments,
                    if first {
                        "{ \"path\":\"a\" }"
                    } else {
                        "{bad json"
                    }
                );
                assert_eq!(
                    call.arguments,
                    if first {
                        json!({"path":"a"})
                    } else {
                        Value::Null
                    }
                );
                call.id = format!("turn-1-call-{index}");
                results.push(Item::ToolResult(ToolResult {
                    id: call.id.clone(),
                    provider_id: Some("WRONG".into()),
                    text: "ok".into(),
                    is_error: false,
                    images: Vec::new(),
                }));
            }
            AssistantPart::Text(text) => *text = "Display copy".into(),
            AssistantPart::Reasoning(_) => {}
        }
    }
    assert_eq!(
        events,
        vec![
            StreamEvent::ReasoningDelta {
                index: 0,
                text: "Preview".into()
            },
            StreamEvent::TextDelta {
                index: 1,
                text: "日本語".into()
            },
            StreamEvent::ToolCallStart {
                index: 3,
                provider_id: Some("call/B".into()),
                provider_item_id: Some("fc_B".into()),
                name: "read".into()
            },
            StreamEvent::ToolCallStart {
                index: 2,
                provider_id: Some("call/A".into()),
                provider_item_id: Some("fc_A".into()),
                name: "read".into()
            },
            StreamEvent::ToolArgumentsDelta {
                index: 3,
                delta: "{bad".into()
            },
            StreamEvent::ToolArgumentsDelta {
                index: 2,
                delta: "{\"path\":".into()
            },
            StreamEvent::ToolArgumentsDelta {
                index: 3,
                delta: " preview".into()
            },
            StreamEvent::ToolArgumentsDelta {
                index: 2,
                delta: "\"preview\"}".into()
            },
            StreamEvent::ToolCallEnd { index: 3 },
            StreamEvent::ToolCallEnd { index: 2 },
            StreamEvent::Usage {
                usage: turn.usage.clone()
            },
            StreamEvent::Finished {
                reason: FinishReason::ToolCalls
            },
        ]
    );
    history.items.push(Item::AssistantTurn(turn));
    history.items.extend(results);
    let before = history.clone();
    request(&adapter, &history, RequestOptions::default())?;
    let records = server.requests()?;
    let sent = &records
        .get(1)
        .ok_or(io::Error::from(io::ErrorKind::NotFound))?
        .body;
    assert_eq!(
        sent.get("input"),
        Some(&json!([
            {"type":"message","role":"user","content":[{"type":"input_text","text":"Environment"},{"type":"input_text","text":"Request"}]},
            reasoning,message("Final text"),a,b,
            {"type":"function_call_output","call_id":"call/A","output":"ok"},
            {"type":"function_call_output","call_id":"call/B","output":"ok"},
        ]))
    );
    assert_eq!(history, before);
    server.shutdown()?;
    Ok(())
}

fn replay_turn(destination: Destination) -> AssistantTurn {
    AssistantTurn {
        parts: vec![
            AssistantPart::Text("Common text".into()),
            AssistantPart::Reasoning(Reasoning {
                text: "Private plan".into(),
                opaque: json!("opaque"),
            }),
        ],
        raw_items: benitoiteh::provider::RawItems {
            destination,
            adapter: AdapterKind::OpenAiResponses,
            items: vec![],
        },
        finish_reason: FinishReason::Completed,
        usage: Usage::default(),
    }
}

#[test]
fn same_destination_replay_filters_only_reasoning_and_unprefixed_top_level_ids() -> TestResult {
    let mut server = MockProvider::start((0..3).map(|_| ResponseScript::sse(simple())).collect())?;
    let mut turn = replay_turn(destination(&server));
    turn.raw_items.items =
        vec![json!({"type":"reasoning","id":"rs_1","summary":[],"encrypted_content":"opaque"})];
    let ids = [
        json!("rs_1"),
        json!("_x"),
        json!("rs_"),
        json!("abc"),
        json!(7),
        Value::Null,
    ];
    for id in &ids {
        turn.raw_items.items.push(json!({"type":"future","id":id,"call_id":"keep","nested":{"id":"abc"},"phase":"commentary","status":"completed"}));
    }
    for (index, replay) in [
        ReasoningReplay::None,
        ReasoningReplay::SameDestination,
        ReasoningReplay::AllTurns,
    ]
    .into_iter()
    .enumerate()
    {
        let adapter = OpenAiResponsesAdapter::new(
            destination(&server),
            Capabilities {
                reasoning_replay: replay,
                ..Capabilities::default()
            },
            String::new(),
        );
        let mut history = conversation();
        history.items.push(Item::AssistantTurn(turn.clone()));
        request(&adapter, &history, RequestOptions::default())?;
        let records = server.requests()?;
        let body = &records
            .get(index)
            .ok_or(io::Error::from(io::ErrorKind::NotFound))?
            .body;
        let input = body
            .get("input")
            .and_then(Value::as_array)
            .ok_or(io::Error::from(io::ErrorKind::InvalidData))?;
        let offset: usize = if replay == ReasoningReplay::None {
            1
        } else {
            2
        };
        assert_eq!(input.len(), offset.saturating_add(ids.len()));
        if replay != ReasoningReplay::None {
            assert_eq!(input.get(1), turn.raw_items.items.first());
        }
        for (i, expected_id) in ids.iter().enumerate() {
            let raw = input
                .get(offset.saturating_add(i))
                .ok_or(io::Error::from(io::ErrorKind::NotFound))?;
            assert_eq!(raw.get("id"), (i == 0).then_some(expected_id));
            assert_eq!(raw.pointer("/nested/id"), Some(&json!("abc")));
            assert_eq!(raw.get("call_id"), Some(&json!("keep")));
            assert_eq!(raw.get("phase"), Some(&json!("commentary")));
            assert_eq!(raw.get("status"), Some(&json!("completed")));
        }
    }
    server.shutdown()?;
    Ok(())
}

#[test]
fn destination_changes_remap_call_ids_and_drop_reasoning_and_item_ids() -> TestResult {
    let mut server = MockProvider::start((0..5).map(|_| ResponseScript::sse(simple())).collect())?;
    let mut turn = replay_turn(destination(&server));
    for (internal, provider) in [
        ("a", format!("{}|c", "ab".repeat(40))),
        ("b", format!("{}|bc", "ab".repeat(40))),
    ] {
        turn.parts.push(AssistantPart::ToolCall(ToolCall {
            id: internal.into(),
            provider_id: Some(provider),
            provider_item_id: Some("fc_1".into()),
            name: "read".into(),
            arguments: json!({"path":"a"}),
            raw_arguments: "{ \"path\":\"a\" }".into(),
        }));
    }
    for index in 0..5 {
        let mut source = turn.clone();
        match index {
            0 => source.raw_items.destination.provider = "other".into(),
            1 => source.raw_items.destination.base_url.push('/'),
            2 => source.raw_items.destination.model = "other".into(),
            3 => source.raw_items.adapter = AdapterKind::AnthropicMessages,
            _ => source.raw_items.adapter = AdapterKind::OpenAiChat,
        }
        let mut history = conversation();
        history.items.push(Item::AssistantTurn(source));
        for id in ["a", "b"] {
            history.items.push(Item::ToolResult(ToolResult {
                id: id.into(),
                provider_id: Some("WRONG".into()),
                text: "ok".into(),
                is_error: false,
                images: Vec::new(),
            }));
        }
        let adapter = OpenAiResponsesAdapter::new(
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
            .pointer("/input/2/call_id")
            .and_then(Value::as_str)
            .ok_or(io::Error::from(io::ErrorKind::InvalidData))?;
        let b = body
            .pointer("/input/3/call_id")
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
        assert_eq!(
            body.pointer("/input/4"),
            Some(&json!({"type":"function_call_output","call_id":a,"output":"ok"}))
        );
        assert_eq!(body.pointer("/input/5/call_id"), Some(&json!(b)));
        assert_eq!(
            body.pointer("/input/1"),
            Some(
                &json!({"type":"message","role":"assistant","content":[{"type":"output_text","text":"Common text","annotations":[]}]})
            )
        );
        assert_eq!(
            body.pointer("/input/2/arguments"),
            Some(&json!("{ \"path\":\"a\" }"))
        );
        let input = body
            .get("input")
            .and_then(Value::as_array)
            .ok_or(io::Error::from(io::ErrorKind::InvalidData))?;
        assert!(input.iter().all(|item| item.get("id").is_none()));
        assert!(!body.to_string().contains("Private plan"));
        assert!(!body.to_string().contains("opaque"));
    }
    server.shutdown()?;
    Ok(())
}

#[test]
fn terminal_events_classify_limits_refusals_and_reasoning_only_turns() -> TestResult {
    let cases = [
        (
            "response.completed",
            None,
            false,
            false,
            FinishReason::Completed,
        ),
        (
            "response.incomplete",
            Some("max_output_tokens"),
            false,
            false,
            FinishReason::OutputLimit,
        ),
        (
            "response.incomplete",
            Some("content_filter"),
            false,
            false,
            FinishReason::Refusal("content_filter".into()),
        ),
        (
            "response.incomplete",
            Some("future"),
            false,
            false,
            FinishReason::Error,
        ),
        (
            "response.completed",
            None,
            true,
            false,
            FinishReason::Refusal("refusal".into()),
        ),
        (
            "response.completed",
            None,
            false,
            true,
            FinishReason::Refusal("refusal".into()),
        ),
    ];
    let mut scripts = Vec::new();
    for (kind, reason, refusal_part, refusal_delta, _) in &cases {
        let item = if *refusal_part {
            json!({"type":"message","id":"msg_1","role":"assistant","content":[{"type":"refusal","refusal":"No"}]})
        } else {
            message("Final")
        };
        let mut chunks = vec![
            json!({"type":"response.output_item.added","output_index":0,"item":{"type":"message","id":"msg_1","role":"assistant","content":[]}}),
            json!({"type":"response.output_item.done","output_index":0,"item":item}),
        ];
        if *refusal_part || *refusal_delta {
            // 拒否を completed と道具の呼び出しより優先する。
            chunks.extend([
                json!({"type":"response.output_item.added","output_index":1,"item":{"type":"function_call","id":"fc_1","call_id":"call_1","name":"read","arguments":""}}),
                json!({"type":"response.output_item.done","output_index":1,"item":{"type":"function_call","id":"fc_1","call_id":"call_1","name":"read","arguments":"{}"}}),
            ]);
        }
        if *refusal_delta {
            chunks.push(json!({"type":"response.refusal.delta","output_index":0,"delta":"No"}));
        }
        chunks.push(json!({"type":kind,"response":{"status":if *kind == "response.completed" {"completed"} else {"incomplete"},
            "incomplete_details":reason.map(|r|json!({"reason":r})),
            "usage":{"input_tokens":0,"input_tokens_details":{"cached_tokens":0},"output_tokens":0,"output_tokens_details":{"reasoning_tokens":0}}}}));
        // data の type を読み、event の名前が違っても捨てない。[DONE] も許容する。
        let body = [
            b"data: [DONE]\n\n".to_vec(),
            String::from_utf8(sse(&chunks))?
                .replace("event: response.", "event: different.")
                .into_bytes(),
        ]
        .concat();
        scripts.push(ResponseScript::sse(body));
    }
    scripts.push(ResponseScript::sse(sse(&[
        json!({"type":"response.output_item.added","output_index":0,"item":{"type":"reasoning","id":"rs_1","summary":[]}}),
        json!({"type":"response.output_item.done","output_index":0,"item":{"type":"reasoning","id":"rs_1","summary":[],"encrypted_content":"opaque"}}),
        json!({"type":"response.completed","response":{"status":"completed"}}),
    ])));
    scripts.push(ResponseScript::sse(sse(&[
        json!({"type":"response.refusal.delta","output_index":0,"delta":"No"}),
        json!({"type":"response.completed","response":{"status":"completed"}}),
    ])));
    let mut server = MockProvider::start(scripts)?;
    let adapter =
        OpenAiResponsesAdapter::new(destination(&server), Capabilities::default(), String::new());
    for (_, _, _, _, expected) in cases {
        let (events, turn) = request(&adapter, &conversation(), RequestOptions::default())?;
        assert_eq!(turn.finish_reason, expected);
        assert_eq!(
            events.last(),
            Some(&StreamEvent::Finished { reason: expected })
        );
        assert_eq!(turn.usage.input_tokens, Some(0));
        assert_eq!(turn.usage.output_tokens, Some(0));
        assert_eq!(turn.usage.cache_read_tokens, Some(0));
        assert_eq!(turn.usage.reasoning_tokens, Some(0));
        assert_eq!(turn.usage.cache_write_tokens, None);
    }
    let (_, turn) = request(&adapter, &conversation(), RequestOptions::default())?;
    assert_eq!(
        turn.parts,
        vec![AssistantPart::Reasoning(Reasoning {
            text: String::new(),
            opaque: json!("opaque")
        })]
    );
    assert_eq!(turn.finish_reason, FinishReason::Completed);
    assert_eq!(turn.usage, Usage::default());
    let (_, turn) = request(&adapter, &conversation(), RequestOptions::default())?;
    assert!(turn.parts.is_empty());
    assert_eq!(turn.finish_reason, FinishReason::Refusal("refusal".into()));
    server.shutdown()?;
    Ok(())
}

#[test]
fn http_and_stream_errors_classify_quota_context_authentication_and_retries() -> TestResult {
    let retry = ProviderError::Retryable {
        retry_after_secs: Some(7),
    };
    let invalid = ProviderError::NonRetryable {
        reason: NonRetryableKind::InvalidRequest,
    };
    let rejected = ProviderError::NonRetryable {
        reason: NonRetryableKind::RequestRejected,
    };
    let auth = ProviderError::NonRetryable {
        reason: NonRetryableKind::Authentication,
    };
    let quota = ProviderError::NonRetryable {
        reason: NonRetryableKind::Quota,
    };
    let mut cases = vec![
        (Some(500), "server_error", "error_type", retry.clone()),
        (Some(503), "server_error", "error_type", retry.clone()),
        (Some(599), "server_error", "error_type", retry.clone()),
        (Some(429), "slow_down", "error_type", retry.clone()),
        (
            Some(429),
            "rate_limit_exceeded",
            "error_type",
            retry.clone(),
        ),
        (Some(401), "invalid_api_key", "error_type", auth.clone()),
        (Some(403), "forbidden", "error_type", auth),
        (
            Some(400),
            "context_length_exceeded",
            "error_type",
            ProviderError::ContextOverflow,
        ),
        (Some(400), "bad_request", "error_type", rejected.clone()),
        (Some(413), "bad_request", "error_type", rejected.clone()),
        (Some(422), "bad_request", "error_type", invalid.clone()),
        (
            Some(400),
            "not_context_length_exceeded",
            "error_type",
            rejected,
        ),
        (
            None,
            "server_error",
            "error_type",
            ProviderError::Retryable {
                retry_after_secs: None,
            },
        ),
        (
            None,
            "rate_limit_exceeded",
            "error_type",
            ProviderError::Retryable {
                retry_after_secs: None,
            },
        ),
        (
            None,
            "slow_down",
            "error_type",
            ProviderError::Retryable {
                retry_after_secs: None,
            },
        ),
        (
            None,
            "context_length_exceeded",
            "error_type",
            ProviderError::ContextOverflow,
        ),
        (None, "bad_request", "error_type", invalid),
    ];
    for status in [Some(400), Some(429), None] {
        for code in [
            "credit_balance_exhausted",
            "organization_spend_limit_exceeded",
            "project_spend_limit_exceeded",
            "organization_usage_limit_exceeded",
            "insufficient_quota",
        ] {
            cases.push((status, code, "error_type", quota.clone()));
        }
        cases.push((status, "other", "insufficient_quota", quota.clone()));
    }
    for (code, reason) in [
        (
            "subscription_sharing_usage_limit_exceeded",
            NonRetryableKind::SubscriptionUsageLimit,
        ),
        (
            "subscription_sharing_user_not_eligible",
            NonRetryableKind::SubscriptionNotEligible,
        ),
        (
            "subscription_sharing_unsupported_capability",
            NonRetryableKind::SubscriptionUnsupported,
        ),
    ] {
        for status in [Some(400), Some(403), Some(429), Some(500), None] {
            let expected = ProviderError::NonRetryable { reason };
            cases.push((status, code, "error_type", expected.clone()));
            cases.push((status, "other", code, expected));
        }
    }
    let mut scripts = Vec::new();
    for (status, code, error_type, _) in &cases {
        let error = json!({"code":code,"type":error_type,"message":KEY});
        if let Some(status) = status {
            let mut script = ResponseScript::json(*status, &json!({"error":error}))?;
            script.headers.push(("Retry-After".into(), "7".into()));
            scripts.push(script);
        } else {
            // error と response.failed の包みの違いを、全分類について確かめる。
            scripts.push(ResponseScript::sse(sse(&[
                json!({"type":"error","code":code,"message":KEY}),
            ])));
            scripts.push(ResponseScript::sse(sse(&[
                json!({"type":"response.failed","response":{"status":"failed","error":error}}),
            ])));
        }
    }
    let mut server = MockProvider::start(scripts)?;
    let adapter =
        OpenAiResponsesAdapter::new(destination(&server), Capabilities::default(), KEY.into());
    for (status, code, error_type, expected) in cases {
        for kind in 0..if status.is_some() { 1 } else { 2 } {
            let error = request(&adapter, &conversation(), RequestOptions::default())
                .err()
                .ok_or(io::Error::from(io::ErrorKind::InvalidData))?;
            // error の最上位 type はイベントの種類なので、error.type による quota は failed だけ。
            let expected = if status.is_none()
                && kind == 0
                && (error_type == "insufficient_quota"
                    || error_type.starts_with("subscription_sharing_"))
            {
                ProviderError::NonRetryable {
                    reason: NonRetryableKind::InvalidRequest,
                }
            } else {
                expected.clone()
            };
            assert_eq!(error.classification(), expected, "{status:?} {code} {kind}");
            let message = match expected {
                ProviderError::NonRetryable {
                    reason: NonRetryableKind::SubscriptionUsageLimit,
                } => Some(
                    "ChatGPT plan usage limit reached. See https://chatgpt.com/settings/usage.",
                ),
                ProviderError::NonRetryable {
                    reason: NonRetryableKind::SubscriptionNotEligible,
                } => Some("This ChatGPT account is not eligible for ChatGPT plan usage."),
                ProviderError::NonRetryable {
                    reason: NonRetryableKind::SubscriptionUnsupported,
                } => Some("ChatGPT plan usage does not support this request."),
                ProviderError::Retryable { .. }
                | ProviderError::ContextOverflow
                | ProviderError::NonRetryable { .. }
                | ProviderError::StreamDisconnected
                | ProviderError::InvalidResponse
                | ProviderError::EmptyResponse => None,
            };
            if let Some(message) = message {
                assert_eq!(error.classification().to_string(), message);
            }
            assert!(!format!("{error:?} {error}").contains(KEY));
            let OpenAiResponsesError::Provider {
                details: Some(details),
                retry_after,
                ..
            } = error
            else {
                return Err(io::Error::from(io::ErrorKind::InvalidData).into());
            };
            assert_eq!(details.code.as_deref(), Some(code));
            assert_eq!(details.message.as_deref(), Some("[redacted]"));
            assert_eq!(retry_after.as_deref(), status.map(|_| "7"));
            assert!(!format!("{details:?}").contains(KEY));
        }
    }
    server.shutdown()?;
    Ok(())
}

#[test]
fn disconnected_and_unfinished_items_never_commit_partial_turns() -> TestResult {
    let prefix = sse(&[
        json!({"type":"response.output_item.added","output_index":0,"item":{"type":"message","id":"msg_1","role":"assistant","content":[]}}),
        json!({"type":"response.output_text.delta","output_index":0,"delta":"Partial"}),
        json!({"type":"response.output_item.done","output_index":0,"item":message("Partial")}),
    ]);
    let mut cut = ResponseScript::sse(
        [
            prefix.clone(),
            sse(&[json!({"type":"response.completed","response":{"status":"completed"}})]),
        ]
        .concat(),
    );
    cut.delivery = Delivery::Disconnect {
        after: prefix.len(),
    };
    let scripts = vec![
        ResponseScript::sse(prefix),
        cut,
        ResponseScript::sse(b"event: response.completed\ndata: {broken}\n\n".to_vec()),
        ResponseScript::sse(sse(&[
            json!({"type":"response.output_item.added","output_index":0,"item":{"type":"reasoning","id":"rs_1","summary":[]}}),
            json!({"type":"response.completed","response":{"status":"completed"}}),
        ])),
        ResponseScript::sse(sse(&[
            json!({"type":"response.completed","response":{"status":"completed"}}),
        ])),
        ResponseScript::sse(sse(&[
            json!({"type":"response.output_item.added","output_index":0,"item":{"type":"function_call","id":"fc_1","call_id":"call_1","name":"read","arguments":""}}),
            json!({"type":"response.output_item.done","output_index":0,"item":{"type":"function_call","id":"fc_1","call_id":"CHANGED","name":"read","arguments":"{}"}}),
            json!({"type":"response.completed","response":{"status":"completed"}}),
        ])),
    ];
    let mut server = MockProvider::start(scripts)?;
    let adapter =
        OpenAiResponsesAdapter::new(destination(&server), Capabilities::default(), String::new());
    let client = client();
    for index in 0..6 {
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
        if index >= 2 {
            assert!(matches!(error, OpenAiResponsesError::InvalidResponse));
        }
        assert_eq!(error.classification(), ProviderError::StreamDisconnected);
        assert_eq!(
            stream
                .poll(Duration::ZERO)
                .err()
                .map(|e| e.classification()),
            Some(ProviderError::StreamDisconnected)
        );
        assert_eq!(
            stream.finish().err().map(|e| e.classification()),
            Some(ProviderError::StreamDisconnected)
        );
    }
    server.shutdown()?;
    Ok(())
}

#[test]
fn header_only_streams_support_cancellation_and_idle_timeouts() -> TestResult {
    for cancel in [true, false] {
        let mut script = ResponseScript::sse(Vec::new());
        script.delivery = Delivery::HeadersOnly;
        let mut server = MockProvider::start(vec![script])?;
        let adapter =
            OpenAiResponsesAdapter::new(destination(&server), Capabilities::default(), KEY.into());
        let request = adapter.build_request(
            &conversation(),
            "System",
            &tools(),
            RequestOptions::default(),
        )?;
        let client = HttpClient::new(HttpTimeouts {
            idle: Duration::from_millis(150),
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
        if cancel {
            assert!(matches!(error, OpenAiResponsesError::Cancelled));
        } else {
            assert_eq!(error.classification(), ProviderError::StreamDisconnected);
        }
        assert!(!format!("{stream:?}").contains(KEY));
        stream.join()?;
        assert!(stream.is_finished());
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
            .map(|_| Ok(ResponseScript::sse(simple())))
            .collect::<Result<Vec<_>, Box<dyn Error>>>()?,
    )?;
    for (enabled, available, is_error) in cases {
        let adapter = OpenAiResponsesAdapter::new(
            destination(&server),
            Capabilities {
                image_input: enabled,
                ..Capabilities::default()
            },
            KEY.into(),
        );
        let history = image_support::history(available, is_error);
        let request =
            adapter.build_request(&history, "System", &tools(), RequestOptions::default())?;
        receive(adapter.stream(&client(), &request)?)?;
    }
    let requests = server.requests()?;
    for (index, (enabled, available, is_error)) in cases.into_iter().enumerate() {
        let body = &requests.get(index).ok_or("missing request")?.body;
        let content = body.pointer("/input/2/output").ok_or("missing output")?;
        if enabled && available && !is_error {
            assert_eq!(
                content,
                &json!([
                    {"type":"input_text", "text":"Read image file image/png (1x1, 6 bytes)"},
                    {"type":"input_image", "image_url":"data:image/png;base64,Zm9vYmFy", "detail":"auto"}
                ])
            );
            assert_eq!(body.pointer("/input/3/output"), Some(content));
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
        ResponseScript::sse(simple()),
        ResponseScript::sse(simple()),
    ])?;
    let adapter = OpenAiResponsesAdapter::new(
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
        let request =
            adapter.build_request(&history, "System", &tools(), RequestOptions::default())?;
        receive(adapter.stream(&client(), &request)?)?;
    }
    let requests = server.requests()?;
    for (index, partial) in [false, true].into_iter().enumerate() {
        let body = &requests.get(index).ok_or("missing request")?.body;
        let content = body
            .pointer(if partial {
                "/input/1/output"
            } else {
                "/input/13/output"
            })
            .ok_or("missing content")?;
        let notice = "(This image was removed to keep the request small. Read the file again if you need it.)";
        assert_eq!(content.to_string().matches(notice).count(), 1);
        assert_eq!(
            body.to_string().matches("\"type\":\"input_image\"").count(),
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

#[path = "support/chatgpt.rs"]
mod chatgpt;

struct ChatGptFixture(std::path::PathBuf);
impl ChatGptFixture {
    fn new() -> Result<Self, Box<dyn Error>> {
        let mut random = [0; 16];
        getrandom::fill(&mut random)?;
        let path =
            std::env::temp_dir().join(format!("bh-requests-{:032x}", u128::from_ne_bytes(random)));
        std::fs::create_dir(&path)?;
        Ok(Self(path))
    }
    fn store(&self, expires_at: u64) -> Result<benitoiteh::auth::Store, Box<dyn Error>> {
        let store = benitoiteh::auth::Store::open(&self.0.join("state/benitoiteh/auth"))?;
        store.lock()?.save(&chatgpt::credentials(expires_at))?;
        Ok(store)
    }
}
impl Drop for ChatGptFixture {
    fn drop(&mut self) {
        let _removed = std::fs::remove_dir_all(&self.0);
    }
}

fn chatgpt_adapter(
    server: &MockProvider,
    store: benitoiteh::auth::Store,
) -> OpenAiResponsesAdapter {
    OpenAiResponsesAdapter::new(
        destination(server),
        Capabilities {
            max_output_tokens: Some(123),
            ..AdapterKind::OpenAiResponses
                .official_capabilities()
                .unwrap_or_default()
        },
        String::new(),
    )
    .with_chatgpt_tokens(benitoiteh::provider::openai_responses::ChatGptTokens::new(
        store,
        chatgpt::endpoints(&server.base_url()),
    ))
}

fn refreshed_token() -> io::Result<ResponseScript> {
    ResponseScript::json(
        200,
        &json!({"access_token":chatgpt::NEW_ACCESS,"refresh_token":"rotated-refresh","expires_in":3600}),
    )
}

// 関門: 本文と保存の接続をアダプタの公開の境界で確かめる。
// API キーの試験では、保存の再読み取りや ChatGPT 固有の欄の省略は検出できない。
#[test]
fn chatgpt_requests_omit_unsupported_fields_and_read_tokens_for_each_send() -> TestResult {
    let fixture = ChatGptFixture::new()?;
    let store = fixture.store(4_000_000_000)?;
    let writer = benitoiteh::auth::Store::open(&fixture.0.join("state/benitoiteh/auth"))?;
    let mut server = MockProvider::start(vec![
        ResponseScript::sse(simple()),
        ResponseScript::sse(simple()),
    ])?;
    let adapter = chatgpt_adapter(&server, store);
    let request = adapter.build_request(
        &conversation(),
        "System",
        &tools(),
        RequestOptions {
            prompt_cache_key: Some("test-session"),
            ..RequestOptions::default()
        },
    )?;
    for index in 0..2 {
        if index == 1 {
            let mut saved = chatgpt::credentials(4_000_000_000);
            let tokens = saved.tokens.as_mut().ok_or("tokens")?;
            tokens.access_token = benitoiteh::auth::Secret::new(chatgpt::NEW_ACCESS.into());
            writer.lock()?.save(&saved)?;
        }
        let stream = adapter.stream(&client(), &request)?;
        for secret in [chatgpt::ACCESS, chatgpt::REFRESH, chatgpt::NEW_ACCESS] {
            assert!(!format!("{adapter:?} {request:?} {stream:?}").contains(secret));
        }
        receive(stream)?;
    }
    let requests = server.requests()?;
    assert_eq!(requests.len(), 2);
    for (request, token) in requests.iter().zip([chatgpt::ACCESS, chatgpt::NEW_ACCESS]) {
        assert_eq!(
            request.headers.get("authorization"),
            Some(&format!("Bearer {token}"))
        );
        assert_eq!(request.path, "/responses");
        let body = &request.body;
        assert_eq!(body.get("instructions"), Some(&json!("System")));
        assert_eq!(body.get("store"), Some(&json!(false)));
        assert_eq!(body.get("stream"), Some(&json!(true)));
        assert_eq!(body.get("prompt_cache_key"), Some(&json!("test-session")));
        for field in [
            "background",
            "conversation",
            "max_output_tokens",
            "max_tool_calls",
            "metadata",
            "moderation",
            "prompt",
            "prompt_cache_retention",
            "safety_identifier",
            "temperature",
            "top_logprobs",
            "top_p",
            "truncation",
            "user",
            "previous_response_id",
        ] {
            assert!(body.get(field).is_none(), "{field}");
        }
        assert!(
            !body
                .get("input")
                .and_then(Value::as_array)
                .ok_or("input")?
                .iter()
                .any(|item| item.get("role") == Some(&json!("system")))
        );
        for token in [chatgpt::ACCESS, chatgpt::REFRESH, chatgpt::NEW_ACCESS] {
            assert!(!body.to_string().contains(token));
        }
    }
    server.shutdown()?;
    Ok(())
}

// 関門: 401 の一度だけの更新と分類は、HTTP の順序と認証ヘッダを観察して確かめる。
#[test]
fn chatgpt_refreshes_once_on_401_and_classifies_refresh_failures() -> TestResult {
    for case in [
        "success",
        "second-401",
        "permanent",
        "communication",
        "forbidden",
        "before-send",
    ] {
        let fixture = ChatGptFixture::new()?;
        let store = fixture.store(if case == "before-send" {
            1
        } else {
            4_000_000_000
        })?;
        let unauthorized = || {
            ResponseScript::json(
                401,
                &json!({"error":{"code":"invalid_token","message":chatgpt::ACCESS}}),
            )
        };
        let scripts = match case {
            "success" => vec![
                unauthorized()?,
                refreshed_token()?,
                ResponseScript::sse(simple()),
            ],
            "second-401" => vec![
                unauthorized()?,
                refreshed_token()?,
                ResponseScript::json(
                    401,
                    &json!({"error":{"code":"invalid_token","message":chatgpt::NEW_ACCESS}}),
                )?,
            ],
            "permanent" => vec![
                unauthorized()?,
                ResponseScript::json(400, &json!({"error":"invalid_grant"}))?,
            ],
            "communication" => vec![
                unauthorized()?,
                ResponseScript::json(503, &json!({"error":"unavailable"}))?,
            ],
            "forbidden" => vec![ResponseScript::json(
                403,
                &json!({"error":{"code":"forbidden"}}),
            )?],
            "before-send" => vec![ResponseScript::json(503, &json!({"error":"unavailable"}))?],
            _ => return Err("unknown case".into()),
        };
        let mut server = MockProvider::start(scripts)?;
        let adapter = chatgpt_adapter(&server, store);
        let result = request(&adapter, &conversation(), RequestOptions::default());
        if case == "success" {
            assert!(result.is_ok());
        } else {
            let error = result.err().ok_or("expected failure")?;
            let expected = match case {
                "second-401" | "permanent" => ProviderError::NonRetryable {
                    reason: NonRetryableKind::LoginRequired,
                },
                "before-send" => ProviderError::Retryable {
                    retry_after_secs: None,
                },
                _ => ProviderError::NonRetryable {
                    reason: NonRetryableKind::Authentication,
                },
            };
            assert_eq!(error.classification(), expected, "{case}");
            if case == "second-401" {
                let OpenAiResponsesError::Provider {
                    details: Some(details),
                    status,
                    ..
                } = &error
                else {
                    return Err("missing error details".into());
                };
                assert_eq!(*status, Some(401));
                assert_eq!(details.message, None);
            }
            for token in [chatgpt::ACCESS, chatgpt::NEW_ACCESS, chatgpt::REFRESH] {
                assert!(!format!("{error:?} {error}").contains(token));
            }
        }
        let requests = server.requests()?;
        assert_eq!(
            requests.len(),
            match case {
                "success" | "second-401" => 3,
                "permanent" | "communication" => 2,
                _ => 1,
            }
        );
        if let Some(refresh) = requests.iter().find(|request| request.path == "/token") {
            assert!(!refresh.headers.contains_key("authorization"));
            let form = std::str::from_utf8(&refresh.raw_body)?;
            assert!(form.contains("grant_type=refresh_token"));
            assert!(form.contains(chatgpt::REFRESH));
        }
        if let Some(resent) = requests.get(2) {
            assert_eq!(
                resent.headers.get("authorization"),
                Some(&format!("Bearer {}", chatgpt::NEW_ACCESS))
            );
            assert_eq!(resent.body, requests.first().ok_or("first request")?.body);
        }
        server.shutdown()?;
    }
    Ok(())
}

// 関門: 更新中と再送中で同じ取り消しハンドルが働くことを、同期した模擬通信で確かめる。
#[test]
fn chatgpt_cancellation_survives_resend_and_prevents_send_after_refresh() -> TestResult {
    use std::sync::mpsc;
    for during_refresh in [true, false] {
        let fixture = ChatGptFixture::new()?;
        let (entered, arrival) = mpsc::channel();
        let (release, permit) = mpsc::channel();
        let controlled = Delivery::Controlled {
            entered,
            release: permit,
        };
        let mut refresh = refreshed_token()?;
        let mut response = ResponseScript::sse(simple());
        if during_refresh {
            refresh.delivery = controlled;
        } else {
            response.delivery = controlled;
        }
        let mut server = MockProvider::start(vec![
            ResponseScript::json(401, &json!({"error":{"code":"invalid_token"}}))?,
            refresh,
            response,
        ])?;
        let adapter = chatgpt_adapter(&server, fixture.store(4_000_000_000)?);
        let request = adapter.build_request(
            &conversation(),
            "System",
            &tools(),
            RequestOptions::default(),
        )?;
        let mut stream = adapter.stream(&client(), &request)?;
        let cancellation = stream.cancellation();
        let (probe, probes) = mpsc::channel();
        let (observed, observation) = mpsc::channel();
        let worker = std::thread::spawn(move || {
            loop {
                let polled = stream.poll(Duration::from_millis(20));
                if probes.try_recv().is_ok() {
                    let _sent = observed.send(());
                }
                if let Err(error) = polled {
                    stream.cancellation().cancel();
                    stream
                        .join()
                        .map_err(|_| OpenAiResponsesError::InvalidResponse)?;
                    return Err::<(), _>(error);
                }
            }
        });
        // 本文が保留されてから poll の往復を確かめる。秒数で進捗を判定しない。
        let arrived = arrival.recv_timeout(Duration::from_secs(5));
        let _sent = probe.send(());
        let responsive = observation.recv_timeout(Duration::from_secs(5));
        cancellation.cancel();
        if during_refresh {
            release.send(())?;
        } else {
            server.shutdown()?;
        }
        let result = worker.join().map_err(|_| "receiver thread failed")?;
        arrived?;
        responsive?;
        assert!(matches!(result, Err(OpenAiResponsesError::Cancelled)));
        let paths = server
            .requests()?
            .into_iter()
            .map(|r| r.path)
            .collect::<Vec<_>>();
        let expected = if during_refresh {
            vec!["/responses", "/token"]
        } else {
            vec!["/responses", "/token", "/responses"]
        };
        assert_eq!(paths, expected);
        assert!(cancellation.is_cancelled());
        let saved = benitoiteh::auth::Store::open(&fixture.0.join("state/benitoiteh/auth"))?
            .load()?
            .ok_or("saved authorization")?;
        assert_eq!(
            saved
                .tokens
                .ok_or("saved tokens")?
                .access_token
                .expose_secret(),
            chatgpt::NEW_ACCESS
        );
        server.shutdown()?;
    }
    Ok(())
}

// 関門: 再送の応答に旧・新の両トークンが含まれる場合も、詳細と Retry-After を外へ漏らさない。
#[test]
fn chatgpt_resend_errors_redact_both_rejected_and_current_tokens() -> TestResult {
    for status in [Some(401), Some(400), None] {
        let fixture = ChatGptFixture::new()?;
        let echoed = format!(
            "{} | {} | {}",
            chatgpt::ACCESS,
            chatgpt::NEW_ACCESS,
            chatgpt::ACCESS.get(..10).ok_or("prefix")?
        );
        let error = json!({"type":echoed,"code":echoed,"message":echoed});
        let response = if let Some(status) = status {
            let mut response = ResponseScript::json(status, &json!({"error":error}))?;
            response.headers.push(("Retry-After".into(), echoed));
            response
        } else {
            ResponseScript::sse(sse(&[
                json!({"type":"response.failed","response":{"status":"failed","error":error}}),
            ]))
        };
        let mut server = MockProvider::start(vec![
            ResponseScript::json(401, &json!({"error":{"code":"invalid_token"}}))?,
            refreshed_token()?,
            response,
        ])?;
        let adapter = chatgpt_adapter(&server, fixture.store(4_000_000_000)?);
        let error = request(&adapter, &conversation(), RequestOptions::default())
            .err()
            .ok_or("expected rejection")?;
        let OpenAiResponsesError::Provider {
            details: Some(details),
            retry_after,
            ..
        } = &error
        else {
            return Err("missing error details".into());
        };
        for value in [&details.code, &details.error_type, &details.message] {
            assert_eq!(value, &None);
        }
        assert_eq!(retry_after, &None);
        assert_eq!(
            server
                .requests()?
                .iter()
                .map(|r| r.path.as_str())
                .collect::<Vec<_>>(),
            ["/responses", "/token", "/responses"]
        );
        for secret in [
            chatgpt::ACCESS,
            chatgpt::NEW_ACCESS,
            chatgpt::ACCESS.get(..10).ok_or("prefix")?,
        ] {
            assert!(!format!("{error:?} {error} {details:?}").contains(secret));
        }
        server.shutdown()?;
    }
    Ok(())
}

// 関門: 送信前の更新待ちでも stream を返し、取り消しの後には推論を開始しない。
// 応答の送信をチャネルで制御し、要求の順序と保存の結果で確かめる。
#[test]
fn chatgpt_initial_token_wait_can_be_cancelled_without_sending_inference() -> TestResult {
    use std::sync::mpsc;
    let fixture = ChatGptFixture::new()?;
    let (entered, arrival) = mpsc::channel();
    let (release, permit) = mpsc::channel();
    let mut refresh = refreshed_token()?;
    refresh.delivery = Delivery::Controlled {
        entered,
        release: permit,
    };
    let mut server = MockProvider::start(vec![refresh, ResponseScript::sse(simple())])?;
    let adapter = chatgpt_adapter(&server, fixture.store(1)?);
    let request = adapter.build_request(
        &conversation(),
        "System",
        &tools(),
        RequestOptions::default(),
    )?;
    let (created, ready) = mpsc::channel();
    let creator = std::thread::spawn(move || {
        let stream = adapter.stream(&client(), &request);
        let _sent = created.send(stream);
    });
    // タイムアウトは不具合によるデッドロックの打ち切りだけに使う。
    let arrival_result = arrival.recv_timeout(Duration::from_secs(5));
    let stream_result = ready.recv_timeout(Duration::from_secs(5));
    if arrival_result.is_err() || stream_result.is_err() {
        let _released = release.send(());
        creator.join().map_err(|_| "creator thread failed")?;
        return Err("stream() must return while the initial token response is withheld".into());
    }
    let mut stream = stream_result??;
    creator.join().map_err(|_| "creator thread failed")?;
    stream.cancellation().cancel();
    release.send(())?;
    assert!(matches!(
        stream.poll(Duration::ZERO),
        Err(OpenAiResponsesError::Cancelled)
    ));
    drop(stream);
    assert_eq!(
        server
            .requests()?
            .iter()
            .map(|r| r.path.as_str())
            .collect::<Vec<_>>(),
        ["/token"]
    );
    let saved = benitoiteh::auth::Store::open(&fixture.0.join("state/benitoiteh/auth"))?
        .load()?
        .ok_or("saved authorization")?;
    let tokens = saved.tokens.ok_or("saved tokens")?;
    assert_eq!(tokens.access_token.expose_secret(), chatgpt::NEW_ACCESS);
    assert_eq!(tokens.refresh_token.expose_secret(), "rotated-refresh");
    server.shutdown()?;
    Ok(())
}

// 関門: 本物の flock を保持したまま捨て、錠の解放を待つ退行を検出する。
#[test]
fn chatgpt_cancels_lock_wait_before_drop_or_join() -> TestResult {
    use std::sync::mpsc;
    for explicit_cancel in [false, true] {
        let fixture = ChatGptFixture::new()?;
        let store = fixture.store(1)?;
        let writer = benitoiteh::auth::Store::open(&fixture.0.join("state/benitoiteh/auth"))?;
        let held = writer.lock()?;
        let mut server = MockProvider::start(vec![refreshed_token()?])?;
        let adapter = chatgpt_adapter(&server, store);
        let request = adapter.build_request(
            &conversation(),
            "System",
            &tools(),
            RequestOptions::default(),
        )?;
        let mut stream = adapter.stream(&client(), &request)?;
        // 古い実装のスレッドが錠を待ち始める時間を与える。
        std::thread::sleep(Duration::from_millis(50));
        let (completed, completion) = mpsc::channel();
        let worker = std::thread::spawn(move || {
            if explicit_cancel {
                stream.cancellation().cancel();
                assert!(matches!(
                    stream.poll(Duration::ZERO),
                    Err(OpenAiResponsesError::Cancelled)
                ));
                stream.join().map_err(|e| e.to_string())?;
            }
            drop(stream);
            let _sent = completed.send(());
            Ok::<_, String>(())
        });
        let promptly = completion.recv_timeout(Duration::from_secs(1));
        // 退行時もテストのスレッドを残さず終える。
        drop(held);
        worker
            .join()
            .map_err(|_| "stream worker")?
            .map_err(io::Error::other)?;
        promptly?;
        assert!(server.requests()?.is_empty());
        server.shutdown()?;
    }
    Ok(())
}

// 関門: 誤りの詳細の許可条件を、最初の HTTP 応答の公開の境界で確かめる。
#[test]
fn chatgpt_errors_keep_only_safe_codes_types_and_numeric_retry_after() -> TestResult {
    for (code, retry, expected_code, expected_retry) in [
        ("safe_code.123".to_owned(), "007", true, true),
        ("a".repeat(64), "7", true, true),
        ("a".repeat(65), "+7", false, false),
        (String::new(), "", false, false),
        ("UPPER".to_owned(), "7 0", false, false),
        (
            "hyphen-code".to_owned(),
            "Wed, 21 Oct 2015 07:28:00 GMT",
            false,
            false,
        ),
        ("非ASCII".to_owned(), "seven", false, false),
    ] {
        let fixture = ChatGptFixture::new()?;
        let mut response = ResponseScript::json(
            429,
            &json!({"error":{"code":code,"type":code,"message":"provider text"}}),
        )?;
        response.headers.push(("Retry-After".into(), retry.into()));
        let mut server = MockProvider::start(vec![response])?;
        let adapter = chatgpt_adapter(&server, fixture.store(4_000_000_000)?);
        let error = request(&adapter, &conversation(), RequestOptions::default())
            .err()
            .ok_or("expected rejection")?;
        let OpenAiResponsesError::Provider {
            details: Some(details),
            retry_after,
            ..
        } = error
        else {
            return Err("error details".into());
        };
        assert_eq!(
            details.code.as_deref(),
            expected_code.then_some(code.as_str())
        );
        assert_eq!(
            details.error_type.as_deref(),
            expected_code.then_some(code.as_str())
        );
        assert_eq!(details.message, None);
        assert_eq!(retry_after.as_deref(), expected_retry.then_some(retry));
        server.shutdown()?;
    }
    Ok(())
}
