//! ループバックの戻り先を制限し、期限と取り消しを監視する（設計書 02-01「Sign in with ChatGPT」）。

use super::{AuthError, constant_time_eq, decode_form, text};
use rustix::event::{PollFd, PollFlags, Timespec, poll};
use std::collections::{BTreeMap, VecDeque};
use std::io::{Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

const MAX_READERS: usize = 64;
const READ_LIMIT: Duration = Duration::from_secs(5);
const HEAD_LIMIT: usize = 8192;
type Fields = BTreeMap<String, String>;
struct Reader {
    socket: TcpStream,
    bytes: Vec<u8>,
    accepted_at: Instant,
}
enum Head {
    Waiting,
    Progress,
    Complete(Fields),
}
pub(super) fn wait(
    listener: &TcpListener,
    state: &str,
    timeout: Duration,
    cancelled: &AtomicBool,
) -> Result<Fields, AuthError> {
    listener
        .set_nonblocking(true)
        .map_err(|_| AuthError::Communication)?;
    let start = Instant::now();
    let host = listener
        .local_addr()
        .map_err(|_| AuthError::Communication)?
        .to_string();
    let mut readers: VecDeque<Reader> = VecDeque::new();
    loop {
        if cancelled.load(Ordering::Acquire) {
            return Err(AuthError::Cancelled);
        }
        if start.elapsed() >= timeout {
            return Err(AuthError::Timeout);
        }
        let budget = readers.iter().fold(
            timeout
                .saturating_sub(start.elapsed())
                .min(Duration::from_millis(50)),
            |budget, reader| budget.min(READ_LIMIT.saturating_sub(reader.accepted_at.elapsed())),
        );
        let poll_timeout = Timespec::try_from(budget).map_err(|_| AuthError::Communication)?;
        let ready = {
            let mut fds = Vec::with_capacity(readers.len().saturating_add(1));
            fds.push(PollFd::new(listener, PollFlags::IN));
            fds.extend(
                readers
                    .iter()
                    .map(|reader| PollFd::new(&reader.socket, PollFlags::IN)),
            );
            match poll(&mut fds, Some(&poll_timeout)) {
                Ok(_) => {}
                Err(rustix::io::Errno::INTR) => continue,
                Err(_) => return Err(AuthError::Communication),
            }
            fds.iter()
                .map(|fd| !fd.revents().is_empty())
                .collect::<Vec<_>>()
        };
        if cancelled.load(Ordering::Acquire) {
            return Err(AuthError::Cancelled);
        }
        if start.elapsed() >= timeout {
            return Err(AuthError::Timeout);
        }
        // 既存の接続を先に読み、受け付けによる追い出しと戻りの完了を競合させない
        // （設計書 02-01「ログイン」）。
        for readable in ready.iter().skip(1) {
            let Some(mut reader) = readers.pop_front() else {
                break;
            };
            if reader.accepted_at.elapsed() >= READ_LIMIT {
                close_with_reply(&mut reader.socket, 400);
                continue;
            }
            if *readable {
                match read_available(&mut reader, &host, state) {
                    Ok(Head::Complete(fields)) => {
                        close_with_reply(&mut reader.socket, 200);
                        return Ok(fields);
                    }
                    Err(status) => {
                        close_with_reply(&mut reader.socket, status);
                        continue;
                    }
                    Ok(Head::Waiting | Head::Progress) => {}
                }
            }
            readers.push_back(reader);
        }
        if !ready.first().copied().unwrap_or(false) {
            continue;
        }
        let accepting = Instant::now();
        while let Some(socket) = accepted(listener.accept())? {
            if socket.set_nonblocking(true).is_err() {
                continue;
            }
            readers.push_back(Reader {
                socket,
                bytes: Vec::new(),
                accepted_at: Instant::now(),
            });
            if readers.len() > MAX_READERS {
                let _oldest = readers.pop_front();
            }
            // 受け付けが続いても、期限と取り消しを見る間隔を超えて回し続けない。
            if cancelled.load(Ordering::Acquire) {
                return Err(AuthError::Cancelled);
            }
            if accepting.elapsed() >= Duration::from_millis(50) {
                break;
            }
        }
    }
}
fn read_available(reader: &mut Reader, host: &str, state: &str) -> Result<Head, u16> {
    loop {
        match read_head(&mut reader.socket, &mut reader.bytes, host, state)? {
            Head::Progress => continue,
            result @ (Head::Waiting | Head::Complete(_)) => return Ok(result),
        }
    }
}
fn close_with_reply(socket: &mut TcpStream, status: u16) {
    if socket.set_nonblocking(false).is_ok() {
        let _sent = reply(socket, status);
    }
    let _closed = socket.shutdown(Shutdown::Write);
}
fn read_head(
    socket: &mut TcpStream,
    bytes: &mut Vec<u8>,
    host: &str,
    state: &str,
) -> Result<Head, u16> {
    let remaining = HEAD_LIMIT.saturating_sub(bytes.len());
    if remaining == 0 {
        return Err(400);
    }
    let mut buffer = [0; 1024];
    let available = remaining.min(buffer.len());
    match socket.read(buffer.get_mut(..available).ok_or(400u16)?) {
        Ok(0) => Err(400),
        Ok(count) => {
            bytes.extend_from_slice(buffer.get(..count).ok_or(400u16)?);
            if let Some(position) = bytes.windows(4).position(|value| value == b"\r\n\r\n") {
                let end = position.saturating_add(4);
                parse(bytes.get(..end).ok_or(400u16)?, host, state).map(Head::Complete)
            } else if bytes.len() >= HEAD_LIMIT {
                Err(400)
            } else {
                Ok(Head::Progress)
            }
        }
        Err(error)
            if matches!(
                error.kind(),
                std::io::ErrorKind::WouldBlock
                    | std::io::ErrorKind::TimedOut
                    | std::io::ErrorKind::Interrupted
            ) =>
        {
            Ok(Head::Waiting)
        }
        Err(_) => Err(400),
    }
}
fn parse(bytes: &[u8], host: &str, state: &str) -> Result<Fields, u16> {
    let text = std::str::from_utf8(bytes).map_err(|_| 400u16)?;
    let mut lines = text.split("\r\n");
    let mut first = lines.next().ok_or(400u16)?.split_whitespace();
    if first.next() != Some("GET") {
        return Err(405);
    }
    let target = first.next().ok_or(400u16)?;
    if first.next() != Some("HTTP/1.1") || first.next().is_some() {
        return Err(400);
    }
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    if path != "/auth/callback" {
        return Err(404);
    }
    let mut hosts = Vec::new();
    for line in lines.take_while(|line| !line.is_empty()) {
        let (name, value) = line.split_once(':').ok_or(400u16)?;
        if name.eq_ignore_ascii_case("host") {
            hosts.push(value.trim());
        }
    }
    if hosts.len() != 1 || hosts.first().copied() != Some(host) {
        return Err(400);
    }
    let fields = decode_form(query).map_err(|_| 400u16)?;
    if !fields
        .get("state")
        .is_some_and(|value| constant_time_eq(value.as_bytes(), state.as_bytes()))
    {
        return Err(400);
    }
    Ok(fields)
}
fn reply(socket: &mut TcpStream, status: u16) -> std::io::Result<()> {
    socket.set_write_timeout(Some(Duration::from_millis(100)))?;
    let body = if status == 200 { text::CALLBACK } else { "" };
    write!(
        socket,
        "HTTP/1.1 {status} Callback\r\nContent-Length: {}\r\nContent-Type: text/plain; charset=utf-8\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
}
// accept の結果を、接続・短い休みを挟む再試行・終了の三つに分ける。
fn accepted(
    result: std::io::Result<(TcpStream, std::net::SocketAddr)>,
) -> Result<Option<TcpStream>, AuthError> {
    match result {
        Ok((socket, _)) => Ok(Some(socket)),
        Err(error) if transient_accept(&error) => Ok(None),
        Err(_) => Err(AuthError::Communication),
    }
}
fn transient_accept(error: &std::io::Error) -> bool {
    use rustix::io::Errno;
    if matches!(
        error.kind(),
        std::io::ErrorKind::WouldBlock
            | std::io::ErrorKind::Interrupted
            | std::io::ErrorKind::ConnectionAborted
    ) {
        return true;
    }
    let code = error.raw_os_error();
    let common = [
        Errno::CONNABORTED,
        Errno::PROTO,
        Errno::NETDOWN,
        Errno::NOPROTOOPT,
        Errno::HOSTDOWN,
        Errno::HOSTUNREACH,
        Errno::OPNOTSUPP,
        Errno::NETUNREACH,
        Errno::MFILE,
        Errno::NFILE,
    ];
    if common
        .iter()
        .any(|errno| code == Some(errno.raw_os_error()))
    {
        return true;
    }
    #[cfg(target_os = "linux")]
    if code == Some(Errno::NONET.raw_os_error()) {
        return true;
    }
    false
}
#[cfg(test)]
mod tests;
