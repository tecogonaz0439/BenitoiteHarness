//! HTTP と TLS、応答の上限とストリームの打ち切りを扱う（設計書 02-01「HTTP の通信」、01-03「web_fetch」）。

mod stream;
mod text;
mod transport;

use std::fmt;
use std::io::{self, Read};
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use ureq::config::Config;
use ureq::http::{Request, header};
use ureq::unversioned::resolver::{DefaultResolver, ResolvedSocketAddrs, Resolver};
use ureq::unversioned::transport::{Connector, NextTimeout, RustlsConnector};

pub use stream::{Cancellation, HttpStream, StreamEvent};
pub use ureq::http::{HeaderMap, HeaderValue, Uri};

const ERROR_BODY_LIMIT: u64 = 64 * 1024;

/// 要求の段階ごとの上限。`idle` は次の本文のデータまでの時間である。
#[derive(Clone, Copy, Debug)]
pub struct HttpTimeouts {
    /// 接続と名前の解決の上限。
    pub connect: Duration,
    /// 応答のヘッダを受け取るまでの上限。
    pub headers: Duration,
    /// 本文のデータが届かない時間の上限。
    pub idle: Duration,
    /// 要求全体の上限。
    pub total: Duration,
}

impl Default for HttpTimeouts {
    fn default() -> Self {
        Self {
            connect: Duration::from_secs(10),
            headers: Duration::from_secs(120),
            idle: Duration::from_secs(300),
            total: Duration::from_secs(30 * 60),
        }
    }
}

/// 上限に達した段階。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TimeoutKind {
    /// 接続または名前の解決。
    Connect,
    /// 応答のヘッダ。
    Headers,
    /// 本文のデータが届かない時間。
    Idle,
    /// 要求の全体。
    Total,
}

/// HTTP の応答の状態とヘッダ。
#[derive(Clone)]
pub struct ResponseHead {
    /// HTTP の状態の値。
    pub status: u16,
    /// 応答のヘッダ。リダイレクト先の判定も呼び出す側で行う。
    pub headers: HeaderMap,
}

impl fmt::Debug for ResponseHead {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ResponseHead")
            .field("status", &self.status)
            .finish_non_exhaustive()
    }
}

/// 大きさを制限して読んだ HTTP の応答。
#[derive(Clone)]
pub struct HttpResponse {
    /// 状態と応答のヘッダ。
    pub head: ResponseHead,
    /// 本文のバイト列。
    pub body: Vec<u8>,
}

impl fmt::Debug for HttpResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HttpResponse")
            .field("head", &self.head)
            .field("body_len", &self.body.len())
            .finish()
    }
}

/// 2xx 以外の応答。本文は先頭 64 KiB に限り、診断出力には含めない。
#[derive(Clone)]
pub struct HttpStatusError {
    /// 状態と応答のヘッダ。3xx の Location も保つ。
    pub head: ResponseHead,
    /// Retry-After の未解釈の値。
    pub retry_after: Option<HeaderValue>,
    /// 提供者の誤りの分類に用いる本文の先頭。
    pub body: Vec<u8>,
}

impl fmt::Debug for HttpStatusError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HttpStatusError")
            .field("head", &self.head)
            .field("body_len", &self.body.len())
            .finish_non_exhaustive()
    }
}

/// HTTP の失敗。要求の URL、ヘッダの値、本文を診断の文に含めない。
#[derive(Clone, Debug)]
pub enum HttpError {
    /// 名前の解決、接続、通信または HTTP の形式の失敗。
    Connection,
    /// 段階ごとの時間の上限。
    Timeout(TimeoutKind),
    /// TLS の交渉または証明書の検証の失敗。
    Tls,
    /// 呼び出す側による打ち切り。
    Cancelled,
    /// 指定された本文の上限を超えた。
    BodyLimit {
        /// 本文のバイト数の上限。
        limit: u64,
    },
    /// 2xx 以外の HTTP の状態。
    Status(Box<HttpStatusError>),
    /// URL またはヘッダの指定が不正である。
    InvalidRequest,
    /// 読み取りのスレッドを作れない、または正常に終わらなかった。
    Worker,
}

