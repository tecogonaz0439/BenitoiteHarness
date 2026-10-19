//! 模擬の口と本物のファイル・錠で認証の契約を確かめる（実装プラン P5-21「テスト」）。

#[path = "support/auth_key.rs"]
mod auth_key;
#[allow(dead_code)]
mod support;

use benitoiteh::auth::{
    self, Account, AuthError, Credentials, Endpoints, LogoutOutcome, Secret, Store, Tokens,
};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::fs;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::os::unix::fs::{MetadataExt, PermissionsExt, symlink};
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Barrier, Mutex};
use std::time::Duration;
use support::mock_provider::{Delivery, MockProvider, ResponseScript};

type Error = Box<dyn std::error::Error>;
type TestResult = Result<(), Error>;
const NOW: u64 = 1_700_000_000;
const ISSUER: &str = "https://auth.openai.com";
const CLIENT: &str = "oaiapp_test";
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Result<Self, Error> {
        let mut random = [0; 16];
        getrandom::fill(&mut random)?;
        let name: String = random.iter().map(|b| format!("{b:02x}")).collect();
        let path = std::env::temp_dir().join(format!("benitoiteh-auth-{name}"));
        fs::create_dir(&path)?;
        Ok(Self(path))
    }
    fn store(&self) -> Result<Store, AuthError> {
        Store::open(&self.0.join("data/auth"))
    }
    fn file(&self) -> PathBuf {
        self.0.join("data/auth/chatgpt.json")
    }
    fn command(&self) -> std::process::Command {
        let mut command = std::process::Command::new(env!("CARGO_BIN_EXE_benitoiteh"));
        command
            .current_dir(&self.0)
            .env("HOME", &self.0)
            .env("XDG_CONFIG_HOME", self.0.join("config"))
            .env("XDG_STATE_HOME", &self.0)
            .env("TMPDIR", &self.0);
        command
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _removed = fs::remove_dir_all(&self.0);
    }
}
fn credentials(expires_at: u64) -> Credentials {
    Credentials {
        version: 1,
        client_id: CLIENT.into(),
        client_confirmed: true,
        ext_agent_host_id: "urn:uuid:01234567-89ab-4cde-8fab-0123456789ab".into(),
        account: Some(Account {
            email: Some("test@example.invalid".into()),
            subject: "subject".into(),
        }),
        tokens: Some(Tokens {
            access_token: Secret::new("access-secret".into()),
            refresh_token: Secret::new("refresh-secret".into()),
            expires_at,
            earliest_refresh_at: None,
            scopes: vec!["chatgpt.tokens.use.direct".into()],
        }),
        saved_at: NOW,
    }
}
fn endpoints(server: &MockProvider) -> Endpoints {
    Endpoints {
        authorize: format!("{}/authorize", server.base_url()),
        token: format!("{}/token", server.base_url()),
        discovery: format!("{}/discovery", server.base_url()),
        issuer: ISSUER.into(),
    }
}
fn discovery(server: &MockProvider, jwks: &MockProvider) -> Value {
    json!({"issuer":ISSUER,"jwks_uri":format!("{}/jwks",jwks.base_url()),"revocation_endpoint":format!("{}/revoke",server.base_url())})
}
fn form(value: &str) -> Result<BTreeMap<String, String>, Error> {
    fn decode(value: &str) -> Result<String, Error> {
        let mut bytes = value.bytes();
        let mut output = Vec::new();
        while let Some(byte) = bytes.next() {
            output.push(match byte {
                b'+' => b' ',
                b'%' => {
                    let a = char::from(bytes.next().ok_or("short escape")?)
                        .to_digit(16)
                        .ok_or("escape")?;
                    let b = char::from(bytes.next().ok_or("short escape")?)
                        .to_digit(16)
                        .ok_or("escape")?;
                    u8::try_from((a << 4) | b)?
                }
                other => other,
            });
        }
        Ok(String::from_utf8(output)?)
    }
    value
        .split('&')
        .map(|pair| {
            let (key, value) = pair.split_once('=').ok_or("pair")?;
            Ok((decode(key)?, decode(value)?))
        })
        .collect()
}
fn get<'a>(fields: &'a BTreeMap<String, String>, key: &str) -> Result<&'a str, Error> {
    Ok(fields.get(key).ok_or("missing field")?)
}
fn callback(url: &str, method: &str, path: &str, host: &str) -> Result<u16, Error> {
    let uri: benitoiteh::http::Uri = url.parse()?;
    let mut socket = TcpStream::connect(uri.authority().ok_or("authority")?.as_str())?;
    socket.set_read_timeout(Some(Duration::from_secs(30)))?;
    write!(socket, "{method} {path} HTTP/1.1\r\n")?;
    if !host.is_empty() {
        write!(socket, "{host}\r\n")?;
    }
    write!(socket, "Connection: close\r\n\r\n")?;
    let mut response = String::new();
    socket.read_to_string(&mut response)?;
    Ok(response
        .split_whitespace()
        .nth(1)
        .ok_or("status")?
        .parse()?)
}
fn login_with(
    store: &Store,
    ep: &Endpoints,
    browser: impl FnOnce(&str) -> TestResult + Send,
    cancelled: &AtomicBool,
    timeout: Duration,
) -> Result<Result<Account, AuthError>, Error> {
    let result = std::thread::scope(|scope| {
        let browser_worker = Mutex::new(None);
        let result = auth::login(
            store,
            ep,
            |url| {
                let url = url.to_owned();
                // ブラウザは別の実行主体である。受け付け開始前に戻りを待ってデッドロックしない。
                if let Ok(mut worker) = browser_worker.lock() {
                    *worker = Some(scope.spawn(move || {
                        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                            browser(&url)
                        }));
                        match result {
                            Ok(Ok(())) => Ok(()),
                            Ok(Err(error)) => {
                                cancelled.store(true, std::sync::atomic::Ordering::Release);
                                Err(error.to_string())
                            }
                            Err(_) => {
                                cancelled.store(true, std::sync::atomic::Ordering::Release);
                                Err("browser panicked".to_owned())
                            }
                        }
                    }));
                }
            },
            timeout,
            || NOW,
            cancelled,
        );
        let worker = browser_worker
            .into_inner()
            .map_err(|_| "browser lock")?
            .ok_or("browser not called")?;
        worker
            .join()
            .map_err(|_| "browser panicked")?
            .map_err(std::io::Error::other)?;
        Ok::<_, Error>(result)
    })?;
    Ok(result)
}

