//! プロキシへの要求の頭を読み、平文の HTTP の要求を宛先へ渡す形に直す（設計書 03-02「通信の中継」）。

/// 読んだ要求。`forward` がなければ `CONNECT` のトンネル。
pub(super) struct Request {
    pub host: String,
    pub port: u16,
    pub forward: Option<Forward>,
}

/// 平文の HTTP の要求を、宛先へ渡す形に直すための材料。
pub(super) struct Forward {
    method: String,
    target: String,
    headers: Vec<(String, String)>,
}

#[derive(Debug, Eq, PartialEq)]
pub(super) enum ParseError {
    Invalid,
    /// `https://` を絶対形で送った。暗号化の前の要求になるので、中継しない。
    TlsWithoutConnect(String, u16),
    /// URL に利用者の情報（`user:pass@`）を含む。
    Credentials(String, u16),
}

/// 宛先へ渡さないヘッダ。プロキシ向けのものと、接続の扱いを決めるもの。
const HOP_HEADERS: &[&str] = &[
    "proxy-authorization",
    "proxy-connection",
    "connection",
    "keep-alive",
    "host",
];

impl Request {
    pub(super) fn parse(head: &[u8]) -> Result<Self, ParseError> {
        let head = std::str::from_utf8(head).map_err(|_| ParseError::Invalid)?;
        let head = head.strip_suffix("\r\n\r\n").ok_or(ParseError::Invalid)?;
        let mut lines = head.split("\r\n");
        let line = lines.next().ok_or(ParseError::Invalid)?;
        let mut parts = line.split(' ');
        let (Some(method), Some(target), Some(version), None) =
            (parts.next(), parts.next(), parts.next(), parts.next())
        else {
            return Err(ParseError::Invalid);
        };
        if method.is_empty()
            || !method
                .bytes()
                .all(|byte| byte.is_ascii_uppercase() || byte == b'-')
            || !matches!(version, "HTTP/1.1" | "HTTP/1.0")
            // 要求の行の対象に制御の文字を入れて、宛先へ別の要求の行やヘッダを流し込ませない。
            || target.bytes().any(|byte| byte < 0x21 || byte == 0x7f)
        {
            return Err(ParseError::Invalid);
        }
        let mut headers = Vec::new();
        for header in lines {
            // 折り返したヘッダと、制御の文字を含むヘッダは受け付けない。
            if header.starts_with([' ', '\t'])
                || header.bytes().any(|byte| byte < 0x20 && byte != b'\t')
            {
                return Err(ParseError::Invalid);
            }
            let (name, value) = header.split_once(':').ok_or(ParseError::Invalid)?;
            if name.is_empty() || !name.bytes().all(|byte| byte.is_ascii_graphic()) {
                return Err(ParseError::Invalid);
            }
            headers.push((name.to_owned(), value.trim().to_owned()));
        }
        if method == "CONNECT" {
            let (host, port) = authority(target, None)?;
            return Ok(Self {
                host,
                port,
                forward: None,
            });
        }
        let lower = target.get(..8).map(str::to_ascii_lowercase);
        if lower.as_deref() == Some("https://") {
            let rest = target.get(8..).ok_or(ParseError::Invalid)?;
            let (host, port) = authority(split_authority(rest).0, Some(443))?;
            return Err(ParseError::TlsWithoutConnect(host, port));
        }
        if target.get(..7).map(str::to_ascii_lowercase).as_deref() != Some("http://") {
            return Err(ParseError::Invalid);
        }
        let rest = target.get(7..).ok_or(ParseError::Invalid)?;
        let (authority_part, path) = split_authority(rest);
        if let Some((_, host)) = authority_part.rsplit_once('@') {
            let (host, port) = authority(host, Some(80)).unwrap_or((host.to_owned(), 80));
            return Err(ParseError::Credentials(host, port));
        }
        let (host, port) = authority(authority_part, Some(80))?;
        let target = if path.starts_with('/') {
            path.to_owned()
        } else {
            format!("/{path}")
        };
        Ok(Self {
            host,
            port,
            forward: Some(Forward {
                method: method.to_owned(),
                target,
                headers,
            }),
        })
    }
}