impl fmt::Display for HttpError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::Connection => text::CONNECTION,
            Self::Timeout(_) => text::TIMEOUT,
            Self::Tls => text::TLS,
            Self::Cancelled => text::CANCELLED,
            Self::BodyLimit { .. } => text::BODY_LIMIT,
            Self::Status(_) => text::STATUS,
            Self::InvalidRequest => text::INVALID_REQUEST,
            Self::Worker => text::WORKER,
        };
        f.write_str(message)
    }
}

impl std::error::Error for HttpError {}

/// 通常の GET の接続を使い回すクライアント。複製した値も同じプールを共有する。
#[derive(Clone)]
pub struct HttpClient {
    agent: ureq::Agent,
    timeouts: HttpTimeouts,
    tls: ureq::tls::TlsConfig,
}

impl fmt::Debug for HttpClient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HttpClient")
            .field("timeouts", &self.timeouts)
            .finish_non_exhaustive()
    }
}

impl HttpClient {
    /// OS の証明書の検証と graviola を用いるクライアントを作る。
    pub fn new(timeouts: HttpTimeouts) -> Self {
        let tls = ureq::tls::TlsConfig::builder()
            .unversioned_rustls_crypto_provider(Arc::new(rustls_graviola::default_provider()))
            .root_certs(ureq::tls::RootCerts::PlatformVerifier)
            .build();
        Self::with_tls(timeouts, tls)
    }

    /// TLS の設定を保ち、時間の上限だけを変えたクライアントを作る。
    pub fn with_timeouts(&self, timeouts: HttpTimeouts) -> Self {
        Self::with_tls(timeouts, self.tls.clone())
    }

    pub(crate) fn with_tls(timeouts: HttpTimeouts, tls: ureq::tls::TlsConfig) -> Self {
        let agent = make_agent(timeouts, tls.clone(), None, None);
        Self {
            agent,
            timeouts,
            tls,
        }
    }

    /// リダイレクトを辿らず GET を行い、本文の上限を超えたら読み取りを止める。
    ///
    /// `uri` は呼び出す側が一度解析した値を渡す。Accept-Encoding は送らない。
    pub fn get(
        &self,
        uri: &Uri,
        headers: &HeaderMap,
        limit: u64,
    ) -> Result<HttpResponse, HttpError> {
        self.get_with_agent(&self.agent, uri, headers, limit, false)
    }

    /// 判定済みのアドレスだけに接続する GET。空の一覧は接続の失敗になる。
    ///
    /// 名前を解き直さず、Host と TLS の名前には `uri` の元のホストを使う。
    /// この要求は既存の接続プールを使わない。アドレスの安全の判定は呼び出す側の責任である。
    pub fn get_resolved(
        &self,
        uri: &Uri,
        headers: &HeaderMap,
        addresses: &[SocketAddr],
        limit: u64,
    ) -> Result<HttpResponse, HttpError> {
        let agent = make_agent(
            self.timeouts,
            self.tls.clone(),
            Some(addresses.to_vec()),
            None,
        );
        self.get_with_agent(&agent, uri, headers, limit, true)
    }

    fn get_with_agent(
        &self,
        agent: &ureq::Agent,
        uri: &Uri,
        headers: &HeaderMap,
        limit: u64,
        bound_status: bool,
    ) -> Result<HttpResponse, HttpError> {
        let request = request(uri, headers, None, Vec::new())?;
        let response = agent.run(request).map_err(classify)?;
        let (parts, body) = response.into_parts();
        let head = ResponseHead {
            status: parts.status.as_u16(),
            headers: parts.headers,
        };
        let mut reader = body.into_reader();
        if !(200..300).contains(&head.status) && !bound_status {
            return Err(status_error(head, &mut reader));
        }
        let mut body = Vec::new();
        reader
            .take(limit.saturating_add(1))
            .read_to_end(&mut body)
            .map_err(|e| classify(e.into()))?;
        if u64::try_from(body.len()).map_err(|_| HttpError::BodyLimit { limit })? > limit {
            return Err(HttpError::BodyLimit { limit });
        }
        if !(200..300).contains(&head.status) {
            // 本文全体の上限を検査した後も、既存の診断用の接頭辞の契約は保つ。
            body.truncate(usize::try_from(ERROR_BODY_LIMIT).unwrap_or(65536));
            return Err(HttpError::Status(Box::new(HttpStatusError {
                retry_after: head.headers.get(header::RETRY_AFTER).cloned(),
                head,
                body,
            })));
        }
        Ok(HttpResponse { head, body })
    }

