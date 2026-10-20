//! Responses API の要求とストリームを共通の形に変える（設計書 02-01「OpenAI の Responses API」）。

mod assembly;
mod authentication;
mod request;
mod text;

use std::fmt;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use serde_json::Value;

use crate::auth::Secret;
use crate::http::{HttpClient, HttpError, HttpStream};
use authentication::authorize;
pub use authentication::{ChatGptTokens, ResponsesCancellation};

use super::sse::{SseDecoder, SseError};
use super::{Capabilities, Destination, NonRetryableKind, ProviderError, StreamEvent};

pub use assembly::OpenAiResponsesAssembler;
pub use request::{OpenAiResponsesRequest, RequestOptions};

/// API が返した誤りの分類と文。診断や記録には出さない。
#[derive(Clone, PartialEq, Eq)]
pub struct ApiErrorDetails {
    /// API の error.type。
    pub error_type: Option<String>,
    /// API の error.code。
    pub code: Option<String>,
    /// API の error.message。受け取ったキーと一致する部分は伏せる。
    pub message: Option<String>,
}

impl fmt::Debug for ApiErrorDetails {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ApiErrorDetails").finish_non_exhaustive()
    }
}

/// 共通の分類に加え、要求の誤りの詳細と Retry-After の元の値を保つ。
#[derive(Clone)]
pub enum OpenAiResponsesError {
    /// 提供者または通信の失敗。
    Provider {
        /// ループがやり直しを判定する分類。
        error: ProviderError,
        /// HTTP の状態。ストリーム内の誤りにはない。
        status: Option<u16>,
        /// API の誤り。表示と記録には使わない。
        details: Option<ApiErrorDetails>,
        /// HTTP の Retry-After の未解釈の値。
        retry_after: Option<String>,
    },
    /// URL、認証ヘッダ、会話の指定が不正。
    InvalidRequest,
    /// 既知の SSE イベントの形や順序が不正。
    InvalidResponse,
    /// SSE のバイト列を読み取れない。
    Sse(SseError),
    /// 利用者による取り消し。
    Cancelled,
}

impl OpenAiResponsesError {
    /// 既存のループの誤りの型に変える。取り消しは呼び出す側で先に扱う。
    pub fn classification(&self) -> ProviderError {
        match self {
            Self::Provider { error, .. } => error.clone(),
            Self::InvalidRequest | Self::Cancelled => ProviderError::NonRetryable {
                reason: NonRetryableKind::InvalidRequest,
            },
            Self::InvalidResponse | Self::Sse(_) => ProviderError::StreamDisconnected,
        }
    }

    fn redact(mut self, key: &str) -> Self {
        if !key.is_empty()
            && let Self::Provider {
                details: Some(details),
                ..
            } = &mut self
        {
            details.error_type = details
                .error_type
                .take()
                .map(|s| s.replace(key, "[redacted]"));
            details.message = details.message.take().map(|s| s.replace(key, "[redacted]"));
            details.code = details.code.take().map(|s| s.replace(key, "[redacted]"));
        }
        if !key.is_empty()
            && let Self::Provider {
                retry_after: Some(value),
                ..
            } = &mut self
        {
            *value = value.replace(key, "[redacted]");
        }
        self
    }
    // 提供者の任意の文は、トークンの一部を含みうるため渡さない
    // （設計書 02-01「誤り」）。分類はこの絞り込みより先に済ませる。
    fn chatgpt_details(mut self) -> Self {
        fn safe_code(value: &str) -> bool {
            (1..=64).contains(&value.len())
                && value.bytes().all(|b| {
                    b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'_' | b'.')
                })
        }
        if let Self::Provider {
            details,
            retry_after,
            ..
        } = &mut self
        {
            if let Some(details) = details {
                details.message = None;
                details.code = details.code.take().filter(|value| safe_code(value));
                details.error_type = details.error_type.take().filter(|value| safe_code(value));
            }
            *retry_after = retry_after
                .take()
                .filter(|value| !value.is_empty() && value.bytes().all(|b| b.is_ascii_digit()));
        }
        self
    }
}

impl fmt::Debug for OpenAiResponsesError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OpenAiResponsesError")
            .field("classification", &self.classification())
            .finish_non_exhaustive()
    }
}