/// 要求の頭の `Proxy-Authorization` が `Basic base64("benitoiteh:<合言葉>")` と一致するか。
/// 要求の行を解釈する前に確かめられるよう、頭のバイト列から直に探す（設計書 03-02「通信の中継」）。
pub(super) fn head_authorized(head: &[u8], token: &str) -> bool {
    let expected = format!("Basic {}", base64(format!("benitoiteh:{token}").as_bytes()));
    head.split(|byte| *byte == b'\n')
        .skip(1)
        .filter_map(|line| {
            let line = line.strip_suffix(b"\r").unwrap_or(line);
            let colon = line.iter().position(|byte| *byte == b':')?;
            let (name, value) = (line.get(..colon)?, line.get(colon.saturating_add(1)..)?);
            name.eq_ignore_ascii_case(b"proxy-authorization")
                .then(|| value.trim_ascii())
        })
        .any(|value| {
            value.len() == expected.len()
                && value
                    .iter()
                    .zip(expected.bytes())
                    .fold(0_u8, |difference, (left, right)| {
                        difference | (left ^ right)
                    })
                    == 0
        })
}

impl Forward {
    /// 宛先へ送る頭。要求の行を通常の形にし、`Host` を宛先に合わせ、接続は一回で閉じる。
    pub(super) fn head(&self, host: &str, port: u16) -> Vec<u8> {
        let host_header = if port == 80 {
            host.to_owned()
        } else {
            format!("{host}:{port}")
        };
        let mut head = format!(
            "{} {} HTTP/1.1\r\nHost: {host_header}\r\n",
            self.method, self.target
        );
        for (name, value) in &self.headers {
            if !HOP_HEADERS.iter().any(|hop| name.eq_ignore_ascii_case(hop)) {
                head.push_str(&format!("{name}: {value}\r\n"));
            }
        }
        head.push_str("Connection: close\r\n\r\n");
        head.into_bytes()
    }
}

/// `host[:port]` を分ける。角括弧の IPv6 の書き方は、ドメインでないので受け付けない。
fn authority(text: &str, default: Option<u16>) -> Result<(String, u16), ParseError> {
    if text.is_empty() || text.starts_with('[') {
        return Err(ParseError::Invalid);
    }
    match text.rsplit_once(':') {
        Some((host, port)) => {
            let port = port.parse::<u16>().map_err(|_| ParseError::Invalid)?;
            if host.is_empty() || port == 0 {
                return Err(ParseError::Invalid);
            }
            Ok((host.to_owned(), port))
        }
        None => Ok((text.to_owned(), default.ok_or(ParseError::Invalid)?)),
    }
}

/// 絶対形の URL の、スキームの後ろを、宛先の部分と残りに分ける。
fn split_authority(rest: &str) -> (&str, &str) {
    let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    (
        rest.get(..end).unwrap_or_default(),
        rest.get(end..).unwrap_or_default(),
    )
}

