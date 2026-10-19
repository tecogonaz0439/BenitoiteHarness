//! 署名と claim の独立した検証、および BH-114 の DER の対応を確かめる。

use super::*;
use serde_json::{Value, json};
#[path = "../../../tests/support/auth_key.rs"]
mod auth_key;

type Error = Box<dyn std::error::Error>;
const ISSUER: &str = "https://auth.openai.com";
fn claims() -> Value {
    json!({"iss":ISSUER,"aud":"oaiapp_test","exp":1001,"nonce":"nonce","sub":"subject","email":"test@example.invalid"})
}

// 作成時の関門: DER の独立した実装との一致と、検証の各条件を個別に壊す。
// HTTP のテストはこの暗号の契約を重ねず、全体の配線を一つの署名で確かめる。
#[test]
fn rs256_jwks_and_claims_are_verified() -> Result<(), Error> {
    let (n, e) = auth_key::integers()?;
    assert_eq!(public_key_der(&n, &e)?, auth_key::der()?);
    let jwks: Jwks = serde_json::from_value(auth_key::jwks()?)?;
    let header = json!({"alg":"RS256","kid":"test-key"});
    let token = auth_key::sign(&header, &claims())?;
    assert_eq!(
        verify(&token, &jwks, ISSUER, "oaiapp_test", "nonce", 1000)?.subject,
        "subject"
    );
    for (field, value) in [
        ("iss", json!("wrong")),
        ("aud", json!("wrong")),
        ("exp", json!(1000)),
        ("nonce", json!("wrong")),
        ("sub", json!("")),
        ("aud", json!(["oaiapp_test", "other"])),
    ] {
        let mut claims = claims();
        *claims.get_mut(field).ok_or("missing claim")? = value;
        assert_eq!(
            verify(
                &auth_key::sign(&header, &claims)?,
                &jwks,
                ISSUER,
                "oaiapp_test",
                "nonce",
                1000
            ),
            Err(AuthError::Validation),
            "{field}"
        );
    }
    let mut many = claims();
    *many.get_mut("aud").ok_or("missing aud")? = json!(["other", "oaiapp_test"]);
    many.as_object_mut()
        .ok_or("claims object")?
        .insert("azp".into(), json!("oaiapp_test"));
    assert!(
        verify(
            &auth_key::sign(&header, &many)?,
            &jwks,
            ISSUER,
            "oaiapp_test",
            "nonce",
            1000
        )
        .is_ok()
    );
    for bad_header in [
        json!({"alg":"none","kid":"test-key"}),
        json!({"alg":"RS256"}),
        json!({"alg":"RS256","kid":"unknown"}),
        json!({"alg":"RS256","kid":"test-key","crit":null}),
    ] {
        assert_eq!(
            verify(
                &auth_key::sign(&bad_header, &claims())?,
                &jwks,
                ISSUER,
                "oaiapp_test",
                "nonce",
                1000
            ),
            Err(AuthError::Validation)
        );
    }
    let (message, signature) = token.rsplit_once('.').ok_or("token parts")?;
    let mut signature = base64url::decode(signature)?;
    if let Some(byte) = signature.first_mut() {
        *byte ^= 1;
    }
    let corrupt = format!("{message}.{}", base64url::encode(&signature));
    assert_eq!(
        verify(&corrupt, &jwks, ISSUER, "oaiapp_test", "nonce", 1000),
        Err(AuthError::Validation)
    );
    let mut signature = base64url::decode(token.rsplit_once('.').ok_or("token")?.1)?;
    signature.insert(0, 0);
    assert_eq!(
        verify(
            &format!("{message}.{}", base64url::encode(&signature)),
            &jwks,
            ISSUER,
            "oaiapp_test",
            "nonce",
            1000
        ),
        Err(AuthError::Validation)
    );
    for bad in [
        format!("{token}.extra"),
        format!("{token}="),
        "x".repeat(16385),
    ] {
        assert!(verify(&bad, &jwks, ISSUER, "oaiapp_test", "nonce", 1000).is_err());
    }
    for (field, value) in [("kty", "EC"), ("use", "enc"), ("alg", "RS512")] {
        let mut value_json = auth_key::jwks()?;
        let key = value_json
            .get_mut("keys")
            .and_then(Value::as_array_mut)
            .and_then(|v| v.first_mut())
            .and_then(Value::as_object_mut)
            .ok_or("key")?;
        key.insert(field.into(), json!(value));
        let bad: Jwks = serde_json::from_value(value_json)?;
        assert!(verify(&token, &bad, ISSUER, "oaiapp_test", "nonce", 1000).is_err());
    }
    Ok(())
}

#[test]
fn security_rsa_rejects_exponent_one_forgery_and_non_odd_exponents() -> Result<(), Error> {
    use sha2::{Digest, Sha256};
    let head = auth_key::encode(&serde_json::to_vec(
        &json!({"alg":"RS256","kid":"test-key"}),
    )?);
    let payload = auth_key::encode(&serde_json::to_vec(&claims())?);
    let message = format!("{head}.{payload}");
    // e=1 なら秘密鍵なしで、PKCS#1 の符号化そのものを署名として偽造できる。
    let mut encoded = vec![0, 1];
    encoded.extend(std::iter::repeat_n(0xff, 202));
    encoded.extend([
        0, 0x30, 0x31, 0x30, 0x0d, 0x06, 0x09, 0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x02,
        0x01, 0x05, 0x00, 0x04, 0x20,
    ]);
    encoded.extend(Sha256::digest(message.as_bytes()));
    assert_eq!(encoded.len(), 256);
    let (n, _) = auth_key::integers()?;
    let jwks: Jwks = serde_json::from_value(
        json!({"keys":[{"kid":"test-key","kty":"RSA","n":auth_key::encode(&n),"e":"AQ"}]}),
    )?;
    assert_eq!(
        verify(
            &format!("{message}.{}", auth_key::encode(&encoded)),
            &jwks,
            ISSUER,
            "oaiapp_test",
            "nonce",
            1000
        ),
        Err(AuthError::Validation)
    );
    for exponent in [
        vec![0],
        vec![1],
        vec![2],
        vec![4],
        vec![0, 0, 2],
        vec![1, 0, 0, 0, 1],
    ] {
        assert!(public_key_der(&n, &exponent).is_err(), "{exponent:?}");
    }
    for exponent in [vec![3], vec![0, 3], vec![1, 0, 1], vec![255, 255, 255, 255]] {
        assert!(public_key_der(&n, &exponent).is_ok());
    }
    Ok(())
}
