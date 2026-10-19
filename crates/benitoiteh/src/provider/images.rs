//! 画像を送れる条件と base64 の符号化を共有する（設計書 02-01「画像の受け渡し」）。

use std::collections::{BTreeSet, HashSet, VecDeque};

use super::{Conversation, Item, ToolImage, ToolResult, text};

const MAX_BYTES: u64 = 12_582_912;
const TARGET_BYTES: u64 = 6_291_456;
const MAX_COUNT: usize = 20;
const TARGET_COUNT: usize = 10;

/// 会話の順から決まる送信候補と除外位置（設計書 02-01「送る画像の上限」）。
pub(crate) struct ImagePlan {
    dropped: BTreeSet<(usize, usize)>,
    sent: usize,
}

impl ImagePlan {
    pub(super) fn dropped(&self, item: usize, image: usize) -> bool {
        self.dropped.contains(&(item, image))
    }

    /// この要求が画像を少なくとも一枚送るか。
    pub(crate) fn has_images(&self) -> bool {
        self.sent > 0
    }
}

/// 外部の状態を使わず、古い画像から上限を適用する。
pub(crate) fn plan(conversation: &Conversation, enabled: bool) -> ImagePlan {
    let mut selected = VecDeque::new();
    let mut dropped = BTreeSet::new();
    let mut bytes = 0_u64;
    if enabled {
        for (item_index, item) in conversation.items.iter().enumerate() {
            let Item::ToolResult(result) = item else {
                continue;
            };
            if result.is_error {
                continue;
            }
            for (image_index, image) in result.images.iter().enumerate() {
                if image.data.is_none() {
                    continue;
                }
                if bytes.saturating_add(image.bytes) > MAX_BYTES || selected.len() >= MAX_COUNT {
                    while bytes > TARGET_BYTES || selected.len() > TARGET_COUNT {
                        let Some((position, size)) = selected.pop_front() else {
                            break;
                        };
                        bytes = bytes.saturating_sub(size);
                        dropped.insert(position);
                    }
                }
                selected.push_back(((item_index, image_index), image.bytes));
                bytes = bytes.saturating_add(image.bytes);
            }
        }
    }
    ImagePlan {
        dropped,
        sent: selected.len(),
    }
}

/// 指定した結果（None なら全結果）から画像を除き、再開と同じ文を残す。
pub(crate) fn remove_images(
    conversation: &mut Conversation,
    results: Option<&[String]>,
) -> Vec<String> {
    let results = results.map(|ids| ids.iter().map(String::as_str).collect::<HashSet<_>>());
    let mut removed = Vec::new();
    for item in &mut conversation.items {
        let Item::ToolResult(result) = item else {
            continue;
        };
        if result.images.is_empty()
            || results
                .as_ref()
                .is_some_and(|ids| !ids.contains(result.id.as_str()))
        {
            continue;
        }
        result.images.clear();
        result.text.push('\n');
        result.text.push_str(text::IMAGE_REMOVED);
        removed.push(result.id.clone());
    }
    removed
}

pub(super) fn prepare<'a>(
    result: &'a ToolResult,
    enabled: bool,
    plan: &ImagePlan,
    item: usize,
) -> (String, Vec<&'a ToolImage>) {
    let mut body = result.text.clone();
    let mut images = Vec::new();
    let mut unsupported = false;
    let mut unavailable = false;
    let mut removed = false;
    for (index, image) in result.images.iter().enumerate() {
        if !enabled {
            unsupported = true;
        } else if image.data.is_none() {
            unavailable = true;
        } else if plan.dropped(item, index) {
            removed = true;
        } else if !result.is_error {
            images.push(image);
        }
    }
    for (omitted, notice) in [
        (unsupported, text::IMAGE_UNSUPPORTED),
        (unavailable, text::IMAGE_UNAVAILABLE),
        (removed, text::IMAGE_REMOVED),
    ] {
        if omitted {
            body.push('\n');
            body.push_str(notice);
        }
    }
    (body, images)
}

pub(super) fn data_url(image: &ToolImage) -> String {
    format!(
        "data:{};base64,{}",
        image.media_type,
        encode(image.data.as_deref().unwrap_or_default())
    )
}

pub(super) fn encode(data: &[u8]) -> String {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut output = String::new();
    for chunk in data.chunks(3) {
        let a = u32::from(chunk.first().copied().unwrap_or(0));
        let b = u32::from(chunk.get(1).copied().unwrap_or(0));
        let c = u32::from(chunk.get(2).copied().unwrap_or(0));
        let value = (a << 16) | (b << 8) | c;
        for (position, shift) in [18, 12, 6, 0].into_iter().enumerate() {
            if position > chunk.len() {
                output.push('=');
            } else {
                // マスクした値は 0..64 に収まる（設計書 02-01「画像の受け渡し」）。
                let index = ((value >> shift) & 63) as usize;
                output.push(char::from(ALPHABET.get(index).copied().unwrap_or(b'A')));
            }
        }
    }
    output
}

