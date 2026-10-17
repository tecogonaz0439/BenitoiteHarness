//! URL の正規化と相対参照の解決を一か所で行う（設計書 01-03「web_fetch」）。

use super::FetchError;
use crate::http::Uri;

#[derive(Clone, Debug)]
pub(super) struct Url {
    pub uri: Uri,
    pub domain: String,
}

pub(super) fn parse(input: &str) -> Result<Url, FetchError> {
    let input = input.split('#').next().unwrap_or_default();
    let (_, rest) = input.split_once("://").ok_or(FetchError::Url)?;
    let authority = rest.split(['/', '?']).next().unwrap_or_default();
    if authority.contains('@') || authority.starts_with('[') {
        return Err(FetchError::Url);
    }
    let host = match authority.split_once(':') {
        Some((host, "443")) => host,
        Some(_) => return Err(FetchError::Url),
        None => authority,
    };
    let domain = crate::config::domain::normalize(host).ok_or(FetchError::Url)?;
    let encoded = encode(input);
    let original: Uri = encoded.parse().map_err(|_| FetchError::Url)?;
    if original.scheme_str() != Some("https") || original.authority().is_none() {
        return Err(FetchError::Url);
    }
    let path = original.path_and_query().map_or("/", |p| p.as_str());
    let uri = format!("https://{domain}{path}")
        .parse()
        .map_err(|_| FetchError::Url)?;
    Ok(Url { uri, domain })
}

fn encode(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    for byte in input.bytes() {
        if byte >= 128 {
            output.push('%');
            output.push(char::from(
                *HEX.get(usize::from(byte >> 4)).unwrap_or(&b'0'),
            ));
            output.push(char::from(
                *HEX.get(usize::from(byte & 15)).unwrap_or(&b'0'),
            ));
        } else {
            output.push(char::from(byte));
        }
    }
    output
}

pub(super) fn resolve(base: &Url, reference: &str) -> Result<Url, FetchError> {
    parse(&reference_url(base, reference)?)
}

pub(super) fn link(base: &Url, reference: &str) -> Option<String> {
    let resolved = reference_url(base, reference).ok()?;
    let uri: Uri = resolved.parse().ok()?;
    (uri.scheme_str() == Some("https") && uri.authority().is_some()).then(|| uri.to_string())
}

fn reference_url(base: &Url, reference: &str) -> Result<String, FetchError> {
    let reference = reference.split('#').next().unwrap_or_default();
    let first = reference.split(['/', '?']).next().unwrap_or_default();
    if first.contains(':') || reference.starts_with("//") {
        let absolute = if reference.starts_with("//") {
            format!("https:{reference}")
        } else {
            reference.to_owned()
        };
        let uri: Uri = encode(&absolute).parse().map_err(|_| FetchError::Url)?;
        let scheme = uri.scheme_str().ok_or(FetchError::Url)?;
        let authority = uri.authority().ok_or(FetchError::Url)?;
        return Ok(format!(
            "{scheme}://{authority}{}{}",
            remove_dots(uri.path()),
            uri.query().map_or_else(String::new, |q| format!("?{q}"))
        ));
    }
    let (path, query) = reference
        .split_once('?')
        .map_or((reference, None), |(p, q)| (p, Some(q)));
    let merged = if path.is_empty() {
        base.uri.path().to_owned()
    } else if path.starts_with('/') {
        path.to_owned()
    } else {
        let directory = base.uri.path().rsplit_once('/').map_or("", |(d, _)| d);
        format!("{directory}/{path}")
    };
    let query = if reference.is_empty() {
        base.uri.query()
    } else {
        query
    };
    Ok(encode(&format!(
        "https://{}{}{}",
        base.domain,
        remove_dots(&merged),
        query.map_or_else(String::new, |q| format!("?{q}"))
    )))
}

fn remove_dots(path: &str) -> String {
    let mut segments = Vec::new();
    for segment in path.split('/').skip(1) {
        match segment {
            "." => {}
            ".." => {
                segments.pop();
            }
            _ => segments.push(segment),
        }
    }
    if path.ends_with("/.") || path.ends_with("/..") {
        segments.push("");
    }
    format!("/{}", segments.join("/"))
}

impl std::fmt::Display for Url {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.uri.fmt(f)
    }
}
