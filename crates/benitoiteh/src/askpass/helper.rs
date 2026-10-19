//! argv[0] で分けた補助。標準入力と利用者の設定を読まない（設計書 03-04「段 2」）。
use super::{
    AskpassError, Kind, SOCKET_ENV, TOKEN_ENV,
    protocol::{self, Request},
};
use std::ffi::OsStr;
use std::io::{self, Read, Write};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::process::ExitCode;
use zeroize::Zeroizing;

/// 起動したパスの末尾だけで、補助の種類を判定する。
pub fn kind(name: &OsStr) -> Option<Kind> {
    let name = Path::new(name).file_name()?;
    [Kind::Ssh, Kind::Git, Kind::Sudo]
        .into_iter()
        .find(|kind| name == kind.helper())
}

/// 設定と標準入力を読まず、窓口の応答を終了状態と標準出力に返す。
pub fn run(kind: Kind) -> ExitCode {
    match exchange(kind) {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            let _written = writeln!(io::stderr().lock(), "{message}");
            ExitCode::FAILURE
        }
    }
}
fn exchange(kind: Kind) -> Result<(), AskpassError> {
    let socket = std::env::var_os(SOCKET_ENV)
        .filter(|v| !v.is_empty())
        .ok_or(AskpassError::Unavailable)?;
    let token = std::env::var(TOKEN_ENV)
        .ok()
        .filter(|v| !v.is_empty())
        .ok_or(AskpassError::Unavailable)?;
    let ssh_prompt = if kind == Kind::Ssh {
        std::env::var("SSH_ASKPASS_PROMPT").ok()
    } else {
        None
    };
    if !matches!(ssh_prompt.as_deref(), None | Some("confirm" | "none")) {
        return Err(AskpassError::Refused);
    }
    let prompt = std::env::args_os().nth(1).unwrap_or_default();
    let bytes = prompt
        .as_bytes()
        .get(..prompt.as_bytes().len().min(4096))
        .unwrap_or_default();
    let mut prompt = String::from_utf8_lossy(bytes).into_owned();
    while prompt.len() > 4096 {
        prompt.pop();
    }
    let request = Request {
        v: 1,
        token,
        kind,
        ssh_prompt,
        prompt,
        pid: std::process::id(),
        ppid: rustix::process::getppid().map_or(0, |pid| pid.as_raw_nonzero().get().unsigned_abs()),
    };
    let touch = request.ssh_prompt.as_deref() == Some("none");
    let mut stream = UnixStream::connect(socket).map_err(|_| AskpassError::Closed)?;
    protocol::write_request(&mut stream, &request).map_err(|_| AskpassError::Closed)?;
    let mut status = [0];
    let count = stream.read(&mut status).map_err(|_| AskpassError::Closed)?;
    if touch && count == 0 {
        return Ok(());
    }
    match status {
        [b'Y'] if count == 1 => Ok(()),
        [b'S'] if count == 1 && !touch && request.ssh_prompt.is_none() => {
            let secret = read_secret(&mut stream)?;
            let mut output = io::stdout().lock();
            output
                .write_all(&secret)
                .and_then(|()| output.write_all(b"\n"))
                .map_err(|_| AskpassError::Closed)
        }
        [_] => Err(AskpassError::Refused),
    }
}

// 上限 + 1 を一度だけ確保し、増設時に平文の古い領域を残さない。
fn read_secret(reader: &mut impl Read) -> Result<Zeroizing<Vec<u8>>, AskpassError> {
    let limit = protocol::MAX_SECRET.saturating_add(1);
    let mut secret = Zeroizing::new(Vec::with_capacity(limit));
    let mut buffer = Zeroizing::new([0u8; 512]);
    while secret.len() < limit {
        let capacity = buffer.len().min(limit.saturating_sub(secret.len()));
        let bytes = buffer.get_mut(..capacity).ok_or(AskpassError::Refused)?;
        let count = reader.read(bytes).map_err(|_| AskpassError::Closed)?;
        if count == 0 {
            break;
        }
        secret.extend_from_slice(bytes.get(..count).ok_or(AskpassError::Refused)?);
    }
    if secret.len() > protocol::MAX_SECRET || secret.iter().any(|b| matches!(b, 0 | b'\r' | b'\n'))
    {
        return Err(AskpassError::Refused);
    }
    Ok(secret)
}

#[cfg(test)]
mod tests {
    use super::*;
    // 関門: 上限いっぱいの秘密でも平文の領域を再確保しない資源の契約を守る。
    #[test]
    fn helper_secret_capacity_is_reserved_before_reading_and_oversize_is_refused()
    -> Result<(), AskpassError> {
        let bytes = vec![b'x'; protocol::MAX_SECRET];
        let secret = read_secret(&mut bytes.as_slice())?;
        assert_eq!(secret.as_slice(), bytes);
        assert!(secret.capacity() >= protocol::MAX_SECRET.saturating_add(1));
        for bytes in [
            vec![b'x'; protocol::MAX_SECRET.saturating_add(1)],
            b"x\ny".to_vec(),
            b"x\0y".to_vec(),
        ] {
            assert!(matches!(
                read_secret(&mut bytes.as_slice()),
                Err(AskpassError::Refused)
            ));
        }
        Ok(())
    }
}