// 作成時の関門: HTTP の要求・コールバックの拒否・保存までを認証の公開の入口で確かめる。
// JWT の条件別の拒否は jwt の持ち主で検査し、ここは署名付き応答との結線を守る。
#[test]
fn login_checks_callback_and_saves_verified_credentials() -> TestResult {
    let fixture = Fixture::new()?;
    let store = fixture.store()?;
    let (mut server, scripts) = MockProvider::start_channel()?;
    let mut jwks = MockProvider::start(vec![ResponseScript::json(200, &auth_key::jwks()?)?])?;
    scripts.send(ResponseScript::json(200, &discovery(&server, &jwks))?)?;
    let ep = endpoints(&server);
    let browser_ep = ep.clone();
    let challenge_seen = Arc::new(Mutex::new(String::new()));
    let browser_challenge = Arc::clone(&challenge_seen);
    let result = login_with(
        &store,
        &ep,
        move |url| {
            let (base, query) = url.split_once('?').ok_or("URL query")?;
            assert_eq!(base, browser_ep.authorize);
            assert!(query.contains("scope=openid%20profile%20email%20offline_access%20resource.invoke%20chatgpt.tokens.use.direct"));
            let fields = form(query)?;
            *browser_challenge.lock().map_err(|_| "challenge lock")? =
                get(&fields, "code_challenge")?.to_owned();
            assert_eq!(
                fields.keys().cloned().collect::<Vec<_>>(),
                [
                    "agent_name_hint",
                    "client_id",
                    "code_challenge",
                    "code_challenge_method",
                    "ext_agent_host_id",
                    "nonce",
                    "redirect_uri",
                    "resource",
                    "response_type",
                    "scope",
                    "state"
                ]
            );
            assert_eq!(get(&fields, "client_id")?, "dynamic_agent_client");
            assert_eq!(get(&fields, "code_challenge_method")?, "S256");
            assert_eq!(get(&fields, "response_type")?, "code");
            assert_eq!(get(&fields, "agent_name_hint")?, "benitoiteh");
            assert_eq!(get(&fields, "resource")?, "https://api.openai.com/v1");
            for key in ["nonce", "state", "code_challenge"] {
                assert_eq!(get(&fields, key)?.len(), 43);
            }
            let host_id = get(&fields, "ext_agent_host_id")?;
            assert!(host_id.starts_with("urn:uuid:"));
            assert_eq!(host_id.as_bytes().get(23), Some(&b'4'));
            assert!(matches!(
                host_id.as_bytes().get(28),
                Some(b'8' | b'9' | b'a' | b'b')
            ));
            let redirect = get(&fields, "redirect_uri")?;
            let uri: benitoiteh::http::Uri = redirect.parse()?;
            let host = format!("Host: {}", uri.authority().ok_or("host")?);
            for (method, path, header, status) in [
                ("GET", "/other", host.as_str(), 404),
                ("POST", "/auth/callback", host.as_str(), 405),
                ("GET", "/auth/callback?state=wrong", host.as_str(), 400),
                ("GET", "/auth/callback", host.as_str(), 400),
                ("GET", "/auth/callback", "Host: attacker.invalid", 400),
                ("GET", "/auth/callback", "", 400),
            ] {
                assert_eq!(callback(redirect, method, path, header)?, status);
            }
            let state = get(&fields, "state")?;
            for (query, header) in [
                (format!("state={state}&state={state}"), host.clone()),
                (format!("state={state}&code=%ZZ"), host.clone()),
                (format!("state={state}"), format!("{host}\r\n{host}")),
            ] {
                assert_eq!(
                    callback(redirect, "GET", &format!("/auth/callback?{query}"), &header)?,
                    400
                );
            }
            let token = auth_key::sign(
                &json!({"alg":"RS256","kid":"test-key"}),
                &json!({"iss":ISSUER,"aud":CLIENT,"exp":NOW.saturating_add(3600),"nonce":get(&fields,"nonce")?,"sub":"subject","email":"test@example.invalid"}),
            )?;
            scripts.send(ResponseScript::json(200,&json!({"access_token":"new-access","refresh_token":"new-refresh","expires_in":3600,"scope":"openid chatgpt.tokens.use.direct","id_token":token,"token_type":"bEaReR","earliest_refresh_at":"2023-11-14T22:15:00Z"}))?)?;
            assert_eq!(
                callback(
                    redirect,
                    "GET",
                    &format!(
                        "/auth/callback?state={state}&code=code%2B%20value&client_id={CLIENT}"
                    ),
                    &host
                )?,
                200
            );
            Ok(())
        },
        &AtomicBool::new(false),
        Duration::from_secs(5),
    )??;
    assert_eq!(result.email.as_deref(), Some("test@example.invalid"));
    let saved = store.load()?.ok_or("credentials")?;
    assert_eq!(saved.client_id, CLIENT);
    assert!(saved.client_confirmed);
    let tokens = saved.tokens.as_ref().ok_or("tokens")?;
    assert_eq!(tokens.access_token.expose_secret(), "new-access");
    assert_eq!(tokens.earliest_refresh_at, Some(1_700_000_100));
    assert_eq!(fs::metadata(fixture.file())?.mode() & 0o7777, 0o600);
    assert_eq!(
        fs::metadata(fixture.file().parent().ok_or("parent")?)?.mode() & 0o7777,
        0o700
    );
    assert!(!format!("{saved:?}").contains("new-access"));
    assert!(!format!("{saved:?}").contains("new-refresh"));
    let requests = server.requests()?;
    assert_eq!(requests.len(), 2);
    let request = requests.get(1).ok_or("token request")?;
    assert_eq!(request.method, "POST");
    assert_eq!(request.path, "/token");
    assert_eq!(
        request.headers.get("content-type").map(String::as_str),
        Some("application/x-www-form-urlencoded")
    );
    let fields = form(std::str::from_utf8(&request.raw_body)?)?;
    assert_eq!(get(&fields, "grant_type")?, "authorization_code");
    assert_eq!(get(&fields, "code")?, "code+ value");
    assert_eq!(get(&fields, "code_verifier")?.len(), 43);
    use sha2::{Digest, Sha256};
    // 送った verifier と authorize の challenge の対応も、保存前の URL で得た値と比べる。
    assert_eq!(
        auth_key::encode(&Sha256::digest(get(&fields, "code_verifier")?.as_bytes())),
        *challenge_seen.lock().map_err(|_| "challenge lock")?
    );
    server.shutdown()?;
    jwks.shutdown()?;
    Ok(())
}

