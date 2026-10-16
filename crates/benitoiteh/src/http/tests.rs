//! 公開の HTTP API と本物のループバック接続で契約を確かめる（設計書 07-01「模擬の提供者」、実装プラン P0-03「テスト」）。

use std::error::Error;
use std::io::{self, Read, Write};
use std::net::{SocketAddr, TcpListener};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use super::*;

// この鍵はループバックの模擬サーバ専用に生成した公開のテストデータである。
const CERT: &[u8] = include_bytes!("fixtures/localhost-cert.der");
const KEY: &[u8] = include_bytes!("fixtures/localhost-key.der");
const WAIT: Duration = Duration::from_secs(3);

trait Connection: Read + Write {}
impl<T: Read + Write> Connection for T {}

struct Server {
    address: SocketAddr,
    worker: JoinHandle<io::Result<()>>,
}

impl Server {
    fn start(
        tls: bool,
        handler: impl FnOnce(&mut dyn Connection) -> io::Result<()> + Send + 'static,
    ) -> io::Result<Self> {
        let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))?;
        let address = listener.local_addr()?;
        listener.set_nonblocking(true)?;
        let worker = thread::spawn(move || {
            let started = Instant::now();
            let mut socket = loop {
                match listener.accept() {
                    Ok((socket, _)) => break socket,
                    Err(e) if e.kind() == io::ErrorKind::WouldBlock && started.elapsed() < WAIT => {
                        thread::sleep(Duration::from_millis(5))
                    }
                    Err(e) => return Err(e),
                }
            };
            socket.set_nonblocking(false)?;
            socket.set_nodelay(true)?;
            socket.set_read_timeout(Some(WAIT))?;
            socket.set_write_timeout(Some(WAIT))?;
            if tls {
                let provider = Arc::new(rustls_graviola::default_provider());
                let config = rustls::ServerConfig::builder_with_provider(provider)
                    .with_safe_default_protocol_versions()
                    .map_err(io::Error::other)?
                    .with_no_client_auth()
                    .with_single_cert(
                        vec![rustls::pki_types::CertificateDer::from(CERT.to_vec())],
                        rustls::pki_types::PrivatePkcs8KeyDer::from(KEY.to_vec()).into(),
                    )
                    .map_err(io::Error::other)?;
                let connection =
                    rustls::ServerConnection::new(Arc::new(config)).map_err(io::Error::other)?;
                handler(&mut rustls::StreamOwned::new(connection, socket))
            } else {
                handler(&mut socket)
            }
        });
        Ok(Self { address, worker })
    }

    fn uri(&self, tls: bool) -> Result<Uri, Box<dyn Error>> {
        Ok(format!(
            "{}://localhost:{}/api",
            if tls { "https" } else { "http" },
            self.address.port()
        )
        .parse()?)
    }

    fn finish(self) -> io::Result<()> {
        self.worker
            .join()
            .map_err(|_| io::Error::other("server panicked"))?
    }
}

fn client(tls: bool, timeouts: HttpTimeouts) -> HttpClient {
    if !tls {
        return HttpClient::new(timeouts);
    }
    // 信頼する証明書の差し替えはこの非公開のテスト内だけに限る。
    let tls = ureq::tls::TlsConfig::builder()
        .unversioned_rustls_crypto_provider(Arc::new(rustls_graviola::default_provider()))
        .root_certs(ureq::tls::RootCerts::new_with_certs(&[
            ureq::tls::Certificate::from_der(CERT),
        ]))
        .build();
    HttpClient::with_tls(timeouts, tls)
}

