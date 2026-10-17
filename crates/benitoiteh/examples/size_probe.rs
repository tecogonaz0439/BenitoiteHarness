//! 依存の主な機能を呼び、三つの対象でリンク後の大きさを測る（設計書 06-02「測り方」）。

use std::fmt;
use std::io;
use std::path::Path;
use std::process::ExitCode;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use benitoiteh::config::Mode;
use benitoiteh::context::{ContextError, SystemContext};
use benitoiteh::http::{HeaderMap, HttpClient, HttpError, Uri};
use benitoiteh::provider::sse::SseDecoder;
use benitoiteh::sandbox::{PolicyConfig, SandboxError, SandboxPolicy};
use benitoiteh::tools::ToolSet;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use unicode_width::UnicodeWidthStr;

const CONFIG: &str = r#"
pattern = "[[:alpha:]]+"
glob = "**/*.rs"
before = "Benitoite\n"
after = "BenitoiteHarness ベニトアイト\n"
"#;
const SSE: &[u8] = b"event: message\ndata: {\"text\":\"size probe\"}\n\n";

#[derive(Debug, Deserialize, Serialize)]
struct ProbeConfig {
    pattern: String,
    glob: String,
    before: String,
    after: String,
}

#[derive(Debug)]
enum ProbeError {
    Io(io::Error),
    Toml(toml::de::Error),
    Json(serde_json::Error),
    Regex(regex::Error),
    Glob(globset::Error),
    Sandbox(SandboxError),
    Context(ContextError),
    Http(HttpError),
    Arguments,
}

impl fmt::Display for ProbeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => error.fmt(f),
            Self::Toml(error) => error.fmt(f),
            Self::Json(error) => error.fmt(f),
            Self::Regex(error) => error.fmt(f),
            Self::Glob(error) => error.fmt(f),
            Self::Sandbox(error) => error.fmt(f),
            Self::Context(error) => error.fmt(f),
            Self::Http(error) => error.fmt(f),
            Self::Arguments => f.write_str(text::USAGE),
        }
    }
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{}", text::failed(&error));
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), ProbeError> {
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        None => probe(Path::new(".")),
        Some("--help") => {
            println!("{}", text::USAGE);
            Ok(())
        }
        Some("--http") => {
            let url = args.next().ok_or(ProbeError::Arguments)?;
            if args.next().is_some() {
                return Err(ProbeError::Arguments);
            }
            // 接続する分岐も残し、TLS と OS の証明書の検証を含めて測る。
            // 引数なしの実行では通信しない（設計書 06-02「目標」）。
            let uri: Uri = url
                .parse()
                .map_err(|_| ProbeError::Http(HttpError::InvalidRequest))?;
            let response = HttpClient::default()
                .get(&uri, &HeaderMap::new(), 64 * 1024)
                .map_err(ProbeError::Http)?;
            println!("{}", text::value(text::HTTP, response));
            Ok(())
        }
        Some("--terminal") => terminal(),
        Some(path) => {
            if args.next().is_some() {
                return Err(ProbeError::Arguments);
            }
            probe(Path::new(path))
        }
    }
}

