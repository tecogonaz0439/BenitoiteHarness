//! 提供者の共通の公開 API の契約（設計書 02-01「共通の形」「道具の定義の変換」「HTTP の通信」）。

use std::error::Error;

use benitoiteh::provider::ids::remap_tool_call_id;
use benitoiteh::provider::sse::{MAX_EVENT_BYTES, SseDecoder, SseError, SseEvent};
use benitoiteh::provider::{
    ArgumentSpec, ArgumentType, AssistantPart, AssistantTurn, Conversation, Item, SchemaKind,
    ToolSpec, UserMessage, UserPart, UserPartKind,
};
use serde_json::{Value, json};

#[test]
fn sse_events_do_not_depend_on_chunk_boundaries() -> Result<(), Box<dyn Error>> {
    // 手書きのイベント列を基準にし、分割しない場合の出力を期待値にはしない。
    let input = concat!(
        ": keepalive\r\n\r\n",
        "event: replaced\r\nevent: content_block_delta\r\n",
        "data: {\"text\": \"日本語\"}\r\n",
        ": ignored\r\ndata:  second:line\r\nid: ignored\r\n\r\n",
        "event: discarded\n\n",
        "data: [DONE]\n\n",
        "event:\ndata\n\n",
        "event: incomplete\ndata: not dispatched\n"
    )
    .as_bytes();
    let expected = vec![
        SseEvent {
            event: "content_block_delta".into(),
            data: "{\"text\": \"日本語\"}\n second:line".into(),
        },
        SseEvent {
            event: "message".into(),
            data: "[DONE]".into(),
        },
        SseEvent {
            event: "message".into(),
            data: String::new(),
        },
    ];
    for split in 0..=input.len() {
        let mut decoder = SseDecoder::new();
        let (first, second) = input.split_at(split);
        let mut events = decoder.push(first)?;
        events.extend(decoder.push(second)?);
        decoder.finish();
        assert_eq!(events, expected, "split at {split}");
        assert_eq!(decoder.push(b"\n"), Err(SseError::Closed));
    }
    for chunk_size in 1..=input.len() {
        let mut decoder = SseDecoder::new();
        let mut events = Vec::new();
        for chunk in input.chunks(chunk_size) {
            events.extend(decoder.push(chunk)?);
        }
        decoder.finish();
        assert_eq!(events, expected, "chunk size {chunk_size}");
    }
    Ok(())
}

#[test]
fn sse_limit_includes_unterminated_lines_comments_and_unknown_fields() -> Result<(), Box<dyn Error>>
{
    // 区切りを含めてちょうど上限のイベントは受け付け、次のイベントで計数を戻す。
    let payload = "a".repeat(MAX_EVENT_BYTES.saturating_sub(8));
    let mut decoder = SseDecoder::new();
    assert_eq!(
        decoder.push(format!("data: {payload}\n\n").as_bytes())?,
        vec![SseEvent {
            event: "message".into(),
            data: payload,
        }]
    );
    assert_eq!(
        decoder.push(b"data: next\n\n")?,
        vec![SseEvent {
            event: "message".into(),
            data: "next".into(),
        }]
    );

    for prefix in ["data: ", ": ", "unknown: ", "data: first\ndata: "] {
        let mut decoder = SseDecoder::new();
        assert!(decoder.push(prefix.as_bytes())?.is_empty());
        let remaining = vec![b'a'; MAX_EVENT_BYTES.saturating_sub(prefix.len())];
        for chunk in remaining.chunks(8192) {
            assert!(decoder.push(chunk)?.is_empty());
        }
        assert_eq!(decoder.push(b"a"), Err(SseError::EventTooLarge));
        assert_eq!(
            decoder.push(b"\n\ndata: later\n\n"),
            Err(SseError::EventTooLarge)
        );
        decoder.finish();
        assert_eq!(decoder.push(b""), Err(SseError::EventTooLarge));
    }
    Ok(())
}

#[test]
fn sse_invalid_utf8_stops_instead_of_changing_provider_data() {
    let mut decoder = SseDecoder::new();
    assert_eq!(decoder.push(b"data: \xff\n\n"), Err(SseError::InvalidUtf8));
    assert_eq!(decoder.push(b"data: later\n\n"), Err(SseError::InvalidUtf8));
}

fn tool_spec() -> ToolSpec {
    ToolSpec {
        name: "sample".into(),
        description: "Example tool".into(),
        arguments: vec![
            ArgumentSpec {
                name: "path".into(),
                kind: ArgumentType::String,
                required: true,
                description: "Path".into(),
            },
            ArgumentSpec {
                name: "limit".into(),
                kind: ArgumentType::Integer,
                required: false,
                description: "Limit".into(),
            },
            ArgumentSpec {
                name: "enabled".into(),
                kind: ArgumentType::Boolean,
                required: false,
                description: "Enabled".into(),
            },
            ArgumentSpec {
                name: "names".into(),
                kind: ArgumentType::StringArray,
                required: false,
                description: "Names".into(),
            },
        ],
    }
}

