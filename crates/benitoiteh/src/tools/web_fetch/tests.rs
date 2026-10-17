//! 取得の安全条件と変換を、外の網を使わずに確かめる（設計書 07-01、実装プラン P4-04）。

use super::*;
use crate::config::{Mode, ReadConfig, paths::DataPaths};
use crate::safe_fs::ScopeRoot;
use crate::tools::{ToolSet, WebFetchContext, exclude::ExcludeRules};
use serde_json::json;
use std::error::Error;
use std::fs;
use std::io::{Read, Write};
use std::net::{Ipv4Addr, TcpListener};
use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::atomic::AtomicUsize;
use std::thread::{self, JoinHandle};

type TestResult = Result<(), Box<dyn Error>>;
// 二つの二ラベルの名前を持つループバック試験専用の証明書。鍵は既存の公開の試験データである。
const CERT: &[u8] = include_bytes!("fixtures/domains-cert.der");
const KEY: &[u8] = include_bytes!("../../http/fixtures/localhost-key.der");

struct Fixture {
    root: PathBuf,
    context: ToolContext,
}

impl Fixture {
    fn new(allowed: &[&str]) -> Result<Self, Box<dyn Error>> {
        let mut random = [0_u8; 16];
        getrandom::fill(&mut random)?;
        let root = std::env::temp_dir().join(format!(
            "benitoiteh-web-{:032x}",
            u128::from_ne_bytes(random)
        ));
        fs::create_dir(&root)?;
        let root = fs::canonicalize(root)?;
        for name in ["project", "tmp", "spill"] {
            fs::create_dir(root.join(name))?;
        }
        let paths = DataPaths::from_values(Some(&root), None, None, Some(&root))?;
        let context = ToolContext::new(
            Mode::Restricted,
            ScopeRoot::open(root.join("project"))?,
            ScopeRoot::open(root.join("tmp"))?,
            ScopeRoot::open(root.join("spill"))?,
            ExcludeRules::new(
                &root.join("project"),
                &paths,
                &root.join("spill"),
                &ReadConfig::default(),
                &ReadConfig::default(),
            )?,
        )
        .with_web_fetch(WebFetchContext {
            allowed_domains: allowed.iter().map(|s| (*s).to_owned()).collect(),
            client: test_client(),
        });
        Ok(Self { root, context })
    }
    fn options(&self, addresses: Vec<SocketAddr>) -> Options<'_> {
        Options {
            client: &self.context.web_fetch.client,
            resolve: Arc::new(move |_| Ok(addresses.clone())),
            allowed_address: |_| true,
            total: Duration::from_secs(3),
        }
    }
    fn run(
        &self,
        url: &str,
        approval: ApprovalStatus,
        domains: &BTreeSet<String>,
        options: Options<'_>,
    ) -> ToolOutput {
        execute_with(
            &self.context,
            &json!({"url":url}),
            approval,
            Invocation {
                call_id: "web",
                cancelled: &AtomicBool::new(false),
                session_domains: domains,
                confirmed_effects: &std::collections::BTreeMap::new(),
            },
            options,
        )
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _removed = fs::remove_dir_all(&self.root);
    }
}

fn test_client() -> HttpClient {
    HttpClient::with_tls(
        HttpTimeouts::default(),
        ureq::tls::TlsConfig::builder()
            .unversioned_rustls_crypto_provider(Arc::new(rustls_graviola::default_provider()))
            .root_certs(ureq::tls::RootCerts::new_with_certs(&[
                ureq::tls::Certificate::from_der(CERT),
            ]))
            .build(),
    )
}