#[test]
fn login_rejects_authorization_client_scope_and_account_errors() -> TestResult {
    for case in [
        "denied",
        "unsafe_error",
        "missing_client",
        "wrong_client",
        "bad_client",
        "scope",
        "account",
        "token_type",
        "expires",
        "missing_refresh",
        "missing_scope",
    ] {
        let fixture = Fixture::new()?;
        let store = fixture.store()?;
        if matches!(case, "wrong_client" | "account") {
            store.lock()?.save(&credentials(NOW.saturating_add(3600)))?;
        }
        let (mut server, scripts) = MockProvider::start_channel()?;
        let mut jwks = MockProvider::start(vec![ResponseScript::json(200, &auth_key::jwks()?)?])?;
        scripts.send(ResponseScript::json(200, &discovery(&server, &jwks))?)?;
        let result = login_with(
            &store,
            &endpoints(&server),
            move |url| {
                let fields = form(url.split_once('?').ok_or("query")?.1)?;
                let redirect = get(&fields, "redirect_uri")?;
                let uri: benitoiteh::http::Uri = redirect.parse()?;
                let host = format!("Host: {}", uri.authority().ok_or("host")?);
                let state = get(&fields, "state")?;
                let suffix = match case {
                    "denied" => "error=access_denied&error_description=SECRET",
                    "unsafe_error" => "error=%1Bsecret",
                    "missing_client" => "code=code",
                    "bad_client" => "code=code&client_id=evil",
                    "wrong_client" => "code=code&client_id=oaiapp_other",
                    _ => "code=code&client_id=oaiapp_test",
                };
                if matches!(
                    case,
                    "scope"
                        | "account"
                        | "token_type"
                        | "expires"
                        | "missing_refresh"
                        | "missing_scope"
                ) {
                    let token = auth_key::sign(
                        &json!({"alg":"RS256","kid":"test-key"}),
                        &json!({"iss":ISSUER,"aud":CLIENT,"exp":NOW.saturating_add(1000),"nonce":get(&fields,"nonce")?,"sub":if case == "account" {"other"} else {"subject"}}),
                    )?;
                    let mut response = json!({"access_token":"secret-access","refresh_token":"secret-refresh","expires_in":1000,"scope":if case == "scope" {"openid"} else {"chatgpt.tokens.use.direct"},"id_token":token});
                    if case == "token_type" {
                        response
                            .as_object_mut()
                            .ok_or("response")?
                            .insert("token_type".into(), json!("Basic"));
                    }
                    if case == "expires" {
                        response
                            .as_object_mut()
                            .ok_or("response")?
                            .insert("expires_in".into(), json!(0));
                    }
                    if case == "missing_refresh" {
                        response
                            .as_object_mut()
                            .ok_or("response")?
                            .remove("refresh_token");
                    }
                    if case == "missing_scope" {
                        response.as_object_mut().ok_or("response")?.remove("scope");
                    }
                    scripts.send(ResponseScript::json(200, &response)?)?;
                }
                assert_eq!(
                    callback(
                        redirect,
                        "GET",
                        &format!("/auth/callback?state={state}&{suffix}"),
                        &host
                    )?,
                    200
                );
                Ok(())
            },
            &AtomicBool::new(false),
            Duration::from_secs(5),
        )?;
        let expected = match case {
            "denied" => AuthError::Rejected("access_denied".into()),
            "unsafe_error" => AuthError::Rejected("authorization_error".into()),
            "scope" => AuthError::Scope,
            "account" => AuthError::Account,
            _ => AuthError::Validation,
        };
        assert_eq!(result, Err(expected), "{case}");
        let saved = store.load()?.ok_or("saved host ID")?;
        if matches!(
            case,
            "scope" | "token_type" | "expires" | "missing_refresh" | "missing_scope"
        ) {
            assert_eq!(saved.client_id, CLIENT);
            assert!(saved.tokens.is_none());
        }
        if case == "account" {
            assert_eq!(saved.account.ok_or("account")?.subject, "subject");
        }
        let message = result.err().ok_or("failure")?.to_string();
        assert!(!message.contains("secret-access") && !message.contains("SECRET"));
        if matches!(
            case,
            "denied" | "unsafe_error" | "missing_client" | "wrong_client" | "bad_client"
        ) {
            assert_eq!(server.requests()?.len(), 1);
        }
        server.shutdown()?;
        jwks.shutdown()?;
    }
    Ok(())
}

#[test]
fn callback_wait_has_deadline_and_cancellation() -> TestResult {
    for cancel in [false, true] {
        let fixture = Fixture::new()?;
        let store = fixture.store()?;
        let (mut server, scripts) = MockProvider::start_channel()?;
        scripts.send(ResponseScript::json(200, &discovery(&server, &server))?)?;
        let result = auth::login(
            &store,
            &endpoints(&server),
            |_| {},
            Duration::from_millis(30),
            || NOW,
            &AtomicBool::new(cancel),
        );
        assert_eq!(
            result,
            Err(if cancel {
                AuthError::Cancelled
            } else {
                AuthError::Timeout
            })
        );
        server.shutdown()?;
    }
    Ok(())
}