#[test]
fn tool_schemas_match_each_provider_contract() {
    let cases = [
        (
            SchemaKind::Plain,
            json!({
                "type": "object",
                "properties": {
                    "path": {"type": "string", "description": "Path"},
                    "limit": {"type": "integer", "description": "Limit"},
                    "enabled": {"type": "boolean", "description": "Enabled"},
                    "names": {"type": "array", "items": {"type": "string"}, "description": "Names"}
                },
                "required": ["path"]
            }),
        ),
        (
            SchemaKind::AnthropicStrict,
            json!({
                "type": "object",
                "properties": {
                    "path": {"type": "string", "description": "Path"},
                    "limit": {"type": "integer", "description": "Limit"},
                    "enabled": {"type": "boolean", "description": "Enabled"},
                    "names": {"type": "array", "items": {"type": "string"}, "description": "Names"}
                },
                "required": ["path"],
                "additionalProperties": false
            }),
        ),
        (
            SchemaKind::OpenAiStrict,
            json!({
                "type": "object",
                "properties": {
                    "path": {"type": "string", "description": "Path"},
                    "limit": {"type": ["integer", "null"], "description": "Limit"},
                    "enabled": {"type": ["boolean", "null"], "description": "Enabled"},
                    "names": {"type": ["array", "null"], "items": {"type": "string"}, "description": "Names"}
                },
                "required": ["path", "limit", "enabled", "names"],
                "additionalProperties": false
            }),
        ),
    ];
    for (kind, expected) in cases {
        assert_eq!(tool_spec().input_schema(kind), expected, "{kind:?}");
    }
    let no_arguments = ToolSpec {
        name: "empty".into(),
        description: String::new(),
        arguments: Vec::new(),
    };
    assert_eq!(
        no_arguments.input_schema(SchemaKind::OpenAiStrict),
        json!({"type": "object", "properties": {}, "required": [], "additionalProperties": false})
    );
}

#[test]
fn openai_null_normalization_only_omits_known_optional_arguments() {
    let cases = [
        (
            json!({"path": "x", "limit": null, "enabled": null, "names": null}),
            json!({"path": "x"}),
        ),
        (
            json!({"path": null, "unknown": null, "limit": 0, "enabled": false, "names": []}),
            json!({"path": null, "unknown": null, "limit": 0, "enabled": false, "names": []}),
        ),
        (json!({}), json!({})),
        (Value::Null, Value::Null),
        (json!([null]), json!([null])),
    ];
    let spec = tool_spec();
    for (mut arguments, expected) in cases {
        spec.normalize_openai_arguments(&mut arguments);
        assert_eq!(arguments, expected);
    }
}

#[test]
fn remapped_ids_keep_distinct_sources_and_fit_anthropic() {
    // 文字を削るだけなら、ともに abc になる ID を区別する。
    assert_ne!(remap_tool_call_id("ab|c"), remap_tool_call_id("a|bc"));
    for original in [
        "".to_owned(),
        "abc".into(),
        "日本語|id".into(),
        "x".repeat(500),
    ] {
        let id = remap_tool_call_id(&original);
        assert!(!id.is_empty() && id.len() <= 64);
        assert!(
            id.bytes()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'_' | b'-'))
        );
        assert_eq!(remap_tool_call_id(&original), id);
    }
    // 既知の SHA-256 の値で、元の文字列全体を使う変換であることを確かめる。
    assert_eq!(
        remap_tool_call_id("abc"),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
}

#[test]
fn assistant_turn_json_preserves_order_ids_opaque_data_and_unreported_usage()
-> Result<(), Box<dyn Error>> {
    let fixture = json!({
        "parts": [
            {"kind": "text", "content": "before"},
            {"kind": "reasoning", "content": {"text": "", "opaque": {"encrypted_content": "opaque+/=", "signature": "signed"}}},
            {"kind": "text", "content": "after"},
            {"kind": "tool_call", "content": {
                "id": "internal-1", "provider_id": "call_1", "provider_item_id": "fc_1",
                "name": "sample", "arguments": {"path": "日本語"}, "raw_arguments": "{ \"path\": \"日本語\" }"
            }}
        ],
        "raw_items": {
            "destination": {"provider": "example", "base_url": "https://example.invalid/v1", "model": "test"},
            "adapter": "openai-responses",
            "items": [
                {"id": "msg_1", "type": "message", "content": [{"text": "before"}]},
                {"id": "rs_1", "type": "reasoning", "encrypted_content": "opaque+/=", "signature": "signed"},
                {"id": "msg_2", "type": "message", "content": [{"text": "after"}]},
                {"id": "fc_1", "type": "function_call", "call_id": "call_1", "arguments": "{ \"path\": \"日本語\" }"}
            ]
        },
        "finish_reason": {"kind": "tool_calls"},
        "usage": {
            "input_tokens": 42, "cache_read_tokens": 0, "cache_write_tokens": null,
            "output_tokens": 12, "reasoning_tokens": null,
            "raw": {"input_tokens": 42, "output_tokens": 12, "unknown_future_field": {"count": 5}}
        }
    });
    let turn: AssistantTurn = serde_json::from_value(fixture.clone())?;
    assert_eq!(serde_json::to_value(&turn)?, fixture);
    assert_eq!(
        serde_json::from_str::<AssistantTurn>(&serde_json::to_string(&turn)?)?,
        turn
    );
    assert!(matches!(
        turn.parts.get(1),
        Some(AssistantPart::Reasoning(_))
    ));
    assert_eq!(turn.usage.cache_read_tokens, Some(0));
    assert_eq!(turn.usage.cache_write_tokens, None);
    let conversation = Conversation {
        items: vec![
            Item::UserMessage(UserMessage {
                parts: vec![
                    UserPart {
                        kind: UserPartKind::Environment,
                        text: "environment".into(),
                    },
                    UserPart {
                        kind: UserPartKind::Request,
                        text: "request".into(),
                    },
                    UserPart {
                        kind: UserPartKind::Steering,
                        text: "steering".into(),
                    },
                ],
            }),
            Item::AssistantTurn(turn),
            Item::ToolResult(benitoiteh::provider::ToolResult {
                id: "internal-1".into(),
                provider_id: Some("call_1".into()),
                text: "result".into(),
                is_error: false,
            }),
        ],
    };
    assert_eq!(
        serde_json::from_slice::<Conversation>(&serde_json::to_vec(&conversation)?)?,
        conversation
    );
    Ok(())
}
