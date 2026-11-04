//! サンドボックスの中の通信を、利用者が許したドメインにだけ中継する（設計書 03-02「通信の中継」）。
//!
//! 中継は HTTP のプロキシとして働く。`CONNECT` のトンネルは中身を覗かずに渡し、平文の HTTP の要求は、
//! 要求の行を絶対形から通常の形に直し、プロキシ向けのヘッダを除いてから渡す。どちらも、宛先のドメインを
//! 許したか（利用者の設定の一覧、このセッション、このコマンド）を確かめ、許していなければ利用者に尋ねる。
//! 名前は一度だけ解決し、私的なアドレスや特殊用途のアドレスには中継しない。

mod http;
#[cfg(test)]
mod tests;

use std::collections::BTreeSet;
use std::io::{self, Read, Write};
use std::net::{IpAddr, Shutdown, SocketAddr, TcpListener, TcpStream, ToSocketAddrs};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::JoinHandle;
use std::time::Duration;

use crate::approval::{
    ApprovalFacts, ApprovalKind, ApprovalOutcome, ApprovalPrompt, ApprovalRecord,
};

use super::text;

/// Linux のネットワークの名前空間の中で、補助が待ち受ける固定のポート。名前空間ごとに別なので重ならない。
pub const LINUX_PORT: u16 = 3128;

/// 要求の頭の大きさの上限。
const HEAD_LIMIT: usize = 16 * 1024;
/// 同時に扱う接続の数の上限。
const CONNECTION_LIMIT: usize = 64;
/// 宛先への接続を待つ時間。
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
/// 要求の頭を読み終えるまで待つ時間。一回の読み取りごとでなく、頭の全体に掛ける。
const HEAD_TIMEOUT: Duration = Duration::from_secs(30);
/// セッションの全体で、利用者の答えを待つ接続の数の上限。答えを待つ間は糸と記述子を一つずつ持つので、
/// 呼び出しを重ねても主のプロセスの資源を枯らさないようにする。
const PENDING_LIMIT: usize = 16;
/// 確認の答えと錠を待つ間に、コマンドが終わったかを確かめる間隔。
const DECIDE_POLL: Duration = Duration::from_millis(100);
/// 記録に載せる、正規化できなかったホストの長さの上限。
const INVALID_HOST_LIMIT: usize = 64;

/// 名前の解決。テストと評価の実行器は、手元の模擬のサーバへ向けて差し替える。
pub type Resolve = Arc<dyn Fn(&str, u16) -> io::Result<Vec<SocketAddr>> + Send + Sync>;

/// セッションの間に共有する、ドメインの判断の材料。
pub struct NetworkGate {
    allowed: BTreeSet<String>,
    session: Mutex<BTreeSet<String>>,
    prompts: Mutex<Option<Sender<ApprovalPrompt>>>,
    ask: bool,
    asking: Mutex<()>,
    resolve: Resolve,
    allowed_address: fn(IpAddr) -> bool,
    pending: AtomicUsize,
}

impl std::fmt::Debug for NetworkGate {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NetworkGate")
            .field("allowed", &self.allowed)
            .field("ask", &self.ask)
            .finish_non_exhaustive()
    }
}

impl NetworkGate {
    /// `allowed` は正規化したドメインの完全一致の集合。`ask` が偽なら、一覧にないものを尋ねずに断る。
    pub fn new(allowed: BTreeSet<String>, ask: bool) -> Self {
        Self {
            allowed,
            session: Mutex::new(BTreeSet::new()),
            prompts: Mutex::new(None),
            ask,
            asking: Mutex::new(()),
            resolve: Arc::new(|host, port| (host, port).to_socket_addrs().map(Iterator::collect)),
            allowed_address: crate::tools::address_allowed,
            pending: AtomicUsize::new(0),
        }
    }

    /// 名前の解決とアドレスの判定を差し替える。製品の設定からは使わない。
    pub fn with_upstream(mut self, resolve: Resolve, allowed_address: fn(IpAddr) -> bool) -> Self {
        self.resolve = resolve;
        self.allowed_address = allowed_address;
        self
    }

    /// 対話の画面へ確認を送る口をつなぐ。つながっていなければ、一覧にないドメインを断る。
    pub fn connect_prompts(&self, prompts: Sender<ApprovalPrompt>) {
        *self.prompts.lock().unwrap_or_else(PoisonError::into_inner) = Some(prompts);
    }