struct Reply {
    status: u16,
    headers: String,
    body: Vec<u8>,
    slow: bool,
}
impl Reply {
    fn text(kind: &str, body: impl AsRef<[u8]>) -> Self {
        Self {
            status: 200,
            headers: format!("Content-Type: {kind}\r\n"),
            body: body.as_ref().to_vec(),
            slow: false,
        }
    }
    fn redirect(location: &str) -> Self {
        Self {
            status: 302,
            headers: format!("Location: {location}\r\n"),
            body: Vec::new(),
            slow: false,
        }
    }
}
struct Server {
    address: SocketAddr,
    requests: Arc<Mutex<Vec<String>>>,
    worker: JoinHandle<io::Result<()>>,
}
impl Server {
    fn start(replies: Vec<Reply>) -> io::Result<Self> {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))?;
        listener.set_nonblocking(true)?;
        let address = listener.local_addr()?;
        let requests = Arc::new(Mutex::new(Vec::new()));
        let record = Arc::clone(&requests);
        let worker = thread::spawn(move || {
            let config = rustls::ServerConfig::builder_with_provider(Arc::new(
                rustls_graviola::default_provider(),
            ))
            .with_safe_default_protocol_versions()
            .map_err(io::Error::other)?
            .with_no_client_auth()
            .with_single_cert(
                vec![rustls::pki_types::CertificateDer::from(CERT.to_vec())],
                rustls::pki_types::PrivatePkcs8KeyDer::from(KEY.to_vec()).into(),
            )
            .map_err(io::Error::other)?;
            let config = Arc::new(config);
            for reply in replies {
                let start = Instant::now();
                let socket = loop {
                    match listener.accept() {
                        Ok((socket, _)) => break socket,
                        Err(e)
                            if e.kind() == io::ErrorKind::WouldBlock
                                && start.elapsed() < Duration::from_secs(3) =>
                        {
                            thread::sleep(Duration::from_millis(2))
                        }
                        Err(e) => return Err(e),
                    }
                };
                socket.set_nonblocking(false)?;
                socket.set_read_timeout(Some(Duration::from_secs(3)))?;
                socket.set_write_timeout(Some(Duration::from_secs(3)))?;
                let connection =
                    rustls::ServerConnection::new(Arc::clone(&config)).map_err(io::Error::other)?;
                let mut stream = rustls::StreamOwned::new(connection, socket);
                let mut request = Vec::new();
                while !request.ends_with(b"\r\n\r\n") {
                    let mut byte = [0];
                    stream.read_exact(&mut byte)?;
                    request.extend(byte);
                    if request.len() > 65536 {
                        return Err(io::Error::other("large request"));
                    }
                }
                record
                    .lock()
                    .map_err(|_| io::Error::other("record lock"))?
                    .push(String::from_utf8(request).map_err(io::Error::other)?);
                write!(
                    stream,
                    "HTTP/1.1 {} Test\r\nContent-Length: {}\r\nConnection: close\r\n{}\r\n",
                    reply.status,
                    reply.body.len(),
                    reply.headers
                )?;
                let step = if reply.slow { 1 } else { 16384 };
                for chunk in reply.body.chunks(step) {
                    if stream
                        .write_all(chunk)
                        .and_then(|()| stream.flush())
                        .is_err()
                    {
                        break;
                    }
                    if reply.slow {
                        thread::sleep(Duration::from_millis(10));
                    }
                }
            }
            Ok(())
        });
        Ok(Self {
            address,
            requests,
            worker,
        })
    }
    fn finish(self) -> io::Result<Vec<String>> {
        self.worker
            .join()
            .map_err(|_| io::Error::other("server panicked"))??;
        let requests = self
            .requests
            .lock()
            .map_err(|_| io::Error::other("record lock"))?
            .clone();
        Ok(requests)
    }
}

// 作成時の関門: URL と承認の境界で、数字のホストや別の authority の解釈による迂回を捕まえる。
#[test]
fn urls_reject_ambiguous_authorities_and_canonicalize_before_approval() -> TestResult {
    let f = Fixture::new(&[])?;
    let tools = ToolSet::new(Mode::Restricted);
    let mut invalid = vec![
        "http://example.com/".to_owned(),
        "https:ex.com".into(),
        "https://user@example.com/".into(),
        "https://a@b@c/".into(),
        "https://localhost/".into(),
        "https://machine./".into(),
        "https://日本語.jp/".into(),
        "https://[::1]/".into(),
        "https://[fe80::1%25en0]/".into(),
        "https://example.com:8443/".into(),
        "https://example.com:99999/".into(),
        "https://example.com:/".into(),
        "https://2130706433/".into(),
        "https://0x7f.1/".into(),
        "https://127.1/".into(),
        "https://017700000001/".into(),
        "https://0x7f000001/".into(),
        "https://a..com/".into(),
        "https://.example/".into(),
        "https://example.com../".into(),
        "https://example.0xFF/".into(),
        "https://example.com\\@other.com/".into(),
    ];
    invalid.push(format!("https://{}.com/", "a".repeat(64)));
    invalid.push(format!(
        "https://{}.com/",
        vec!["a".repeat(63); 4].join(".")
    ));
    for url in invalid {
        let output = tools.execute(&f.context, "web_fetch", json!({"url":url}));
        assert!(
            output.is_error && output.approval.is_none(),
            "{url}: {output:?}"
        );
        assert!(output.text.contains("xn--"));
    }
    for (input, canonical, domain) in [
        (
            "https://EXAMPLE.COM.:443/日本?q=界#fragment",
            "https://example.com/%E6%97%A5%E6%9C%AC?q=%E7%95%8C",
            "example.com",
        ),
        (
            "https://xn--wgv71a.jp",
            "https://xn--wgv71a.jp/",
            "xn--wgv71a.jp",
        ),
        (
            "https://EXAMPLE.COM:443?q=x",
            "https://example.com/?q=x",
            "example.com",
        ),
    ] {
        let output = tools.execute(&f.context, "web_fetch", json!({"url":input}));
        let request = output.approval.ok_or("missing approval")?;
        assert_eq!(request.facts.url.as_deref(), Some(canonical));
        assert_eq!(request.facts.domain.as_deref(), Some(domain));
        assert_eq!(request.kind, ApprovalKind::WebFetch);
    }
    Ok(())
}