#[cfg(test)]
mod tests {
    //! 画像の選別規則と RFC 4648 の符号化の互換性を確かめる。

    use super::*;

    fn result(count: usize, bytes: u64, available: bool, is_error: bool) -> Item {
        Item::ToolResult(ToolResult {
            id: "result".into(),
            provider_id: None,
            text: "image".into(),
            is_error,
            images: (0..count)
                .map(|_| ToolImage {
                    media_type: "image/gif".into(),
                    sha256: "0".repeat(64),
                    width: 1,
                    height: 1,
                    bytes,
                    data: available.then(|| std::sync::Arc::from(b"image".as_slice())),
                })
                .collect(),
        })
    }

    // 作成時の関門: 規則の境界値と、追加したときに除外が戻らない契約をここで守る。
    // API の包みはアダプタのテストが持ち、ここでは送る画像の位置だけを確かめる。
    #[test]
    fn limits_drop_oldest_before_adding_and_never_restore_with_appended_images() {
        for (bytes, trigger, removed, sent) in [(1_048_576, 13, 6, 7), (1, 21, 10, 11)] {
            let mut previous = BTreeSet::new();
            for count in 1..=100 {
                let conversation = Conversation {
                    items: vec![result(count, bytes, true, false)],
                };
                let plan = plan(&conversation, true);
                assert!(previous.is_subset(&plan.dropped));
                if count < trigger {
                    assert!(plan.dropped.is_empty());
                }
                if count == trigger {
                    assert_eq!(plan.sent, sent);
                    assert_eq!(plan.dropped, (0..removed).map(|i| (0, i)).collect());
                }
                previous = plan.dropped;
            }
        }
    }

    #[test]
    fn only_available_successful_images_count_in_conversation_order() {
        let conversation = Conversation {
            items: vec![
                result(30, 1_048_576, false, false),
                result(30, 1_048_576, true, true),
                result(8, 1_048_576, true, false),
                result(5, 1_048_576, true, false),
            ],
        };
        let selected = plan(&conversation, true);
        assert_eq!(selected.sent, 7);
        assert_eq!(selected.dropped, (0..6).map(|i| (2, i)).collect());
        let disabled = plan(&conversation, false);
        assert!(!disabled.has_images());
        assert!(disabled.dropped.is_empty());
    }

    // 作成時の関門: 書き換えた記録の大きい一覧でも、指定した結果だけを外す契約を守る。
    // 計算時間は測らず、空の一覧と未指定の結果、画像のない結果も同じ境界で確かめる。
    #[test]
    fn removal_selects_only_matching_nonempty_results_from_large_or_empty_lists() {
        let mut conversation = Conversation {
            items: ["id-0", "keep", "id-99999", "id-50000", "id-1"]
                .into_iter()
                .map(|id| {
                    let mut item = result(usize::from(id != "id-1"), 10, true, false);
                    if let Item::ToolResult(result) = &mut item {
                        result.id = id.into();
                    }
                    item
                })
                .collect(),
        };
        let before = conversation.clone();
        assert!(remove_images(&mut conversation, Some(&[])).is_empty());
        assert_eq!(conversation, before);
        let ids = (0..100_000)
            .map(|id| format!("id-{id}"))
            .collect::<Vec<_>>();
        assert_eq!(
            remove_images(&mut conversation, Some(&ids)),
            ["id-0", "id-99999", "id-50000"]
        );
        for item in &conversation.items {
            if let Item::ToolResult(result) = item {
                if result.id == "keep" {
                    assert_eq!(result.images.len(), 1);
                    assert_eq!(result.text, "image");
                } else {
                    assert!(result.images.is_empty());
                    assert_eq!(
                        result.text.contains(text::IMAGE_REMOVED),
                        result.id != "id-1"
                    );
                }
            }
        }
    }

    #[test]
    fn rfc4648_vectors() {
        for (input, expected) in [
            ("", ""),
            ("f", "Zg=="),
            ("fo", "Zm8="),
            ("foo", "Zm9v"),
            ("foob", "Zm9vYg=="),
            ("fooba", "Zm9vYmE="),
            ("foobar", "Zm9vYmFy"),
        ] {
            assert_eq!(super::encode(input.as_bytes()), expected);
        }
    }
}
