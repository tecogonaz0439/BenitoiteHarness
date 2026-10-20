//! ChatGPT の許可を取得・保存・更新・取り消す（設計書 02-01「Sign in with ChatGPT」、05-01「login と logout」）。

mod base64url;
mod callback;
mod jwt;
mod store;
pub mod text;

use crate::http::{HeaderMap, HttpClient, HttpError, HttpTimeouts, Uri};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::net::TcpListener;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
pub use store::{Account, Credentials, LockedStore, Secret, Store, Tokens};
use zeroize::Zeroizing;

const DYNAMIC_CLIENT: &str = "dynamic_agent_client";
const RESOURCE: &str = "https://api.openai.com/v1";
const DIRECT: &str = "chatgpt.tokens.use.direct";
const SCOPE: &str = "openid profile email offline_access resource.invoke chatgpt.tokens.use.direct";
const JSON_LIMIT: u64 = 64 * 1024;

/// 秘密やサーバーの診断を保持しない認証の失敗。
#[derive(Debug, Clone, Eq, PartialEq)]
pub enum AuthError {
    /// 応答・署名・claim の検証に失敗。
    Validation,
    /// 保存の型・権限・入出力の失敗。
    Storage,
    /// HTTP または待ち受けの失敗。
    Communication,
    /// 利用者による取り消し。
    Cancelled,
    /// 戻りを待つ期限。
    Timeout,
    /// 許可を取り直す必要がある。
    LoginRequired,
    /// 登録されたクライアントが拒まれた。
    InvalidClient,
    /// ChatGPT のプランの許可がない。
    Scope,
    /// 保存したアカウントと異なる。
    Account,
    /// OS の乱数を取得できなかった。
    Random,
    /// 安全な文字だけに限った OAuth の誤りの符号。
    Rejected(String),
}
impl std::fmt::Display for AuthError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let message = match self {
            Self::Validation => text::VALIDATION,
            Self::Storage => text::STORAGE,
            Self::Communication => text::COMMUNICATION,
            Self::Cancelled => text::CANCELLED,
            Self::Timeout => text::TIMEOUT,
            Self::LoginRequired => text::LOGIN_REQUIRED,
            Self::InvalidClient => text::INVALID_CLIENT,
            Self::Scope => text::SCOPE,
            Self::Account => text::ACCOUNT,
            Self::Random => text::RANDOM,
            Self::Rejected(code) => return f.write_str(&text::rejected(code)),
        };
        f.write_str(message)
    }
}
impl std::error::Error for AuthError {}
impl From<std::io::Error> for AuthError {
    fn from(_: std::io::Error) -> Self {
        Self::Storage
    }
}
impl From<rustix::io::Errno> for AuthError {
    fn from(_: rustix::io::Errno) -> Self {
        Self::Storage
    }
}