// 作成時の関門: 変換された IPv6 と範囲の境界は、公開の取得では別の拒否に隠れやすい純関数の安全契約。
#[test]
fn address_table_rejects_special_ranges_and_checks_embedded_ipv4() -> TestResult {
    for (ip, allowed) in [
        ("0.0.0.0", false),
        ("0.255.255.255", false),
        ("10.0.0.0", false),
        ("10.255.255.255", false),
        ("127.0.0.1", false),
        ("127.255.255.255", false),
        ("172.16.0.0", false),
        ("172.31.255.255", false),
        ("192.168.0.1", false),
        ("169.254.169.254", false),
        ("100.64.0.0", false),
        ("100.127.255.255", false),
        ("224.0.0.0", false),
        ("239.255.255.255", false),
        ("240.0.0.0", false),
        ("255.255.255.255", false),
        ("192.0.0.1", false),
        ("192.0.2.1", false),
        ("198.51.100.1", false),
        ("203.0.113.1", false),
        ("198.18.0.0", false),
        ("198.19.255.255", false),
        ("8.8.8.8", true),
        ("1.1.1.1", true),
        ("100.63.255.255", true),
        ("100.128.0.0", true),
        ("172.15.255.255", true),
        ("172.32.0.0", true),
        ("198.17.255.255", true),
        ("198.20.0.0", true),
        ("223.255.255.255", true),
        ("::", false),
        ("::1", false),
        ("fc00::1", false),
        ("fdff::1", false),
        ("fe80::1", false),
        ("::ffff:127.0.0.1", false),
        ("::127.0.0.1", false),
        ("::ffff:8.8.8.8", true),
        ("::8.8.8.8", true),
        ("64:ff9b::a00:1", false),
        ("64:ff9b::808:808", true),
        ("64:ff9b:1:a00:0:100::", false),
        ("64:ff9b:1:808:8:800::", false),
        ("64:ff9b:1:808:8:800:a00:1", false),
        ("64:ff9b:1::808:808", false),
        ("2002:808:808::1", false),
        ("2001::1", false),
        ("2001:db8::1", false),
        ("100::1", false),
        ("4000::1", false),
        ("ff02::1", false),
        ("2606:4700:4700::1111", true),
    ] {
        assert_eq!(address::allowed(ip.parse()?), allowed, "{ip}");
    }
    Ok(())
}

#[test]
fn resolution_waits_for_approval_and_rejects_any_forbidden_answer() -> TestResult {
    let f = Fixture::new(&["example.com"])?;
    let calls = Arc::new(AtomicUsize::new(0));
    for (url, approval, asks, count) in [
        (
            "https://sub.example.com/",
            ApprovalStatus::Unapproved,
            true,
            0,
        ),
        (
            "https://elsewhere.invalid/",
            ApprovalStatus::Unapproved,
            true,
            0,
        ),
        ("https://example.com/", ApprovalStatus::Unapproved, false, 1),
        (
            "https://elsewhere.invalid/",
            ApprovalStatus::Approved,
            false,
            2,
        ),
    ] {
        let counter = Arc::clone(&calls);
        let options = Options {
            resolve: Arc::new(move |_| {
                counter.fetch_add(1, Ordering::SeqCst);
                Ok(vec![
                    SocketAddr::from(([8, 8, 8, 8], 443)),
                    SocketAddr::from(([127, 0, 0, 1], 443)),
                ])
            }),
            allowed_address: address::allowed,
            ..f.options(Vec::new())
        };
        let output = f.run(url, approval, &BTreeSet::new(), options);
        assert_eq!(output.approval.is_some(), asks);
        assert_eq!(calls.load(Ordering::SeqCst), count);
        if !asks {
            assert!(output.is_error && output.text.contains("private or special-use"));
        }
    }
    Ok(())
}