impl fmt::Display for OpenAiResponsesError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Provider { .. } => text::PROVIDER,
            Self::InvalidRequest => text::INVALID_REQUEST,
            Self::InvalidResponse | Self::Sse(_) => text::INVALID_RESPONSE,
            Self::Cancelled => text::CANCELLED,
        })
    }
}

impl std::error::Error for OpenAiResponsesError {}

/// API キーの解決は呼び出す側が行う。アダプタはキーを診断に出さない。
pub struct OpenAiResponsesAdapter {
    destination: Destination,
    capabilities: Capabilities,
    authentication: Authentication,
}

enum Authentication {
    ApiKey(String),
    Chatgpt(ChatGptTokens),
}

impl fmt::Debug for OpenAiResponsesAdapter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OpenAiResponsesAdapter")
            .finish_non_exhaustive()
    }
}

impl OpenAiResponsesAdapter {
    /// 送り先と機能と、解決済みの API キーを受け取る。空のキーなら認証を送らない。
    pub fn new(destination: Destination, capabilities: Capabilities, api_key: String) -> Self {
        Self {
            destination,
            capabilities,
            authentication: Authentication::ApiKey(api_key),
        }
    }

    /// API キーに代えて、保存した ChatGPT の許可を使う。
    pub fn with_chatgpt_tokens(mut self, source: ChatGptTokens) -> Self {
        self.authentication = Authentication::Chatgpt(source);
        self
    }

    /// 構築した要求を HTTP の経路へ渡す。受け取りの取り消しは戻り値から行える。
    pub fn stream(
        &self,
        client: &HttpClient,
        request: &OpenAiResponsesRequest,
    ) -> Result<OpenAiResponsesStream, OpenAiResponsesError> {
        let cancelled = Arc::new(AtomicBool::new(false));
        let (http, token, resend) = match &self.authentication {
            Authentication::ApiKey(key) => {
                let http = client
                    .post_json(&request.uri, &request.headers, &request.body)
                    .map_err(|e| classify_http(e).redact(key))?;
                (Some(http), Some(Secret::new(key.clone())), None)
            }
            Authentication::Chatgpt(source) => {
                let mut request = request.clone();
                request.headers.remove("authorization");
                let worker_source = source.clone();
                let worker_cancelled = Arc::clone(&cancelled);
                // 錠や通信を待つ処理を、エージェントの取り消しの監視から分ける。
                let refresh = std::thread::Builder::new()
                    .spawn(move || worker_source.access_token(&worker_cancelled))
                    .map_err(|_| {
                        provider_error(ProviderError::Retryable {
                            retry_after_secs: None,
                        })
                    })?;
                (
                    None,
                    None,
                    Some(Box::new(Resend {
                        source: source.clone(),
                        cancelled: Arc::clone(&cancelled),
                        client: client.clone(),
                        request,
                        refreshed: false,
                        refresh: Some(refresh),
                    })),
                )
            }
        };
        let cancellation =
            ResponsesCancellation::new(http.as_ref().map(HttpStream::cancellation), cancelled);
        Ok(OpenAiResponsesStream {
            http,
            cancellation,
            decoder: SseDecoder::new(),
            assembler: Some(OpenAiResponsesAssembler::new(self.destination.clone())),
            token,
            rejected_token: None,
            resend,
            delivered_events: false,
            ended: false,
            received_headers: false,
            failed: None,
        })
    }
}

/// HTTP と SSE の読み取りを結び、応答全体の確認まで部分的な結果を保持する。
pub struct OpenAiResponsesStream {
    http: Option<HttpStream>,
    decoder: SseDecoder,
    assembler: Option<OpenAiResponsesAssembler>,
    token: Option<Secret>,
    rejected_token: Option<Secret>,
    cancellation: ResponsesCancellation,
    resend: Option<Box<Resend>>,
    delivered_events: bool,
    ended: bool,
    received_headers: bool,
    failed: Option<OpenAiResponsesError>,
}

struct Resend {
    source: ChatGptTokens,
    cancelled: Arc<AtomicBool>,
    client: HttpClient,
    request: OpenAiResponsesRequest,
    refreshed: bool,
    refresh: Option<std::thread::JoinHandle<Result<Secret, OpenAiResponsesError>>>,
}

