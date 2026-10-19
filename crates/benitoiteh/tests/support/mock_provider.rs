//! API の形に依らない台本付き HTTP/1.1 サーバー（設計書 07-01「模擬の提供者」）。

use std::collections::HashMap;
use std::io::{self, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use serde_json::Value;

const TICK: Duration = Duration::from_millis(5);
const REQUEST_LIMIT: usize = 16 * 1024 * 1024;

/// 台本の本文を渡す方法。
pub enum Delivery {
    /// 本文をまとめて送る。
    Once,
    /// バイト数で分け、塊の間に指定した時間を置く。
    Chunks { size: usize, delay: Duration },
    /// 本文の指定したバイト数だけ送り、接続を閉じる。
    Disconnect { after: usize },
    /// ヘッダだけ送り、サーバーが破棄されるまで待つ。
    HeadersOnly,
}

/// 要求の到着順に消費する一回の応答。
pub struct ResponseScript {
    /// HTTP の状態。
    pub status: u16,
    /// Content-Length と Connection 以外のヘッダ。
    pub headers: Vec<(String, String)>,
    /// SSE または JSON などの、API に依らないバイト列。
    pub body: Vec<u8>,
    /// 送り方。
    pub delivery: Delivery,
}

impl ResponseScript {
    /// 一度に送る SSE の成功応答を作る。
    pub fn sse(body: impl Into<Vec<u8>>) -> Self {
        Self {
            status: 200,
            headers: vec![("content-type".into(), "text/event-stream".into())],
            body: body.into(),
            delivery: Delivery::Once,
        }
    }

    /// JSON の HTTP 誤りなどを返す応答を作る。
    pub fn json(status: u16, body: &Value) -> io::Result<Self> {
        Ok(Self {
            status,
            headers: vec![("content-type".into(), "application/json".into())],
            body: serde_json::to_vec(body)?,
            delivery: Delivery::Once,
        })
    }
}

/// テストの中だけで読む要求。キーを含むヘッダと本文を Debug に出さない。
#[derive(Clone)]
pub struct RecordedRequest {
    /// 接続を受け付けた直後の時刻。
    #[allow(dead_code)]
    pub accepted_at: Instant,
    /// 要求のバイト列を読み終えた時刻。
    #[allow(dead_code)]
    pub received_at: Instant,
    /// HTTP のメソッド。
    pub method: String,
    /// 要求のパス。
    pub path: String,
    /// 小文字の名前を使うヘッダの表。認証の値をテストの外へ出さないこと。
    pub headers: HashMap<String, String>,
    /// 受信した JSON。
    pub body: Value,
    /// フォームも含めた生の本文。
    #[allow(dead_code)]
    pub raw_body: Vec<u8>,
}

impl std::fmt::Debug for RecordedRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RecordedRequest").finish_non_exhaustive()
    }
}

/// 所有するテストの終わりに停止し、スレッドを回収するサーバー。
pub struct MockProvider {
    address: SocketAddr,
    stop: Arc<AtomicBool>,
    requests: Arc<Mutex<Vec<RecordedRequest>>>,
    #[allow(dead_code)]
    chunk_times: Arc<Mutex<Vec<Vec<Instant>>>>,
    worker: Option<JoinHandle<io::Result<()>>>,
}

impl MockProvider {
    /// 手元の空いたポートで台本を順に返す。キーや受け取った本文を出力しない。
    pub fn start(scripts: Vec<ResponseScript>) -> io::Result<Self> {
        let (sender, receiver) = mpsc::channel();
        for script in scripts {
            sender.send(script).map_err(|_| io::ErrorKind::Other)?;
        }
        drop(sender);
        Self::start_receiving(receiver)
    }

    /// 認可の nonce を受け取った後などに、次の応答を渡せる模擬の口。
    #[allow(dead_code)]
    pub fn start_channel() -> io::Result<(Self, mpsc::Sender<ResponseScript>)> {
        let (sender, receiver) = mpsc::channel();
        Ok((Self::start_receiving(receiver)?, sender))
    }

