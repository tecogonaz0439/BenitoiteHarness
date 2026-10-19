//! ChatGPT のログインとログアウトを端末から始める（設計書 05-01「login と logout」）。

use super::{CliError, signals::Signals};
use crate::auth::{self, AuthError, Endpoints, LogoutOutcome, Store, text};
use std::ffi::OsString;
use std::io::{self, Write};
use std::process::{Command, Stdio};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub(super) fn run(args: impl Iterator<Item = OsString>, logout: bool) -> Result<u8, CliError> {
    let mut args = args;
    if args.next().as_deref() != Some(std::ffi::OsStr::new("chatgpt")) || args.next().is_some() {
        return Err(CliError::Usage);
    }
    let paths = crate::config::paths::DataPaths::from_env()
        .map_err(|_| CliError::Auth(AuthError::Storage))?;
    let store = Store::open(&paths.auth_dir).map_err(CliError::Auth)?;
    let endpoints = Endpoints::official();
    if logout {
        let (message, status) =
            match auth::logout(&store, &endpoints, now()).map_err(CliError::Auth)? {
                LogoutOutcome::NotSignedIn => (text::NOT_SIGNED_IN, 0),
                LogoutOutcome::Revoked => (text::SIGNED_OUT, 0),
                LogoutOutcome::Unconfirmed => (text::REVOCATION_UNCONFIRMED, 1),
            };
        writeln!(io::stderr().lock(), "{message}").map_err(CliError::Output)?;
        return Ok(status);
    }
    let signals = Signals::new().map_err(CliError::Output)?;
    let result = auth::login(
        &store,
        &endpoints,
        open_url,
        Duration::from_secs(600),
        now,
        &signals.cancelled,
    );
    let account = match result {
        Err(AuthError::Cancelled) => return Ok(130),
        result => result.map_err(CliError::Auth)?,
    };
    let message = account
        .email
        .as_deref()
        .map(text::signed_in)
        .unwrap_or_else(|| text::SIGNED_IN.into());
    writeln!(
        io::stderr().lock(),
        "{}",
        crate::ui::terminal_text(&message, false)
    )
    .map_err(CliError::Output)?;
    Ok(0)
}
fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
fn open_url(url: &str) {
    // URL は失敗時だけでなく常に示し、ブラウザが起動しない環境でも続けられるようにする。
    let _shown = writeln!(
        io::stderr().lock(),
        "{}\n{url}\n{}",
        text::WAITING,
        text::CANCEL
    );
    #[cfg(target_os = "macos")]
    let mut command = Command::new("/usr/bin/open");
    #[cfg(not(target_os = "macos"))]
    let mut command = Command::new("xdg-open");
    for variable in crate::config::PROVIDER_KEY_ENV_VARS {
        command.env_remove(variable);
    }
    if let Ok(mut child) = command
        .arg(url)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    {
        // xdg-open が戻らなくても戻り先の受け付けを止めない。
        let _reaper = std::thread::Builder::new().spawn(move || {
            let _status = child.wait();
        });
    }
}
