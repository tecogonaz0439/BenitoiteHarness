//! 保存した許可と、再送をまたぐ取り消しを管理する（設計書 02-01「Sign in with ChatGPT」）。

use std::fmt;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::auth::{self, AuthError, Endpoints, Secret, Store};
use crate::http::{Cancellation, HeaderMap, HeaderValue};
use crate::provider::{NonRetryableKind, ProviderError};

use super::{OpenAiResponsesError, provider_error};

/// 要求ごとに保存を読み直す ChatGPT の認証元。秘密は診断に出さない。
#[derive(Clone)]
pub struct ChatGptTokens {
    store: Arc<Store>,
    endpoints: Endpoints,
}

impl fmt::Debug for ChatGptTokens {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ChatGptTokens").finish_non_exhaustive()
    }
}

impl ChatGptTokens {
    /// 検査済みの保存と認証の口を受け取る。CLI は公式の口だけを渡す。
    pub fn new(store: Store, endpoints: Endpoints) -> Self {
        Self {
            store: Arc::new(store),
            endpoints,
        }
    }

    pub(super) fn access_token(&self) -> Result<Secret, OpenAiResponsesError> {
        auth::access_token(&self.store, &self.endpoints, now()).map_err(|e| classify(e, false))
    }

    pub(super) fn refresh(&self, rejected: &Secret) -> Result<Secret, OpenAiResponsesError> {
        auth::force_refresh(&self.store, &self.endpoints, rejected, now())
            .map_err(|e| classify(e, true))
    }
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn classify(error: AuthError, forced: bool) -> OpenAiResponsesError {
    provider_error(match error {
        AuthError::Communication | AuthError::Timeout if !forced => ProviderError::Retryable {
            retry_after_secs: None,
        },
        AuthError::Communication | AuthError::Timeout => ProviderError::NonRetryable {
            reason: NonRetryableKind::Authentication,
        },
        AuthError::Validation
        | AuthError::Storage
        | AuthError::Cancelled
        | AuthError::LoginRequired
        | AuthError::InvalidClient
        | AuthError::Scope
        | AuthError::Account
        | AuthError::Random
        | AuthError::Rejected(_) => ProviderError::NonRetryable {
            reason: NonRetryableKind::LoginRequired,
        },
    })
}

pub(super) fn authorize(
    headers: &mut HeaderMap,
    token: &Secret,
) -> Result<(), OpenAiResponsesError> {
    let mut value = HeaderValue::from_str(&format!("Bearer {}", token.expose_secret()))
        .map_err(|_| OpenAiResponsesError::InvalidRequest)?;
    value.set_sensitive(true);
    headers.insert("authorization", value);
    Ok(())
}

struct State {
    cancelled: bool,
    current: Option<Cancellation>,
}

/// 再送前に取得したハンドルでも、現在の要求を取り消せる。
#[derive(Clone)]
pub struct ResponsesCancellation(Arc<Mutex<State>>);

impl fmt::Debug for ResponsesCancellation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ResponsesCancellation")
            .finish_non_exhaustive()
    }
}

impl ResponsesCancellation {
    pub(super) fn new(current: Option<Cancellation>) -> Self {
        Self(Arc::new(Mutex::new(State {
            cancelled: false,
            current,
        })))
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// 現在の要求を取り消し、更新中なら再送を止める。
    pub fn cancel(&self) {
        let mut state = self.lock();
        state.cancelled = true;
        if let Some(current) = &state.current {
            current.cancel();
        }
    }

    /// 明示的な取り消しが要求されたか。
    pub fn is_cancelled(&self) -> bool {
        self.lock().cancelled
    }

    pub(super) fn replace(&self, current: Cancellation) {
        let mut state = self.lock();
        // 差し替えと cancel を同じ錠で守り、古い接続だけを止める競合を避ける。
        if state.cancelled {
            current.cancel();
        }
        state.current = Some(current);
    }
}
