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
    ApprovalAnswer, ApprovalFacts, ApprovalKind, ApprovalOutcome, ApprovalPrompt, ApprovalRecord,
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
/// 要求の頭を読み終えるまで待つ時間。
const HEAD_TIMEOUT: Duration = Duration::from_secs(30);

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
    token: Option<String>,
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
            token: None,
        }
    }

    /// 名前の解決とアドレスの判定を差し替える。製品の設定からは使わない。
    pub fn with_upstream(mut self, resolve: Resolve, allowed_address: fn(IpAddr) -> bool) -> Self {
        self.resolve = resolve;
        self.allowed_address = allowed_address;
        self
    }

    /// 手元のほかのプロセスからも届く待ち受け（macOS）では、要求に合言葉を求める。
    pub fn with_token(mut self, token: String) -> Self {
        self.token = Some(token);
        self
    }

    /// 対話の画面へ確認を送る口をつなぐ。つながっていなければ、一覧にないドメインを断る。
    pub fn connect_prompts(&self, prompts: Sender<ApprovalPrompt>) {
        *self.prompts.lock().unwrap_or_else(PoisonError::into_inner) = Some(prompts);
    }

    /// 子に渡すプロキシの URL。合言葉があれば利用者の情報として含める。
    pub fn proxy_url(&self, port: u16) -> String {
        match &self.token {
            Some(token) => format!("http://benitoiteh:{token}@127.0.0.1:{port}"),
            None => format!("http://127.0.0.1:{port}"),
        }
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

/// 一回のコマンドの間の中継の状態。このコマンドだけに許したドメインと、断った記録を持つ。
pub struct ProxyRun {
    gate: Arc<NetworkGate>,
    command: String,
    cwd: PathBuf,
    allowed: Mutex<BTreeSet<String>>,
    denials: Mutex<Vec<String>>,
    active: AtomicUsize,
}

impl ProxyRun {
    pub fn new(gate: Arc<NetworkGate>, command: String, cwd: PathBuf) -> Arc<Self> {
        Arc::new(Self {
            gate,
            command,
            cwd,
            allowed: Mutex::new(BTreeSet::new()),
            denials: Mutex::new(Vec::new()),
            active: AtomicUsize::new(0),
        })
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
        let prompts = self
            .gate
            .prompts
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        let Some(prompts) = prompts.filter(|_| self.gate.ask) else {
            return Err(Denied::NotAllowed);
        };
        // 同じドメインへの接続が重なっても、確認は一度だけ出す。
        let _asking = self
            .gate
            .asking
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if self.permitted(host) {
            return Ok(());
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
        match response.recv() {
            Ok(ApprovalAnswer {
                outcome: ApprovalOutcome::Approved,
                ..
            }) => {
                self.allowed
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .insert(host.to_owned());
                Ok(())
            }
            Ok(ApprovalAnswer {
                outcome: ApprovalOutcome::ApprovedForSession,
                ..
            }) => {
                self.gate
                    .session
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .insert(host.to_owned());
                Ok(())
            }
            Ok(_) | Err(_) => Err(Denied::UserDeclined),
        }
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

/// 受け付けの糸を起こす。止める印は、短い間隔の待ちで確かめる。
pub fn serve(listener: TcpListener, run: Arc<ProxyRun>) -> io::Result<ProxyServer> {
    listener.set_nonblocking(true)?;
    let stop = Arc::new(AtomicBool::new(false));
    let stopping = Arc::clone(&stop);
    let thread = std::thread::Builder::new()
        .name("benitoiteh-proxy".into())
        .spawn(move || accept_loop(&listener, &run, &stopping))?;
    Ok(ProxyServer {
        stop,
        thread: Some(thread),
    })
}

fn accept_loop(listener: &TcpListener, run: &Arc<ProxyRun>, stop: &AtomicBool) {
    while !stop.load(Ordering::Acquire) {
        match listener.accept() {
            Ok((stream, _)) => {
                if run.active.fetch_add(1, Ordering::AcqRel) >= CONNECTION_LIMIT {
                    run.active.fetch_sub(1, Ordering::AcqRel);
                    let _refused = respond(stream, 503, text::NETWORK_BUSY);
                    continue;
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
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(20));
            }
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            // 待ち受けが使えなくなったら終える。子からは、つながらない通信として見える。
            Err(_) => return,
        }
    }
}

fn handle(mut client: TcpStream, run: &ProxyRun) {
    // 受け付けた接続は、待ち受けの非同期の指定を引き継ぎうるので、同期に戻す。
    if client.set_nonblocking(false).is_err()
        || client.set_read_timeout(Some(HEAD_TIMEOUT)).is_err()
    {
        return;
    }
    let (head, rest) = match read_head(&mut client) {
        Ok(read) => read,
        Err(status) => {
            let _ignored = respond(client, status, text::NETWORK_BAD_REQUEST);
            return;
        }
    };
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
    if let Some(token) = &run.gate.token
        && !request.authorized(token)
    {
        let _ignored = respond(client, 407, text::NETWORK_PROXY_AUTH);
        return;
    }
    let Some(host) = crate::config::domain::normalize(&request.host) else {
        let line = run.deny(&request.host, request.port, Denied::InvalidHost);
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

/// 要求の頭（空の行まで）と、その後に読んでしまった分を返す。誤りは返す応答の状態。
fn read_head(client: &mut TcpStream) -> Result<(Vec<u8>, Vec<u8>), u16> {
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