    fn start_receiving(receiver: mpsc::Receiver<ResponseScript>) -> io::Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let address = listener.local_addr()?;
        listener.set_nonblocking(true)?;
        let stop = Arc::new(AtomicBool::new(false));
        let requests = Arc::new(Mutex::new(Vec::new()));
        let chunk_times = Arc::new(Mutex::new(Vec::new()));
        let thread_chunks = Arc::clone(&chunk_times);
        let thread_stop = Arc::clone(&stop);
        let thread_requests = Arc::clone(&requests);
        let worker = thread::Builder::new().spawn(move || {
            loop {
                if thread_stop.load(Ordering::Acquire) {
                    return Ok(());
                }
                let script = match receiver.recv_timeout(TICK) {
                    Ok(script) => script,
                    Err(mpsc::RecvTimeoutError::Timeout) => continue,
                    Err(mpsc::RecvTimeoutError::Disconnected) => break,
                };
                let mut stream = loop {
                    if thread_stop.load(Ordering::Acquire) {
                        return Ok(());
                    }
                    match listener.accept() {
                        Ok((stream, _)) => break stream,
                        Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                            thread::park_timeout(TICK)
                        }
                        Err(error) => return Err(error),
                    }
                };
                let accepted_at = Instant::now();
                stream.set_read_timeout(Some(Duration::from_millis(20)))?;
                stream.set_write_timeout(Some(Duration::from_secs(1)))?;
                stream.set_nodelay(true)?;
                let request = read_request(&mut stream, &thread_stop, accepted_at)?;
                let Some(request) = request else {
                    return Ok(());
                };
                thread_requests
                    .lock()
                    .map_err(|_| io::Error::from(io::ErrorKind::Other))?
                    .push(request);
                thread_chunks
                    .lock()
                    .map_err(|_| io::ErrorKind::Other)?
                    .push(Vec::new());
                respond(&mut stream, &script, &thread_stop, &thread_chunks)?;
            }
            Ok(())
        })?;
        Ok(Self {
            address,
            stop,
            requests,
            chunk_times,
            worker: Some(worker),
        })
    }

    /// アダプタの送り先に使うベースの URL。
    pub fn base_url(&self) -> String {
        format!("http://{}", self.address)
    }

    /// 今までに受信した要求の写し。認証の値はテストの中だけで扱う。
    pub fn requests(&self) -> io::Result<Vec<RecordedRequest>> {
        self.requests
            .lock()
            .map(|r| r.clone())
            .map_err(|_| io::ErrorKind::Other.into())
    }

    /// 台本ごとの、各塊を書き終えた時刻の写し。
    #[allow(dead_code)]
    pub fn chunk_times(&self) -> io::Result<Vec<Vec<Instant>>> {
        self.chunk_times
            .lock()
            .map(|times| times.clone())
            .map_err(|_| io::ErrorKind::Other.into())
    }

    /// 台本の処理中の失敗も、テストで確かめてから停止できる。
    pub fn shutdown(&mut self) -> io::Result<()> {
        self.stop.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            worker.thread().unpark();
            worker
                .join()
                .map_err(|_| io::Error::from(io::ErrorKind::Other))??;
        }
        Ok(())
    }
}

impl Drop for MockProvider {
    fn drop(&mut self) {
        if self.shutdown().is_err() { /* Drop では診断に要求を出さない。 */ }
    }
}