fn read_request(socket: &mut dyn Connection) -> io::Result<(String, Vec<u8>)> {
    let mut bytes = Vec::new();
    while !bytes.ends_with(b"\r\n\r\n") {
        let mut byte = [0];
        socket.read_exact(&mut byte)?;
        bytes.extend_from_slice(&byte);
        if bytes.len() > 65536 {
            return Err(io::Error::other("large test request"));
        }
    }
    let headers = String::from_utf8(bytes).map_err(io::Error::other)?;
    let length = headers
        .lines()
        .filter_map(|line| line.split_once(':'))
        .find(|(name, _)| name.eq_ignore_ascii_case("content-length"))
        .map(|(_, value)| value.trim().parse::<usize>())
        .transpose()
        .map_err(io::Error::other)?
        .unwrap_or(0);
    if length > 65536 {
        return Err(io::Error::other("large test body"));
    }
    let mut body = vec![0; length];
    socket.read_exact(&mut body)?;
    Ok((headers, body))
}

fn next(stream: &mut HttpStream) -> Result<StreamEvent, Box<dyn Error>> {
    let started = Instant::now();
    loop {
        let event = stream.poll(Duration::from_millis(20))?;
        if !matches!(event, StreamEvent::Pending) {
            return Ok(event);
        }
        if started.elapsed() > WAIT {
            return Err(io::Error::other("no stream event").into());
        }
    }
}

fn failure(stream: &mut HttpStream) -> Result<HttpError, Box<dyn Error>> {
    let started = Instant::now();
    loop {
        match stream.poll(Duration::from_millis(20)) {
            Err(error) => return Ok(error),
            Ok(StreamEvent::Finished) => return Err(io::Error::other("unexpected EOF").into()),
            Ok(StreamEvent::Headers(_) | StreamEvent::Data(_) | StreamEvent::Pending) => {}
        }
        if started.elapsed() > WAIT {
            return Err(io::Error::other("no stream failure").into());
        }
    }
}

fn ended_within_one_second(stream: &mut HttpStream) -> Result<(), HttpError> {
    let started = Instant::now();
    while !stream.is_finished() && started.elapsed() < Duration::from_secs(1) {
        thread::sleep(Duration::from_millis(5));
    }
    assert!(
        stream.is_finished(),
        "HTTP reader did not stop within one second"
    );
    stream.join()
}

// 打ち切り後の EOF または TLS の切断で終える。サーバを待たせることで、クライアントの
// read が shutdown で起こされたかを、時間切れに頼らず確かめる。
fn wait_for_disconnect(socket: &mut dyn Connection) {
    let mut byte = [0];
    while let Ok(amount) = socket.read(&mut byte) {
        if amount == 0 {
            break;
        }
    }
}