#[test]
fn approved_fetch_pins_resolution_and_sends_only_canonical_host_and_public_headers() -> TestResult {
    let f = Fixture::new(&["mock.invalid"])?;
    let server = Server::start(vec![Reply::text("Text/Plain; CHARSET=\"UTF-8\"", "hello")])?;
    let counter = Arc::new(AtomicUsize::new(0));
    let calls = Arc::clone(&counter);
    let address = server.address;
    let options = Options {
        resolve: Arc::new(move |domain| {
            assert_eq!(domain, "mock.invalid.");
            calls.fetch_add(1, Ordering::SeqCst);
            Ok(vec![address])
        }),
        ..f.options(Vec::new())
    };
    let output = f.run(
        "https://MOCK.INVALID.:443/a?q=b#hidden",
        ApprovalStatus::Unapproved,
        &BTreeSet::new(),
        options,
    );
    assert_eq!(
        output.text,
        "URL: https://mock.invalid/a?q=b\nContent-Type: text/plain\n\nhello"
    );
    assert!(!output.is_error && output.approval.is_none());
    assert_eq!(counter.load(Ordering::SeqCst), 1);
    let requests = server.finish()?;
    let request = requests.first().ok_or("no request")?.to_ascii_lowercase();
    assert!(request.starts_with("get /a?q=b http/1.1\r\n"));
    assert!(request.contains("host: mock.invalid\r\n"));
    assert!(request.contains("user-agent: benitoiteh/0.0.1\r\n"));
    assert!(request.contains("accept: text/html, text/*;q=0.9, application/json;q=0.8\r\n"));
    for header in ["cookie:", "authorization:", "accept-encoding:"] {
        assert!(!request.contains(header));
    }
    Ok(())
}

#[test]
fn redirects_resolve_relative_references_recheck_domains_and_enforce_five_hops() -> TestResult {
    let f = Fixture::new(&[])?;
    let mut redirect = Reply::redirect("/next");
    redirect.headers.push_str("Content-Encoding: gzip\r\n");
    let server = Server::start(vec![redirect, Reply::text("text/plain", "ok")])?;
    let output = f.run(
        "https://mock.invalid/",
        ApprovalStatus::Approved,
        &BTreeSet::new(),
        f.options(vec![server.address]),
    );
    assert!(!output.is_error, "{}", output.text);
    assert_eq!(server.finish()?.len(), 2);
    for (location, path) in [
        ("https://mock.invalid/x/../result", "/result"),
        ("//mock.invalid/result", "/result"),
        ("/result", "/result"),
        ("../result", "/result"),
        ("./x/../../result", "/result"),
        ("?next=1", "/dir/page?next=1"),
        ("", "/dir/page?old=1"),
        ("x//y/../result", "/dir/x//result"),
        ("/result/.", "/result/"),
    ] {
        let server = Server::start(vec![
            Reply::redirect(location),
            Reply::text("text/plain", "ok"),
        ])?;
        let output = f.run(
            "https://mock.invalid/dir/page?old=1",
            ApprovalStatus::Approved,
            &BTreeSet::new(),
            f.options(vec![server.address]),
        );
        assert!(!output.is_error, "{location}: {}", output.text);
        let requests = server.finish()?;
        assert!(
            requests
                .get(1)
                .is_some_and(|r| r.starts_with(&format!("GET {path} HTTP/1.1\r\n"))),
            "{location}: {requests:?}"
        );
    }
    for (location, valid, message) in [
        ("https://other.invalid/next", true, "requiring approval"),
        ("http://mock.invalid/next", false, "Invalid web URL"),
        ("https://127.0.0.1/", false, "Invalid web URL"),
        ("https://mock.invalid:8443/", false, "Invalid web URL"),
    ] {
        let server = Server::start(vec![Reply::redirect(location)])?;
        let output = f.run(
            "https://mock.invalid/",
            ApprovalStatus::Approved,
            &BTreeSet::new(),
            f.options(vec![server.address]),
        );
        assert_eq!(output.is_error, !valid, "{}", output.text);
        assert!(output.text.contains(message));
        if valid {
            assert!(output.text.ends_with("URL: https://other.invalid/next"));
        }
        assert_eq!(server.finish()?.len(), 1);
    }
    for session_allow in [false, true] {
        let f = Fixture::new(if session_allow {
            &[]
        } else {
            &["mock.invalid", "other.invalid"]
        })?;
        let server = Server::start(vec![
            Reply::redirect("https://other.invalid/result"),
            Reply::text("text/plain", "cross"),
        ])?;
        let domains = if session_allow {
            BTreeSet::from(["other.invalid".into()])
        } else {
            BTreeSet::new()
        };
        let output = f.run(
            "https://mock.invalid/",
            ApprovalStatus::Approved,
            &domains,
            f.options(vec![server.address]),
        );
        assert_eq!(
            output.text,
            "URL: https://other.invalid/result\nContent-Type: text/plain\n\ncross"
        );
        assert!(
            server
                .finish()?
                .get(1)
                .is_some_and(|r| r.to_ascii_lowercase().contains("host: other.invalid\r\n"))
        );
    }
    for count in [5_usize, 6] {
        let mut replies = (0..count)
            .map(|_| Reply::redirect("/next"))
            .collect::<Vec<_>>();
        if count == 5 {
            replies.push(Reply::text("text/plain", "done"));
        }
        let server = Server::start(replies)?;
        let output = f.run(
            "https://mock.invalid/",
            ApprovalStatus::Approved,
            &BTreeSet::new(),
            f.options(vec![server.address]),
        );
        assert_eq!(output.is_error, count == 6, "{}", output.text);
        if count == 6 {
            assert!(output.text.contains("five redirects"));
        }
        assert_eq!(server.finish()?.len(), 6);
    }
    Ok(())
}