impl Drop for Resend {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Release);
        // 更新でローテーションしたトークンは保存まで終える必要がある。
        // 呼び出し側が取り消してストリームを捨てても、更新だけは回収する
        // （設計書 02-01「Sign in with ChatGPT」）。
        if let Some(worker) = self.refresh.take() {
            let _result = worker.join();
        }
    }
}

impl fmt::Debug for OpenAiResponsesStream {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OpenAiResponsesStream")
            .field("ended", &self.ended)
            .finish_non_exhaustive()
    }
}

impl OpenAiResponsesStream {
    /// 取り消した要求の読み取りのスレッドが終了したかを返す。
    pub fn is_finished(&self) -> bool {
        self.http.as_ref().is_none_or(HttpStream::is_finished)
            && self.resend.as_ref().is_none_or(|resend| {
                resend
                    .refresh
                    .as_ref()
                    .is_none_or(std::thread::JoinHandle::is_finished)
            })
    }

    /// 読み取りのスレッドの終了を待つ。途中では先に取り消す。
    pub fn join(&mut self) -> Result<(), HttpError> {
        if let Some(http) = &mut self.http {
            http.join()?;
        }
        if let Some(resend) = &mut self.resend
            && let Some(worker) = resend.refresh.take()
        {
            // 更新の結果の分類は poll が担当し、回収ではスレッドの失敗だけを返す。
            let _result = worker.join().map_err(|_| HttpError::Worker)?;
        }
        Ok(())
    }

    /// 別のスレッドからも要求を取り消せるハンドルを返す。
    pub fn cancellation(&self) -> ResponsesCancellation {
        self.cancellation.clone()
    }

    /// 最大 wait 待って、今届いた共通のイベントを返す。空なら受け取りを続ける。
    /// Finished が届いた後に finish で応答を取り出す。誤りの後は同じ誤りを返す。
    pub fn poll(&mut self, wait: Duration) -> Result<Vec<StreamEvent>, OpenAiResponsesError> {
        if let Some(error) = &self.failed {
            return Err(error.clone());
        }
        if self.ended {
            return Ok(Vec::new());
        }
        let result = self.poll_inner(wait);
        match result {
            Ok(events) => {
                self.delivered_events |= !events.is_empty();
                Ok(events)
            }
            Err(error) => {
                if let Some(http) = &self.http {
                    http.cancel();
                }
                let current = self
                    .token
                    .as_ref()
                    .map(Secret::expose_secret)
                    .unwrap_or_default();
                let rejected = self
                    .rejected_token
                    .as_ref()
                    .map(Secret::expose_secret)
                    .unwrap_or_default();
                // 一方が他方の接頭辞でも、長い値を先に伏せて一部を残さない。
                let (first, second) = if current.len() >= rejected.len() {
                    (current, rejected)
                } else {
                    (rejected, current)
                };
                let mut error = error.redact(first).redact(second);
                if self.resend.is_some() {
                    error = error.chatgpt_details();
                }
                self.failed = Some(error.clone());
                Err(error)
            }
        }
    }

    fn poll_inner(&mut self, wait: Duration) -> Result<Vec<StreamEvent>, OpenAiResponsesError> {
        if self.cancellation.is_cancelled() {
            return Err(OpenAiResponsesError::Cancelled);
        }
        if self
            .resend
            .as_ref()
            .is_some_and(|resend| resend.refresh.is_some())
        {
            return self.poll_refresh(wait);
        }
        let http = self
            .http
            .as_mut()
            .ok_or(OpenAiResponsesError::InvalidRequest)?;
        let event = match http.poll(wait) {
            Ok(event) => event,
            Err(error) => {
                let error = if self.received_headers && !matches!(error, HttpError::Cancelled) {
                    provider_error(ProviderError::StreamDisconnected)
                } else {
                    classify_http(error)
                };
                if matches!(
                    error,
                    OpenAiResponsesError::Provider {
                        status: Some(401),
                        error: ProviderError::NonRetryable {
                            reason: NonRetryableKind::Authentication
                        },
                        ..
                    }
                ) && !self.delivered_events
                    && self.resend.is_some()
                {
                    return self.refresh_and_resend(error);
                }
                return Err(error);
            }
        };
        let assembler = self
            .assembler
            .as_mut()
            .ok_or(OpenAiResponsesError::InvalidResponse)?;
        match event {
            crate::http::StreamEvent::Headers(_) => {
                self.received_headers = true;
                Ok(Vec::new())
            }
            crate::http::StreamEvent::Pending => Ok(Vec::new()),
            crate::http::StreamEvent::Data(bytes) => {
                let mut events = Vec::new();
                for event in self
                    .decoder
                    .push(&bytes)
                    .map_err(OpenAiResponsesError::Sse)?
                {
                    events.extend(assembler.read_event(&event)?);
                    if assembler.is_finished() {
                        self.ended = true;
                        // 終端イベントを確認したので、HTTP の EOF を待たずに止める。
                        http.cancel();
                        break;
                    }
                }
                Ok(events)
            }
            crate::http::StreamEvent::Finished => {
                self.decoder.finish();
                self.ended = true;
                Err(provider_error(ProviderError::StreamDisconnected))
            }
        }
    }

