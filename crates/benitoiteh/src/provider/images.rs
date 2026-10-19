//! 画像を送れる条件と base64 の符号化を共有する（設計書 02-01「画像の受け渡し」）。

use super::{ToolImage, ToolResult, text};

pub(super) fn prepare(result: &ToolResult, enabled: bool) -> (String, Vec<&ToolImage>) {
    let mut body = result.text.clone();
    let mut images = Vec::new();
    let mut unsupported = false;
    let mut unavailable = false;
    for image in &result.images {
        if !enabled {
            unsupported = true;
        } else if image.data.is_none() {
            unavailable = true;
        } else if !result.is_error {
            images.push(image);
        }
    }
    for (omitted, notice) in [
        (unsupported, text::IMAGE_UNSUPPORTED),
        (unavailable, text::IMAGE_UNAVAILABLE),
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
    //! RFC 4648 の値で API に渡す符号化の互換性を確かめる。

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
