//! Gemini の互換の応答と不透明なデータの再送の契約（設計書 02-01「OpenAI の Chat Completions API」）。

use super::*;
use benitoiteh::provider::openai_chat::OpenAiChatAssembler;
use benitoiteh::provider::sse::SseEvent;

fn assembler() -> OpenAiChatAssembler {
    OpenAiChatAssembler::new(Destination {
        provider: "gemini".into(),
        base_url: "https://generativelanguage.googleapis.com/v1beta/openai".into(),
        model: "mock-model".into(),
    })
}

fn feed(a: &mut OpenAiChatAssembler, data: &Value) -> Result<Vec<StreamEvent>, OpenAiChatError> {
    a.read_event(&SseEvent {
        event: "message".into(),
        data: data.to_string(),
    })
}

fn done(a: &mut OpenAiChatAssembler) -> Result<Vec<StreamEvent>, OpenAiChatError> {
    a.read_event(&SseEvent {
        event: "message".into(),
        data: "[DONE]".into(),
    })
}

// 作成時の関門: total にだけ含まれる推論を、最後の使用量から一度だけ数える契約。
// 既存のテストは推論の明示値か completion だけを扱い、この過少計上を捕まえない。
#[test]
fn final_usage_counts_reasoning_reported_only_in_total_once() -> TestResult {
    let mut a = assembler();
    feed(
        &mut a,
        &json!({"choices":[{"delta":{"content":"Text"}}],
            "usage":{"prompt_tokens":36,"completion_tokens":1,"total_tokens":37}}),
    )?;
    let raw = json!({"prompt_tokens":36,"completion_tokens":5,"total_tokens":290});
    let expected = Usage {
        input_tokens: Some(36),
        output_tokens: Some(254),
        reasoning_tokens: Some(249),
        raw: raw.clone(),
        ..Usage::default()
    };
    let events = feed(
        &mut a,
        &json!({"choices":[{"delta":{},"finish_reason":"stop"}],"usage":raw}),
    )?;
    assert_eq!(
        events,
        vec![StreamEvent::Usage {
            usage: expected.clone()
        }]
    );
    done(&mut a)?;
    assert_eq!(a.finish()?.usage, expected);
    // 明示された推論は内数のままにし、不足した報告や溢れる和から推測しない。
    for (raw, output, reasoning) in [
        (
            json!({"prompt_tokens":36,"completion_tokens":5,"total_tokens":290,
            "completion_tokens_details":{"reasoning_tokens":2}}),
            5,
            Some(2),
        ),
        (
            json!({"prompt_tokens":36,"completion_tokens":5,"total_tokens":290,
            "completion_tokens_details":{"reasoning_tokens":0}}),
            5,
            Some(0),
        ),
        (
            json!({"prompt_tokens":36,"completion_tokens":5,"total_tokens":41}),
            5,
            None,
        ),
        (
            json!({"prompt_tokens":36,"completion_tokens":5,"total_tokens":40}),
            5,
            None,
        ),
        (json!({"prompt_tokens":36,"completion_tokens":5}), 5, None),
        (
            json!({"prompt_tokens":u64::MAX,"completion_tokens":5,"total_tokens":u64::MAX}),
            5,
            None,
        ),
        (
            json!({"prompt_tokens":36,"completion_tokens":5,"total_tokens":290,
            "completion_tokens_details":{"reasoning_tokens":null}}),
            254,
            Some(249),
        ),
    ] {
        let mut a = assembler();
        feed(
            &mut a,
            &json!({"choices":[{"delta":{"content":"Text"},"finish_reason":"stop"}],"usage":raw}),
        )?;
        done(&mut a)?;
        let usage = a.finish()?.usage;
        assert_eq!(usage.output_tokens, Some(output));
        assert_eq!(usage.reasoning_tokens, reasoning);
        assert_eq!(usage.raw, raw);
    }
    Ok(())
}

// 2026-10-18 に受け取った形。署名は短い偽の値だけを使う（実装プラン P1-12）。
fn call(id: &str) -> Value {
    json!({"id":id,"type":"function","function":{"name":"read","arguments":"{\"path\":\"a.bnt\"}"},
        "extra_content":{"google":{"thought_signature":"fake-call-signature"},"future":[1,null]}})
}

