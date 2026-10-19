//! ループバックの戻り先を制限し、期限と取り消しを監視する（設計書 02-01「Sign in with ChatGPT」）。

use super::{AuthError, constant_time_eq, decode_form, text};
use std::collections::{BTreeMap, VecDeque};
use std::io::{Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::time::{Duration, Instant};

const MAX_READERS: usize = 4;
const HEAD_LIMIT: usize = 8192;
type Fields = BTreeMap<String, String>;
struct Reader {
    socket: TcpStream,
    worker: std::thread::JoinHandle<()>,
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
    let (sender, receiver) = mpsc::sync_channel(MAX_READERS);
    let mut readers: VecDeque<Reader> = VecDeque::new();
    loop {
        if cancelled.load(Ordering::Acquire) {
            return Err(AuthError::Cancelled);
        }
        if start.elapsed() >= timeout {
            return Err(AuthError::Timeout);
        }
        if let Ok((mut socket, fields)) = receiver.try_recv() {
            let _sent = reply(&mut socket, 200);
            return Ok(fields);
        }
        readers.retain(|reader| !reader.worker.is_finished());
        let Some(mut socket) = accepted(listener.accept())? else {
            std::thread::sleep(Duration::from_millis(50));
            continue;
        };
        // macOS で引き継いだ指定をまず解除し、失敗した接続は使わない。
        if socket.set_nonblocking(false).is_err() {
            continue;
        }
        if socket.set_nonblocking(true).is_err() {
            continue;
        }
        let mut bytes = Vec::new();
        // 満杯でも、既に届いている完全な戻りを捨てない。非ブロックで頭だけ読む。
        let initial = loop {
            match read_head(&mut socket, &mut bytes, &host, state) {
                Ok(Head::Progress) => continue,
                result => break result,
            }
        };
        if socket.set_nonblocking(false).is_err() {
            continue;
        }
        match initial {
            Ok(Head::Complete(fields)) => {
                let _sent = reply(&mut socket, 200);
                return Ok(fields);
            }
            Err(status) => {
                let _sent = reply(&mut socket, status);
                continue;
            }
            Ok(Head::Waiting | Head::Progress) => {}
        }
        // 未完の接続だけをスレッドへ回す。最も古い未完の読み取りを終了して場所を空ける。
        if readers.len() >= MAX_READERS
            && let Some(reader) = readers.pop_front()
        {
            let _closed = reader.socket.shutdown(Shutdown::Both);
            let _finished = reader.worker.join();
        }
        let control = match socket.try_clone() {
            Ok(control) => control,
            Err(_) => continue,
        };
        let budget = timeout
            .saturating_sub(start.elapsed())
            .min(Duration::from_secs(1));
        let sender = sender.clone();
        let host = host.clone();
        let state = state.to_owned();
        if let Ok(worker) = std::thread::Builder::new().spawn(move || {
            match read(&mut socket, &host, &state, budget, bytes) {
                Ok(fields) => {
                    let _sent = sender.send((socket, fields));
                }
                Err(status) => {
                    let _sent = reply(&mut socket, status);
                }
            }
        }) {
            readers.push_back(Reader {
                socket: control,
                worker,
            });
        }
    }
}
fn read(
    socket: &mut TcpStream,
    host: &str,
    state: &str,
    budget: Duration,
    mut bytes: Vec<u8>,
) -> Result<Fields, u16> {
    let start = Instant::now();
    socket
        .set_read_timeout(Some(Duration::from_millis(50)))
        .map_err(|_| 400u16)?;
    loop {
        if start.elapsed() >= budget {
            return Err(400);
        }
        match read_head(socket, &mut bytes, host, state)? {
            Head::Complete(fields) => return Ok(fields),
            Head::Progress => {}
            Head::Waiting => std::thread::park_timeout(Duration::from_millis(1)),
        }
    }
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
