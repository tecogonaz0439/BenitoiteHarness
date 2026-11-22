//! `session/prompt` の `ContentBlock` の検証と変換、および base64 復号
//! （設計書 05-03「依頼の受け取りと終わりの理由」）。

use serde_json::Value;

use super::text;

/// 1 回の依頼に含められる画像の最大枚数（設計書 05-03）。
pub const MAX_PROMPT_IMAGES: usize = 20;
/// 復号後の 1 枚の画像の最大バイト数（縮小前の入力上限 64 MiB。設計書 05-03）。
pub const MAX_IMAGE_DECODED_BYTES: usize = 64 * 1024 * 1024;

/// 検証済みの `session/prompt` の入力。
pub(super) struct ParsedPrompt {
    pub text: String,
    pub inline_images: Vec<(Vec<u8>, String)>,
}

/// `params.prompt` を ACP 版 1 の規則に従って検証・変換する。
/// 知らない `type`・`audio`・`resource.blob`・上限超過・`image_input == false` はすべて拒む。
pub(super) fn parse_prompt(
    prompt: &Value,
    image_input: bool,
) -> Result<ParsedPrompt, &'static str> {
    let Some(blocks) = prompt.as_array() else {
        return Err(text::INVALID_PROMPT);
    };
    if blocks.is_empty() {
        return Err(text::INVALID_PROMPT);
    }
    let mut parts = Vec::new();
    let mut inline_images = Vec::new();
    for block in blocks {
        let Some(object) = block.as_object() else {
            return Err(text::INVALID_PROMPT);
        };
        let Some(kind) = object.get("type").and_then(Value::as_str) else {
            return Err(text::INVALID_PROMPT);
        };
        match kind {
            "text" => {
                let Some(value) = object.get("text").and_then(Value::as_str) else {
                    return Err(text::INVALID_PROMPT);
                };
                parts.push(value.to_owned());
            }
            "resource_link" => {
                // ファイルは勝手に開かず、参照情報だけを文字として依頼に添える（設計書 05-03）。
                let Some(uri) = object.get("uri").and_then(Value::as_str) else {
                    return Err(text::INVALID_PROMPT);
                };
                let name = object
                    .get("name")
                    .and_then(Value::as_str)
                    .or_else(|| object.get("title").and_then(Value::as_str));
                let formatted = match name {
                    Some(name) if !name.is_empty() => format!("[resource_link: {name} ({uri})]"),
                    _ => format!("[resource_link: {uri}]"),
                };
                parts.push(formatted);
            }
            "resource" => {
                let Some(resource) = object.get("resource").and_then(Value::as_object) else {
                    return Err(text::INVALID_PROMPT);
                };
                // バイナリの blob は拒む（設計書 05-03）。
                if resource.contains_key("blob") {
                    return Err(text::UNSUPPORTED_CONTENT_BLOCK);
                }
                let Some(uri) = resource.get("uri").and_then(Value::as_str) else {
                    return Err(text::INVALID_PROMPT);
                };
                let Some(body) = resource.get("text").and_then(Value::as_str) else {
                    return Err(text::UNSUPPORTED_CONTENT_BLOCK);
                };
                let escaped_uri = uri.replace('"', "&quot;");
                parts.push(format!("<resource uri=\"{escaped_uri}\">{body}</resource>"));
            }
            "image" => {
                if !image_input {
                    return Err(text::IMAGE_UNSUPPORTED);
                }
                if inline_images.len() >= MAX_PROMPT_IMAGES {
                    return Err(text::IMAGE_LIMIT_EXCEEDED);
                }
                let Some(data) = object.get("data").and_then(Value::as_str) else {
                    return Err(text::IMAGE_INVALID);
                };
                let Some(mime_type) = object.get("mimeType").and_then(Value::as_str) else {
                    return Err(text::IMAGE_INVALID);
                };
                if !matches!(
                    mime_type,
                    "image/png" | "image/jpeg" | "image/gif" | "image/webp"
                ) {
                    return Err(text::IMAGE_INVALID);
                }
                let bytes =
                    decode_base64(data, MAX_IMAGE_DECODED_BYTES).ok_or(text::IMAGE_INVALID)?;
                if bytes.is_empty() {
                    return Err(text::IMAGE_INVALID);
                }
                inline_images.push((bytes, mime_type.to_owned()));
            }
            _ => return Err(text::UNSUPPORTED_CONTENT_BLOCK),
        }
    }
    Ok(ParsedPrompt {
        text: parts.join("\n"),
        inline_images,
    })
}

/// 標準の RFC 4648 base64 を復号する。`unsafe` と添字アクセスを使わずに上限とパディングを厳密に検査する。
pub(super) fn decode_base64(input: &str, max_bytes: usize) -> Option<Vec<u8>> {
    let bytes = input.as_bytes();
    if bytes.is_empty() || !bytes.len().is_multiple_of(4) {
        return None;
    }
    let max_encoded = (max_bytes / 3).saturating_mul(4).saturating_add(4);
    if bytes.len() > max_encoded {
        return None;
    }
    let (chunks, remainder) = bytes.as_chunks::<4>();
    if !remainder.is_empty() {
        return None;
    }
    let total_chunks = chunks.len();
    let mut output = Vec::with_capacity((bytes.len() / 4).saturating_mul(3));
    for (index, chunk) in chunks.iter().enumerate() {
        let [c0, c1, c2, c3] = *chunk;
        let is_last = index.saturating_add(1) == total_chunks;
        let v0 = decode_sextet(c0)?;
        let v1 = decode_sextet(c1)?;
        if c2 == b'=' {
            if !is_last || c3 != b'=' || (v1 & 0x0f) != 0 {
                return None;
            }
            output.push((v0 << 2) | (v1 >> 4));
        } else if c3 == b'=' {
            if !is_last {
                return None;
            }
            let v2 = decode_sextet(c2)?;
            if (v2 & 0x03) != 0 {
                return None;
            }
            output.push((v0 << 2) | (v1 >> 4));
            output.push((v1 << 4) | (v2 >> 2));
        } else {
            let v2 = decode_sextet(c2)?;
            let v3 = decode_sextet(c3)?;
            output.push((v0 << 2) | (v1 >> 4));
            output.push((v1 << 4) | (v2 >> 2));
            output.push((v2 << 6) | v3);
        }
        if output.len() > max_bytes {
            return None;
        }
    }
    Some(output)
}

fn decode_sextet(byte: u8) -> Option<u8> {
    match byte {
        b'A'..=b'Z' => Some(byte.saturating_sub(b'A')),
        b'a'..=b'z' => Some(byte.saturating_sub(b'a').saturating_add(26)),
        b'0'..=b'9' => Some(byte.saturating_sub(b'0').saturating_add(52)),
        b'+' => Some(62),
        b'/' => Some(63),
        _ => None,
    }
}
