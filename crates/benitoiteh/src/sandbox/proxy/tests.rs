//! 中継の判断と転送を、本物のソケットと手元の模擬の宛先で確かめる（設計書 03-02「通信の中継」）。

use super::*;
use crate::approval::ApprovalAnswer;
use std::error::Error;
use std::sync::mpsc;

type TestResult = Result<(), Box<dyn Error>>;
/// 宛先の待ち受けのポートと、受け取った頭を受ける口。
type Upstream = (u16, mpsc::Receiver<Vec<u8>>);

/// 一回だけ受け付け、受け取った頭を返して、決まった応答を返す宛先。
fn upstream(response: &'static [u8]) -> Result<Upstream, Box<dyn Error>> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let port = listener.local_addr()?.port();
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        if let Ok((mut stream, _)) = listener.accept() {
            let _timeout = stream.set_read_timeout(Some(Duration::from_secs(5)));
            let mut received = Vec::new();
            let mut chunk = [0_u8; 1024];
            while !received.windows(4).any(|window| window == b"\r\n\r\n") {
                match stream.read(&mut chunk) {
                    Ok(0) | Err(_) => break,
                    Ok(count) => received.extend_from_slice(chunk.get(..count).unwrap_or_default()),
                }
            }
            let _sent = sender.send(received);
            let _written = stream.write_all(response);
        }
    });
    Ok((port, receiver))
}

/// 名前を手元の宛先へ向ける門。
fn gate(allowed: &[&str], port: u16, ask: bool) -> NetworkGate {
    NetworkGate::new(allowed.iter().map(|&name| name.to_owned()).collect(), ask).with_upstream(
        Arc::new(move |_, _| Ok(vec![SocketAddr::from(([127, 0, 0, 1], port))])),
        |_| true,
    )
}

fn start(run: &Arc<ProxyRun>) -> Result<(u16, ProxyServer), Box<dyn Error>> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let port = listener.local_addr()?.port();
    Ok((port, serve(vec![listener], Arc::clone(run))?))
}

fn exchange(port: u16, request: &str) -> Result<String, Box<dyn Error>> {
    let mut stream = TcpStream::connect(("127.0.0.1", port))?;
    stream.set_read_timeout(Some(Duration::from_secs(10)))?;
    stream.write_all(request.as_bytes())?;
    stream.shutdown(Shutdown::Write)?;
    let mut response = String::new();
    stream.read_to_string(&mut response)?;
    Ok(response)
}