#[test]
fn streaming_post_preserves_headers_json_and_all_body_bytes() -> Result<(), Box<dyn Error>> {
    let expected = vec![b'x'; 20000];
    let server_body = expected.clone();
    let server = Server::start(false, move |socket| {
        let (headers, body) = read_request(socket)?;
        assert!(headers.starts_with("POST /api HTTP/1.1\r\n"));
        assert!(
            headers
                .to_ascii_lowercase()
                .contains("authorization: test-secret\r\n")
        );
        assert!(
            headers
                .to_ascii_lowercase()
                .contains("content-type: application/json\r\n")
        );
        assert_eq!(body, br#"{"stream":true}"#);
        write!(
            socket,
            "HTTP/1.1 201 Created\r\nContent-Length: {}\r\nX-Test: yes\r\n\r\n",
            server_body.len()
        )?;
        socket.write_all(&server_body)?;
        socket.flush()
    })?;
    let mut headers = HeaderMap::new();
    headers.insert(
        header::AUTHORIZATION,
        HeaderValue::from_static("test-secret"),
    );
    let mut stream =
        HttpClient::default().post_json(&server.uri(false)?, &headers, br#"{"stream":true}"#)?;
    let StreamEvent::Headers(head) = next(&mut stream)? else {
        return Err(io::Error::other("missing headers").into());
    };
    assert_eq!(head.status, 201);
    assert_eq!(
        head.headers.get("x-test"),
        Some(&HeaderValue::from_static("yes"))
    );
    let mut body = Vec::new();
    loop {
        match next(&mut stream)? {
            StreamEvent::Data(bytes) => body.extend(bytes),
            StreamEvent::Finished => break,
            StreamEvent::Headers(_) | StreamEvent::Pending => {
                return Err(io::Error::other("unexpected event").into());
            }
        }
    }
    assert_eq!(body, expected);
    stream.join()?;
    server.finish()?;
    Ok(())
}

#[test]
fn cancellation_stops_plain_and_tls_readers_and_full_channels() -> Result<(), Box<dyn Error>> {
    // read 待ち、少しずつ届く本文、受け手が遅い場合は、互いに別の待ちを生む。
    for tls in [false, true] {
        for mode in 0..3 {
            let server = Server::start(tls, move |socket| {
                read_request(socket)?;
                socket.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 10000000\r\n\r\nx")?;
                socket.flush()?;
                if mode == 0 {
                    wait_for_disconnect(socket);
                } else {
                    let data = vec![b'x'; if mode == 1 { 1 } else { 65536 }];
                    while socket
                        .write_all(&data)
                        .and_then(|()| socket.flush())
                        .is_ok()
                    {
                        if mode == 1 {
                            thread::sleep(Duration::from_millis(5));
                        }
                    }
                }
                Ok(())
            })?;
            let mut stream = client(tls, HttpTimeouts::default()).post_json(
                &server.uri(tls)?,
                &HeaderMap::new(),
                b"{}",
            )?;
            assert!(matches!(next(&mut stream)?, StreamEvent::Headers(_)));
            assert!(matches!(next(&mut stream)?, StreamEvent::Data(_)));
            thread::sleep(Duration::from_millis(80));
            let cancellation = stream.cancellation();
            let other_thread = cancellation.clone();
            thread::spawn(move || other_thread.cancel())
                .join()
                .map_err(|_| io::Error::other("cancel thread panicked"))?;
            assert!(cancellation.is_cancelled());
            assert!(matches!(
                stream.poll(Duration::ZERO),
                Err(HttpError::Cancelled)
            ));
            ended_within_one_second(&mut stream)?;
            server.finish()?;
        }
    }
    Ok(())
}

#[test]
fn idle_deadline_stops_after_headers_and_mid_body() -> Result<(), Box<dyn Error>> {
    for chunks in [0, 1, 6] {
        let server = Server::start(false, move |socket| {
            read_request(socket)?;
            socket.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 10\r\n\r\n")?;
            socket.flush()?;
            for _ in 0..chunks {
                thread::sleep(Duration::from_millis(50));
                socket.write_all(b"x")?;
                socket.flush()?;
            }
            wait_for_disconnect(socket);
            Ok(())
        })?;
        let timeouts = HttpTimeouts {
            idle: Duration::from_millis(200),
            ..HttpTimeouts::default()
        };
        let mut stream =
            client(false, timeouts).post_json(&server.uri(false)?, &HeaderMap::new(), b"{}")?;
        assert!(matches!(next(&mut stream)?, StreamEvent::Headers(_)));
        let mut received = 0_usize;
        while received < chunks {
            let StreamEvent::Data(bytes) = next(&mut stream)? else {
                return Err(io::Error::other("missing body").into());
            };
            received = received.saturating_add(bytes.len());
        }
        let last_received = stream.last_received();
        assert!(matches!(
            failure(&mut stream)?,
            HttpError::Timeout(TimeoutKind::Idle)
        ));
        assert!(last_received.elapsed() >= timeouts.idle);
        ended_within_one_second(&mut stream)?;
        server.finish()?;
    }
    Ok(())
}

#[test]
fn get_enforces_body_limit_and_returns_redirect_for_caller() -> Result<(), Box<dyn Error>> {
    for (body, limit, succeeds) in [
        ("", 0, true),
        ("abc", 3, true),
        ("abcd", 3, false),
        ("a", 0, false),
    ] {
        let server = Server::start(false, move |socket| {
            let (headers, _) = read_request(socket)?;
            assert!(!headers.to_ascii_lowercase().contains("accept-encoding:"));
            write!(
                socket,
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n{body}",
                body.len()
            )?;
            socket.flush()
        })?;
        let mut headers = HeaderMap::new();
        headers.insert(header::ACCEPT_ENCODING, HeaderValue::from_static("gzip"));
        let result = HttpClient::default().get(&server.uri(false)?, &headers, limit);
        if succeeds {
            assert_eq!(result?.body, body.as_bytes());
        } else {
            assert!(
                matches!(result, Err(HttpError::BodyLimit { limit: actual }) if actual == limit)
            );
        }
        server.finish()?;
    }
    let server = Server::start(false, |socket| {
        read_request(socket)?;
        socket.write_all(
            b"HTTP/1.1 302 Found\r\nLocation: http://127.0.0.1:1/\r\nContent-Length: 0\r\n\r\n",
        )?;
        socket.flush()
    })?;
    let Err(HttpError::Status(error)) =
        HttpClient::default().get(&server.uri(false)?, &HeaderMap::new(), 10)
    else {
        return Err(io::Error::other("missing redirect status").into());
    };
    assert_eq!(error.head.status, 302);
    assert_eq!(
        error.head.headers.get(header::LOCATION),
        Some(&HeaderValue::from_static("http://127.0.0.1:1/"))
    );
    server.finish()?;
    Ok(())
}

#[test]
fn status_errors_keep_bounded_prefix_and_retry_after_without_logging_secrets()
-> Result<(), Box<dyn Error>> {
    for post in [false, true] {
        let server = Server::start(false, |socket| {
            read_request(socket)?;
            socket.write_all(b"HTTP/1.1 429 Too Many Requests\r\nRetry-After: 23\r\nContent-Length: 70000\r\n\r\n")?;
            let mut body = b"test-secret".to_vec();
            body.resize(70000, b'x');
            if socket.write_all(&body).is_err() {
                return Ok(());
            }
            socket.flush()
        })?;
        let mut headers = HeaderMap::new();
        headers.insert(
            header::AUTHORIZATION,
            HeaderValue::from_static("test-secret"),
        );
        let client = HttpClient::default();
        let error = if post {
            let mut stream = client.post_json(&server.uri(false)?, &headers, b"{}")?;
            let error = failure(&mut stream)?;
            stream.join()?;
            error
        } else {
            let result = client.get(&server.uri(false)?, &headers, 1);
            match result {
                Err(error) => error,
                Ok(_) => return Err(io::Error::other("unexpected success").into()),
            }
        };
        assert!(!format!("{error} {error:?}").contains("test-secret"));
        let HttpError::Status(error) = error else {
            return Err(io::Error::other("missing HTTP status").into());
        };
        assert_eq!(error.head.status, 429);
        assert_eq!(error.retry_after, Some(HeaderValue::from_static("23")));
        assert_eq!(error.body.len(), 65536);
        assert!(error.body.starts_with(b"test-secret"));
        server.finish()?;
    }
    Ok(())
}

#[test]
fn resolved_addresses_keep_host_and_tls_name_and_never_reuse_another_pool()
-> Result<(), Box<dyn Error>> {
    let client = client(true, HttpTimeouts::default());
    // 同じ URI に二回、異なる接続先を渡す。以前のプールに残った接続を使えば失敗する。
    let uri: Uri = "https://mock.invalid/path".parse()?;
    for _ in 0..2 {
        let server = Server::start(true, |socket| {
            let (headers, _) = read_request(socket)?;
            assert!(
                headers
                    .to_ascii_lowercase()
                    .contains("host: mock.invalid\r\n")
            );
            socket.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 3\r\n\r\nyes")?;
            socket.flush()
        })?;
        let response = client.get_resolved(&uri, &HeaderMap::new(), &[server.address], 3)?;
        assert_eq!(response.body, b"yes");
        server.finish()?;
    }
    assert!(matches!(
        client.get_resolved(&uri, &HeaderMap::new(), &[], 3),
        Err(HttpError::Connection)
    ));
    for platform_roots in [false, true] {
        let server = Server::start(true, |socket| {
            read_request(socket)?;
            Ok(())
        })?;
        let uri: Uri = if platform_roots {
            "https://mock.invalid/"
        } else {
            "https://wrong.invalid/"
        }
        .parse()?;
        let client = if platform_roots {
            HttpClient::default()
        } else {
            client.clone()
        };
        assert!(matches!(
            client.get_resolved(&uri, &HeaderMap::new(), &[server.address], 3),
            Err(HttpError::Tls)
        ));
        assert!(server.finish().is_err());
    }
    Ok(())
}

#[test]
fn ordinary_get_reuses_the_connection() -> Result<(), Box<dyn Error>> {
    let server = Server::start(false, |socket| {
        for _ in 0..2 {
            read_request(socket)?;
            socket.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 1\r\n\r\nx")?;
            socket.flush()?;
        }
        Ok(())
    })?;
    let client = HttpClient::default();
    for _ in 0..2 {
        assert_eq!(
            client.get(&server.uri(false)?, &HeaderMap::new(), 1)?.body,
            b"x"
        );
    }
    server.finish()?;
    Ok(())
}

#[test]
fn header_and_total_deadlines_are_independent_of_idle() -> Result<(), Box<dyn Error>> {
    for headers_sent in [false, true] {
        let server = Server::start(false, move |socket| {
            read_request(socket)?;
            if headers_sent {
                socket.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 1000000\r\n\r\n")?;
                while socket.write_all(b"x").and_then(|()| socket.flush()).is_ok() {
                    thread::sleep(Duration::from_millis(10));
                }
            } else {
                wait_for_disconnect(socket);
            }
            Ok(())
        })?;
        let timeouts = HttpTimeouts {
            headers: Duration::from_millis(100),
            total: if headers_sent {
                Duration::from_millis(250)
            } else {
                WAIT
            },
            idle: WAIT,
            ..HttpTimeouts::default()
        };
        let mut stream =
            client(false, timeouts).post_json(&server.uri(false)?, &HeaderMap::new(), b"{}")?;
        let expected = if headers_sent {
            TimeoutKind::Total
        } else {
            TimeoutKind::Headers
        };
        assert!(matches!(failure(&mut stream)?, HttpError::Timeout(kind) if kind == expected));
        ended_within_one_second(&mut stream)?;
        server.finish()?;
    }
    Ok(())
}

#[test]
fn full_channel_does_not_outlive_total_deadline() -> Result<(), Box<dyn Error>> {
    let server = Server::start(false, |socket| {
        read_request(socket)?;
        socket.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 10000000\r\n\r\n")?;
        let bytes = vec![b'x'; 65536];
        while socket
            .write_all(&bytes)
            .and_then(|()| socket.flush())
            .is_ok()
        {}
        Ok(())
    })?;
    // 並べて走るテストと Rosetta の遅さで、ヘッダの前に上限が切れないよう 500 ミリ秒とする。
    let timeouts = HttpTimeouts {
        total: Duration::from_millis(500),
        ..HttpTimeouts::default()
    };
    let mut stream =
        client(false, timeouts).post_json(&server.uri(false)?, &HeaderMap::new(), b"{}")?;
    assert!(matches!(next(&mut stream)?, StreamEvent::Headers(_)));
    // poll で見張らなくても、満杯のチャネルへの送信待ちは全体の上限で終える。
    ended_within_one_second(&mut stream)?;
    assert!(matches!(
        stream.poll(Duration::ZERO),
        Err(HttpError::Timeout(TimeoutKind::Total))
    ));
    server.finish()?;
    Ok(())
}