#[test]
fn refresh_observes_expiry_earliest_time_and_serializes_rotation() -> TestResult {
    let fixture = Fixture::new()?;
    let store = fixture.store()?;
    let mut saved = credentials(NOW.saturating_add(60));
    store.lock()?.save(&saved)?;
    let mut server = MockProvider::start(vec![ResponseScript::json(
        200,
        &json!({"access_token":"rotated","refresh_token":"rotated-refresh","expires_in":3600,"earliest_refresh_at":NOW.saturating_add(100)}),
    )?])?;
    let ep = endpoints(&server);
    assert_eq!(
        auth::access_token(&store, &ep, NOW)?.expose_secret(),
        "access-secret"
    );
    saved.tokens.as_mut().ok_or("tokens")?.expires_at = NOW;
    saved.tokens.as_mut().ok_or("tokens")?.earliest_refresh_at = Some(NOW.saturating_add(1));
    store.lock()?.save(&saved)?;
    assert_eq!(
        auth::access_token(&store, &ep, NOW)?.expose_secret(),
        "access-secret"
    );
    saved.tokens.as_mut().ok_or("tokens")?.earliest_refresh_at = Some(NOW);
    store.lock()?.save(&saved)?;
    let barrier = Arc::new(Barrier::new(2));
    std::thread::scope(|scope| -> TestResult {
        let mut workers = Vec::new();
        for _ in 0..2 {
            let barrier = Arc::clone(&barrier);
            let directory = fixture.0.join("data/auth");
            let ep = ep.clone();
            workers.push(scope.spawn(move || -> Result<Secret, AuthError> {
                let store = Store::open(&directory)?;
                barrier.wait();
                auth::access_token(&store, &ep, NOW)
            }));
        }
        for worker in workers {
            assert_eq!(
                worker
                    .join()
                    .map_err(|_| "refresh panicked")??
                    .expose_secret(),
                "rotated"
            );
        }
        Ok(())
    })?;
    let saved = store.load()?.ok_or("saved")?;
    let tokens = saved.tokens.ok_or("tokens")?;
    assert_eq!(tokens.refresh_token.expose_secret(), "rotated-refresh");
    assert_eq!(tokens.scopes, vec!["chatgpt.tokens.use.direct"]);
    assert_eq!(tokens.earliest_refresh_at, Some(NOW.saturating_add(100)));
    let requests = server.requests()?;
    assert_eq!(requests.len(), 1);
    let fields = form(std::str::from_utf8(
        &requests.first().ok_or("request")?.raw_body,
    )?)?;
    assert_eq!(
        fields,
        BTreeMap::from([
            ("grant_type".into(), "refresh_token".into()),
            ("client_id".into(), CLIENT.into()),
            ("refresh_token".into(), "refresh-secret".into()),
            ("resource".into(), "https://api.openai.com/v1".into())
        ])
    );
    server.shutdown()?;
    Ok(())
}

#[test]
fn forced_refresh_ignores_earliest_and_reuses_another_writers_token() -> TestResult {
    let fixture = Fixture::new()?;
    let store = fixture.store()?;
    let mut saved = credentials(NOW.saturating_add(3600));
    saved.tokens.as_mut().ok_or("tokens")?.earliest_refresh_at = Some(NOW.saturating_add(3600));
    store.lock()?.save(&saved)?;
    let mut server = MockProvider::start(vec![ResponseScript::json(
        200,
        &json!({"access_token":"forced","expires_in":3600,"scope":"openid chatgpt.tokens.use.direct","earliest_refresh_at":"bad","id_token":"ignored"}),
    )?])?;
    let ep = endpoints(&server);
    let old = Secret::new("access-secret".into());
    assert_eq!(
        auth::force_refresh(&store, &ep, &old, NOW)?.expose_secret(),
        "forced"
    );
    assert_eq!(
        auth::force_refresh(&store, &ep, &old, NOW)?.expose_secret(),
        "forced"
    );
    assert_eq!(server.requests()?.len(), 1);
    let saved = store.load()?.ok_or("saved")?;
    let tokens = saved.tokens.ok_or("tokens")?;
    assert_eq!(tokens.refresh_token.expose_secret(), "refresh-secret");
    assert_eq!(tokens.earliest_refresh_at, None);
    server.shutdown()?;
    Ok(())
}

#[test]
fn refresh_removes_only_permanently_rejected_credentials() -> TestResult {
    for (status, code, expected, clear) in [
        (400, "invalid_grant", AuthError::LoginRequired, true),
        (400, "invalid_refresh_token", AuthError::LoginRequired, true),
        (400, "token_expired", AuthError::LoginRequired, true),
        (400, "refresh_token_expired", AuthError::LoginRequired, true),
        (
            400,
            "refresh_token_invalidated",
            AuthError::LoginRequired,
            true,
        ),
        (400, "refresh_token_reused", AuthError::LoginRequired, true),
        (400, "invalid_client", AuthError::InvalidClient, false),
        (500, "invalid_grant", AuthError::Communication, false),
    ] {
        let fixture = Fixture::new()?;
        let store = fixture.store()?;
        store.lock()?.save(&credentials(NOW))?;
        let mut server = MockProvider::start(vec![ResponseScript::json(
            status,
            &json!({"error":code,"error_description":"secret-refresh"}),
        )?])?;
        let result = auth::access_token(&store, &endpoints(&server), NOW);
        assert_eq!(result.err(), Some(expected), "{code} {status}");
        assert_eq!(store.load()?.ok_or("saved")?.tokens.is_none(), clear);
        server.shutdown()?;
    }
    // 接続できないときもローカルの許可を消さない。
    let fixture = Fixture::new()?;
    let store = fixture.store()?;
    store.lock()?.save(&credentials(NOW))?;
    let server = MockProvider::start(vec![])?;
    let ep = endpoints(&server);
    drop(server);
    assert_eq!(
        auth::access_token(&store, &ep, NOW).err(),
        Some(AuthError::Communication)
    );
    assert!(store.load()?.ok_or("saved")?.tokens.is_some());
    Ok(())
}

#[test]
fn logout_revokes_and_always_removes_tokens_but_retains_registration() -> TestResult {
    for success in [false, true] {
        let fixture = Fixture::new()?;
        let store = fixture.store()?;
        store.lock()?.save(&credentials(NOW))?;
        let (mut server, scripts) = MockProvider::start_channel()?;
        scripts.send(ResponseScript::json(200, &discovery(&server, &server))?)?;
        scripts.send(ResponseScript {
            status: if success { 200 } else { 500 },
            headers: vec![],
            body: vec![],
            delivery: Delivery::Once,
        })?;
        assert_eq!(
            auth::logout(&store, &endpoints(&server), NOW)?,
            if success {
                LogoutOutcome::Revoked
            } else {
                LogoutOutcome::Unconfirmed
            }
        );
        let saved = store.load()?.ok_or("saved")?;
        assert!(saved.tokens.is_none() && saved.account.is_none());
        assert_eq!(saved.client_id, CLIENT);
        assert_eq!(saved.ext_agent_host_id, credentials(NOW).ext_agent_host_id);
        assert!(!fs::read_to_string(fixture.file())?.contains("secret"));
        let requests = server.requests()?;
        let request = requests.get(1).ok_or("revocation")?;
        assert_eq!(request.path, "/revoke");
        assert_eq!(
            form(std::str::from_utf8(&request.raw_body)?)?,
            BTreeMap::from([
                ("token".into(), "refresh-secret".into()),
                ("token_type_hint".into(), "refresh_token".into()),
                ("client_id".into(), CLIENT.into())
            ])
        );
        assert_eq!(
            auth::logout(&store, &endpoints(&server), NOW)?,
            LogoutOutcome::NotSignedIn
        );
        assert_eq!(server.requests()?.len(), 2);
        server.shutdown()?;
    }
    // discovery 自体が失敗しても、ローカルの許可は消す。
    let fixture = Fixture::new()?;
    let store = fixture.store()?;
    store.lock()?.save(&credentials(NOW))?;
    let mut server = MockProvider::start(vec![ResponseScript::json(500, &json!({}))?])?;
    assert_eq!(
        auth::logout(&store, &endpoints(&server), NOW)?,
        LogoutOutcome::Unconfirmed
    );
    assert!(store.load()?.ok_or("saved")?.tokens.is_none());
    server.shutdown()?;
    Ok(())
}