    fn refresh_and_resend(
        &mut self,
        mut original: OpenAiResponsesError,
    ) -> Result<Vec<StreamEvent>, OpenAiResponsesError> {
        let resend = self
            .resend
            .as_mut()
            .ok_or(OpenAiResponsesError::InvalidRequest)?;
        if resend.refreshed {
            if let OpenAiResponsesError::Provider { error, .. } = &mut original {
                *error = ProviderError::NonRetryable {
                    reason: NonRetryableKind::LoginRequired,
                };
            }
            return Err(original);
        }
        resend.refreshed = true;
        // 401 を受けた接続は終わっている。回収してから新しい接続へ移る。
        self.http
            .as_mut()
            .ok_or(OpenAiResponsesError::InvalidRequest)?
            .join()
            .map_err(classify_http)?;
        let source = resend.source.clone();
        let rejected = self
            .token
            .as_ref()
            .ok_or(OpenAiResponsesError::InvalidRequest)?
            .clone();
        self.rejected_token = Some(rejected.clone());
        // 更新待ちでも poll を返し、呼び出し側が取り消しを印に伝えられるようにする。
        let cancelled = Arc::clone(&resend.cancelled);
        resend.refresh = Some(
            std::thread::Builder::new()
                .spawn(move || source.refresh(&rejected, &cancelled))
                .map_err(|_| {
                    provider_error(ProviderError::NonRetryable {
                        reason: NonRetryableKind::Authentication,
                    })
                })?,
        );
        Ok(Vec::new())
    }

    fn poll_refresh(&mut self, wait: Duration) -> Result<Vec<StreamEvent>, OpenAiResponsesError> {
        let resend = self
            .resend
            .as_mut()
            .ok_or(OpenAiResponsesError::InvalidRequest)?;
        let worker = resend
            .refresh
            .as_ref()
            .ok_or(OpenAiResponsesError::InvalidRequest)?;
        if !worker.is_finished() {
            std::thread::park_timeout(wait.min(Duration::from_millis(10)));
            return Ok(Vec::new());
        }
        let worker = resend
            .refresh
            .take()
            .ok_or(OpenAiResponsesError::InvalidRequest)?;
        let refreshed = worker.join().map_err(|_| {
            provider_error(if resend.refreshed {
                ProviderError::NonRetryable {
                    reason: NonRetryableKind::Authentication,
                }
            } else {
                ProviderError::Retryable {
                    retry_after_secs: None,
                }
            })
        })?;
        // 更新の送信後は保存まで待つが、取り消した要求の推論を送ってはならない
        // （設計書 02-01「Sign in with ChatGPT」、P5-22 の追加指示）。
        if self.cancellation.is_cancelled() {
            return Err(OpenAiResponsesError::Cancelled);
        }
        let token = refreshed?;
        let mut headers = resend.request.headers.clone();
        authorize(&mut headers, &token)?;
        self.token = Some(token);
        let http = resend
            .client
            .post_json(&resend.request.uri, &headers, &resend.request.body)
            .map_err(classify_http)?;
        self.cancellation.replace(http.cancellation());
        self.http = Some(http);
        Ok(Vec::new())
    }