fn read_request(
    stream: &mut TcpStream,
    stop: &AtomicBool,
    accepted_at: Instant,
) -> io::Result<Option<RecordedRequest>> {
    let started = Instant::now();
    let mut bytes = Vec::new();
    let mut buffer = [0; 4096];
    let mut head_end = None;
    let mut length = 0;
    loop {
        if stop.load(Ordering::Acquire) {
            return Ok(None);
        }
        if started.elapsed() > Duration::from_secs(5) {
            return Err(io::ErrorKind::TimedOut.into());
        }
        match stream.read(&mut buffer) {
            Ok(0) => return Err(io::ErrorKind::UnexpectedEof.into()),
            Ok(n) => bytes.extend_from_slice(buffer.get(..n).ok_or(io::ErrorKind::InvalidData)?),
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                ) =>
            {
                continue;
            }
            Err(error) => return Err(error),
        }
        if bytes.len() > REQUEST_LIMIT {
            return Err(io::ErrorKind::InvalidData.into());
        }
        if head_end.is_none()
            && let Some(position) = bytes.windows(4).position(|b| b == b"\r\n\r\n")
        {
            let end = position.saturating_add(4);
            let head = std::str::from_utf8(bytes.get(..end).ok_or(io::ErrorKind::InvalidData)?)
                .map_err(|_| io::ErrorKind::InvalidData)?;
            length = head
                .lines()
                .filter_map(|l| l.split_once(':'))
                .find(|(name, _)| name.eq_ignore_ascii_case("content-length"))
                .and_then(|(_, value)| value.trim().parse::<usize>().ok())
                .unwrap_or(0);
            head_end = Some(end);
        }
        if let Some(end) = head_end
            && bytes.len() >= end.saturating_add(length)
        {
            let received_at = Instant::now();
            let head = std::str::from_utf8(bytes.get(..end).ok_or(io::ErrorKind::InvalidData)?)
                .map_err(|_| io::ErrorKind::InvalidData)?;
            let mut lines = head.lines();
            let mut first = lines
                .next()
                .ok_or(io::ErrorKind::InvalidData)?
                .split_whitespace();
            let method = first.next().ok_or(io::ErrorKind::InvalidData)?.into();
            let path = first.next().ok_or(io::ErrorKind::InvalidData)?.into();
            let headers = lines
                .filter_map(|l| l.split_once(':'))
                .map(|(k, v)| (k.to_ascii_lowercase(), v.trim().to_owned()))
                .collect();
            let raw_body = bytes
                .get(end..end.saturating_add(length))
                .ok_or(io::ErrorKind::InvalidData)?
                .to_vec();
            let body = serde_json::from_slice(&raw_body).unwrap_or(Value::Null);
            return Ok(Some(RecordedRequest {
                accepted_at,
                received_at,
                method,
                path,
                headers,
                body,
                raw_body,
            }));
        }
    }
}

fn respond(
    stream: &mut TcpStream,
    script: &ResponseScript,
    stop: &AtomicBool,
    chunk_times: &Mutex<Vec<Vec<Instant>>>,
) -> io::Result<()> {
    let length = if matches!(script.delivery, Delivery::HeadersOnly) {
        script.body.len().max(1)
    } else {
        script.body.len()
    };
    write!(
        stream,
        "HTTP/1.1 {} Scripted\r\nContent-Length: {length}\r\nConnection: close\r\n",
        script.status
    )?;
    for (name, value) in &script.headers {
        write!(stream, "{name}: {value}\r\n")?;
    }
    stream.write_all(b"\r\n")?;
    match script.delivery {
        Delivery::Once => stream.write_all(&script.body),
        Delivery::Chunks { size, delay } => {
            for chunk in script.body.chunks(size.max(1)) {
                if stop.load(Ordering::Acquire) {
                    return Ok(());
                }
                stream.write_all(chunk)?;
                let written_at = Instant::now();
                chunk_times
                    .lock()
                    .map_err(|_| io::ErrorKind::Other)?
                    .last_mut()
                    .ok_or(io::ErrorKind::Other)?
                    .push(written_at);
                pause(delay, stop);
            }
            Ok(())
        }
        Delivery::Disconnect { after } => stream.write_all(
            script
                .body
                .get(..after.min(script.body.len()))
                .ok_or(io::ErrorKind::InvalidInput)?,
        ),
        Delivery::HeadersOnly => {
            while !stop.load(Ordering::Acquire) {
                thread::park_timeout(TICK);
            }
            Ok(())
        }
    }
}

fn pause(duration: Duration, stop: &AtomicBool) {
    let started = Instant::now();
    while !stop.load(Ordering::Acquire) && started.elapsed() < duration {
        thread::park_timeout(duration.saturating_sub(started.elapsed()).min(TICK));
    }
}