#[test]
fn storage_rejects_unsafe_paths_and_replaces_files_atomically() -> TestResult {
    let fixture = Fixture::new()?;
    let store = fixture.store()?;
    store.lock()?.save(&credentials(NOW))?;
    let old = fs::File::open(fixture.file())?;
    let inode = old.metadata()?.ino();
    let mut changed = credentials(NOW);
    changed.tokens = None;
    store.lock()?.save(&changed)?;
    assert_ne!(fs::metadata(fixture.file())?.ino(), inode);
    assert_eq!(old.metadata()?.nlink(), 0);
    let names =
        fs::read_dir(fixture.file().parent().ok_or("parent")?)?.collect::<Result<Vec<_>, _>>()?;
    assert_eq!(names.len(), 2);
    fs::set_permissions(fixture.file(), fs::Permissions::from_mode(0o644))?;
    assert!(matches!(store.load(), Err(AuthError::Storage)));
    fs::set_permissions(fixture.file(), fs::Permissions::from_mode(0o600))?;
    let other = fixture.0.join("other");
    fs::hard_link(fixture.file(), &other)?;
    assert!(store.load().is_err());
    fs::remove_file(&other)?;
    fs::remove_file(fixture.file())?;
    symlink(&other, fixture.file())?;
    assert!(store.load().is_err());
    fs::remove_file(fixture.file())?;
    fs::write(fixture.file(), "x".repeat(65537))?;
    fs::set_permissions(fixture.file(), fs::Permissions::from_mode(0o600))?;
    assert!(store.load().is_err());
    let directory = fixture.file().parent().ok_or("parent")?.to_path_buf();
    fs::set_permissions(&directory, fs::Permissions::from_mode(0o755))?;
    assert!(fixture.store().is_err());
    fs::set_permissions(&directory, fs::Permissions::from_mode(0o700))?;
    fs::remove_file(fixture.file())?;
    fs::set_permissions(
        directory.join("chatgpt.lock"),
        fs::Permissions::from_mode(0o644),
    )?;
    assert!(store.lock().is_err());
    Ok(())
}

#[test]
fn cli_requires_exact_chatgpt_argument_and_logout_without_tokens_is_offline() -> TestResult {
    let fixture = Fixture::new()?;
    for command in ["login", "logout"] {
        for arguments in [
            vec![command],
            vec![command, "other"],
            vec![command, "chatgpt", "extra"],
        ] {
            let output = fixture.command().args(arguments).output()?;
            assert_eq!(output.status.code(), Some(2));
        }
    }
    let output = fixture.command().args(["logout", "chatgpt"]).output()?;
    assert!(output.status.success());
    assert!(String::from_utf8(output.stderr)?.contains("Not signed in to ChatGPT."));
    // 保存の権限の誤りは通信の前に 1 で終える。
    fs::set_permissions(
        fixture.0.join("benitoiteh/auth"),
        fs::Permissions::from_mode(0o755),
    )?;
    let output = fixture.command().args(["login", "chatgpt"]).output()?;
    assert_eq!(output.status.code(), Some(1));
    Ok(())
}

#[test]
fn registered_login_reuses_client_and_host_and_code_exchange_is_never_retried() -> TestResult {
    for success in [true, false] {
        let fixture = Fixture::new()?;
        let store = fixture.store()?;
        store.lock()?.save(&credentials(NOW))?;
        let (mut server, scripts) = MockProvider::start_channel()?;
        let mut jwks = MockProvider::start(vec![ResponseScript::json(200, &auth_key::jwks()?)?])?;
        scripts.send(ResponseScript::json(200, &discovery(&server, &jwks))?)?;
        let result = login_with(
            &store,
            &endpoints(&server),
            move |url| {
                let query = url.split_once('?').ok_or("query")?.1;
                let fields = form(query)?;
                assert_eq!(get(&fields, "client_id")?, CLIENT);
                assert_eq!(
                    get(&fields, "ext_agent_host_id")?,
                    credentials(NOW).ext_agent_host_id
                );
                assert!(!fields.contains_key("agent_name_hint"));
                let token = auth_key::sign(
                    &json!({"alg":"RS256","kid":"test-key"}),
                    &json!({"iss":ISSUER,"aud":CLIENT,"exp":NOW.saturating_add(3600),"nonce":get(&fields,"nonce")?,"sub":"subject"}),
                )?;
                let mut script = ResponseScript::json(
                    200,
                    &json!({"access_token":"second-access","refresh_token":"second-refresh","expires_in":3600,"scope":"chatgpt.tokens.use.direct","id_token":token}),
                )?;
                if !success {
                    script.delivery = Delivery::Disconnect { after: 0 };
                }
                scripts.send(script)?;
                if !success {
                    // 再送された要求も観測できるよう、失敗の後にも一つ応答を用意する。
                    scripts.send(ResponseScript::json(
                        500,
                        &json!({"error":"unexpected_retry"}),
                    )?)?;
                }
                let redirect = get(&fields, "redirect_uri")?;
                let uri: benitoiteh::http::Uri = redirect.parse()?;
                let host = format!("Host: {}", uri.authority().ok_or("host")?);
                assert_eq!(
                    callback(
                        redirect,
                        "GET",
                        &format!("/auth/callback?state={}&code=code", get(&fields, "state")?),
                        &host
                    )?,
                    200
                );
                Ok(())
            },
            &AtomicBool::new(false),
            Duration::from_secs(5),
        )?;
        if success {
            assert!(result.is_ok());
        } else {
            assert_eq!(result, Err(AuthError::Communication));
        }
        assert_eq!(server.requests()?.len(), 2);
        let saved = store.load()?.ok_or("saved")?;
        assert_eq!(saved.client_id, CLIENT);
        assert_eq!(saved.ext_agent_host_id, credentials(NOW).ext_agent_host_id);
        assert_eq!(
            saved.tokens.ok_or("tokens")?.access_token.expose_secret(),
            if success {
                "second-access"
            } else {
                "access-secret"
            }
        );
        server.shutdown()?;
        jwks.shutdown()?;
    }
    Ok(())
}

