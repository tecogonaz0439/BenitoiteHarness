//! ID トークンを JWKS の RSA 鍵で検証する（設計書 02-01「Sign in with ChatGPT」、BH-114）。

use super::{Account, AuthError, base64url, constant_time_eq};
use graviola::signing::rsa::VerifyingKey;
use serde::Deserialize;

#[derive(Deserialize)]
struct Header {
    alg: String,
    kid: String,
    #[serde(default)]
    crit: Option<serde_json::Value>,
}
#[derive(Deserialize)]
pub(super) struct Jwks {
    keys: Vec<Jwk>,
}
#[derive(Deserialize)]
struct Jwk {
    kid: Option<String>,
    kty: String,
    n: Option<String>,
    e: Option<String>,
    r#use: Option<String>,
    alg: Option<String>,
}
#[derive(Deserialize)]
#[serde(untagged)]
enum Audience {
    One(String),
    Many(Vec<String>),
}
#[derive(Deserialize)]
struct Claims {
    iss: String,
    aud: Audience,
    azp: Option<String>,
    exp: u64,
    sub: String,
    nonce: String,
    email: Option<String>,
}

pub(super) fn verify(
    token: &str,
    jwks: &Jwks,
    issuer: &str,
    client: &str,
    nonce: &str,
    now: u64,
) -> Result<Account, AuthError> {
    if token.len() > 16 * 1024 {
        return Err(AuthError::Validation);
    }
    let mut parts = token.split('.');
    let head = parts.next().ok_or(AuthError::Validation)?;
    let payload = parts.next().ok_or(AuthError::Validation)?;
    let signature = parts.next().ok_or(AuthError::Validation)?;
    if parts.next().is_some() {
        return Err(AuthError::Validation);
    }
    // crit は null も含め、欄が存在するだけで断る。
    let header_bytes = base64url::decode(head)?;
    let header: Header =
        serde_json::from_slice(&header_bytes).map_err(|_| AuthError::Validation)?;
    let raw: serde_json::Value =
        serde_json::from_slice(&header_bytes).map_err(|_| AuthError::Validation)?;
    if header.alg != "RS256"
        || header.kid.is_empty()
        || header.crit.is_some()
        || raw.get("crit").is_some()
    {
        return Err(AuthError::Validation);
    }
    let mut matches = jwks
        .keys
        .iter()
        .filter(|key| key.kid.as_deref() == Some(&header.kid));
    let key = matches.next().ok_or(AuthError::Validation)?;
    if matches.next().is_some()
        || key.kty != "RSA"
        || key.r#use.as_deref().is_some_and(|v| v != "sig")
        || key.alg.as_deref().is_some_and(|v| v != "RS256")
    {
        return Err(AuthError::Validation);
    }
    let n = base64url::decode(key.n.as_deref().ok_or(AuthError::Validation)?)?;
    let e = base64url::decode(key.e.as_deref().ok_or(AuthError::Validation)?)?;
    let signature = base64url::decode(signature)?;
    let n = trim_zeroes(&n);
    if signature.len() != n.len() {
        return Err(AuthError::Validation);
    }
    let der = public_key_der(n, &e)?;
    VerifyingKey::from_pkcs1_der(&der)
        .map_err(|_| AuthError::Validation)?
        .verify_pkcs1_sha256(&signature, format!("{head}.{payload}").as_bytes())
        .map_err(|_| AuthError::Validation)?;
    let claims: Claims =
        serde_json::from_slice(&base64url::decode(payload)?).map_err(|_| AuthError::Validation)?;
    let audience = match claims.aud {
        Audience::One(aud) => aud == client,
        Audience::Many(aud) => {
            aud.iter().any(|v| v == client)
                && (aud.len() < 2 || claims.azp.as_deref() == Some(client))
        }
    };
    if claims.iss != issuer
        || !audience
        || claims.exp <= now
        || claims.sub.is_empty()
        || !constant_time_eq(claims.nonce.as_bytes(), nonce.as_bytes())
    {
        return Err(AuthError::Validation);
    }
    Ok(Account {
        email: claims.email,
        subject: claims.sub,
    })
}
fn trim_zeroes(bytes: &[u8]) -> &[u8] {
    let start = bytes
        .iter()
        .position(|byte| *byte != 0)
        .unwrap_or(bytes.len());
    bytes.get(start..).unwrap_or_default()
}
pub(super) fn public_key_der(n: &[u8], e: &[u8]) -> Result<Vec<u8>, AuthError> {
    // graviola は 1 と偶数も受けるので、RSA の指数の条件をここで守る。
    let exponent = trim_zeroes(e)
        .iter()
        .try_fold(0u32, |value, byte| {
            value.checked_mul(256)?.checked_add(u32::from(*byte))
        })
        .ok_or(AuthError::Validation)?;
    if exponent < 3 || exponent & 1 == 0 {
        return Err(AuthError::Validation);
    }
    let mut body = Vec::new();
    for value in [n, e] {
        let value = trim_zeroes(value);
        let first = value.first().ok_or(AuthError::Validation)?;
        let pad = first & 0x80 != 0;
        body.push(2);
        length(
            value
                .len()
                .checked_add(usize::from(pad))
                .ok_or(AuthError::Validation)?,
            &mut body,
        )?;
        if pad {
            body.push(0);
        }
        body.extend_from_slice(value);
    }
    let mut result = vec![0x30];
    length(body.len(), &mut result)?;
    result.extend(body);
    Ok(result)
}
fn length(size: usize, output: &mut Vec<u8>) -> Result<(), AuthError> {
    if size < 128 {
        output.push(u8::try_from(size).map_err(|_| AuthError::Validation)?);
    } else {
        let bytes = size.to_be_bytes();
        let bytes = trim_zeroes(&bytes);
        output.push(0x80 | u8::try_from(bytes.len()).map_err(|_| AuthError::Validation)?);
        output.extend_from_slice(bytes);
    }
    Ok(())
}

#[cfg(test)]
mod tests;