    /// 終端イベントを確かめた応答を取り出す。途中や誤りのある応答は返さない。
    pub fn finish(mut self) -> Result<super::AssistantTurn, OpenAiResponsesError> {
        if let Some(error) = self.failed.take() {
            return Err(error);
        }
        self.assembler
            .take()
            .ok_or(OpenAiResponsesError::InvalidResponse)?
            .finish()
    }
}

fn provider_error(error: ProviderError) -> OpenAiResponsesError {
    OpenAiResponsesError::Provider {
        error,
        status: None,
        details: None,
        retry_after: None,
    }
}

fn api_error(
    body: &Value,
    status: Option<u16>,
    retry_after: Option<String>,
) -> OpenAiResponsesError {
    let details = ApiErrorDetails {
        error_type: body
            .pointer("/error/type")
            .and_then(Value::as_str)
            .map(str::to_owned),
        code: body
            .pointer("/error/code")
            .and_then(Value::as_str)
            .map(str::to_owned),
        message: body
            .pointer("/error/message")
            .and_then(Value::as_str)
            .map(str::to_owned),
    };
    let subscription = [details.code.as_deref(), details.error_type.as_deref()]
        .into_iter()
        .find_map(|code| match code {
            Some("subscription_sharing_usage_limit_exceeded") => {
                Some(NonRetryableKind::SubscriptionUsageLimit)
            }
            Some("subscription_sharing_user_not_eligible") => {
                Some(NonRetryableKind::SubscriptionNotEligible)
            }
            Some("subscription_sharing_unsupported_capability") => {
                Some(NonRetryableKind::SubscriptionUnsupported)
            }
            _ => None,
        });
    let error = if let Some(reason) = subscription {
        ProviderError::NonRetryable { reason }
    } else if matches!(status, Some(401 | 403)) {
        ProviderError::NonRetryable {
            reason: NonRetryableKind::Authentication,
        }
    } else if status.is_some_and(|s| (500..=599).contains(&s)) {
        ProviderError::Retryable {
            retry_after_secs: retry_after.as_deref().and_then(|s| s.trim().parse().ok()),
        }
    } else if details.code.as_deref() == Some("context_length_exceeded") {
        ProviderError::ContextOverflow
    } else if is_quota(details.code.as_deref(), details.error_type.as_deref()) {
        ProviderError::NonRetryable {
            reason: NonRetryableKind::Quota,
        }
    } else if status == Some(429)
        || (status.is_none()
            && matches!(
                details.code.as_deref(),
                Some("server_error" | "rate_limit_exceeded" | "slow_down")
            ))
    {
        ProviderError::Retryable {
            retry_after_secs: retry_after.as_deref().and_then(|s| s.trim().parse().ok()),
        }
    } else {
        ProviderError::NonRetryable {
            reason: if matches!(status, Some(400 | 413)) {
                NonRetryableKind::RequestRejected
            } else {
                NonRetryableKind::InvalidRequest
            },
        }
    };
    OpenAiResponsesError::Provider {
        error,
        status,
        details: Some(details),
        retry_after,
    }
}

fn classify_http(error: HttpError) -> OpenAiResponsesError {
    match error {
        HttpError::Status(status) => {
            let body = serde_json::from_slice(&status.body).unwrap_or(Value::Null);
            let retry_after = status
                .retry_after
                .as_ref()
                .and_then(|s| s.to_str().ok())
                .map(str::to_owned);
            api_error(&body, Some(status.head.status), retry_after)
        }
        HttpError::Cancelled => OpenAiResponsesError::Cancelled,
        HttpError::InvalidRequest => OpenAiResponsesError::InvalidRequest,
        HttpError::Connection | HttpError::Timeout(_) | HttpError::Tls | HttpError::Worker => {
            provider_error(ProviderError::Retryable {
                retry_after_secs: None,
            })
        }
        HttpError::BodyLimit { .. } => OpenAiResponsesError::InvalidResponse,
    }
}

fn is_quota(code: Option<&str>, error_type: Option<&str>) -> bool {
    matches!(
        code,
        Some(
            "credit_balance_exhausted"
                | "organization_spend_limit_exceeded"
                | "project_spend_limit_exceeded"
                | "organization_usage_limit_exceeded"
                | "insufficient_quota"
        )
    ) || error_type == Some("insufficient_quota")
}