#[test]
fn complete_calls_without_indices_preserve_arrival_order_metadata_and_final_usage() -> TestResult {
    for (count, batched) in [(1, false), (2, false), (2, true)] {
        let mut a = assembler();
        feed(
            &mut a,
            &json!({"choices":[{"delta":{"role":"assistant","extra_content":{
            "google":{"thought_signature":"old"},"retained":true}}}],
            "usage":{"prompt_tokens":10,"completion_tokens":1}}),
        )?;
        let calls: Vec<_> = ["call_z", "call_a"]
            .into_iter()
            .take(count)
            .map(call)
            .collect();
        for batch in calls.chunks(if batched { count } else { 1 }) {
            feed(
                &mut a,
                &json!({"choices":[{"delta":{"tool_calls":batch}}],
                "usage":{"prompt_tokens":10,"completion_tokens":2}}),
            )?;
        }
        let events = feed(
            &mut a,
            &json!({"choices":[{"delta":{"extra_content":{
            "google":{"thought_signature":"fake-message-signature"}}},"finish_reason":"stop"}],
            "usage":{"prompt_tokens":10,"completion_tokens":3}}),
        )?;
        let starts: Vec<_> = events
            .iter()
            .filter_map(|e| match e {
                StreamEvent::ToolCallStart {
                    index, provider_id, ..
                } => Some((*index, provider_id.clone())),
                StreamEvent::TextDelta { .. }
                | StreamEvent::ReasoningDelta { .. }
                | StreamEvent::ToolArgumentsDelta { .. }
                | StreamEvent::ToolCallEnd { .. }
                | StreamEvent::Usage { .. }
                | StreamEvent::Finished { .. } => None,
            })
            .collect();
        assert_eq!(
            starts,
            calls
                .iter()
                .enumerate()
                .map(|(i, c)| (i, c.get("id").and_then(Value::as_str).map(str::to_owned)))
                .collect::<Vec<_>>()
        );
        assert_eq!(
            done(&mut a)?,
            vec![StreamEvent::Finished {
                reason: FinishReason::ToolCalls
            }]
        );
        let turn = a.finish()?;
        assert_eq!(turn.finish_reason, FinishReason::ToolCalls);
        assert_eq!(
            turn.raw_items.items,
            vec![json!({"role":"assistant","content":null,
            "tool_calls":calls,"extra_content":{"google":{"thought_signature":"fake-message-signature"},"retained":true}})]
        );
        let ids: Vec<_> = turn
            .parts
            .iter()
            .filter_map(|p| match p {
                AssistantPart::ToolCall(c) => Some(c.provider_id.clone()),
                AssistantPart::Text(_) | AssistantPart::Reasoning(_) => None,
            })
            .collect();
        assert_eq!(
            ids,
            starts.into_iter().map(|(_, id)| id).collect::<Vec<_>>()
        );
        assert_eq!(turn.usage.input_tokens, Some(10));
        assert_eq!(turn.usage.output_tokens, Some(3));
    }
    Ok(())
}

#[test]
fn malformed_calls_and_metadata_are_rejected_without_committing_a_turn() -> TestResult {
    let mut indexed = call("indexed");
    indexed
        .as_object_mut()
        .ok_or(io::Error::from(io::ErrorKind::InvalidData))?
        .insert("index".into(), json!(0));
    let mut deltas = vec![
        vec![json!({"tool_calls":[indexed.clone(),call("plain")]})],
        vec![json!({"tool_calls":[call("plain"),indexed.clone()]})],
        vec![
            json!({"tool_calls":[indexed.clone()]}),
            json!({"tool_calls":[call("plain")]}),
        ],
        vec![
            json!({"tool_calls":[call("plain")]}),
            json!({"tool_calls":[indexed]}),
        ],
    ];
    for invalid in [json!("signature"), json!([]), json!(42)] {
        deltas.push(vec![json!({"extra_content":invalid})]);
        let mut c = call("bad-extra");
        c.as_object_mut()
            .ok_or(io::Error::from(io::ErrorKind::InvalidData))?
            .insert("extra_content".into(), invalid);
        c.as_object_mut()
            .ok_or(io::Error::from(io::ErrorKind::InvalidData))?
            .insert("index".into(), json!(0));
        deltas.push(vec![json!({"tool_calls":[c]})]);
    }
    for pointer in ["/id", "/type", "/function/name", "/function/arguments"] {
        let mut c = call("incomplete");
        *c.pointer_mut(pointer)
            .ok_or(io::Error::from(io::ErrorKind::InvalidData))? = Value::Null;
        deltas.push(vec![json!({"tool_calls":[c]})]);
    }
    for deltas in deltas {
        let mut a = assembler();
        let error = deltas
            .into_iter()
            .find_map(|delta| feed(&mut a, &json!({"choices":[{"delta":delta}]})).err())
            .ok_or(io::Error::from(io::ErrorKind::InvalidData))?;
        assert!(matches!(error, OpenAiChatError::InvalidResponse));
        assert_eq!(
            error.classification().to_string(),
            "The provider returned a response that benitoiteh could not read."
        );
        assert!(a.finish().is_err());
    }
    Ok(())
}