#[test]
fn redirect_to_forbidden_dns_is_not_connected_and_target_needs_no_premature_lookup() -> TestResult {
    let f = Fixture::new(&["mock.invalid", "other.invalid"])?;
    let server = Server::start(vec![Reply::redirect("https://other.invalid/")])?;
    let address = server.address;
    let calls = Arc::new(Mutex::new(Vec::new()));
    let record = Arc::clone(&calls);
    let options = Options {
        resolve: Arc::new(move |domain| {
            record
                .lock()
                .map_err(|_| io::Error::other("lock"))?
                .push(domain.to_owned());
            if domain == "mock.invalid." {
                Ok(vec![address])
            } else {
                Ok(vec![SocketAddr::from(([10, 0, 0, 1], 443))])
            }
        }),
        allowed_address: |ip| ip == IpAddr::V4(Ipv4Addr::LOCALHOST),
        ..f.options(Vec::new())
    };
    let output = f.run(
        "https://mock.invalid/",
        ApprovalStatus::Unapproved,
        &BTreeSet::new(),
        options,
    );
    assert!(output.is_error && output.text.contains("private or special-use"));
    assert_eq!(
        *calls.lock().map_err(|_| "lock")?,
        vec!["mock.invalid.", "other.invalid."]
    );
    assert_eq!(server.finish()?.len(), 1);
    // 許可がないドメインなら、リダイレクトの応答を受けた後にも名前を解かない。
    let f = Fixture::new(&["mock.invalid"])?;
    let server = Server::start(vec![Reply::redirect("https://other.invalid/")])?;
    let address = server.address;
    let options = Options {
        resolve: Arc::new(move |domain| {
            assert_eq!(domain, "mock.invalid.");
            Ok(vec![address])
        }),
        ..f.options(Vec::new())
    };
    let output = f.run(
        "https://mock.invalid/",
        ApprovalStatus::Unapproved,
        &BTreeSet::new(),
        options,
    );
    assert!(!output.is_error && output.text.contains("requiring approval"));
    server.finish()?;
    Ok(())
}

#[test]
fn response_formats_charsets_encoding_status_and_size_are_checked() -> TestResult {
    let f = Fixture::new(&["mock.invalid"])?;
    let mut cases = vec![
        (Reply::text("text/plain; charset=ISO-8859-1", [b'c',b'a',b'f',0xe9]), false, "café"),
        (Reply::text("text/plain; charset=latin1", [0xe9,0x9b]), false, "é"),
        (Reply::text("text/plain; charset=us-ascii", [b'a',0xff]), false, "a�"),
        (Reply::text("text/plain", [0xef,0xbb,0xbf,b'x',0xff]), false, "x�"),
        (Reply::text("Application/Problem+Json", "{}"), false, "{}"),
        (Reply::text("image/png", "png"), true, "Content-Type"),
        (Reply::text("garbage+json", "{}"), true, "Content-Type"),
        (Reply::text("text/plain extra", "a"), true, "Content-Type"),
        (Reply::text("\"TEXT/PLAIN\"; charset=\"UTF8\"", "quoted"), false, "quoted"),
        (Reply::text("text/plain; charset=shift_jis", "a"), true, "charset"),
        (Reply::text("text/plain", [0xff,0xfe,b'x',0]), true, "UTF-16"),
        (Reply::text("text/plain", [0xfe,0xff,0,b'x']), true, "UTF-16"),
        (Reply { headers: String::new(), ..Reply::text("text/plain", "missing") }, true, "Content-Type"),
        (Reply { headers: "Content-Type: text/plain\r\nContent-Encoding: gzip\r\n".into(), ..Reply::text("text/plain", "x") }, true, "encoded response"),
        (Reply { headers: "Content-Type: text/plain\r\nContent-Encoding: identity, gzip\r\n".into(), ..Reply::text("text/plain", "x") }, true, "encoded response"),
        (Reply { headers: "Content-Type: text/plain\r\nContent-Encoding: identity\r\nContent-Encoding: identity\r\n".into(), ..Reply::text("text/plain", "x") }, true, "encoded response"),
        (Reply { headers: "Content-Type: text/plain\r\nContent-Encoding: Identity\r\n".into(), ..Reply::text("text/plain", "ok") }, false, "ok"),
        (Reply::text("text/plain", vec![b'x'; 2 * 1024 * 1024 + 1]), true, "body"),
        (Reply { status: 404, ..Reply::text("text/html", "<p>not found</p>") }, true, "HTTP status 404"),
        (Reply { status: 500, ..Reply::text("text/plain", "z".repeat(3000)) }, true, "HTTP status 500"),
    ];
    for status in [300, 304, 305, 306, 309] {
        cases.push((
            Reply {
                status,
                ..Reply::text("text/plain", "unexpected redirect")
            },
            true,
            "HTTP status",
        ));
    }
    for (reply, error, expected) in cases {
        let status = reply.status;
        let server = Server::start(vec![reply])?;
        let output = f.run(
            "https://mock.invalid/",
            ApprovalStatus::Unapproved,
            &BTreeSet::new(),
            f.options(vec![server.address]),
        );
        assert_eq!(output.is_error, error, "{expected}: {}", output.text);
        assert!(
            output.text.contains(expected),
            "{expected}: {}",
            output.text
        );
        if status == 404 {
            assert!(output.text.ends_with("not found"));
        }
        if status == 500 {
            assert_eq!(output.text.matches('z').count(), 2048);
        }
        server.finish()?;
    }
    for headers in [
        "",
        "Location: http://mock.invalid/\r\n",
        "Location: https://\r\n",
        "Location: /a\r\nLocation: /b\r\n",
        "Location: https://mock.invalid:99999/\r\n",
    ] {
        let server = Server::start(vec![Reply {
            status: 302,
            headers: headers.into(),
            body: Vec::new(),
            slow: false,
        }])?;
        let output = f.run(
            "https://mock.invalid/",
            ApprovalStatus::Unapproved,
            &BTreeSet::new(),
            f.options(vec![server.address]),
        );
        assert!(output.is_error, "{headers}: {}", output.text);
        server.finish()?;
    }
    Ok(())
}

