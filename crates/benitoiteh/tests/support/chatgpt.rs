//! ChatGPT の要求と診断の試験に使う偽の保存（実装プラン P5-22「テスト」）。

use benitoiteh::auth::{Account, Credentials, Endpoints, Secret, Tokens};

pub const ACCESS: &str = "synthetic-chatgpt-access-never-log";
pub const REFRESH: &str = "synthetic-chatgpt-refresh-never-log";
pub const NEW_ACCESS: &str = "synthetic-chatgpt-new-access-never-log";

pub fn credentials(expires_at: u64) -> Credentials {
    Credentials {
        version: 1,
        client_id: "oaiapp_test".into(),
        client_confirmed: true,
        ext_agent_host_id: "urn:uuid:01234567-89ab-4cde-8fab-0123456789ab".into(),
        account: Some(Account {
            email: Some("test@example.invalid".into()),
            subject: "synthetic-subject".into(),
        }),
        tokens: Some(Tokens {
            access_token: Secret::new(ACCESS.into()),
            refresh_token: Secret::new(REFRESH.into()),
            expires_at,
            earliest_refresh_at: None,
            scopes: vec!["chatgpt.tokens.use.direct".into()],
        }),
        saved_at: 1_700_000_000,
    }
}

pub fn endpoints(base_url: &str) -> Endpoints {
    Endpoints {
        authorize: format!("{base_url}/authorize"),
        token: format!("{base_url}/token"),
        discovery: format!("{base_url}/discovery"),
        issuer: base_url.into(),
    }
}
