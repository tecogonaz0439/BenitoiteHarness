//! 補助との長さ付き要求と、秘密を JSON に入れない応答（設計書 03-04「段 2」）。
use serde::{Deserialize, Serialize};
use std::io::{self, Read, Write};
use std::os::unix::net::UnixStream;
use std::time::{Duration, Instant};
use zeroize::Zeroizing;

pub(super) const MAX_REQUEST: usize = 8192;
pub const MAX_SECRET: usize = 4096;

/// 起動の名前が報告した、秘密を求める仕組み。
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Ssh,
    Git,
    Sudo,
}
impl Kind {
    /// 求めの種類の、記録に使う固定の名前。
    pub fn name(self) -> &'static str {
        match self {
            Self::Ssh => "ssh",
            Self::Git => "git",
            Self::Sudo => "sudo",
        }
    }
    /// argv[0] で区別する補助の固定の名前。
    pub fn helper(self) -> &'static str {
        match self {
            Self::Ssh => "benitoiteh-ssh-askpass",
            Self::Git => "benitoiteh-git-askpass",
            Self::Sudo => "benitoiteh-sudo-askpass",
        }
    }
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Request {
    pub v: u8,
    pub token: String,
    pub kind: Kind,
    pub ssh_prompt: Option<String>,
    pub prompt: String,
    pub pid: u32,
    pub ppid: u32,
}
impl Drop for Request {
    fn drop(&mut self) {
        zeroize::Zeroize::zeroize(&mut self.token);
    }
}

pub(super) fn read_request(
    stream: &mut UnixStream,
    stopped: impl Fn() -> bool,
) -> io::Result<Request> {
    let mut reader = DeadlineReader {
        stream,
        started: Instant::now(),
        stopped,
    };
    let mut size = [0; 4];
    reader.read_exact(&mut size)?;
    let size = usize::try_from(u32::from_be_bytes(size)).map_err(io::Error::other)?;
    if size > MAX_REQUEST {
        return Err(io::ErrorKind::InvalidData.into());
    }
    let mut bytes = Zeroizing::new(vec![0; size]);
    reader.read_exact(&mut bytes)?;
    let request: Request = serde_json::from_slice(&bytes).map_err(io::Error::other)?;
    if request.v != 1
        || request.prompt.len() > MAX_SECRET
        || request.token.len() != 64
        || !request.token.bytes().all(|b| b.is_ascii_hexdigit())
        || !matches!(
            request.ssh_prompt.as_deref(),
            None | Some("confirm" | "none")
        )
        || (request.kind != Kind::Ssh && request.ssh_prompt.is_some())
    {
        return Err(io::ErrorKind::InvalidData.into());
    }
    Ok(request)
}

// read_exact は短い読み込みを繰り返す。各 read の上限だけでは、少しずつ
// 送る相手が窓口を占有できるため、要求全体の期限から毎回残りを計算する。
struct DeadlineReader<'a, F> {
    stream: &'a mut UnixStream,
    started: Instant,
    stopped: F,
}

// OS が保持する値には丸めがあるため、設定する区切りはここで決める。
fn read_slice(remaining: Duration) -> Duration {
    remaining.min(Duration::from_millis(50))
}

impl<F: Fn() -> bool> Read for DeadlineReader<'_, F> {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        loop {
            if (self.stopped)() {
                return Err(io::ErrorKind::ConnectionAborted.into());
            }
            let remaining = Duration::from_secs(5).saturating_sub(self.started.elapsed());
            if remaining.is_zero() {
                return Err(io::ErrorKind::TimedOut.into());
            }
            self.stream.set_read_timeout(Some(read_slice(remaining)))?;
            match self.stream.read(bytes) {
                Err(error)
                    if matches!(
                        error.kind(),
                        io::ErrorKind::TimedOut
                            | io::ErrorKind::WouldBlock
                            | io::ErrorKind::Interrupted
                    ) => {}
                result => return result,
            }
        }
    }
}

pub(super) fn write_request(writer: &mut impl Write, request: &Request) -> io::Result<()> {
    let bytes = Zeroizing::new(serde_json::to_vec(request).map_err(io::Error::other)?);
    let size = u32::try_from(bytes.len()).map_err(io::Error::other)?;
    writer.write_all(&size.to_be_bytes())?;
    writer.write_all(&bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    // 期限が長い要求でも、一回の read が持つ OS の期限を短く保つ。
    // 待ち時間を測らず、実ソケットの設定と停止の観測回数を確かめる。
    #[test]
    fn partial_request_bounds_each_socket_read_and_checks_stop_again() -> io::Result<()> {
        for remaining in [
            Duration::from_nanos(1),
            Duration::from_millis(1),
            Duration::from_millis(50),
            Duration::from_secs(5),
        ] {
            let timeout = read_slice(remaining);
            assert!(!timeout.is_zero() && timeout <= Duration::from_millis(50));
            assert!(timeout <= remaining);
        }
        let (mut client, mut server) = UnixStream::pair()?;
        let probe = server.try_clone()?;
        client.write_all(&[0, 0])?;
        let checks = Cell::new(0usize);
        let result = read_request(&mut server, || {
            checks.set(checks.get().saturating_add(1));
            if checks.get() > 1 {
                // Linux は SO_RCVTIMEO を時計の刻みに丸める。設定値の上限と
                // OS から読み返した値を分け、ここには丸めの余裕を持たせる。
                assert!(
                    matches!(probe.read_timeout(), Ok(Some(timeout)) if !timeout.is_zero() && timeout <= Duration::from_millis(100))
                );
                true
            } else {
                false
            }
        });
        assert!(matches!(result, Err(error) if error.kind() == io::ErrorKind::ConnectionAborted));
        assert_eq!(checks.get(), 2);
        Ok(())
    }
}
