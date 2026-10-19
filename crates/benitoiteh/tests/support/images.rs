//! アダプタの画像の契約で共有する入力。要求の期待値は各 API のテストに手書きする。

use benitoiteh::provider::{
    AdapterKind, AssistantPart, AssistantTurn, Conversation, Destination, FinishReason, Item,
    RawItems, ToolCall, ToolImage, ToolResult, Usage,
};

/// 続けて二つの画像の結果を持つ会話を作る。
pub fn history(available: bool, is_error: bool) -> Conversation {
    sized_history(available, is_error, 2, 6)
}

/// 任意の枚数・記録上の大きさで上限の要求を作る。中身は短いままにする。
pub fn sized_history(available: bool, is_error: bool, count: usize, bytes: u64) -> Conversation {
    let calls = (0..count)
        .map(|index| {
            let id = format!("image-{index}");
            ToolCall {
                id: id.clone(),
                provider_id: Some(id),
                provider_item_id: None,
                name: "read".into(),
                arguments: serde_json::json!({"path":"image"}),
                raw_arguments: "{\"path\":\"image\"}".into(),
            }
        })
        .collect::<Vec<_>>();
    let mut items = vec![Item::AssistantTurn(AssistantTurn {
        parts: calls.iter().cloned().map(AssistantPart::ToolCall).collect(),
        // 別の送り先からの会話とし、三つのアダプタが呼び出しを変換する。
        raw_items: RawItems {
            destination: Destination {
                provider: "previous".into(),
                base_url: "https://previous.invalid".into(),
                model: "model".into(),
            },
            adapter: AdapterKind::OpenAiChat,
            items: Vec::new(),
        },
        finish_reason: FinishReason::ToolCalls,
        usage: Usage::default(),
    })];
    for call in calls {
        items.push(Item::ToolResult(ToolResult {
            id: call.id,
            provider_id: call.provider_id,
            text: "Read image file image/png (1x1, 6 bytes)".into(),
            is_error,
            images: vec![ToolImage {
                media_type: "image/png".into(),
                sha256: "0".repeat(64),
                width: 1,
                height: 1,
                bytes,
                data: available.then(|| std::sync::Arc::from(b"foobar".as_slice())),
            }],
        }));
    }
    Conversation { items }
}