/// 認証の口。CLI は official だけを使い、模擬の口は呼び出し側から渡す。
#[derive(Clone, Debug)]
pub struct Endpoints {
    /// 認可の URL。
    pub authorize: String,
    /// トークン交換と更新の URL。
    pub token: String,
    /// OpenID の構成の URL。
    pub discovery: String,
    /// 完全一致を求める発行者。
    pub issuer: String,
}
impl Endpoints {
    /// 公式の固定の値。環境変数や利用者の設定では変更しない。
    pub fn official() -> Self {
        Self {
            authorize: "https://auth.openai.com/api/accounts/authorize".into(),
            token: "https://auth.openai.com/api/accounts/oauth/token".into(),
            discovery: "https://auth.openai.com/.well-known/openid-configuration".into(),
            issuer: "https://auth.openai.com".into(),
        }
    }
    fn uri(&self, value: &str) -> Result<Uri, AuthError> {
        let uri: Uri = value.parse().map_err(|_| AuthError::Validation)?;
        let loopback = [&self.authorize, &self.token, &self.discovery]
            .iter()
            .all(|value| {
                value.parse::<Uri>().is_ok_and(|uri| {
                    uri.scheme_str() == Some("http") && uri.host() == Some("127.0.0.1")
                })
            });
        if uri.authority().is_none_or(|a| a.as_str().contains('@'))
            || !(uri.scheme_str() == Some("https")
                || (loopback
                    && uri.scheme_str() == Some("http")
                    && uri.host() == Some("127.0.0.1")))
        {
            return Err(AuthError::Validation);
        }
        Ok(uri)
    }
}
#[derive(Deserialize)]
struct Discovery {
    issuer: String,
    jwks_uri: String,
    revocation_endpoint: Option<String>,
}
#[derive(Deserialize)]
struct TokenResponse {
    access_token: Secret,
    refresh_token: Option<Secret>,
    id_token: Option<Secret>,
    expires_in: u64,
    token_type: Option<String>,
    scope: Option<String>,
    #[serde(default, deserialize_with = "refresh_time")]
    earliest_refresh_at: Option<u64>,
}
impl TokenResponse {
    fn tokens(self, now: u64, previous: Option<&Tokens>) -> Result<Tokens, AuthError> {
        if self.expires_in == 0
            || self.access_token.expose_secret().is_empty()
            || self
                .token_type
                .as_deref()
                .is_some_and(|value| !value.eq_ignore_ascii_case("Bearer"))
        {
            return Err(AuthError::Validation);
        }
        let refresh_token = self
            .refresh_token
            .or_else(|| previous.map(|tokens| tokens.refresh_token.clone()))
            .ok_or(AuthError::Validation)?;
        if refresh_token.expose_secret().is_empty() {
            return Err(AuthError::Validation);
        }
        let scopes = self
            .scope
            .map(|value| value.split_whitespace().map(str::to_owned).collect())
            .or_else(|| previous.map(|tokens| tokens.scopes.clone()))
            .ok_or(AuthError::Validation)?;
        Ok(Tokens {
            access_token: self.access_token,
            refresh_token,
            expires_at: now
                .checked_add(self.expires_in)
                .ok_or(AuthError::Validation)?,
            earliest_refresh_at: self.earliest_refresh_at,
            scopes,
        })
    }
}
fn client(logout: bool) -> HttpClient {
    HttpClient::new(HttpTimeouts {
        connect: Duration::from_secs(10),
        headers: Duration::from_secs(if logout { 10 } else { 30 }),
        idle: Duration::from_secs(if logout { 10 } else { 30 }),
        total: Duration::from_secs(if logout { 10 } else { 60 }),
    })
}
fn discovery(endpoints: &Endpoints, http: &HttpClient) -> Result<Discovery, LoginFailure> {
    let response = http
        .get(
            &endpoints.uri(&endpoints.discovery)?,
            &HeaderMap::new(),
            JSON_LIMIT,
        )
        .map_err(LoginFailure::from)?;
    let discovery: Discovery =
        serde_json::from_slice(&response.body).map_err(|_| AuthError::Validation)?;
    if discovery.issuer != endpoints.issuer {
        return Err(AuthError::Validation.into());
    }
    endpoints.uri(&discovery.jwks_uri)?;
    if let Some(uri) = &discovery.revocation_endpoint {
        endpoints.uri(uri)?;
    }
    Ok(discovery)
}
struct LoginFailure {
    error: AuthError,
    preserve_client: bool,
    refresh_token: Option<Secret>,
    subject: Option<String>,
}
impl From<AuthError> for LoginFailure {
    fn from(error: AuthError) -> Self {
        let preserve_client = matches!(error, AuthError::Communication);
        Self {
            error,
            preserve_client,
            refresh_token: None,
            subject: None,
        }
    }
}
// ログインの失敗を一か所で片付けるため、HTTP の結果が不確かな場合だけ印を残す。
impl From<HttpError> for LoginFailure {
    fn from(error: HttpError) -> Self {
        match error {
            HttpError::Status(status) => {
                #[derive(Deserialize)]
                struct Failure {
                    error: ErrorCode,
                }
                let error = if (400..500).contains(&status.head.status) {
                    serde_json::from_slice::<Failure>(&status.body)
                        .map(|failure| failure.error.classify())
                        .unwrap_or(AuthError::Communication)
                } else {
                    AuthError::Communication
                };
                Self {
                    error,
                    preserve_client: matches!(status.head.status, 408 | 429 | 500..=599),
                    refresh_token: None,
                    subject: None,
                }
            }
            HttpError::Connection
            | HttpError::Timeout(_)
            | HttpError::Tls
            | HttpError::Cancelled
            | HttpError::Worker => Self {
                error: AuthError::Communication,
                preserve_client: true,
                refresh_token: None,
                subject: None,
            },
            HttpError::BodyLimit { .. } | HttpError::InvalidRequest => Self {
                error: AuthError::Communication,
                preserve_client: false,
                refresh_token: None,
                subject: None,
            },
        }
    }
}
#[derive(Deserialize)]
#[serde(untagged)]
enum ErrorCode {
    Text(String),
    Fields {
        code: Option<String>,
        r#type: Option<String>,
    },
}
impl ErrorCode {
    fn classify(&self) -> AuthError {
        let values = match self {
            Self::Text(value) => [Some(value.as_str()), None],
            Self::Fields { code, r#type } => [code.as_deref(), r#type.as_deref()],
        };
        if values.iter().flatten().any(|code| {
            matches!(
                *code,
                "invalid_grant"
                    | "invalid_refresh_token"
                    | "token_expired"
                    | "refresh_token_expired"
                    | "refresh_token_invalidated"
                    | "refresh_token_reused"
            )
        }) {
            AuthError::LoginRequired
        } else if values.contains(&Some("invalid_client")) {
            AuthError::InvalidClient
        } else {
            AuthError::Communication
        }
    }
}
fn exchange(endpoints: &Endpoints, fields: &[(&str, &str)]) -> Result<TokenResponse, LoginFailure> {
    let bytes = exchange_bytes(endpoints, fields, None)?;
    serde_json::from_slice(&bytes).map_err(|_| LoginFailure::from(AuthError::Validation))
}
fn exchange_bytes(
    endpoints: &Endpoints,
    fields: &[(&str, &str)],
    cancelled: Option<&AtomicBool>,
) -> Result<Zeroizing<Vec<u8>>, LoginFailure> {
    let body = Zeroizing::new(encode_form(fields));
    let http = client(false);
    let uri = endpoints.uri(&endpoints.token)?;
    if let Some(cancelled) = cancelled {
        check_cancelled(cancelled)?;
    }
    // ここから保存までは取り消さず、ローテーションした値を失わない
    // （設計書 02-01「更新」）。
    let response = http
        .post_form(&uri, &HeaderMap::new(), body.as_bytes(), JSON_LIMIT)
        .map_err(LoginFailure::from)?;
    Ok(Zeroizing::new(response.body))
}

/// ブラウザの操作だけを呼び出し側に任せてログインする。now は Unix の秒を返す。
/// 戻り待ち以降の code の交換と保存は取り消しを受けない。
pub fn login(
    store: &Store,
    endpoints: &Endpoints,
    open_url: impl FnOnce(&str),
    wait_limit: Duration,
    now: impl Fn() -> u64,
    cancelled: &AtomicBool,
) -> Result<Account, AuthError> {
    let mut credentials = {
        let locked = store.lock()?;
        let mut saved = locked.load()?.unwrap_or(Credentials {
            version: 1,
            client_id: DYNAMIC_CLIENT.into(),
            client_confirmed: false,
            ext_agent_host_id: String::new(),
            account: None,
            tokens: None,
            saved_at: now(),
        });
        if saved.ext_agent_host_id.is_empty() {
            saved.ext_agent_host_id = host_id()?;
            saved.saved_at = now();
            locked.save(&saved)?;
        }
        saved
    };
    let result = login_attempt(
        store,
        endpoints,
        open_url,
        wait_limit,
        &now,
        cancelled,
        &mut credentials,
    );
    match result {
        Ok(account) => Ok(account),
        Err(failure) => {
            let _revoked = revoke_failed_login(store, endpoints, &credentials, &failure);
            if !failure.preserve_client && !credentials.client_confirmed {
                let _cleanup = reset_unconfirmed_client(store, &credentials, now());
            }
            Err(failure.error)
        }
    }
}
fn login_attempt(
    store: &Store,
    endpoints: &Endpoints,
    open_url: impl FnOnce(&str),
    wait_limit: Duration,
    now: &impl Fn() -> u64,
    cancelled: &AtomicBool,
    credentials: &mut Credentials,
) -> Result<Account, LoginFailure> {
    let http = client(false);
    let discovery = discovery(endpoints, &http)?;
    let verifier = Zeroizing::new(random_value()?);
    let state = random_value()?;
    let nonce = random_value()?;
    let challenge = base64url::encode(&Sha256::digest(verifier.as_bytes()));
    let listener = TcpListener::bind("127.0.0.1:0").map_err(|_| AuthError::Communication)?;
    let redirect = format!(
        "http://{}/auth/callback",
        listener
            .local_addr()
            .map_err(|_| AuthError::Communication)?
    );
    endpoints.uri(&endpoints.authorize)?;
    let mut fields = vec![
        ("client_id", credentials.client_id.as_str()),
        ("response_type", "code"),
        ("redirect_uri", &redirect),
        ("scope", SCOPE),
        ("resource", RESOURCE),
        ("state", &state),
        ("nonce", &nonce),
        ("code_challenge", &challenge),
        ("code_challenge_method", "S256"),
        ("ext_agent_host_id", &credentials.ext_agent_host_id),
    ];
    if credentials.client_id == DYNAMIC_CLIENT {
        fields.push(("agent_name_hint", "benitoiteh"));
    }
    open_url(&format!("{}?{}", endpoints.authorize, encode_form(&fields)));
    let callback = callback::wait(&listener, &state, wait_limit, cancelled)?;
    drop(listener);
    if let Some(error) = callback.get("error") {
        let safe = if !error.is_empty()
            && error.len() <= 64
            && error
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
        {
            error.as_str()
        } else {
            "authorization_error"
        };
        return Err(AuthError::Rejected(safe.into()).into());
    }
    let code = callback
        .get("code")
        .filter(|code| !code.is_empty())
        .ok_or(AuthError::Validation)?;
    let callback_client = callback.get("client_id");
    let client_id = match callback_client {
        Some(id)
            if valid_client(id)
                && (credentials.client_id == DYNAMIC_CLIENT || credentials.client_id == *id) =>
        {
            id.clone()
        }
        None if credentials.client_id != DYNAMIC_CLIENT => credentials.client_id.clone(),
        _ => return Err(AuthError::Validation.into()),
    };
    // 戻りの ID は未確認として先に残す。交換に成功した ID だけを固定する。
    {
        let locked = store.lock()?;
        let mut latest = locked.load()?.ok_or(AuthError::Storage)?;
        if latest.client_id != credentials.client_id && latest.client_id != client_id {
            return Err(AuthError::Validation.into());
        }
        if latest.client_id != client_id {
            latest.client_confirmed = false;
        }
        latest.client_id = client_id.clone();
        latest.saved_at = now();
        // rename 後の同期だけが失敗しても、片付けるべき ID を失わない。
        *credentials = latest;
        locked.save(credentials)?;
    }
    let response = exchange(
        endpoints,
        &[
            ("grant_type", "authorization_code"),
            ("client_id", &client_id),
            ("code", code),
            ("code_verifier", &verifier),
            ("redirect_uri", &redirect),
            ("resource", RESOURCE),
        ],
    )?;
    let refresh_token = response.refresh_token.clone();
    let mut subject = None;
    let result = (|| -> Result<Account, LoginFailure> {
        // 交換の成功を、確認済みの印の保存が失敗した場合にも取り消さない。
        credentials.client_confirmed = true;
        {
            let locked = store.lock()?;
            let mut latest = locked.load()?.ok_or(AuthError::Storage)?;
            if latest.client_id != client_id {
                return Err(AuthError::Validation.into());
            }
            latest.client_confirmed = true;
            latest.saved_at = now();
            locked.save(&latest)?;
            *credentials = latest;
        }
        let id_token = response.id_token.as_ref().ok_or(AuthError::Validation)?;
        let jwks_response = http
            .get(
                &endpoints.uri(&discovery.jwks_uri)?,
                &HeaderMap::new(),
                256 * 1024,
            )
            .map_err(LoginFailure::from)?;
        let jwks: jwt::Jwks =
            serde_json::from_slice(&jwks_response.body).map_err(|_| AuthError::Validation)?;
        let account = jwt::verify(
            id_token.expose_secret(),
            &jwks,
            &endpoints.issuer,
            &client_id,
            &nonce,
            now(),
        )?;
        subject = Some(account.subject.clone());
        let tokens = response.tokens(now(), None)?;
        if !tokens.scopes.iter().any(|scope| scope == DIRECT) {
            return Err(AuthError::Scope.into());
        }
        let locked = store.lock()?;
        let latest = locked.load()?.ok_or(AuthError::Storage)?;
        if latest.client_id != client_id {
            return Err(AuthError::Validation.into());
        }
        if latest
            .account
            .as_ref()
            .is_some_and(|previous| previous.subject != account.subject)
        {
            return Err(AuthError::Account.into());
        }
        credentials.account = Some(account.clone());
        credentials.tokens = Some(tokens);
        credentials.saved_at = now();
        locked.save(credentials)?;
        Ok(account)
    })();
    result.map_err(|mut failure| {
        failure.refresh_token = refresh_token;
        failure.subject = subject;
        failure
    })
}

/// 要求の前に使えるアクセストークンを返し、必要なら錠の中で更新する。
/// 錠待ちと送信前は cancelled を見て、送信後は保存まで終える。
pub fn access_token(
    store: &Store,
    endpoints: &Endpoints,
    now: u64,
    cancelled: &AtomicBool,
) -> Result<Secret, AuthError> {
    check_cancelled(cancelled)?;
    access_token_with_snapshot(store, endpoints, now, store.load(), cancelled)
}
fn access_token_with_snapshot(
    store: &Store,
    endpoints: &Endpoints,
    now: u64,
    snapshot: Result<Option<Credentials>, AuthError>,
    cancelled: &AtomicBool,
) -> Result<Secret, AuthError> {
    // rename の直前に開いた inode は、錠なしの検査では nlink=0 になりうる。
    // 読めなかったときは錠の中で一度だけ読み直し、恒久的な失敗はそのまま返す。
    let saved = match snapshot {
        Ok(saved) => saved.ok_or(AuthError::LoginRequired)?,
        Err(_) => return refresh(store, endpoints, now, None, cancelled),
    };
    let tokens = saved.tokens.as_ref().ok_or(AuthError::LoginRequired)?;
    if !needs_refresh(tokens, now) {
        return Ok(tokens.access_token.clone());
    }
    refresh(store, endpoints, now, None, cancelled)
}
/// 401 で拒まれたトークンを渡し、一度だけ強制して更新する。
/// 既に別のプロセスが更新した値なら、再度更新せずその値を返す。
/// 錠待ちと送信前は cancelled を見て、送信後は保存まで終える。
pub fn force_refresh(
    store: &Store,
    endpoints: &Endpoints,
    rejected: &Secret,
    now: u64,
    cancelled: &AtomicBool,
) -> Result<Secret, AuthError> {
    refresh(store, endpoints, now, Some(rejected), cancelled)
}
fn needs_refresh(tokens: &Tokens, now: u64) -> bool {
    tokens.expires_at.saturating_sub(now) < 60
        && tokens
            .earliest_refresh_at
            .is_none_or(|earliest| now >= earliest)
}
fn reset_unconfirmed_client(
    store: &Store,
    attempt: &Credentials,
    now: u64,
) -> Result<(), AuthError> {
    let locked = store.lock()?;
    let Some(mut latest) = locked.load()? else {
        return Ok(());
    };
    // 並行したログインが交換を終えた ID や、別の試行の登録は消さない。
    if !latest.client_confirmed
        && latest.client_id == attempt.client_id
        && latest.ext_agent_host_id == attempt.ext_agent_host_id
    {
        latest.client_id = DYNAMIC_CLIENT.into();
        latest.saved_at = now;
        locked.save(&latest)?;
    }
    Ok(())
}
fn refresh(
    store: &Store,
    endpoints: &Endpoints,
    now: u64,
    rejected: Option<&Secret>,
    cancelled: &AtomicBool,
) -> Result<Secret, AuthError> {
    check_cancelled(cancelled)?;
    let locked = store.lock_cancellable(cancelled)?;
    check_cancelled(cancelled)?;
    let mut saved = locked.load()?.ok_or(AuthError::LoginRequired)?;
    let previous = saved.tokens.as_ref().ok_or(AuthError::LoginRequired)?;
    let required = if let Some(rejected) = rejected {
        constant_time_eq(
            previous.access_token.expose_secret().as_bytes(),
            rejected.expose_secret().as_bytes(),
        )
    } else {
        needs_refresh(previous, now)
    };
    if !required {
        return Ok(previous.access_token.clone());
    }
    let response = exchange_bytes(
        endpoints,
        &[
            ("grant_type", "refresh_token"),
            ("client_id", &saved.client_id),
            ("refresh_token", previous.refresh_token.expose_secret()),
            ("resource", RESOURCE),
        ],
        Some(cancelled),
    )
    .map_err(|failure| failure.error);
    let response = match response {
        Err(AuthError::LoginRequired) => {
            saved.tokens = None;
            saved.saved_at = now;
            let _cleanup = locked.save(&saved);
            return Err(AuthError::LoginRequired);
        }
        other => other?,
    };
    let tokens = serde_json::from_slice::<TokenResponse>(&response)
        .map_err(|_| AuthError::Validation)
        .and_then(|response| response.tokens(now, Some(previous)));
    let tokens = match tokens {
        Ok(tokens) => tokens,
        Err(error) => {
            #[derive(Deserialize)]
            struct Rotation {
                refresh_token: Secret,
            }
            if let Ok(rotation) = serde_json::from_slice::<Rotation>(&response)
                && !rotation.refresh_token.expose_secret().is_empty()
                && let Some(tokens) = &mut saved.tokens
            {
                tokens.refresh_token = rotation.refresh_token;
                tokens.earliest_refresh_at = None;
                saved.saved_at = now;
                locked.save(&saved)?;
            }
            return Err(error);
        }
    };
    let access = tokens.access_token.clone();
    saved.tokens = Some(tokens);
    saved.saved_at = now;
    locked.save(&saved)?;
    Ok(access)
}

/// ログアウトの結果。取り消しの失敗でもローカルのトークンは消す。
#[derive(Debug, Eq, PartialEq)]
pub enum LogoutOutcome {
    /// 保存した許可がない。
    NotSignedIn,
    /// 空の 200 で取り消しを確かめた。
    Revoked,
    /// 許可は消したが取り消しを確かめられない。
    Unconfirmed,
}
/// 取り消しの要求を終えてから、同じ錠の中でトークンと表示用の情報を消す。
pub fn logout(store: &Store, endpoints: &Endpoints, now: u64) -> Result<LogoutOutcome, AuthError> {
    let locked = store.lock()?;
    let Some(mut saved) = locked.load()? else {
        return Ok(LogoutOutcome::NotSignedIn);
    };
    let Some(tokens) = &saved.tokens else {
        if saved.account.take().is_some() {
            saved.saved_at = now;
            locked.save(&saved)?;
        }
        return Ok(LogoutOutcome::NotSignedIn);
    };
    let revoked = revoke(endpoints, &saved.client_id, &tokens.refresh_token);
    saved.tokens = None;
    saved.account = None;
    saved.saved_at = now;
    locked.save(&saved)?;
    Ok(if revoked {
        LogoutOutcome::Revoked
    } else {
        LogoutOutcome::Unconfirmed
    })
}
fn revoke(endpoints: &Endpoints, client_id: &str, refresh_token: &Secret) -> bool {
    (|| {
        let http = client(true);
        let discovery = discovery(endpoints, &http).map_err(|failure| failure.error)?;
        let uri = endpoints.uri(
            discovery
                .revocation_endpoint
                .as_deref()
                .ok_or(AuthError::Validation)?,
        )?;
        let body = Zeroizing::new(encode_form(&[
            ("token", refresh_token.expose_secret()),
            ("token_type_hint", "refresh_token"),
            ("client_id", client_id),
        ]));
        let response = http
            .post_form(&uri, &HeaderMap::new(), body.as_bytes(), JSON_LIMIT)
            .map_err(|_| AuthError::Communication)?;
        Ok::<bool, AuthError>(response.head.status == 200 && response.body.is_empty())
    })()
    .unwrap_or(false)
}
fn revoke_failed_login(
    store: &Store,
    endpoints: &Endpoints,
    credentials: &Credentials,
    failure: &LoginFailure,
) -> Result<(), AuthError> {
    let Some(token) = failure
        .refresh_token
        .as_ref()
        .filter(|token| !token.expose_secret().is_empty())
    else {
        return Ok(());
    };
    let locked = store.lock()?;
    let saved = locked.load()?;
    // 既存の許可に取り消しが及びうる場合は送らない。判定から送信まで同じ錠を保つ
    // （設計書 02-01「ログイン」、BH-117）。
    if saved.as_ref().is_some_and(|saved| {
        saved.tokens.is_some()
            && saved.client_id == credentials.client_id
            && (failure.subject.is_none()
                || saved.account.as_ref().map(|account| &account.subject)
                    == failure.subject.as_ref())
    }) {
        return Ok(());
    }
    let _revoked = revoke(endpoints, &credentials.client_id, token);
    Ok(())
}
fn check_cancelled(cancelled: &AtomicBool) -> Result<(), AuthError> {
    if cancelled.load(Ordering::Acquire) {
        Err(AuthError::Cancelled)
    } else {
        Ok(())
    }
}
fn valid_client(id: &str) -> bool {
    id.starts_with("oaiapp_")
        && id.len() <= 128
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}
fn random_value() -> Result<String, AuthError> {
    let mut bytes = [0; 32];
    getrandom::fill(&mut bytes).map_err(|_| AuthError::Random)?;
    Ok(base64url::encode(&bytes))
}
fn host_id() -> Result<String, AuthError> {
    let mut bytes = [0; 16];
    getrandom::fill(&mut bytes).map_err(|_| AuthError::Random)?;
    if let Some(byte) = bytes.get_mut(6) {
        *byte = (*byte & 0x0f) | 0x40;
    }
    if let Some(byte) = bytes.get_mut(8) {
        *byte = (*byte & 0x3f) | 0x80;
    }
    let mut value = String::from("urn:uuid:");
    for (index, byte) in bytes.iter().enumerate() {
        if matches!(index, 4 | 6 | 8 | 10) {
            value.push('-');
        }
        value.push_str(&format!("{byte:02x}"));
    }
    Ok(value)
}
pub(super) fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    left.iter()
        .zip(right)
        .fold(0u8, |difference, (a, b)| difference | (a ^ b))
        == 0
}
pub(super) fn encode_form(fields: &[(&str, &str)]) -> String {
    fields
        .iter()
        .map(|(key, value)| format!("{}={}", percent_encode(key), percent_encode(value)))
        .collect::<Vec<_>>()
        .join("&")
}
fn percent_encode(value: &str) -> String {
    let mut result = String::new();
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            result.push(char::from(byte));
        } else {
            result.push_str(&format!("%{byte:02X}"));
        }
    }
    result
}
pub(super) fn decode_form(value: &str) -> Result<BTreeMap<String, String>, AuthError> {
    fn decode(value: &str) -> Result<String, AuthError> {
        let mut result = Vec::new();
        let mut bytes = value.bytes();
        while let Some(byte) = bytes.next() {
            result.push(match byte {
                b'+' => b' ',
                b'%' => {
                    let a = char::from(bytes.next().ok_or(AuthError::Validation)?)
                        .to_digit(16)
                        .ok_or(AuthError::Validation)?;
                    let b = char::from(bytes.next().ok_or(AuthError::Validation)?)
                        .to_digit(16)
                        .ok_or(AuthError::Validation)?;
                    u8::try_from((a << 4) | b).map_err(|_| AuthError::Validation)?
                }
                other => other,
            });
        }
        String::from_utf8(result).map_err(|_| AuthError::Validation)
    }
    let mut fields = BTreeMap::new();
    if value.is_empty() {
        return Ok(fields);
    }
    for pair in value.split('&') {
        let (key, value) = pair.split_once('=').ok_or(AuthError::Validation)?;
        if fields.insert(decode(key)?, decode(value)?).is_some() {
            return Err(AuthError::Validation);
        }
    }
    Ok(fields)
}
fn refresh_time<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<u64>, D::Error> {
    let value = serde_json::Value::deserialize(deserializer)?;
    Ok(value
        .as_u64()
        .or_else(|| value.as_str().and_then(parse_utc)))
}
fn parse_utc(value: &str) -> Option<u64> {
    let value = value.strip_suffix('Z')?;
    let (date, time) = value.split_once('T')?;
    let clock = time.split('.').next()?;
    if date.len() != 10
        || clock.len() != 8
        || !date.bytes().enumerate().all(|(index, byte)| {
            if matches!(index, 4 | 7) {
                byte == b'-'
            } else {
                byte.is_ascii_digit()
            }
        })
        || !clock.bytes().enumerate().all(|(index, byte)| {
            if matches!(index, 2 | 5) {
                byte == b':'
            } else {
                byte.is_ascii_digit()
            }
        })
    {
        return None;
    }
    let mut date = date.split('-');
    let year = date.next()?.parse::<u64>().ok()?;
    let month = date.next()?.parse::<u64>().ok()?;
    let day = date.next()?.parse::<u64>().ok()?;
    if date.next().is_some() || !(1970..=9999).contains(&year) || !(1..=12).contains(&month) {
        return None;
    }
    let leap = |year: u64| {
        year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400))
    };
    let days_in_month = |month: u64, year: u64| match month {
        2 => {
            if leap(year) {
                29
            } else {
                28
            }
        }
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    };
    if day == 0 || day > days_in_month(month, year) {
        return None;
    }
    let mut time = time.split(':');
    let hour = time.next()?.parse::<u64>().ok()?;
    let minute = time.next()?.parse::<u64>().ok()?;
    let second = time.next()?;
    let (second, fraction) = second
        .split_once('.')
        .map_or((second, None), |(a, b)| (a, Some(b)));
    if fraction.is_some_and(|f| f.is_empty() || !f.bytes().all(|b| b.is_ascii_digit())) {
        return None;
    }
    let second = second.parse::<u64>().ok()?;
    if time.next().is_some() || hour > 23 || minute > 59 || second > 59 {
        return None;
    }
    let mut days = 0u64;
    for y in 1970..year {
        days = days.checked_add(if leap(y) { 366 } else { 365 })?;
    }
    for m in 1..month {
        days = days.checked_add(days_in_month(m, year))?;
    }
    days.checked_add(day.checked_sub(1)?)?
        .checked_mul(86400)?
        .checked_add(hour.checked_mul(3600)?)?
        .checked_add(minute.checked_mul(60)?)?
        .checked_add(second)
}