    fn connect(&self, host: &str, port: u16) -> Result<TcpStream, Denied> {
        let addresses = (self.resolve)(host, port).map_err(|_| Denied::Unresolved)?;
        if addresses.is_empty() {
            return Err(Denied::Unresolved);
        }
        // 一つでも私的なアドレスを含む名前は、どのアドレスにもつながない（設計書 03-02「通信の中継」）。
        if addresses
            .iter()
            .any(|address| !(self.allowed_address)(address.ip()))
        {
            return Err(Denied::PrivateAddress);
        }
        for address in &addresses {
            if let Ok(stream) = TcpStream::connect_timeout(address, CONNECT_TIMEOUT) {
                return Ok(stream);
            }
        }
        Err(Denied::ConnectFailed)
    }
}

/// 一回のコマンドの間の中継の状態。このコマンドだけに許したドメイン、断ったドメイン、断った記録を持つ。
pub struct ProxyRun {
    gate: Arc<NetworkGate>,
    command: String,
    cwd: PathBuf,
    allowed: Mutex<BTreeSet<String>>,
    declined: Mutex<BTreeSet<String>>,
    denials: Mutex<Vec<String>>,
    active: AtomicUsize,
    finished: AtomicBool,
    token: Option<String>,
}

impl ProxyRun {
    /// 合言葉を求めない中継（Linux の名前空間の中の待ち受け）。
    pub fn new(gate: Arc<NetworkGate>, command: String, cwd: PathBuf) -> Arc<Self> {
        Self::build(gate, command, cwd, None)
    }

    /// 手元のほかのプロセスからも届く待ち受け（macOS）のための、合言葉を求める中継。合言葉は呼び出しごとに
    /// 作る。セッションで一つにすると、コマンドの後に残った背景のプロセスが、後の呼び出しの中継を使えるからである。
    pub fn with_token(
        gate: Arc<NetworkGate>,
        command: String,
        cwd: PathBuf,
        token: String,
    ) -> Arc<Self> {
        Self::build(gate, command, cwd, Some(token))
    }

    fn build(
        gate: Arc<NetworkGate>,
        command: String,
        cwd: PathBuf,
        token: Option<String>,
    ) -> Arc<Self> {
        Arc::new(Self {
            gate,
            command,
            cwd,
            allowed: Mutex::new(BTreeSet::new()),
            declined: Mutex::new(BTreeSet::new()),
            denials: Mutex::new(Vec::new()),
            active: AtomicUsize::new(0),
            finished: AtomicBool::new(false),
            token,
        })
    }

    /// 子に渡すプロキシの URL。合言葉があれば利用者の情報として含める。
    pub fn proxy_url(&self, port: u16) -> String {
        match &self.token {
            Some(token) => format!("http://benitoiteh:{token}@127.0.0.1:{port}"),
            None => format!("http://127.0.0.1:{port}"),
        }
    }

    /// コマンドが終わった印を立てる。後に残った接続は、もう尋ねずに断る。コマンドの後に古い確認へ答えて、
    /// そのドメインをセッションの許可に加えることを防ぐ（設計書 03-02「通信の中継」）。
    pub fn finish(&self) {
        self.finished.store(true, Ordering::Release);
    }

    fn is_finished(&self) -> bool {
        self.finished.load(Ordering::Acquire)
    }

