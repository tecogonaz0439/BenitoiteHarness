//! 模擬の認証専用の鍵と JWT。実際の資格は使わない（実装プラン P5-21「テスト」）。

use graviola::signing::rsa::{KeySize, SigningKey};
use serde_json::Value;
use std::sync::OnceLock;

type Error = Box<dyn std::error::Error>;
static KEY: OnceLock<Result<SigningKey, graviola::Error>> = OnceLock::new();
pub fn key() -> Result<&'static SigningKey, Error> {
    KEY.get_or_init(|| SigningKey::generate(KeySize::Rsa2048))
        .as_ref()
        .map_err(|_| std::io::Error::other("test key generation failed").into())
}
pub fn encode(bytes: &[u8]) -> String {
    const ABC: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut value = String::new();
    for chunk in bytes.chunks(3) {
        let mut bits = 0u32;
        for byte in chunk {
            bits = (bits << 8) | u32::from(*byte);
        }
        let missing = 3usize.saturating_sub(chunk.len());
        bits <<= u32::try_from(missing.saturating_mul(8)).unwrap_or(0);
        for shift in [18u32, 12, 6, 0]
            .into_iter()
            .take(4usize.saturating_sub(missing))
        {
            if let Some(byte) = ABC.get(usize::try_from((bits >> shift) & 63).unwrap_or(0)) {
                value.push(char::from(*byte));
            }
        }
    }
    value
}
pub fn sign(header: &Value, claims: &Value) -> Result<String, Error> {
    let message = format!(
        "{}.{}",
        encode(&serde_json::to_vec(header)?),
        encode(&serde_json::to_vec(claims)?)
    );
    let mut buffer = [0; 256];
    let signature = key()?
        .sign_pkcs1_sha256(&mut buffer, message.as_bytes())
        .map_err(|_| std::io::Error::other("test signing failed"))?;
    Ok(format!("{message}.{}", encode(signature)))
}
pub fn der() -> Result<Vec<u8>, Error> {
    let mut buffer = [0; 1024];
    Ok(key()?
        .public_key()
        .to_pkcs1_der(&mut buffer)
        .map_err(|_| std::io::Error::other("test DER failed"))?
        .to_vec())
}
pub fn integers() -> Result<(Vec<u8>, Vec<u8>), Error> {
    fn element<'a>(bytes: &mut &'a [u8], tag: u8) -> Result<&'a [u8], Error> {
        let (actual, rest) = bytes.split_first().ok_or("missing DER tag")?;
        if *actual != tag {
            return Err("wrong DER tag".into());
        }
        let (first, rest) = rest.split_first().ok_or("missing DER length")?;
        let (length, rest) = if first & 128 == 0 {
            (usize::from(*first), rest)
        } else {
            let (length, rest) = rest
                .split_at_checked(usize::from(first & 127))
                .ok_or("short DER length")?;
            (
                length
                    .iter()
                    .fold(0usize, |acc, byte| (acc << 8) | usize::from(*byte)),
                rest,
            )
        };
        let (value, rest) = rest.split_at_checked(length).ok_or("short DER value")?;
        *bytes = rest;
        Ok(value)
    }
    let der = der()?;
    let mut input = der.as_slice();
    let mut sequence = element(&mut input, 0x30)?;
    let n = element(&mut sequence, 2)?.to_vec();
    let e = element(&mut sequence, 2)?.to_vec();
    Ok((n, e))
}
pub fn jwks() -> Result<Value, Error> {
    let (n, e) = integers()?;
    Ok(
        serde_json::json!({"keys":[{"kid":"test-key", "kty":"RSA", "use":"sig", "alg":"RS256", "n":encode(&n), "e":encode(&e)}]}),
    )
}
