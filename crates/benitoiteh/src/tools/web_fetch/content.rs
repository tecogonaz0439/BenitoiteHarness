//! 本文の型と文字コードを検査し、制御文字を落とす（設計書 01-03「web_fetch」）。

use super::{FetchError, html, url::Url};
use crate::http::{HeaderMap, HttpResponse};

pub(super) struct ConvertedText {
    pub text: String,
    pub bytes: usize,
}

pub(super) fn check_encoding(headers: &HeaderMap) -> Result<(), FetchError> {
    let values = headers.get_all("content-encoding");
    let mut values = values.iter();
    if let Some(value) = values.next()
        && (values.next().is_some()
            || !value
                .to_str()
                .is_ok_and(|s| s.trim().eq_ignore_ascii_case("identity")))
    {
        return Err(FetchError::Encoding);
    }
    Ok(())
}

pub(super) fn convert(
    response: &HttpResponse,
    url: &Url,
) -> Result<(String, ConvertedText), FetchError> {
    let values = response.head.headers.get_all("content-type");
    let mut values = values.iter();
    let value = values
        .next()
        .ok_or(FetchError::ContentType)?
        .to_str()
        .map_err(|_| FetchError::ContentType)?;
    if values.next().is_some() {
        return Err(FetchError::ContentType);
    }
    let mut parts = value.split(';');
    let kind = parts.next().unwrap_or_default().trim().to_ascii_lowercase();
    let kind = unquote(&kind).to_owned();
    let token = |s: &str| {
        !s.is_empty()
            && s.bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&b))
    };
    if !kind
        .split_once('/')
        .is_some_and(|(major, minor)| token(major) && token(minor))
    {
        return Err(FetchError::ContentType);
    }
    if !(kind.starts_with("text/") && kind.len() > 5)
        && kind != "application/json"
        && !kind.ends_with("+json")
        && kind != "application/xhtml+xml"
    {
        return Err(FetchError::ContentType);
    }
    let mut charset = None;
    for part in parts {
        if let Some((key, value)) = part.split_once('=')
            && key.trim().eq_ignore_ascii_case("charset")
        {
            let value = unquote(value.trim()).to_ascii_lowercase();
            if charset.as_ref().is_some_and(|old| old != &value) {
                return Err(FetchError::Charset);
            }
            charset = Some(value);
        }
    }
    let bytes = &response.body;
    if bytes.starts_with(&[0xff, 0xfe]) || bytes.starts_with(&[0xfe, 0xff]) {
        return Err(FetchError::Utf16);
    }
    let bytes = bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(bytes);
    let decoded = match charset.as_deref().unwrap_or("utf-8") {
        "utf-8" | "utf8" => String::from_utf8_lossy(bytes).into_owned(),
        "us-ascii" => bytes
            .iter()
            .map(|&b| {
                if b.is_ascii() {
                    char::from(b)
                } else {
                    '\u{fffd}'
                }
            })
            .collect(),
        "iso-8859-1" | "latin1" => bytes.iter().map(|&b| char::from(b)).collect(),
        _ => return Err(FetchError::Charset),
    };
    let mut converted = if kind == "text/html" || kind == "application/xhtml+xml" {
        html::convert(&decoded, url)
    } else {
        let text = clean(&decoded);
        ConvertedText {
            bytes: text.len(),
            text,
        }
    };
    // HTML の出力側でも数える前に除く。ここでも全形式に共通の最終検査を掛ける。
    converted.text = clean(&converted.text);
    Ok((kind, converted))
}

fn clean(input: &str) -> String {
    input
        .chars()
        .filter(|&c| c == '\n' || c == '\t' || (!c.is_control() && c != '\u{7f}'))
        .collect()
}

fn unquote(value: &str) -> &str {
    value
        .strip_prefix('"')
        .and_then(|s| s.strip_suffix('"'))
        .unwrap_or(value)
}