    /// 断った宛先と理由。同じものは一度だけ挙げる。
    pub fn denials(&self) -> Vec<String> {
        self.denials
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    fn deny(&self, host: &str, port: u16, reason: Denied) -> String {
        let line = text::network_denied(host, port, reason.reason());
        let mut denials = self.denials.lock().unwrap_or_else(PoisonError::into_inner);
        if !denials.contains(&line) && denials.len() < 100 {
            denials.push(line.clone());
        }
        line
    }

    fn permitted(&self, host: &str) -> bool {
        self.gate.allowed.contains(host)
            || self
                .gate
                .session
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .contains(host)
            || self
                .allowed
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .contains(host)
    }

    fn decide(&self, host: &str, port: u16) -> Result<(), Denied> {
        if self.permitted(host) {
            return Ok(());
        }
        if self.was_declined(host) {
            return Err(Denied::UserDeclined);
        }
        let prompts = self
            .gate
            .prompts
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        let Some(prompts) = prompts.filter(|_| self.gate.ask) else {
            return Err(Denied::NotAllowed);
        };
        let Some(_pending) = Pending::enter(&self.gate.pending) else {
            return Err(Denied::NotAllowed);
        };
        // 同じドメインへの接続が重なっても、確認は一度だけ出す。錠を待つ間にコマンドが終われば、尋ねない。
        let _asking = loop {
            if self.is_finished() {
                return Err(Denied::NotAllowed);
            }
            match self.gate.asking.try_lock() {
                Ok(guard) => break guard,
                Err(std::sync::TryLockError::Poisoned(poisoned)) => break poisoned.into_inner(),
                Err(std::sync::TryLockError::WouldBlock) => std::thread::sleep(DECIDE_POLL),
            }
        };
        if self.permitted(host) {
            return Ok(());
        }
        // 同じコマンドの中で断ったドメインは、再試行でも尋ね直さない。
        if self.was_declined(host) {
            return Err(Denied::UserDeclined);
        }
        if self.is_finished() {
            return Err(Denied::NotAllowed);
        }
        let (reply, response) = std::sync::mpsc::sync_channel(1);
        let request = ApprovalRecord {
            call_id: format!("network:{host}"),
            kind: ApprovalKind::Network,
            facts: ApprovalFacts {
                domain: Some(host.to_owned()),
                url: Some(format!("{host}:{port}")),
                command: Some(self.command.clone()),
                cwd: Some(self.cwd.clone()),
                ..ApprovalFacts::default()
            },
            justification: None,
            summary: text::network_summary(host, port),
            decision: None,
            user_message: None,
        };
        if prompts.send(ApprovalPrompt { request, reply }).is_err() {
            return Err(Denied::NotAllowed);
        }
        // コマンドが終われば答えを待たない。後から届いた答えは、どの許可にも加えない。
        let answer = loop {
            match response.recv_timeout(DECIDE_POLL) {
                Ok(answer) => break Some(answer),
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) if !self.is_finished() => {}
                Err(_) => break None,
            }
        };
        match answer.map(|answer| answer.outcome) {
            Some(ApprovalOutcome::Approved) => {
                self.allowed
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .insert(host.to_owned());
                Ok(())
            }
            Some(ApprovalOutcome::ApprovedForSession) => {
                self.gate
                    .session
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .insert(host.to_owned());
                Ok(())
            }
            Some(_) => {
                self.declined
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .insert(host.to_owned());
                Err(Denied::UserDeclined)
            }
            None => Err(Denied::UserDeclined),
        }
    }

    fn was_declined(&self, host: &str) -> bool {
        self.declined
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .contains(host)
    }
}

/// 答えを待つ接続の数え。落とすと数から外す。
struct Pending<'a>(&'a AtomicUsize);

impl<'a> Pending<'a> {
    fn enter(count: &'a AtomicUsize) -> Option<Self> {
        if count.fetch_add(1, Ordering::AcqRel) >= PENDING_LIMIT {
            count.fetch_sub(1, Ordering::AcqRel);
            return None;
        }
        Some(Self(count))
    }
}

impl Drop for Pending<'_> {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}

/// 断った理由。道具の結果と、子への 403 の本文に使う。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Denied {
    NotAllowed,
    UserDeclined,
    InvalidHost,
    PrivateAddress,
    Unresolved,
    ConnectFailed,
    TlsWithoutConnect,
    Credentials,
}

impl Denied {
    fn reason(self) -> &'static str {
        match self {
            Self::NotAllowed => text::NETWORK_NOT_ALLOWED,
            Self::UserDeclined => text::NETWORK_USER_DECLINED,
            Self::InvalidHost => text::NETWORK_INVALID_HOST,
            Self::PrivateAddress => text::NETWORK_PRIVATE_ADDRESS,
            Self::Unresolved => text::NETWORK_UNRESOLVED,
            Self::ConnectFailed => text::NETWORK_CONNECT_FAILED,
            Self::TlsWithoutConnect => text::NETWORK_TLS_WITHOUT_CONNECT,
            Self::Credentials => text::NETWORK_CREDENTIALS,
        }
    }
}

