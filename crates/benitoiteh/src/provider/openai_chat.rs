//! Chat Completions API の要求とストリームを共通の形に変える（設計書 02-01「OpenAI の Chat Completions API」）。

mod assembly;
mod request;
mod text;

use std::fmt;
use std::time::Duration;

use serde_json::Value;

use crate::http::{Cancellation, HttpClient, HttpError, HttpStream};

use super::sse::{SseDecoder, SseError};
use super::{Capabilities, Destination, NonRetryableKind, ProviderError, StreamEvent};

pub use assembly::OpenAiChatAssembler;
pub use request::{OpenAiChatRequest, RequestOptions};

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
pub enum OpenAiChatError {
    /// 提供者または通信の失敗。
    Provider {
        /// ループがやり直しを判定する分類。
        error: ProviderError,
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

impl OpenAiChatError {
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
}

impl fmt::Debug for OpenAiChatError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OpenAiChatError")
            .field("classification", &self.classification())
            .finish_non_exhaustive()
    }
}

impl fmt::Display for OpenAiChatError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Provider { .. } => text::PROVIDER,
            Self::InvalidRequest => text::INVALID_REQUEST,
            Self::InvalidResponse | Self::Sse(_) => text::INVALID_RESPONSE,
            Self::Cancelled => text::CANCELLED,
        })
    }
}

impl std::error::Error for OpenAiChatError {}

/// API キーの解決は呼び出す側が行う。アダプタはキーを診断に出さない。
pub struct OpenAiChatAdapter {
    destination: Destination,
    capabilities: Capabilities,
    api_key: String,
}

impl fmt::Debug for OpenAiChatAdapter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OpenAiChatAdapter").finish_non_exhaustive()
    }
}

impl OpenAiChatAdapter {
    /// 送り先と機能と、解決済みの API キーを受け取る。空のキーなら認証を送らない。
    pub fn new(destination: Destination, capabilities: Capabilities, api_key: String) -> Self {
        Self {
            destination,
            capabilities,
            api_key,
        }
    }

    /// 構築した要求を HTTP の経路へ渡す。受け取りの取り消しは戻り値から行える。
    pub fn stream(
        &self,
        client: &HttpClient,
        request: &OpenAiChatRequest,
    ) -> Result<OpenAiChatStream, OpenAiChatError> {
        let http = client
            .post_json(&request.uri, &request.headers, &request.body)
            .map_err(|e| classify_http(e).redact(&self.api_key))?;
        Ok(OpenAiChatStream {
            http,
            decoder: SseDecoder::new(),
            assembler: Some(OpenAiChatAssembler::new(self.destination.clone())),
            api_key: self.api_key.clone(),
            ended: false,
            received_headers: false,
            failed: None,
        })
    }
}

/// HTTP と SSE の読み取りを結び、応答全体の確認まで部分的な結果を保持する。
pub struct OpenAiChatStream {
    http: HttpStream,
    decoder: SseDecoder,
    assembler: Option<OpenAiChatAssembler>,
    api_key: String,
    ended: bool,
    received_headers: bool,
    failed: Option<OpenAiChatError>,
}

impl fmt::Debug for OpenAiChatStream {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OpenAiChatStream")
            .field("ended", &self.ended)
            .finish_non_exhaustive()
    }
}

impl OpenAiChatStream {
    /// 別のスレッドからも要求を取り消せるハンドルを返す。
    pub fn cancellation(&self) -> Cancellation {
        self.http.cancellation()
    }

    /// 最大 wait 待って、今届いた共通のイベントを返す。空なら受け取りを続ける。
    /// Finished が届いた後に finish で応答を取り出す。誤りの後は同じ誤りを返す。
    pub fn poll(&mut self, wait: Duration) -> Result<Vec<StreamEvent>, OpenAiChatError> {
        if let Some(error) = &self.failed {
            return Err(error.clone());
        }
        if self.ended {
            return Ok(Vec::new());
        }
        let result = self.poll_inner(wait);
        match result {
            Ok(events) => Ok(events),
            Err(error) => {
                self.http.cancel();
                let error = error.redact(&self.api_key);
                self.failed = Some(error.clone());
                Err(error)
            }
        }
    }

    fn poll_inner(&mut self, wait: Duration) -> Result<Vec<StreamEvent>, OpenAiChatError> {
        let assembler = self
            .assembler
            .as_mut()
            .ok_or(OpenAiChatError::InvalidResponse)?;
        let event = self.http.poll(wait).map_err(|error| {
            if self.received_headers && !matches!(error, HttpError::Cancelled) {
                provider_error(ProviderError::StreamDisconnected)
            } else {
                classify_http(error)
            }
        })?;
        match event {
            crate::http::StreamEvent::Headers(_) => {
                self.received_headers = true;
                Ok(Vec::new())
            }
            crate::http::StreamEvent::Pending => Ok(Vec::new()),
            crate::http::StreamEvent::Data(bytes) => {
                let mut events = Vec::new();
                for event in self.decoder.push(&bytes).map_err(OpenAiChatError::Sse)? {
                    events.extend(assembler.read_event(&event)?);
                    if assembler.is_finished() {
                        self.ended = true;
                        // [DONE] が応答全体の終わり。HTTP の EOF を待つ必要はない。
                        self.http.cancel();
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

    /// [DONE] を確かめた応答を取り出す。途中や誤りのある応答は返さない。
    pub fn finish(mut self) -> Result<super::AssistantTurn, OpenAiChatError> {
        if let Some(error) = self.failed.take() {
            return Err(error);
        }
        self.assembler
            .take()
            .ok_or(OpenAiChatError::InvalidResponse)?
            .finish()
    }
}

fn provider_error(error: ProviderError) -> OpenAiChatError {
    OpenAiChatError::Provider {
        error,
        details: None,
        retry_after: None,
    }
}

fn api_error(body: &Value, status: Option<u16>, retry_after: Option<String>) -> OpenAiChatError {
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
    let error = if matches!(status, Some(401 | 403)) {
        ProviderError::NonRetryable {
            reason: NonRetryableKind::Authentication,
        }
    } else if status.is_some_and(|s| s == 429 || (500..=599).contains(&s)) {
        ProviderError::Retryable {
            retry_after_secs: retry_after.as_deref().and_then(|s| s.trim().parse().ok()),
        }
    } else if details.code.as_deref() == Some("context_length_exceeded") {
        ProviderError::ContextOverflow
    } else {
        ProviderError::NonRetryable {
            reason: NonRetryableKind::InvalidRequest,
        }
    };
    OpenAiChatError::Provider {
        error,
        details: Some(details),
        retry_after,
    }
}

fn classify_http(error: HttpError) -> OpenAiChatError {
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
        HttpError::Cancelled => OpenAiChatError::Cancelled,
        HttpError::InvalidRequest => OpenAiChatError::InvalidRequest,
        HttpError::Connection | HttpError::Timeout(_) | HttpError::Tls | HttpError::Worker => {
            provider_error(ProviderError::Retryable {
                retry_after_secs: None,
            })
        }
        HttpError::BodyLimit { .. } => OpenAiChatError::InvalidResponse,
    }
}