// 作成時の関門: HTML の表は取得結果を境界とし、タグを残す実装や過剰に消す実装の双方を捕まえる。
#[test]
fn html_table_preserves_structure_links_and_entities_without_active_content() -> TestResult {
    let f = Fixture::new(&["mock.invalid"])?;
    let cases = [
        ("before<script>if(a<!b){}</script>after", "beforeafter"),
        (
            "<script><!-- unclosed comment </script><p>shown</p>",
            "shown",
        ),
        ("<style>if(a<?b){}</style><p>shown</p>", "shown"),
        ("<iframe><![CDATA[unclosed</iframe><p>shown</p>", "shown"),
        ("a <= b", "a <= b"),
        ("a </> b", "a </> b"),
        ("< p>literal</ p>", "< p>literal</ p>"),
        ("<1tag>x</1tag>", "<1tag>x</1tag>"),
        ("before<a title='unclosed <script>hidden</script>", "before"),
        (
            "<template><title>hidden</title></template><p>shown</p>",
            "shown",
        ),
        (
            "<head><template><title>hidden</title></template></head><p>shown</p>",
            "shown",
        ),
        ("<title>A<b>B</b></title><p>body</p>", "# A<b>B</b>\n\nbody"),
        ("before<title>unclosed<p>body</p>", "before"),
        (
            "<title>one</title><title>two</title><p>body</p>",
            "# one\n\nbody",
        ),
        (
            "<head><title>Page</title><meta><body><p>shown</p>",
            "# Page\n\nshown",
        ),
        ("before<script/>hidden</script>after", "beforeafter"),
        ("before<style/>hidden</style>after", "beforeafter"),
        ("before<iframe/>hidden</iframe>after", "beforeafter"),
        ("before<noscript/>hidden</noscript>after", "beforeafter"),
        ("<pre>\n  a\n</pre>", "\n  a\n"),
        (
            "<h1>One</h1><h2>Two</h2><h6>Six</h6><p>a   b</p><div>c<br>d</div>",
            "# One\n## Two\n###### Six\na b\nc\nd",
        ),
        (
            "<ul><li>one<ul><li>two</li></ul></li><li>three</li></ul>",
            "- one\n  - two\n- three",
        ),
        (
            "<table><tr><th>A</th><th>B</th></tr><tr><td>C</td><td>D</td></tr></table>",
            "A | B\nC | D",
        ),
        (
            "<p>before</p><pre>  a\n\tb\n\n c</pre><p>after</p>",
            "before\n  a\n\tb\n\n c\nafter",
        ),
        (
            "<a href='../next?q=1&amp;b=2'>link</a> <a href='https://other.invalid/x'>absolute</a>",
            "[link](https://mock.invalid/next?q=1&b=2) [absolute](https://other.invalid/x)",
        ),
        (
            "<a href='http://other.invalid/'>http</a> <a href='javascript:alert(1)'>js</a> <a href='mailto:x@y'>mail</a>",
            "http js mail",
        ),
        (
            "<a href='//other.invalid:8443/a'>port</a><a href='/next'>root</a>",
            "[port](https://other.invalid:8443/a)[root](https://mock.invalid/next)",
        ),
        (
            "&amp;&lt;&gt;&quot;&#39;&apos;&nbsp;A &#65; &#x1F642; &#0; &#xD800; &#x110000; &unknown;",
            "&<>\"'' A A 🙂 � � � &unknown;",
        ),
        (
            "before<script>if (a < b) evil()</script ><style>x</style><noscript>bad</noscript><template>bad</template><svg>bad</svg><iframe>bad</iframe><object>bad</object><embed>bad</embed><canvas>bad</canvas>after",
            "beforeafter",
        ),
        ("before<SCRIPT>never visible", "before"),
        (
            "<!DOCTYPE html><!--hidden--><![CDATA[hidden]]><p title='a>b'>shown</p>",
            "shown",
        ),
        (
            "<html><head><title>Page &amp; title</title><style>bad</style><meta name=x></head><body><p>body</p></body></html>",
            "# Page & title\n\nbody",
        ),
        (
            "<TITLE>Outside head</TITLE><p>body</p>",
            "# Outside head\n\nbody",
        ),
        (
            "<template><template>hidden</template>still hidden</template>shown",
            "shown",
        ),
    ];
    let replies = cases
        .iter()
        .map(|(input, _)| Reply::text("application/xhtml+xml", input))
        .collect();
    let server = Server::start(replies)?;
    let mut mismatches = Vec::new();
    for (input, expected) in cases {
        let output = f.run(
            "https://mock.invalid/dir/page",
            ApprovalStatus::Unapproved,
            &BTreeSet::new(),
            f.options(vec![server.address]),
        );
        assert!(!output.is_error, "{}", output.text);
        let actual = output.text.split_once("\n\n").map(|(_, b)| b);
        if actual != Some(expected) {
            mismatches.push(format!("{input:?}: expected {expected:?}, got {actual:?}"));
        }
    }
    server.finish()?;
    assert!(mismatches.is_empty(), "{}", mismatches.join("\n"));
    // 深い箇条書きの字下げは展開せず数える。1.8 MiB の入力から巨大な中間文字列を作らない。
    let nested = format!(
        "{}x{}",
        "<ul><li>".repeat(100000),
        "</li></ul>".repeat(100000)
    );
    let server = Server::start(vec![Reply::text("text/html", nested)])?;
    let output = f.run(
        "https://mock.invalid/",
        ApprovalStatus::Unapproved,
        &BTreeSet::new(),
        f.options(vec![server.address]),
    );
    assert!(!output.is_error, "{}", output.text);
    assert!(
        output
            .text
            .ends_with("[Showing the first 51200 of 10000200000 bytes of the converted text.]")
    );
    server.finish()?;
    // 再帰の深さと、閉じていない引用符の再走査を入力サイズに応じて増やさない。
    for input in [
        format!("{}text{}", "<div>".repeat(100000), "</div>".repeat(100000)),
        format!("<a title='{}", "<a ".repeat(100000)),
    ] {
        let server = Server::start(vec![Reply::text("text/html", input)])?;
        let output = f.run(
            "https://mock.invalid/",
            ApprovalStatus::Unapproved,
            &BTreeSet::new(),
            f.options(vec![server.address]),
        );
        assert!(!output.is_error);
        server.finish()?;
    }
    Ok(())
}