#[test]
fn discovery_and_response_bounds_are_checked_before_saving_tokens() -> TestResult {
    for case in ["issuer", "jwks_http", "redirect", "large_discovery"] {
        let fixture = Fixture::new()?;
        let store = fixture.store()?;
        let (mut server, scripts) = MockProvider::start_channel()?;
        let mut body = discovery(&server, &server);
        let script = match case {
            "issuer" => {
                *body.get_mut("issuer").ok_or("issuer")? = json!("wrong");
                ResponseScript::json(200, &body)?
            }
            "jwks_http" => {
                *body.get_mut("jwks_uri").ok_or("jwks")? = json!("http://example.invalid/jwks");
                ResponseScript::json(200, &body)?
            }
            "redirect" => ResponseScript {
                status: 302,
                headers: vec![("Location".into(), "http://127.0.0.1:1/NEVER_FOLLOW".into())],
                body: vec![],
                delivery: Delivery::Once,
            },
            _ => ResponseScript {
                status: 200,
                headers: vec![],
                body: vec![b' '; 65537],
                delivery: Delivery::Once,
            },
        };
        scripts.send(script)?;
        let opened = AtomicBool::new(false);
        assert!(
            auth::login(
                &store,
                &endpoints(&server),
                |_| opened.store(true, std::sync::atomic::Ordering::Release),
                Duration::from_millis(10),
                || NOW,
                &AtomicBool::new(false)
            )
            .is_err()
        );
        assert!(!opened.load(std::sync::atomic::Ordering::Acquire));
        assert!(store.load()?.ok_or("saved")?.tokens.is_none());
        assert_eq!(server.requests()?.len(), 1);
        server.shutdown()?;
    }
    Ok(())
}

// 作成時の関門: 接続・保存・錠の実際の境界で、レビューの問題を決まった順に再現する。
#[test]
fn security_idle_connections_do_not_block_a_callback() -> TestResult {
    for count in [1, 2] {
        let fixture = Fixture::new()?;
        let store = fixture.store()?;
        let (mut server, scripts) = MockProvider::start_channel()?;
        scripts.send(ResponseScript::json(200, &discovery(&server, &server))?)?;
        let result = login_with(
            &store,
            &endpoints(&server),
            move |url| {
                let fields = form(url.split_once('?').ok_or("query")?.1)?;
                let redirect = get(&fields, "redirect_uri")?;
                let uri: benitoiteh::http::Uri = redirect.parse()?;
                let address = uri.authority().ok_or("authority")?.as_str();
                let mut idle = Vec::new();
                for _ in 0..count {
                    idle.push(TcpStream::connect(address)?);
                }
                // connect の完了順に、無送信の接続を先に待ち受けの列へ入れる。
                let host = format!("Host: {address}");
                let status = callback(
                    redirect,
                    "GET",
                    &format!(
                        "/auth/callback?state={}&error=access_denied",
                        get(&fields, "state")?
                    ),
                    &host,
                )?;
                assert_eq!(status, 200);
                for socket in &idle {
                    socket.set_nonblocking(true)?;
                    assert!(
                        matches!(socket.peek(&mut [0]), Err(error) if error.kind() == std::io::ErrorKind::WouldBlock),
                        "idle peer must remain open until the valid callback is accepted"
                    );
                }
                drop(idle);
                Ok(())
            },
            &AtomicBool::new(false),
            Duration::from_secs(30),
        )?;
        assert_eq!(result, Err(AuthError::Rejected("access_denied".into())));
        server.shutdown()?;
    }
    Ok(())
}

#[test]
fn security_unconfirmed_client_recovers_but_successful_exchange_pins_client() -> TestResult {
    for (status, known_client, code) in [
        (400, false, "invalid_grant"),
        (403, false, "other"),
        (200, false, ""),
        (500, false, "invalid_grant"),
        (400, true, "invalid_client"),
    ] {
        let fixture = Fixture::new()?;
        let store = fixture.store()?;
        let first_id = "oaiapp_untrusted";
        if known_client {
            let mut saved = credentials(NOW);
            saved.client_id = first_id.into();
            store.lock()?.save(&saved)?;
        }
        let (mut server, scripts) = MockProvider::start_channel()?;
        scripts.send(ResponseScript::json(200, &discovery(&server, &server))?)?;
        let result = login_with(
            &store,
            &endpoints(&server),
            move |url| {
                let fields = form(url.split_once('?').ok_or("query")?.1)?;
                assert_eq!(
                    get(&fields, "client_id")?,
                    if known_client {
                        first_id
                    } else {
                        "dynamic_agent_client"
                    }
                );
                let response = if status == 200 {
                    // 交換後の検証に失敗しても、登録自体を確かめた ID は残す。
                    json!({"access_token":"access","refresh_token":"refresh","expires_in":3600,"scope":"chatgpt.tokens.use.direct"})
                } else {
                    json!({"error":code})
                };
                scripts.send(ResponseScript::json(status, &response)?)?;
                let redirect = get(&fields, "redirect_uri")?;
                let uri: benitoiteh::http::Uri = redirect.parse()?;
                callback(
                    redirect,
                    "GET",
                    &format!(
                        "/auth/callback?state={}&client_id={first_id}&code=fake",
                        get(&fields, "state")?
                    ),
                    &format!("Host: {}", uri.authority().ok_or("host")?),
                )?;
                Ok(())
            },
            &AtomicBool::new(false),
            Duration::from_secs(30),
        )?;
        assert!(result.is_err());
        server.shutdown()?;
        for attempt in 0..2 {
            let (mut server, scripts) = MockProvider::start_channel()?;
            scripts.send(ResponseScript::json(200, &discovery(&server, &server))?)?;
            let result = login_with(
                &store,
                &endpoints(&server),
                move |url| {
                    let fields = form(url.split_once('?').ok_or("query")?.1)?;
                    let pinned = known_client || status == 200 || (status == 500 && attempt == 0);
                    assert_eq!(
                        get(&fields, "client_id")?,
                        if pinned {
                            first_id
                        } else {
                            "dynamic_agent_client"
                        }
                    );
                    assert_eq!(fields.contains_key("agent_name_hint"), !pinned);
                    let redirect = get(&fields, "redirect_uri")?;
                    let uri: benitoiteh::http::Uri = redirect.parse()?;
                    callback(
                        redirect,
                        "GET",
                        &format!(
                            "/auth/callback?state={}&error=access_denied",
                            get(&fields, "state")?
                        ),
                        &format!("Host: {}", uri.authority().ok_or("host")?),
                    )?;
                    Ok(())
                },
                &AtomicBool::new(false),
                Duration::from_secs(30),
            )?;
            assert_eq!(result, Err(AuthError::Rejected("access_denied".into())));
            server.shutdown()?;
        }
    }
    // 新しい欄がない旧保存は、確認済みとは見なさずに読む。
    let fixture = Fixture::new()?;
    let store = fixture.store()?;
    let mut saved = credentials(NOW);
    saved.tokens = None;
    store.lock()?.save(&saved)?;
    let mut value = serde_json::to_value(saved)?;
    value
        .as_object_mut()
        .ok_or("credentials")?
        .remove("client_confirmed");
    fs::write(fixture.file(), serde_json::to_vec(&value)?)?;
    assert!(!store.load()?.ok_or("saved")?.client_confirmed);
    Ok(())
}