// 作成時の関門: 許したドメインの CONNECT は中身を覗かずに渡し、平文の HTTP は要求の行と
// ヘッダを直して渡す。宛先は、プロキシの合言葉と接続を保つ指定を受け取らない。
#[test]
fn allowed_domains_tunnel_connect_and_rewrite_plain_http() -> TestResult {
    let (upstream_port, received) = upstream(b"tunneled-reply")?;
    let run = ProxyRun::new(
        Arc::new(gate(&["example.com"], upstream_port, false)),
        "curl".into(),
        PathBuf::from("/project"),
    );
    let (port, _server) = start(&run)?;
    let response = exchange(
        port,
        "CONNECT example.com:443 HTTP/1.1\r\nHost: example.com:443\r\n\r\nGET / HTTP/1.1\r\n\r\n",
    )?;
    assert!(
        response.starts_with("HTTP/1.1 200 Connection Established\r\n\r\n"),
        "{response}"
    );
    assert!(response.ends_with("tunneled-reply"), "{response}");
    assert_eq!(
        received.recv_timeout(Duration::from_secs(5))?,
        b"GET / HTTP/1.1\r\n\r\n"
    );
    let (upstream_port, received) = upstream(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok")?;
    let run = ProxyRun::new(
        Arc::new(gate(&["example.com"], upstream_port, false)),
        "curl".into(),
        PathBuf::from("/project"),
    );
    let (port, _server) = start(&run)?;
    let response = exchange(
        port,
        "GET http://example.com/path?q=1 HTTP/1.1\r\nHost: other.test\r\nProxy-Authorization: Basic x\r\nProxy-Connection: keep-alive\r\nUser-Agent: t\r\n\r\n",
    )?;
    assert!(response.ends_with("\r\n\r\nok"), "{response}");
    assert_eq!(
        String::from_utf8(received.recv_timeout(Duration::from_secs(5))?)?,
        "GET /path?q=1 HTTP/1.1\r\nHost: example.com\r\nUser-Agent: t\r\nConnection: close\r\n\r\n"
    );
    assert!(run.denials().is_empty());
    Ok(())
}

// 作成時の関門: 一覧にないドメインは、尋ねられなければ断り、尋ねたときは答えの範囲
// （このコマンド・このセッション・断る）だけを許す。断った宛先と理由を結果に残す。
#[test]
fn unknown_domains_follow_the_users_answer_and_record_denials() -> TestResult {
    let gate = Arc::new(gate(&[], 9, true));
    let run = ProxyRun::new(
        Arc::clone(&gate),
        "npm install".into(),
        PathBuf::from("/project"),
    );
    let (port, server) = start(&run)?;
    let response = exchange(port, "CONNECT registry.example.com:443 HTTP/1.1\r\n\r\n")?;
    assert!(response.starts_with("HTTP/1.1 403"), "{response}");
    assert!(response.contains(text::NETWORK_NOT_ALLOWED), "{response}");
    assert_eq!(
        run.denials(),
        [text::network_denied(
            "registry.example.com",
            443,
            text::NETWORK_NOT_ALLOWED
        )]
    );
    drop(server);
    let (prompts, requests) = mpsc::channel::<ApprovalPrompt>();
    gate.connect_prompts(prompts);
    // 三つの確認にだけ答える。待ちに期限を付け、取り違えても止まらないようにする。
    let answers = std::thread::spawn(move || {
        let mut seen = Vec::new();
        for outcome in [
            ApprovalOutcome::Approved,
            ApprovalOutcome::ApprovedForSession,
            ApprovalOutcome::Denied,
        ] {
            let Ok(prompt) = requests.recv_timeout(Duration::from_secs(10)) else {
                break;
            };
            seen.push((
                prompt.request.kind,
                prompt.request.facts.domain.clone(),
                prompt.request.facts.command.clone(),
            ));
            let _sent = prompt.reply.send(ApprovalAnswer {
                outcome,
                message: None,
            });
        }
        seen
    });
    // 宛先はつながらないが、判断は接続の前に済む。502 は許した印として扱う。
    let first = ProxyRun::new(
        Arc::clone(&gate),
        "npm install".into(),
        PathBuf::from("/project"),
    );
    let (port, server) = start(&first)?;
    for _ in 0..2 {
        let response = exchange(port, "CONNECT once.example.com:443 HTTP/1.1\r\n\r\n")?;
        assert!(!response.starts_with("HTTP/1.1 403"), "{response}");
    }
    let response = exchange(port, "CONNECT session.example.com:443 HTTP/1.1\r\n\r\n")?;
    assert!(!response.starts_with("HTTP/1.1 403"), "{response}");
    drop(server);
    let second = ProxyRun::new(
        Arc::clone(&gate),
        "git fetch".into(),
        PathBuf::from("/project"),
    );
    let (port, server) = start(&second)?;
    // この起動だけに許したドメインは次のコマンドで尋ね直し、断られる。セッションに許したものは尋ねない。
    let response = exchange(port, "CONNECT session.example.com:443 HTTP/1.1\r\n\r\n")?;
    assert!(!response.starts_with("HTTP/1.1 403"), "{response}");
    let response = exchange(port, "CONNECT once.example.com:443 HTTP/1.1\r\n\r\n")?;
    assert!(response.contains(text::NETWORK_USER_DECLINED), "{response}");
    // 同じコマンドの中で断ったドメインは、再試行でも尋ね直さずに断る（答える糸は三つで終わる）。
    let response = exchange(port, "CONNECT once.example.com:443 HTTP/1.1\r\n\r\n")?;
    assert!(response.contains(text::NETWORK_USER_DECLINED), "{response}");
    drop(server);
    let seen = answers.join().map_err(|_| "answers")?;
    assert_eq!(
        seen,
        [
            (
                ApprovalKind::Network,
                Some("once.example.com".into()),
                Some("npm install".into())
            ),
            (
                ApprovalKind::Network,
                Some("session.example.com".into()),
                Some("npm install".into())
            ),
            (
                ApprovalKind::Network,
                Some("once.example.com".into()),
                Some("git fetch".into())
            ),
        ]
    );
    Ok(())
}

// 作成時の関門: 許したドメインでも、私的なアドレスに解決されれば中継しない。IP アドレスの宛先、
// 暗号化の前の https:// の要求、利用者の情報を含む URL も断る。合言葉を求める門は、ない要求を 407 で断る。
#[test]
fn private_addresses_ip_literals_and_missing_tokens_are_refused() -> TestResult {
    let gate = Arc::new(
        NetworkGate::new(
            ["internal.example.com".to_owned()].into_iter().collect(),
            false,
        )
        .with_upstream(
            Arc::new(|_, _| Ok(vec![SocketAddr::from(([10, 0, 0, 5], 443))])),
            crate::tools::address_allowed,
        ),
    );
    let run = ProxyRun::new(Arc::clone(&gate), "curl".into(), PathBuf::from("/project"));
    let (port, _server) = start(&run)?;
    for (request, reason) in [
        (
            "CONNECT internal.example.com:443 HTTP/1.1\r\n\r\n",
            text::NETWORK_PRIVATE_ADDRESS,
        ),
        (
            "CONNECT 93.184.216.34:443 HTTP/1.1\r\n\r\n",
            text::NETWORK_INVALID_HOST,
        ),
        (
            "GET https://internal.example.com/ HTTP/1.1\r\n\r\n",
            text::NETWORK_TLS_WITHOUT_CONNECT,
        ),
        (
            "GET http://u:p@internal.example.com/ HTTP/1.1\r\n\r\n",
            text::NETWORK_CREDENTIALS,
        ),
    ] {
        let response = exchange(port, request)?;
        assert!(
            response.starts_with("HTTP/1.1 403"),
            "{request}: {response}"
        );
        assert!(response.contains(reason), "{request}: {response}");
    }
    let (upstream_port, _received) = upstream(b"secured")?;
    let run = ProxyRun::with_token(
        Arc::new(self::gate(&["example.com"], upstream_port, false)),
        "curl".into(),
        PathBuf::from("/project"),
        "token".into(),
    );
    let (port, _server) = start(&run)?;
    let response = exchange(port, "CONNECT example.com:443 HTTP/1.1\r\n\r\n")?;
    assert!(response.starts_with("HTTP/1.1 407"), "{response}");
    // 合言葉のない要求は、解釈で断る形でも 407 で断り、断った記録（道具の結果に載る文）を残さない。
    let response = exchange(port, "GET https://injected.example/ HTTP/1.1\r\n\r\n")?;
    assert!(response.starts_with("HTTP/1.1 407"), "{response}");
    assert!(run.denials().is_empty(), "{:?}", run.denials());
    let response = exchange(
        port,
        &format!(
            "CONNECT example.com:443 HTTP/1.1\r\nProxy-Authorization: Basic {}\r\n\r\n",
            "YmVuaXRvaXRlaDp0b2tlbg=="
        ),
    )?;
    assert!(response.starts_with("HTTP/1.1 200"), "{response}");
    Ok(())
}

// 作成時の関門: コマンドが終わった後は、答えを待っていた接続も断り、遅れて届いた答えをどの許可にも
// 加えない。終わった後に来た接続は尋ねない（設計書 03-02「通信の中継」）。
#[test]
fn finished_runs_drop_pending_confirmations_and_late_answers() -> TestResult {
    let gate = Arc::new(gate(&[], 9, true));
    let (prompts, requests) = mpsc::channel::<ApprovalPrompt>();
    gate.connect_prompts(prompts);
    let run = ProxyRun::new(Arc::clone(&gate), "curl".into(), PathBuf::from("/project"));
    let (port, _server) = start(&run)?;
    let client = std::thread::spawn(move || {
        exchange(port, "CONNECT late.example.com:443 HTTP/1.1\r\n\r\n").map_err(|e| e.to_string())
    });
    let prompt = requests.recv_timeout(Duration::from_secs(10))?;
    run.finish();
    let response = client.join().map_err(|_| "client")??;
    assert!(response.starts_with("HTTP/1.1 403"), "{response}");
    let _late = prompt.reply.send(ApprovalAnswer {
        outcome: ApprovalOutcome::ApprovedForSession,
        message: None,
    });
    let response = exchange(port, "CONNECT late.example.com:443 HTTP/1.1\r\n\r\n")?;
    assert!(response.starts_with("HTTP/1.1 403"), "{response}");
    assert!(requests.try_recv().is_err());
    assert!(
        !gate
            .session
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .contains("late.example.com")
    );
    Ok(())
}

// 作成時の関門: 承認の方針が尋ねることを許さない門は、画面につながっていても尋ねずに断る。
#[test]
fn gates_that_must_not_ask_refuse_without_prompting() -> TestResult {
    let gate = Arc::new(gate(&[], 9, false));
    let (prompts, requests) = mpsc::channel::<ApprovalPrompt>();
    gate.connect_prompts(prompts);
    let run = ProxyRun::new(Arc::clone(&gate), "curl".into(), PathBuf::from("/project"));
    let (port, _server) = start(&run)?;
    let response = exchange(port, "CONNECT unknown.example.com:443 HTTP/1.1\r\n\r\n")?;
    assert!(response.contains(text::NETWORK_NOT_ALLOWED), "{response}");
    assert!(requests.try_recv().is_err());
    Ok(())
}