fn nested_extra(depth: usize) -> Value {
    let mut value = json!("signature");
    for level in 1..depth {
        value = if level % 2 == 0 {
            json!([value])
        } else {
            json!({"nested":value})
        };
    }
    json!({"nested":value})
}

fn sized_extra(bytes: usize) -> Value {
    // {"x":""} は 8 bytes。改行の JSON 表現は 2 bytes として境界に含める。
    json!({"x":format!("{}\n", "a".repeat(bytes.saturating_sub(10)))})
}

fn extra_delta(extra: &Value, in_call: bool) -> Value {
    if in_call {
        json!({"tool_calls":[{"index":0,"id":"call_a","type":"function",
            "function":{"name":"read","arguments":"{}"},"extra_content":extra}]})
    } else {
        json!({"content":"Text","extra_content":extra})
    }
}

#[test]
fn metadata_depth_counts_objects_and_arrays_and_rejects_more_than_sixteen() -> TestResult {
    for in_call in [false, true] {
        for depth in [16, 17, 122] {
            let mut a = assembler();
            let delta = extra_delta(&nested_extra(depth), in_call);
            let result = feed(
                &mut a,
                &json!({"choices":[{"delta":delta,"finish_reason":"stop"}]}),
            );
            if depth == 16 {
                result?;
                done(&mut a)?;
                a.finish()?;
            } else {
                assert_eq!(
                    result.err().map(|e| e.classification()),
                    Some(ProviderError::InvalidResponse)
                );
                assert!(a.finish().is_err());
            }
        }
    }
    Ok(())
}

#[test]
fn metadata_size_limit_counts_serialized_bytes_across_message_and_calls() -> TestResult {
    for location in 0..3 {
        for excess in [0, 1] {
            let mut a = assembler();
            let delta = if location < 2 {
                extra_delta(
                    &sized_extra(65_536_usize.saturating_add(excess)),
                    location == 1,
                )
            } else {
                json!({"extra_content":sized_extra(32_768),"tool_calls":[
                    {"index":0,"id":"a","function":{"name":"read","arguments":"{}"},"extra_content":sized_extra(16_384)},
                    {"index":1,"id":"b","function":{"name":"read","arguments":"{}"},"extra_content":sized_extra(16_384_usize.saturating_add(excess))}]})
            };
            let result = feed(
                &mut a,
                &json!({"choices":[{"delta":delta,"finish_reason":"stop"}]}),
            );
            if excess == 0 {
                result?;
                done(&mut a)?;
                a.finish()?;
            } else {
                assert_eq!(
                    result.err().map(|e| e.classification()),
                    Some(ProviderError::InvalidResponse)
                );
                assert!(a.finish().is_err());
            }
        }
    }
    // 同じ欄の差し替えでは、受信した総量でなく保持した大きさを数える。
    for in_call in [false, true] {
        let mut a = assembler();
        let extra = sized_extra(65_536);
        feed(
            &mut a,
            &json!({"choices":[{"delta":extra_delta(&extra,in_call)}]}),
        )?;
        let delta = if in_call {
            json!({"tool_calls":[{"index":0,"extra_content":extra}]})
        } else {
            json!({"extra_content":extra})
        };
        feed(
            &mut a,
            &json!({"choices":[{"delta":delta,"finish_reason":"stop"}]}),
        )?;
        done(&mut a)?;
        a.finish()?;
    }
    Ok(())
}