fn probe(root: &Path) -> Result<(), ProbeError> {
    let config: ProbeConfig = toml::from_str(CONFIG).map_err(ProbeError::Toml)?;
    let json = serde_json::to_string(&config).map_err(ProbeError::Json)?;
    let config: ProbeConfig = serde_json::from_str(&json).map_err(ProbeError::Json)?;
    println!("{}", text::value(text::CONFIG, &config));

    let pattern = regex::Regex::new(&config.pattern).map_err(ProbeError::Regex)?;
    let matches: Vec<_> = pattern
        .find_iter(&config.after)
        .map(|m| m.as_str())
        .collect();
    println!("{}", text::value(text::MATCHES, matches));
    let glob = globset::Glob::new(&config.glob)
        .map_err(ProbeError::Glob)?
        .compile_matcher();
    // 走査結果も表示し、ignore の規則と glob の照合を最適化で消させない。
    for entry in ignore::WalkBuilder::new(root).build() {
        match entry {
            Ok(entry) if glob.is_match(entry.path()) => {
                println!("{}", text::value(text::PATH, entry.path()));
            }
            Ok(_) => {}
            Err(error) => println!("{}", text::value(text::WALK_ERROR, error)),
        }
    }
    let diff = similar::TextDiff::from_lines(&config.before, &config.after)
        .unified_diff()
        .to_string();
    println!("{}", text::value(text::DIFF, diff));
    println!("{}", text::value(text::HASH, Sha256::digest(&config.after)));
    println!("{}", text::value(text::WIDTH, config.after.width()));

    let mut random = [0_u8; 16];
    println!(
        "{}",
        text::value(text::RANDOM, getrandom::fill(&mut random))
    );
    println!("{}", text::value(text::SESSION_ID, random));
    println!("{}", text::value(text::PID, rustix::process::getpid()));
    let interrupted = Arc::new(AtomicBool::new(false));
    let signal = signal_hook::flag::register(signal_hook::consts::SIGINT, Arc::clone(&interrupted))
        .map_err(ProbeError::Io)?;
    println!(
        "{}",
        text::value(text::INTERRUPTED, interrupted.load(Ordering::Relaxed))
    );
    println!(
        "{}",
        text::value(
            text::UNREGISTERED,
            signal_hook::low_level::unregister(signal)
        )
    );

    let root = root.canonicalize().map_err(ProbeError::Io)?;
    let policy = SandboxPolicy::new(PolicyConfig::new(vec![root])).map_err(ProbeError::Sandbox)?;
    let json = serde_json::to_string(&policy).map_err(ProbeError::Json)?;
    let policy: SandboxPolicy = serde_json::from_str(&json).map_err(ProbeError::Json)?;
    println!("{}", text::value(text::POLICY, &policy));
    #[cfg(target_os = "macos")]
    println!("{}", text::value(text::SEATBELT, policy.seatbelt_profile()));

    let mut decoder = SseDecoder::new();
    println!("{}", text::value(text::SSE, decoder.push(SSE)));
    decoder.finish();
    // 処理系なしの文脈を表示し、固定のプロンプトの領域も測る。
    // （設計書 06-02「目標」）
    let context = SystemContext::new(&ToolSet::without_benitoite(Mode::Normal), None)
        .map_err(ProbeError::Context)?;
    println!("{}", text::value(text::CONTEXT, context));
    println!("{}", text::value(text::HTTP, HttpClient::default()));
    println!(
        "{}",
        text::value(text::TERMINAL_SIZE, crossterm::terminal::size())
    );
    Ok(())
}

fn terminal() -> Result<(), ProbeError> {
    // 端末に触る分岐を明示して、測定だけの実行でも入力待ちにならないようにする。
    // （設計書 06-02「測り方」）
    crossterm::execute!(
        io::stdout(),
        crossterm::cursor::MoveToColumn(0),
        crossterm::terminal::Clear(crossterm::terminal::ClearType::CurrentLine),
        crossterm::style::Print(text::TERMINAL_PROMPT),
    )
    .map_err(ProbeError::Io)?;
    println!(
        "{}",
        text::value(text::TERMINAL_SIZE, crossterm::terminal::size())
    );
    if crossterm::event::poll(Duration::from_millis(100)).map_err(ProbeError::Io)? {
        let event = crossterm::event::read().map_err(ProbeError::Io)?;
        println!("{}", text::value(text::EVENT, event));
    }
    Ok(())
}

mod text {
    //! 測定用の出力をまとめる（設計書 06-01「lint と実装の規約」）。

    use std::fmt::{Debug, Display};

    pub const USAGE: &str = "Usage: size_probe [ROOT] | --http URL | --terminal | --help";
    pub const CONFIG: &str = "config";
    pub const MATCHES: &str = "matches";
    pub const PATH: &str = "path";
    pub const WALK_ERROR: &str = "walk error";
    pub const DIFF: &str = "diff";
    pub const HASH: &str = "sha256";
    pub const WIDTH: &str = "width";
    pub const RANDOM: &str = "random";
    pub const SESSION_ID: &str = "session id";
    pub const PID: &str = "pid";
    pub const INTERRUPTED: &str = "interrupted";
    pub const UNREGISTERED: &str = "signal unregistered";
    pub const POLICY: &str = "sandbox policy";
    #[cfg(target_os = "macos")]
    pub const SEATBELT: &str = "seatbelt profile";
    pub const SSE: &str = "sse";
    pub const CONTEXT: &str = "context";
    pub const HTTP: &str = "http";
    pub const TERMINAL_SIZE: &str = "terminal size";
    pub const TERMINAL_PROMPT: &str = "Size probe: polling terminal input for 100 ms.\n";
    pub const EVENT: &str = "event";

    pub fn value(label: &str, value: impl Debug) -> String {
        format!("{label}: {value:?}")
    }

    pub fn failed(error: &impl Display) -> String {
        format!("Size probe failed: {error}")
    }
}