/// 待ち受けを受け付け続ける中継。落とすと受け付けをやめる（動いているトンネルは、両端が閉じるまで続く）。
pub struct ProxyServer {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Drop for ProxyServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            let _joined = thread.join();
        }
    }
}

/// 受け付けの糸を起こす。止める印は、短い間隔の待ちで確かめる。待ち受けは、同じ番号の IPv4 と IPv6 の
/// ループバックのように、複数を一つの糸で受け付ける。
pub fn serve(listeners: Vec<TcpListener>, run: Arc<ProxyRun>) -> io::Result<ProxyServer> {
    for listener in &listeners {
        listener.set_nonblocking(true)?;
    }
    let stop = Arc::new(AtomicBool::new(false));
    let stopping = Arc::clone(&stop);
    let thread = std::thread::Builder::new()
        .name("benitoiteh-proxy".into())
        .spawn(move || accept_loop(&listeners, &run, &stopping))?;
    Ok(ProxyServer {
        stop,
        thread: Some(thread),
    })
}

fn accept_loop(listeners: &[TcpListener], run: &Arc<ProxyRun>, stop: &AtomicBool) {
    while !stop.load(Ordering::Acquire) {
        let mut idle = true;
        for listener in listeners {
            match listener.accept() {
                Ok((stream, _)) => {
                    idle = false;
                    accept(stream, run);
                }
                Err(error)
                    if matches!(
                        error.kind(),
                        io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
                    ) => {}
                // 待ち受けが使えなくなったら終える。子からは、つながらない通信として見える。
                Err(_) => return,
            }
        }
        if idle {
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}

fn accept(stream: TcpStream, run: &Arc<ProxyRun>) {
    if run.active.fetch_add(1, Ordering::AcqRel) >= CONNECTION_LIMIT {
        run.active.fetch_sub(1, Ordering::AcqRel);
        let _refused = respond(stream, 503, text::NETWORK_BUSY);
        return;
    }
    let connection = Arc::clone(run);
    let spawned = std::thread::Builder::new()
        .name("benitoiteh-proxy-connection".into())
        .spawn(move || {
            handle(stream, &connection);
            connection.active.fetch_sub(1, Ordering::AcqRel);
        });
    if spawned.is_err() {
        run.active.fetch_sub(1, Ordering::AcqRel);
    }
}

fn handle(mut client: TcpStream, run: &ProxyRun) {
    // 受け付けた接続は、待ち受けの非同期の指定を引き継ぎうるので、同期に戻す。
    if client.set_nonblocking(false).is_err() {
        return;
    }
    let (head, rest) = match read_head(&mut client) {
        Ok(read) => read,
        Err(status) => {
            let _ignored = respond(client, status, text::NETWORK_BAD_REQUEST);
            return;
        }
    };
    // 合言葉は、ほかのどの判断と記録よりも先に確かめる。合言葉を持たない手元のほかのプロセスが、
    // 断った記録を通して道具の結果へ文を差し込めないようにする（設計書 03-02「通信の中継」）。
    if let Some(token) = &run.token
        && !http::head_authorized(&head, token)
    {
        let _ignored = respond(client, 407, text::NETWORK_PROXY_AUTH);
        return;
    }
    let request = match http::Request::parse(&head) {
        Ok(request) => request,
        Err(http::ParseError::TlsWithoutConnect(host, port)) => {
            let line = run.deny(&host, port, Denied::TlsWithoutConnect);
            let _ignored = respond(client, 403, &line);
            return;
        }
        Err(http::ParseError::Credentials(host, port)) => {
            let line = run.deny(&host, port, Denied::Credentials);
            let _ignored = respond(client, 403, &line);
            return;
        }
        Err(http::ParseError::Invalid) => {
            let _ignored = respond(client, 400, text::NETWORK_BAD_REQUEST);
            return;
        }
    };
    let Some(host) = crate::config::domain::normalize(&request.host) else {
        let line = run.deny(
            &shown_host(&request.host),
            request.port,
            Denied::InvalidHost,
        );
        let _ignored = respond(client, 403, &line);
        return;
    };
    if let Err(reason) = run.decide(&host, request.port) {
        let line = run.deny(&host, request.port, reason);
        let _ignored = respond(client, 403, &line);
        return;
    }
    let mut upstream = match run.gate.connect(&host, request.port) {
        Ok(upstream) => upstream,
        Err(reason) => {
            let line = run.deny(&host, request.port, reason);
            let _ignored = respond(
                client,
                if reason == Denied::PrivateAddress {
                    403
                } else {
                    502
                },
                &line,
            );
            return;
        }
    };
    if client.set_read_timeout(None).is_err() {
        return;
    }
    let sent = match &request.forward {
        None => client.write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n"),
        Some(forward) => upstream.write_all(&forward.head(&host, request.port)),
    };
    if sent.is_err() || upstream.write_all(&rest).is_err() {
        return;
    }
    tunnel(client, upstream);
}

/// 正規化できなかったホストを、記録に載せる形にする。表示できる ASCII だけを残し、長さを限る。
/// 長い名前や制御の文字を並べて、道具の結果を膨らませたり崩したりできないようにする。
fn shown_host(host: &str) -> String {
    let mut shown: String = host
        .chars()
        .map(|character| {
            if character.is_ascii_graphic() {
                character
            } else {
                '?'
            }
        })
        .take(INVALID_HOST_LIMIT)
        .collect();
    if host.chars().count() > INVALID_HOST_LIMIT {
        shown.push_str("...");
    }
    shown
}

/// 要求の頭（空の行まで）と、その後に読んでしまった分を返す。誤りは返す応答の状態。
/// 待つ時間は頭の全体に掛ける。一回の読み取りごとに掛けると、少しずつ送り続ける接続が枠を塞げるからである。
fn read_head(client: &mut TcpStream) -> Result<(Vec<u8>, Vec<u8>), u16> {
    let started = std::time::Instant::now();
    let mut buffer = Vec::new();
    let mut chunk = [0_u8; 4096];
    loop {
        if let Some(end) = buffer.windows(4).position(|window| window == b"\r\n\r\n") {
            let split = end.saturating_add(4);
            let rest = buffer.get(split..).unwrap_or_default().to_vec();
            buffer.truncate(split);
            return Ok((buffer, rest));
        }
        if buffer.len() > HEAD_LIMIT {
            return Err(431);
        }
        let remaining = HEAD_TIMEOUT.saturating_sub(started.elapsed());
        if remaining.is_zero() || client.set_read_timeout(Some(remaining)).is_err() {
            return Err(400);
        }
        match client.read(&mut chunk) {
            Ok(0) | Err(_) => return Err(400),
            Ok(count) => buffer.extend_from_slice(chunk.get(..count).unwrap_or_default()),
        }
    }
}

fn respond(mut client: TcpStream, status: u16, body: &str) -> io::Result<()> {
    let phrase = match status {
        400 => "Bad Request",
        403 => "Forbidden",
        407 => "Proxy Authentication Required",
        431 => "Request Header Fields Too Large",
        502 => "Bad Gateway",
        _ => "Service Unavailable",
    };
    let body = format!("{body}\n");
    let authenticate = if status == 407 {
        "Proxy-Authenticate: Basic realm=\"benitoiteh\"\r\n"
    } else {
        ""
    };
    client.write_all(
        format!(
            "HTTP/1.1 {status} {phrase}\r\nContent-Type: text/plain; charset=utf-8\r\n{authenticate}Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )
        .as_bytes(),
    )?;
    client.shutdown(Shutdown::Write)
}

/// 両方向を、片側が閉じるまで写す。
fn tunnel(client: TcpStream, upstream: TcpStream) {
    let (Ok(mut client_read), Ok(mut upstream_write)) = (client.try_clone(), upstream.try_clone())
    else {
        return;
    };
    let forward = std::thread::Builder::new()
        .name("benitoiteh-proxy-forward".into())
        .spawn(move || {
            let _copied = io::copy(&mut client_read, &mut upstream_write);
            let _closed = upstream_write.shutdown(Shutdown::Write);
        });
    let (mut upstream_read, mut client_write) = (upstream, client);
    let _copied = io::copy(&mut upstream_read, &mut client_write);
    let _closed = client_write.shutdown(Shutdown::Write);
    if let Ok(forward) = forward {
        let _joined = forward.join();
    }
}