    /// フォームを新しい接続で一度だけ POST し、上限付きで応答を読む。
    pub fn post_form(
        &self,
        uri: &Uri,
        headers: &HeaderMap,
        body: &[u8],
        limit: u64,
    ) -> Result<HttpResponse, HttpError> {
        let agent = make_agent(self.timeouts, self.tls.clone(), None, None);
        let request = request(
            uri,
            headers,
            Some("application/x-www-form-urlencoded"),
            body.to_vec(),
        )?;
        let response = agent.run(request).map_err(classify)?;
        let (parts, body) = response.into_parts();
        let head = ResponseHead {
            status: parts.status.as_u16(),
            headers: parts.headers,
        };
        let mut bytes = Vec::new();
        body.into_reader()
            .take(limit.saturating_add(1))
            .read_to_end(&mut bytes)
            .map_err(|e| classify(e.into()))?;
        if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > limit {
            return Err(HttpError::BodyLimit { limit });
        }
        if !(200..300).contains(&head.status) {
            bytes.truncate(usize::try_from(ERROR_BODY_LIMIT).unwrap_or(65536));
            return Err(HttpError::Status(Box::new(HttpStatusError {
                retry_after: head.headers.get(header::RETRY_AFTER).cloned(),
                head,
                body: bytes,
            })));
        }
        Ok(HttpResponse { head, body: bytes })
    }

    /// JSON のバイト列を POST し、ヘッダと本文を別のスレッドから順に受け取る。
    ///
    /// JSON の構築と検査は提供者の層で行う。戻り値はヘッダの到着を待たないので、
    /// 呼び出す側はヘッダ待ちの間も取り消しを要求できる。
    pub fn post_json(
        &self,
        uri: &Uri,
        headers: &HeaderMap,
        json: &[u8],
    ) -> Result<HttpStream, HttpError> {
        let request = request(uri, headers, Some("application/json"), json.to_vec())?;
        stream::start(self, request)
    }
}

impl Default for HttpClient {
    fn default() -> Self {
        Self::new(HttpTimeouts::default())
    }
}

fn request(
    uri: &Uri,
    headers: &HeaderMap,
    content_type: Option<&'static str>,
    body: Vec<u8>,
) -> Result<Request<Vec<u8>>, HttpError> {
    if !matches!(uri.scheme_str(), Some("http" | "https"))
        || uri.authority().is_none_or(|a| a.as_str().contains('@'))
        || headers.contains_key(header::HOST)
    {
        return Err(HttpError::InvalidRequest);
    }
    let mut request = Request::builder()
        .method(if content_type.is_some() {
            "POST"
        } else {
            "GET"
        })
        .uri(uri.clone())
        .body(body)
        .map_err(|_| HttpError::InvalidRequest)?;
    *request.headers_mut() = headers.clone();
    if let Some(content_type) = content_type {
        request
            .headers_mut()
            .insert(header::CONTENT_TYPE, HeaderValue::from_static(content_type));
    } else {
        request.headers_mut().remove(header::ACCEPT_ENCODING);
    }
    Ok(request)
}

