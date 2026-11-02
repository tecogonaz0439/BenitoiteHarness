//! 送る前の要求を、認証の値を除いて示す（設計書 05-01「debug prompt-input」）。

use crate::http::{HeaderMap, Uri};

/// 認証の値を除いた要求。`debug prompt-input` が示す。
#[derive(Clone, Debug, PartialEq)]
pub struct RequestPreview {
    pub url: String,
    /// 名前と値。認証の値と印を付けたヘッダは含めない。
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

/// 印がなくても認証に使う名前。アダプタが印を付け忘れても示さない。
const SECRET_HEADERS: &[&str] = &[
    "authorization",
    "x-api-key",
    "cookie",
    "proxy-authorization",
];

impl RequestPreview {
    pub(crate) fn new(uri: &Uri, headers: &HeaderMap, body: &[u8]) -> Self {
        let headers = headers
            .iter()
            .filter(|(name, value)| {
                !value.is_sensitive()
                    && !SECRET_HEADERS
                        .iter()
                        .any(|secret| name.as_str().eq_ignore_ascii_case(secret))
            })
            .filter_map(|(name, value)| {
                value
                    .to_str()
                    .ok()
                    .map(|value| (name.as_str().to_owned(), value.to_owned()))
            })
            .collect();
        Self {
            url: uri.to_string(),
            headers,
            body: body.to_vec(),
        }
    }
}