#[test]
fn control_characters_are_removed_after_conversion_for_every_format() -> TestResult {
    let f = Fixture::new(&["mock.invalid"])?;
    for (kind, input, expected) in [
        (
            "text/html",
            "<p>A&#x1b;[2J&#x9b;B\u{1b}C&#13;&#127;&#128;</p>",
            "A[2JBC",
        ),
        (
            "text/plain",
            "A\u{1b}[2J\u{9b}B\r\u{7f}\u{80}\0\n\tC",
            "A[2JB\n\tC",
        ),
        (
            "application/json",
            "{\"a\":\"A\u{1b}[2J\u{9b}B\r\u{7f}\u{80}\0\"}",
            "{\"a\":\"A[2JB\"}",
        ),
    ] {
        let server = Server::start(vec![Reply::text(kind, input)])?;
        let output = f.run(
            "https://mock.invalid/",
            ApprovalStatus::Unapproved,
            &BTreeSet::new(),
            f.options(vec![server.address]),
        );
        assert_eq!(
            output.text.split_once("\n\n").map(|(_, b)| b),
            Some(expected)
        );
        assert!(
            !output
                .text
                .chars()
                .any(|c| c.is_control() && c != '\n' && c != '\t')
        );
        server.finish()?;
    }
    Ok(())
}