fn make_agent(
    timeouts: HttpTimeouts,
    tls: ureq::tls::TlsConfig,
    addresses: Option<Vec<SocketAddr>>,
    control: Option<Arc<stream::Control>>,
) -> ureq::Agent {
    let streaming = control.is_some();
    let mut config = ureq::Agent::config_builder()
        .proxy(None)
        .max_redirects(0)
        .http_status_as_error(false)
        .tls_config(tls)
        .timeout_resolve(Some(timeouts.connect))
        .timeout_connect(Some(timeouts.connect))
        .timeout_recv_response(Some(timeouts.headers))
        .timeout_global(Some(timeouts.total));
    if streaming {
        config = config.max_idle_connections(0);
    }
    let connector = transport::TcpConnector {
        control,
        idle: (!streaming).then_some(timeouts.idle),
    }
    .chain(RustlsConnector::default());
    ureq::Agent::with_parts(config.build(), connector, AddressResolver(addresses))
}

#[derive(Debug)]
struct AddressResolver(Option<Vec<SocketAddr>>);

impl Resolver for AddressResolver {
    fn resolve(
        &self,
        uri: &Uri,
        config: &Config,
        timeout: NextTimeout,
    ) -> Result<ResolvedSocketAddrs, ureq::Error> {
        let Some(addresses) = &self.0 else {
            return DefaultResolver::default().resolve(uri, config, timeout);
        };
        if addresses.is_empty() {
            return Err(ureq::Error::HostNotFound);
        }
        let mut result = self.empty();
        // ureq 3.4.2 の ArrayVec は 16 件で満杯になり、push が panic するので切る。
        for address in addresses.iter().take(16) {
            result.push(*address);
        }
        Ok(result)
    }
}

fn status_error(head: ResponseHead, reader: &mut impl Read) -> HttpError {
    let retry_after = head.headers.get(header::RETRY_AFTER).cloned();
    let mut body = Vec::new();
    // 誤りの応答の読み取りが途中で失敗しても、得た接頭辞と状態を保つ（実装プラン P0-03「作るもの」）。
    if reader
        .take(ERROR_BODY_LIMIT)
        .read_to_end(&mut body)
        .is_err()
    { /* 接頭辞を保つ。 */ }
    HttpError::Status(Box::new(HttpStatusError {
        head,
        retry_after,
        body,
    }))
}

fn classify(error: ureq::Error) -> HttpError {
    match error {
        ureq::Error::Timeout(reason) => HttpError::Timeout(match reason {
            ureq::Timeout::Global | ureq::Timeout::PerCall => TimeoutKind::Total,
            ureq::Timeout::Resolve | ureq::Timeout::Connect => TimeoutKind::Connect,
            ureq::Timeout::RecvResponse => TimeoutKind::Headers,
            ureq::Timeout::RecvBody => TimeoutKind::Idle,
            ureq::Timeout::SendRequest | ureq::Timeout::Await100 | ureq::Timeout::SendBody => {
                TimeoutKind::Total
            }
            _ => TimeoutKind::Total,
        }),
        ureq::Error::Tls(_) | ureq::Error::Rustls(_) => HttpError::Tls,
        ureq::Error::Io(error) => {
            if error
                .get_ref()
                .is_some_and(|inner| inner.is::<rustls::Error>())
            {
                HttpError::Tls
            } else if matches!(
                error.kind(),
                io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock
            ) {
                HttpError::Timeout(TimeoutKind::Total)
            } else {
                HttpError::Connection
            }
        }
        ureq::Error::StatusCode(_)
        | ureq::Error::Http(_)
        | ureq::Error::BadUri(_)
        | ureq::Error::Protocol(_)
        | ureq::Error::HostNotFound
        | ureq::Error::RedirectFailed
        | ureq::Error::InvalidProxyUrl
        | ureq::Error::ConnectionFailed
        | ureq::Error::BodyExceedsLimit(_)
        | ureq::Error::TooManyRedirects
        | ureq::Error::Pem(_)
        | ureq::Error::RequireHttpsOnly(_)
        | ureq::Error::LargeResponseHeader(_, _)
        | ureq::Error::ConnectProxyFailed(_)
        | ureq::Error::TlsRequired
        | ureq::Error::Other(_)
        | ureq::Error::BodyStalled => HttpError::Connection,
        // 外部の非網羅 enum の追加にも、秘密を含みうる元の診断は転記しない。
        _ => HttpError::Connection,
    }
}

#[cfg(test)]
mod tests;
