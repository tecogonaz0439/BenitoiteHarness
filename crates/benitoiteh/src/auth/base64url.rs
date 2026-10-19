//! パディングなしの base64url を扱う（設計書 02-01「Sign in with ChatGPT」）。

use super::AuthError;
const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

pub(super) fn encode(bytes: &[u8]) -> String {
    let mut output = String::new();
    let mut bits = 0u32;
    let mut count = 0u32;
    for byte in bytes {
        bits = (bits << 8) | u32::from(*byte);
        count = count.saturating_add(8);
        while count >= 6 {
            count = count.saturating_sub(6);
            if let Some(character) =
                ALPHABET.get(usize::try_from((bits >> count) & 63).unwrap_or(0))
            {
                output.push(char::from(*character));
            }
        }
    }
    if count > 0
        && let Some(character) =
            ALPHABET.get(usize::try_from((bits << (6u32.saturating_sub(count))) & 63).unwrap_or(0))
    {
        output.push(char::from(*character));
    }
    output
}

pub(super) fn decode(value: &str) -> Result<Vec<u8>, AuthError> {
    let mut bytes = Vec::new();
    let mut bits = 0u32;
    let mut count = 0u32;
    for byte in value.bytes() {
        let number = ALPHABET
            .iter()
            .position(|candidate| *candidate == byte)
            .ok_or(AuthError::Validation)?;
        bits = (bits << 6) | u32::try_from(number).map_err(|_| AuthError::Validation)?;
        count = count.saturating_add(6);
        if count >= 8 {
            count = count.saturating_sub(8);
            bytes.push(u8::try_from((bits >> count) & 255).map_err(|_| AuthError::Validation)?);
        }
    }
    // 余ったビットも検査し、同じ値の別表現を受け付けない。
    if count >= 6 || bits & ((1u32 << count).saturating_sub(1)) != 0 {
        return Err(AuthError::Validation);
    }
    Ok(bytes)
}