#[test]
fn converted_text_is_limited_by_bytes_without_a_line_limit_or_spill() -> TestResult {
    let f = Fixture::new(&["mock.invalid"])?;
    for body in ["x\n".repeat(25600), "界".repeat(20000)] {
        let server = Server::start(vec![Reply::text("text/plain", &body)])?;
        let output = f.run(
            "https://mock.invalid/",
            ApprovalStatus::Unapproved,
            &BTreeSet::new(),
            f.options(vec![server.address]),
        );
        let converted = output
            .text
            .split_once("\n\n")
            .map(|(_, b)| b)
            .ok_or("no body")?;
        assert!(!output.is_error);
        assert!(output.spill.is_none());
        if body.len() == 51200 {
            assert_eq!(converted, body);
        } else {
            assert!(converted.starts_with(&"界".repeat(17066)));
            assert!(
                converted
                    .ends_with("[Showing the first 51200 of 60000 bytes of the converted text.]")
            );
            assert!(!converted.contains('�'));
        }
        server.finish()?;
    }
    assert_eq!(fs::read_dir(f.root.join("spill"))?.count(), 0);
    Ok(())
}

#[test]
fn total_deadline_and_cancellation_interrupt_resolution_and_slow_responses() -> TestResult {
    let f = Fixture::new(&["mock.invalid"])?;
    let options = Options {
        resolve: Arc::new(|_| {
            thread::sleep(Duration::from_millis(300));
            Ok(Vec::new())
        }),
        total: Duration::from_millis(50),
        ..f.options(Vec::new())
    };
    let started = Instant::now();
    let output = f.run(
        "https://mock.invalid/",
        ApprovalStatus::Unapproved,
        &BTreeSet::new(),
        options,
    );
    assert!(output.is_error && output.text.contains("total time limit"));
    assert!(started.elapsed() < Duration::from_millis(250));
    let cancelled_dns = AtomicBool::new(false);
    let (ready, receive) = mpsc::sync_channel(1);
    let options = Options {
        resolve: Arc::new(move |_| {
            let _sent = ready.send(());
            thread::sleep(Duration::from_millis(300));
            Ok(Vec::new())
        }),
        ..f.options(Vec::new())
    };
    let start = Instant::now();
    let output = thread::scope(|scope| {
        let cancel_flag = &cancelled_dns;
        let worker = scope.spawn(move || {
            if receive.recv_timeout(Duration::from_secs(1)).is_ok() {
                cancel_flag.store(true, Ordering::Release);
            }
        });
        let output = execute_with(
            &f.context,
            &json!({"url":"https://mock.invalid/"}),
            ApprovalStatus::Unapproved,
            Invocation {
                call_id: "dns",
                cancelled: &cancelled_dns,
                confirmed_effects: &std::collections::BTreeMap::new(),
                session_domains: &BTreeSet::new(),
            },
            options,
        );
        assert!(worker.join().is_ok());
        output
    });
    assert!(output.text.contains("cancelled"));
    assert!(start.elapsed() < Duration::from_millis(250));
    for cancel in [false, true] {
        let server = Server::start(vec![Reply {
            slow: true,
            ..Reply::text("text/plain", vec![b'x'; 1000])
        }])?;
        let cancelled = AtomicBool::new(false);
        let started = Instant::now();
        let output = thread::scope(|scope| -> Result<ToolOutput, Box<dyn Error>> {
            let options = Options {
                total: if cancel {
                    Duration::from_secs(2)
                } else {
                    Duration::from_millis(250)
                },
                ..f.options(vec![server.address])
            };
            let worker = scope.spawn(|| {
                execute_with(
                    &f.context,
                    &json!({"url":"https://mock.invalid/"}),
                    ApprovalStatus::Unapproved,
                    Invocation {
                        call_id: "slow",
                        cancelled: &cancelled,
                        confirmed_effects: &std::collections::BTreeMap::new(),
                        session_domains: &BTreeSet::new(),
                    },
                    options,
                )
            });
            if cancel {
                while server.requests.lock().map_err(|_| "lock")?.is_empty() {
                    if started.elapsed() > Duration::from_secs(1) {
                        return Err("no request".into());
                    }
                    thread::sleep(Duration::from_millis(2));
                }
                cancelled.store(true, Ordering::Release);
            }
            worker.join().map_err(|_| "worker panicked".into())
        })?;
        assert!(output.is_error, "{}", output.text);
        if cancel {
            assert!(output.text.contains("cancelled"));
            assert!(started.elapsed() < Duration::from_secs(1));
        } else {
            assert!(output.text.contains("time"), "{}", output.text);
            assert!(started.elapsed() < Duration::from_secs(1));
        }
        server.finish()?;
    }
    Ok(())
}
