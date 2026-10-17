//! 承認後に名前を解き、検査したアドレスからだけ文字を取得する（設計書 01-03「web_fetch」）。

mod address;
mod content;
mod html;
#[cfg(test)]
mod tests;
mod url;

use std::collections::BTreeSet;
use std::fmt;
use std::io;
use std::net::{IpAddr, SocketAddr, ToSocketAddrs};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::{Duration, Instant};

use super::{
    ApprovalRequest, ApprovalStatus, Invocation, ToolContext, ToolError, ToolOutput, text,
};
use crate::approval::{ApprovalFacts, ApprovalKind};
use crate::http::{HeaderMap, HeaderValue, HttpClient, HttpError, HttpResponse, HttpTimeouts};
use serde_json::Value;

const BODY_LIMIT: u64 = 2 * 1024 * 1024;
const TOTAL: Duration = Duration::from_secs(30);

type Resolver = Arc<dyn Fn(&str) -> io::Result<Vec<SocketAddr>> + Send + Sync>;

struct Options<'a> {
    client: &'a HttpClient,
    resolve: Resolver,
    allowed_address: fn(IpAddr) -> bool,
    total: Duration,
}

#[derive(Debug)]
enum FetchError {
    Url,
    Address,
    Dns,
    Timeout,
    Cancelled,
    Worker,
    Redirect,
    RedirectLimit,
    Encoding,
    ContentType,
    Charset,
    Utf16,
    Http(HttpError),
}

impl fmt::Display for FetchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Url => text::WEB_URL,
            Self::Address => text::WEB_ADDRESS,
            Self::Dns => text::WEB_DNS,
            Self::Timeout => text::WEB_TIMEOUT,
            Self::Cancelled => text::WEB_CANCELLED,
            Self::Worker => text::WEB_WORKER,
            Self::Redirect => text::WEB_REDIRECT,
            Self::RedirectLimit => text::WEB_REDIRECT_LIMIT,
            Self::Encoding => text::WEB_ENCODING,
            Self::ContentType => text::WEB_CONTENT_TYPE,
            Self::Charset => text::WEB_CHARSET,
            Self::Utf16 => text::WEB_UTF16,
            Self::Http(error) => return f.write_str(&text::web_http(error)),
        })
    }
}

pub(super) fn execute(
    context: &ToolContext,
    arguments: &Value,
    approval: ApprovalStatus,
    invocation: Invocation<'_>,
) -> Result<ToolOutput, ToolError> {
    let options = Options {
        client: &context.web_fetch.client,
        resolve: Arc::new(|domain| (domain, 443).to_socket_addrs().map(Iterator::collect)),
        allowed_address: address::allowed,
        total: TOTAL,
    };
    Ok(execute_with(
        context, arguments, approval, invocation, options,
    ))
}

fn execute_with(
    context: &ToolContext,
    arguments: &Value,
    approval: ApprovalStatus,
    invocation: Invocation<'_>,
    options: Options<'_>,
) -> ToolOutput {
    let result = (|| {
        let original = url::parse(
            arguments
                .get("url")
                .and_then(Value::as_str)
                .unwrap_or_default(),
        )?;
        if invocation.cancelled.load(Ordering::Acquire) {
            return Err(FetchError::Cancelled);
        }
        if approval == ApprovalStatus::Unapproved
            && !context.web_fetch.allowed_domains.contains(&original.domain)
            && !invocation.session_domains.contains(&original.domain)
        {
            // DNS の問い合わせにも URL の内容を載せられるので、名前を解く前に戻る。
            return Ok(ToolOutput::needs_approval(ApprovalRequest {
                kind: ApprovalKind::WebFetch,
                summary: text::web_approval(&original.to_string(), &original.domain),
                facts: ApprovalFacts {
                    url: Some(original.to_string()),
                    domain: Some(original.domain),
                    ..ApprovalFacts::default()
                },
                justification: None,
            }));
        }
        fetch(
            original,
            &context.web_fetch.allowed_domains,
            &invocation,
            options,
        )
    })();
    result.unwrap_or_else(|error| ToolOutput::error(error.to_string()))
}