#[test]
fn null_metadata_is_absent_and_does_not_clear_previously_retained_fields() -> TestResult {
    for existing in [false, true] {
        let mut a = assembler();
        let extra = if existing {
            json!({"google":{"thought_signature":"fake-signature"}})
        } else {
            Value::Null
        };
        feed(
            &mut a,
            &json!({"choices":[{"delta":{"content":"Text","extra_content":extra,
            "tool_calls":[{"index":0,"id":"a","function":{"name":"read","arguments":"{}"},"extra_content":extra}]}}]}),
        )?;
        feed(
            &mut a,
            &json!({"choices":[{"delta":{"extra_content":null,"tool_calls":[{"index":0,"extra_content":null}]},"finish_reason":"stop"}]}),
        )?;
        done(&mut a)?;
        let turn = a.finish()?;
        let raw = turn
            .raw_items
            .items
            .first()
            .ok_or(io::Error::from(io::ErrorKind::InvalidData))?;
        for pointer in ["/extra_content", "/tool_calls/0/extra_content"] {
            assert_eq!(raw.pointer(pointer), existing.then_some(&extra));
        }
    }
    Ok(())
}

#[test]
fn calls_require_done_and_do_not_override_output_limits() -> TestResult {
    for (reason, ended) in [("stop", false), ("length", true)] {
        let mut a = assembler();
        feed(
            &mut a,
            &json!({"choices":[{"delta":{"tool_calls":[call("call_z")]},"finish_reason":reason}]}),
        )?;
        if ended {
            assert_eq!(
                done(&mut a)?,
                vec![StreamEvent::Finished {
                    reason: FinishReason::OutputLimit
                }]
            );
            assert_eq!(a.finish()?.finish_reason, FinishReason::OutputLimit);
        } else {
            assert_eq!(
                a.finish().err().map(|e| e.classification()),
                Some(ProviderError::StreamDisconnected)
            );
        }
    }
    Ok(())
}

#[test]
fn request_replays_message_and_call_metadata_only_to_the_same_destination() -> TestResult {
    let mut a = assembler();
    feed(
        &mut a,
        &json!({"choices":[{"delta":{"content":"Text","extra_content":{"google":{"thought_signature":"fake-text-signature"}}},"finish_reason":"stop"}]}),
    )?;
    done(&mut a)?;
    let text = a.finish()?;
    let mut a = assembler();
    feed(
        &mut a,
        &json!({"choices":[{"delta":{"tool_calls":[call("call_z")]},"finish_reason":"stop"}]}),
    )?;
    done(&mut a)?;
    let tool = a.finish()?;
    let mut server = MockProvider::start(vec![
        ResponseScript::sse(simple("stop")),
        ResponseScript::sse(simple("stop")),
    ])?;
    for same in [true, false] {
        let mut text = text.clone();
        let mut tool = tool.clone();
        for turn in [&mut text, &mut tool] {
            turn.raw_items.destination = destination(&server);
            if !same {
                turn.raw_items.destination.model = "other".into();
            }
        }
        let mut history = conversation();
        history.items.extend([
            Item::AssistantTurn(text.clone()),
            Item::AssistantTurn(tool.clone()),
        ]);
        let adapter =
            OpenAiChatAdapter::new(destination(&server), Capabilities::default(), String::new());
        request(&adapter, &history, RequestOptions::default())?;
        let records = server.requests()?;
        let body = &records
            .last()
            .ok_or(io::Error::from(io::ErrorKind::InvalidData))?
            .body;
        if same {
            assert_eq!(body.pointer("/messages/2"), text.raw_items.items.first());
            assert_eq!(body.pointer("/messages/3"), tool.raw_items.items.first());
        } else {
            assert!(body.pointer("/messages/2/extra_content").is_none());
            assert!(
                body.pointer("/messages/3/tool_calls/0/extra_content")
                    .is_none()
            );
        }
    }
    server.shutdown()?;
    Ok(())
}