#[test]
fn security_refresh_understands_nested_error_codes_and_types() -> TestResult {
    for (status, body, expected, clear) in [
        (
            400,
            json!({"error":{"code":"refresh_token_reused"}}),
            AuthError::LoginRequired,
            true,
        ),
        (
            400,
            json!({"error":{"type":"invalid_grant"}}),
            AuthError::LoginRequired,
            true,
        ),
        (
            400,
            json!({"error":{"code":"other","type":"refresh_token_expired"}}),
            AuthError::LoginRequired,
            true,
        ),
        (
            400,
            json!({"error":{"code":"invalid_client"}}),
            AuthError::InvalidClient,
            false,
        ),
        (
            500,
            json!({"error":{"code":"invalid_grant"}}),
            AuthError::Communication,
            false,
        ),
    ] {
        let fixture = Fixture::new()?;
        let store = fixture.store()?;
        store.lock()?.save(&credentials(NOW))?;
        let mut server = MockProvider::start(vec![ResponseScript::json(status, &body)?])?;
        assert_eq!(
            auth::access_token(&store, &endpoints(&server), NOW).err(),
            Some(expected)
        );
        assert_eq!(store.load()?.ok_or("saved")?.tokens.is_none(), clear);
        server.shutdown()?;
    }
    Ok(())
}

#[test]
fn security_lock_and_logout_remove_only_owned_regular_temporary_files() -> TestResult {
    let fixture = Fixture::new()?;
    let store = fixture.store()?;
    store.lock()?.save(&credentials(NOW))?;
    let directory = fixture.file().parent().ok_or("auth")?.to_path_buf();
    let orphan = directory.join(".chatgpt-crashed.tmp");
    let target = fixture.0.join("outside");
    fs::write(&target, "must remain")?;
    let link = directory.join(".chatgpt-link.tmp");
    symlink(&target, &link)?;
    fs::create_dir(directory.join(".chatgpt-directory.tmp"))?;
    fs::write(directory.join("unrelated.tmp"), "keep")?;
    fs::write(&orphan, "secret-refresh")?;
    drop(store.lock()?);
    assert!(!orphan.exists());
    assert!(fs::symlink_metadata(&link)?.file_type().is_symlink());
    assert_eq!(fs::read_to_string(&target)?, "must remain");
    assert!(directory.join(".chatgpt-directory.tmp").is_dir());
    assert!(directory.join("unrelated.tmp").is_file());
    // トークンがないログアウトでも、クラッシュの残りを取り除く。
    let mut saved = credentials(NOW);
    saved.tokens = None;
    store.lock()?.save(&saved)?;
    fs::write(&orphan, "secret-access")?;
    let mut server = MockProvider::start(vec![])?;
    auth::logout(&store, &endpoints(&server), NOW)?;
    assert!(!orphan.exists());
    assert!(server.requests()?.is_empty());
    server.shutdown()?;
    Ok(())
}

#[test]
fn security_valid_access_token_does_not_open_or_take_the_lock() -> TestResult {
    let fixture = Fixture::new()?;
    let store = fixture.store()?;
    store.lock()?.save(&credentials(NOW.saturating_add(3600)))?;
    let held = store.lock()?;
    let lock = fixture.file().parent().ok_or("auth")?.join("chatgpt.lock");
    // 錠は既に保持中。新しく開こうとしたことも、権限の検査で待たずに検出する。
    fs::set_permissions(&lock, fs::Permissions::from_mode(0o000))?;
    let mut server = MockProvider::start(vec![])?;
    let result = auth::access_token(&store, &endpoints(&server), NOW);
    fs::set_permissions(&lock, fs::Permissions::from_mode(0o600))?;
    drop(held);
    assert_eq!(result?.expose_secret(), "access-secret");
    assert!(server.requests()?.is_empty());
    server.shutdown()?;
    Ok(())
}

#[test]
fn security_logout_without_tokens_clears_account_and_keeps_registration() -> TestResult {
    let fixture = Fixture::new()?;
    let store = fixture.store()?;
    let mut saved = credentials(NOW);
    saved.tokens = None;
    store.lock()?.save(&saved)?;
    let mut server = MockProvider::start(vec![])?;
    auth::logout(&store, &endpoints(&server), NOW)?;
    let saved = store.load()?.ok_or("saved")?;
    assert!(saved.account.is_none());
    assert_eq!(saved.client_id, CLIENT);
    assert_eq!(saved.ext_agent_host_id, credentials(NOW).ext_agent_host_id);
    assert!(server.requests()?.is_empty());
    assert!(
        AuthError::Account
            .to_string()
            .contains("benitoiteh logout chatgpt")
    );
    server.shutdown()?;
    Ok(())
}

#[test]
fn security_store_uses_auth_dir_without_nesting_another_auth() -> TestResult {
    let fixture = Fixture::new()?;
    let paths = benitoiteh::config::paths::DataPaths::from_values(
        Some(&fixture.0),
        Some(&fixture.0.join("config")),
        Some(&fixture.0.join("state")),
        Some(&fixture.0),
    )?;
    let store = Store::open(&paths.auth_dir)?;
    store.lock()?.save(&credentials(NOW))?;
    assert!(paths.auth_dir.join("chatgpt.json").is_file());
    assert!(!paths.auth_dir.join("auth").exists());
    Ok(())
}