fn fetch(
    original: url::Url,
    configured: &BTreeSet<String>,
    invocation: &Invocation<'_>,
    options: Options<'_>,
) -> Result<ToolOutput, FetchError> {
    let started = Instant::now();
    let mut current = original.clone();
    let mut headers = HeaderMap::new();
    headers.insert(
        "user-agent",
        HeaderValue::from_static(concat!("benitoiteh/", env!("CARGO_PKG_VERSION"))),
    );
    headers.insert(
        "accept",
        HeaderValue::from_static("text/html, text/*;q=0.9, application/json;q=0.8"),
    );
    for redirects in 0..=5 {
        // サーチドメインを補われない絶対名を解く。Host と TLS は正規の URI のまま。
        let domain = format!("{}.", current.domain);
        let resolver = Arc::clone(&options.resolve);
        let addresses = wait(started, options.total, invocation.cancelled, move || {
            resolver(&domain).map_err(|_| FetchError::Dns)
        })?;
        if addresses.is_empty() {
            return Err(FetchError::Dns);
        }
        if addresses
            .iter()
            .any(|address| !(options.allowed_address)(address.ip()))
        {
            return Err(FetchError::Address);
        }
        let remaining = remaining(started, options.total)?;
        let client = options.client.with_timeouts(HttpTimeouts {
            connect: remaining,
            headers: remaining,
            idle: remaining,
            total: remaining,
        });
        let uri = current.uri.clone();
        let request_headers = headers.clone();
        let response = wait(started, options.total, invocation.cancelled, move || {
            Ok(client.get_resolved(&uri, &request_headers, &addresses, BODY_LIMIT))
        })?;
        let (response, status_error) = match response {
            Ok(response) => (response, false),
            Err(HttpError::Status(error)) => (
                HttpResponse {
                    head: error.head,
                    body: error.body,
                },
                true,
            ),
            Err(error) => return Err(FetchError::Http(error)),
        };
        if matches!(response.head.status, 301 | 302 | 303 | 307 | 308) {
            let locations = response.head.headers.get_all("location");
            let mut locations = locations.iter();
            let location = locations.next().ok_or(FetchError::Redirect)?;
            if locations.next().is_some() {
                return Err(FetchError::Redirect);
            }
            let location = location.to_str().map_err(|_| FetchError::Redirect)?;
            let next = url::resolve(&current, location)?;
            if !configured.contains(&next.domain)
                && !invocation.session_domains.contains(&next.domain)
                && next.domain != original.domain
            {
                return Ok(ToolOutput::success(text::web_redirect_approval(
                    &next.to_string(),
                )));
            }
            if redirects == 5 {
                return Err(FetchError::RedirectLimit);
            }
            current = next;
            continue;
        }
        content::check_encoding(&response.head.headers)?;
        if status_error {
            let body = if response.body.is_empty() {
                String::new()
            } else {
                content::convert(&response, &current)
                    .map(|(_, body)| body.text)
                    .unwrap_or_else(|error| error.to_string())
            };
            return Ok(ToolOutput::error(text::web_status(
                response.head.status,
                super::truncate::prefix(&body, 2048),
            )));
        }
        let (kind, body) = content::convert(&response, &current)?;
        return Ok(ToolOutput::success(text::web_result(
            &current.to_string(),
            &kind,
            &body.text,
            body.bytes,
        )));
    }
    Err(FetchError::RedirectLimit)
}

fn remaining(started: Instant, total: Duration) -> Result<Duration, FetchError> {
    let remaining = total.saturating_sub(started.elapsed());
    if remaining.is_zero() {
        Err(FetchError::Timeout)
    } else {
        Ok(remaining)
    }
}

fn wait<T: Send + 'static>(
    started: Instant,
    total: Duration,
    cancelled: &AtomicBool,
    work: impl FnOnce() -> Result<T, FetchError> + Send + 'static,
) -> Result<T, FetchError> {
    if cancelled.load(Ordering::Acquire) {
        return Err(FetchError::Cancelled);
    }
    remaining(started, total)?;
    let (sender, receiver) = mpsc::sync_channel(1);
    std::thread::Builder::new()
        .spawn(move || {
            let _sent = sender.send(work());
        })
        .map_err(|_| FetchError::Worker)?;
    loop {
        if cancelled.load(Ordering::Acquire) {
            return Err(FetchError::Cancelled);
        }
        let timeout = remaining(started, total)?.min(Duration::from_millis(100));
        match receiver.recv_timeout(timeout) {
            Ok(result) => {
                if cancelled.load(Ordering::Acquire) {
                    return Err(FetchError::Cancelled);
                }
                remaining(started, total)?;
                return result;
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return Err(FetchError::Worker),
        }
    }
}
