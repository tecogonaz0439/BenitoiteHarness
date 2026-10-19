//! 開いた inode を残して rename し、読み直しと掃除の失敗を順序で再現する。

use super::*;
use crate::auth::{self, Endpoints};
use serde_json::json;
use std::os::unix::fs::PermissionsExt;

type TestResult = Result<(), Box<dyn std::error::Error>>;
struct Fixture(std::path::PathBuf);
impl Fixture {
    fn new() -> Result<Self, AuthError> {
        let mut bytes = [0; 16];
        getrandom::fill(&mut bytes).map_err(|_| AuthError::Random)?;
        let path =
            std::env::temp_dir().join(format!("benitoiteh-store-{}", base64url::encode(&bytes)));
        std::fs::create_dir(&path)?;
        Ok(Self(path))
    }
    fn store(&self) -> Result<Store, AuthError> {
        Store::open(&self.0.join("data/auth"))
    }
    fn file(&self) -> std::path::PathBuf {
        self.0.join("data/auth/chatgpt.json")
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _removed = std::fs::remove_dir_all(&self.0);
    }
}
fn credentials(access: &str) -> Result<Credentials, serde_json::Error> {
    serde_json::from_value(
        json!({"version":1,"client_id":"oaiapp_test","client_confirmed":true,"ext_agent_host_id":"host","account":null,"tokens":{"access_token":access,"refresh_token":"refresh","expires_at":10000,"earliest_refresh_at":null,"scopes":["chatgpt.tokens.use.direct"]},"saved_at":100}),
    )
}
fn endpoints() -> Endpoints {
    Endpoints {
        authorize: "http://127.0.0.1:1/authorize".into(),
        token: "http://127.0.0.1:1/token".into(),
        discovery: "http://127.0.0.1:1/discovery".into(),
        issuer: "test".into(),
    }
}
// 作成時の関門: 非公開の syscall の代わりを作らず、本物の unlink 済み inode を読む。
#[test]
fn rereview_unlinked_snapshot_retries_once_under_the_lock() -> TestResult {
    let fixture = Fixture::new()?;
    let store = fixture.store()?;
    store.lock()?.save(&credentials("before")?)?;
    let before = File::open(fixture.file())?;
    store.lock()?.save(&credentials("after")?)?;
    assert_eq!(before.metadata()?.nlink(), 0);
    let first = read_credentials(before);
    assert!(matches!(first, Err(AuthError::Storage)));
    assert_eq!(
        auth::access_token_with_snapshot(&store, &endpoints(), 100, first)?.expose_secret(),
        "after"
    );
    // 再読しても権限が不正なら、さらに繰り返さず誤りを返す。
    std::fs::set_permissions(fixture.file(), std::fs::Permissions::from_mode(0o644))?;
    assert_eq!(
        auth::access_token(&store, &endpoints(), 100).err(),
        Some(AuthError::Storage)
    );
    Ok(())
}
#[test]
fn rereview_undeletable_temporary_file_does_not_prevent_locking() -> TestResult {
    if rustix::process::geteuid().is_root() {
        eprintln!(
            "Skipped auth temporary file cleanup permission check: root can remove files in mode 0500 directories."
        );
        return Ok(());
    }
    let fixture = Fixture::new()?;
    let store = fixture.store()?;
    store.lock()?.save(&credentials("access")?)?;
    let directory = fixture
        .file()
        .parent()
        .ok_or("auth directory")?
        .to_path_buf();
    let orphan = directory.join(".chatgpt-undeletable.tmp");
    std::fs::write(&orphan, "test secret")?;
    std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o500))?;
    let result = store.lock();
    // 失敗したテストでも、削除のための権限は先に戻す。
    std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o700))?;
    let locked = result?;
    assert!(orphan.is_file());
    assert!(locked.load()?.is_some());
    locked.save(&credentials("still usable")?)?;
    drop(locked);
    drop(store.lock()?);
    assert!(!orphan.exists());
    Ok(())
}