// 作成時の関門: 戻りと保存、次の認可 URL までを本物の認証の入口で確かめる。
#[test]
fn rereview_missing_code_and_invalid_exchange_cannot_pin_client() -> TestResult {
    for suffix in ["", "&code=", "&code=malformed", "&code=redirect"] {
        let fixture = Fixture::new()?;
        let store = fixture.store()?;
        let (mut server, scripts) = MockProvider::start_channel()?;
        scripts.send(ResponseScript::json(200, &discovery(&server, &server))?)?;
        let result = login_with(
            &store,
            &endpoints(&server),
            move |url| {
                let fields = form(url.split_once('?').ok_or("query")?.1)?;
                let redirect = get(&fields, "redirect_uri")?;
                let uri: benitoiteh::http::Uri = redirect.parse()?;
                if suffix == "&code=malformed" {
                    scripts.send(ResponseScript::json(200, &json!({}))?)?;
                }
                if suffix == "&code=redirect" {
                    scripts.send(ResponseScript {
                        status: 302,
                        headers: vec![(
                            "Location".into(),
                            "http://127.0.0.1:1/never-follow".into(),
                        )],
                        body: vec![],
                        delivery: Delivery::Once,
                    })?;
                }
                assert_eq!(
                    callback(
                        redirect,
                        "GET",
                        &format!(
                            "/auth/callback?state={}&client_id=oaiapp_poison{suffix}",
                            get(&fields, "state")?
                        ),
                        &format!("Host: {}", uri.authority().ok_or("host")?)
                    )?,
                    200
                );
                Ok(())
            },
            &AtomicBool::new(false),
            Duration::from_secs(30),
        )?;
        assert_eq!(
            result,
            Err(if suffix == "&code=redirect" {
                AuthError::Communication
            } else {
                AuthError::Validation
            })
        );
        assert_eq!(
            server.requests()?.len(),
            if suffix.is_empty() || suffix == "&code=" {
                1
            } else {
                2
            }
        );
        server.shutdown()?;
        assert_dynamic_login(&store)?;
    }
    Ok(())
}
fn assert_dynamic_login(store: &Store) -> TestResult {
    let (mut server, scripts) = MockProvider::start_channel()?;
    scripts.send(ResponseScript::json(200, &discovery(&server, &server))?)?;
    let result = login_with(
        store,
        &endpoints(&server),
        move |url| {
            let fields = form(url.split_once('?').ok_or("query")?.1)?;
            assert_eq!(get(&fields, "client_id")?, "dynamic_agent_client");
            assert_eq!(get(&fields, "agent_name_hint")?, "benitoiteh");
            let redirect = get(&fields, "redirect_uri")?;
            let uri: benitoiteh::http::Uri = redirect.parse()?;
            callback(
                redirect,
                "GET",
                &format!(
                    "/auth/callback?state={}&error=access_denied",
                    get(&fields, "state")?
                ),
                &format!("Host: {}", uri.authority().ok_or("host")?),
            )?;
            Ok(())
        },
        &AtomicBool::new(false),
        Duration::from_secs(30),
    )?;
    assert_eq!(result, Err(AuthError::Rejected("access_denied".into())));
    server.shutdown()?;
    Ok(())
}
#[test]
fn rereview_unconfirmed_client_is_reset_after_timeout_cancellation_or_validation() -> TestResult {
    for (case, confirmed) in [
        ("timeout", false),
        ("cancelled", false),
        ("validation", false),
        ("timeout", true),
        ("cancelled", true),
        ("validation", true),
    ] {
        let fixture = Fixture::new()?;
        let store = fixture.store()?;
        let mut saved = credentials(NOW);
        saved.client_confirmed = confirmed;
        saved.account = None;
        saved.tokens = None;
        store.lock()?.save(&saved)?;
        let (mut server, scripts) = MockProvider::start_channel()?;
        scripts.send(ResponseScript::json(200, &discovery(&server, &server))?)?;
        let cancelled = AtomicBool::new(case == "cancelled");
        let result = if case == "validation" {
            login_with(
                &store,
                &endpoints(&server),
                move |url| {
                    let fields = form(url.split_once('?').ok_or("query")?.1)?;
                    let redirect = get(&fields, "redirect_uri")?;
                    let uri: benitoiteh::http::Uri = redirect.parse()?;
                    callback(
                        redirect,
                        "GET",
                        &format!(
                            "/auth/callback?state={}&client_id=oaiapp_other&code=fake",
                            get(&fields, "state")?
                        ),
                        &format!("Host: {}", uri.authority().ok_or("host")?),
                    )?;
                    Ok(())
                },
                &cancelled,
                Duration::from_secs(30),
            )?
        } else {
            // 期限 0 は経過時間を待たずに Timeout の分岐を選ぶ。取り消しも先に印を立てる。
            auth::login(
                &store,
                &endpoints(&server),
                |_| {},
                if case == "timeout" {
                    Duration::ZERO
                } else {
                    Duration::from_secs(30)
                },
                || NOW,
                &cancelled,
            )
        };
        assert_eq!(
            result,
            Err(match case {
                "timeout" => AuthError::Timeout,
                "cancelled" => AuthError::Cancelled,
                _ => AuthError::Validation,
            })
        );
        server.shutdown()?;
        if confirmed {
            assert_eq!(store.load()?.ok_or("saved")?.client_id, CLIENT);
        } else {
            assert_dynamic_login(&store)?;
        }
    }
    Ok(())
}
#[test]
fn rereview_complete_callback_is_accepted_after_four_or_eight_idle_connections() -> TestResult {
    for count in [4, 8] {
        let fixture = Fixture::new()?;
        let store = fixture.store()?;
        let (mut server, scripts) = MockProvider::start_channel()?;
        scripts.send(ResponseScript::json(200, &discovery(&server, &server))?)?;
        let result = login_with(
            &store,
            &endpoints(&server),
            move |url| {
                let fields = form(url.split_once('?').ok_or("query")?.1)?;
                let redirect = get(&fields, "redirect_uri")?;
                let uri: benitoiteh::http::Uri = redirect.parse()?;
                let address = uri.authority().ok_or("host")?.as_str();
                let mut idle = Vec::new();
                for _ in 0..count {
                    idle.push(TcpStream::connect(address)?);
                }
                // 全ての無送信の接続を保持したまま、正しい要求の応答を受け取る。
                assert_eq!(
                    callback(
                        redirect,
                        "GET",
                        &format!(
                            "/auth/callback?state={}&error=access_denied",
                            get(&fields, "state")?
                        ),
                        &format!("Host: {address}")
                    )?,
                    200
                );
                drop(idle);
                Ok(())
            },
            &AtomicBool::new(false),
            Duration::from_secs(30),
        )?;
        assert_eq!(result, Err(AuthError::Rejected("access_denied".into())));
        server.shutdown()?;
    }
    Ok(())
}