fn base64(input: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let symbol = |index: u32| {
        TABLE
            .get(usize::try_from(index & 63).unwrap_or_default())
            .copied()
            .map_or('=', char::from)
    };
    let mut output = String::new();
    for chunk in input.chunks(3) {
        let first = chunk.first().copied().unwrap_or_default();
        let second = chunk.get(1).copied().unwrap_or_default();
        let third = chunk.get(2).copied().unwrap_or_default();
        let value = u32::from(first) << 16 | u32::from(second) << 8 | u32::from(third);
        output.push(symbol(value >> 18));
        output.push(symbol(value >> 12));
        output.push(if chunk.len() > 1 {
            symbol(value >> 6)
        } else {
            '='
        });
        output.push(if chunk.len() > 2 { symbol(value) } else { '=' });
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    // 作成時の関門: 要求の行の二つの形と、断る形を、頭の文字列から確かめる。
    #[test]
    fn requests_parse_connect_and_absolute_forms_and_reject_unsafe_forms() -> TestResult {
        let connect = Request::parse(b"CONNECT example.com:443 HTTP/1.1\r\nHost: x\r\n\r\n")
            .map_err(|error| format!("{error:?}"))?;
        assert_eq!((connect.host.as_str(), connect.port), ("example.com", 443));
        assert!(connect.forward.is_none());
        let plain = Request::parse(
            b"GET http://Example.com/a?b HTTP/1.1\r\nHost: evil.test\r\nProxy-Authorization: Basic x\r\nProxy-Connection: keep-alive\r\nAccept: */*\r\n\r\n",
        )
        .map_err(|error| format!("{error:?}"))?;
        assert_eq!((plain.host.as_str(), plain.port), ("Example.com", 80));
        let forward = plain.forward.as_ref().ok_or("forward")?;
        assert_eq!(
            String::from_utf8(forward.head("example.com", 80))?,
            "GET /a?b HTTP/1.1\r\nHost: example.com\r\nAccept: */*\r\nConnection: close\r\n\r\n"
        );
        assert!(
            String::from_utf8(forward.head("example.com", 8080))?
                .starts_with("GET /a?b HTTP/1.1\r\nHost: example.com:8080\r\n")
        );
        for (input, expected) in [
            (
                &b"GET https://example.com/ HTTP/1.1\r\n\r\n"[..],
                ParseError::TlsWithoutConnect("example.com".into(), 443),
            ),
            (
                b"GET http://u:p@example.com/ HTTP/1.1\r\n\r\n",
                ParseError::Credentials("example.com".into(), 80),
            ),
            (b"GET /relative HTTP/1.1\r\n\r\n", ParseError::Invalid),
            (
                b"GET http://example.com/a\nX:y HTTP/1.1\r\n\r\n",
                ParseError::Invalid,
            ),
            (
                b"GET http://example.com/\x7f HTTP/1.1\r\n\r\n",
                ParseError::Invalid,
            ),
            (b"CONNECT [::1]:443 HTTP/1.1\r\n\r\n", ParseError::Invalid),
            (b"CONNECT example.com HTTP/1.1\r\n\r\n", ParseError::Invalid),
            (
                b"GET http://example.com/ HTTP/2\r\n\r\n",
                ParseError::Invalid,
            ),
            (
                b"GET http://example.com/ HTTP/1.1\r\nA: b\r\n folded\r\n\r\n",
                ParseError::Invalid,
            ),
        ] {
            assert_eq!(
                Request::parse(input).err(),
                Some(expected),
                "{}",
                String::from_utf8_lossy(input)
            );
        }
        Ok(())
    }

    // 作成時の関門: 合言葉の照合を、既知の base64 の値（RFC 4648 の例）とともに確かめる。
    #[test]
    fn proxy_authorization_uses_basic_with_the_session_token() -> TestResult {
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"f"), "Zg==");
        let head = format!(
            "CONNECT example.com:443 HTTP/1.1\r\nproxy-authorization:  Basic {}\r\n\r\n",
            base64(b"benitoiteh:secret")
        );
        assert!(head_authorized(head.as_bytes(), "secret"));
        assert!(!head_authorized(head.as_bytes(), "other"));
        // 解釈できない要求の行でも、合言葉は先に確かめられる。要求の行の中の名前は、ヘッダとして扱わない。
        let unparsable = format!(
            "GET https://x/ HTTP/1.1\r\nProxy-Authorization: Basic {}\r\n\r\n",
            base64(b"benitoiteh:secret")
        );
        assert!(head_authorized(unparsable.as_bytes(), "secret"));
        assert!(!head_authorized(
            b"Proxy-Authorization: Basic YmVuaXRvaXRlaDpzZWNyZXQ= HTTP/1.1\r\n\r\n",
            "secret"
        ));
        Ok(())
    }
}
