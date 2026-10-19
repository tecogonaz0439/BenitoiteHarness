//! 公開の入口と本物の端末で性能を測る手動の道具（設計書 06-02「測り方」）。

#[allow(dead_code, unused_imports)]
#[path = "support/exec_fixture.rs"]
mod fixture;
#[allow(dead_code)]
mod support;
#[allow(dead_code)]
#[path = "support/tty.rs"]
mod tty;

use benitoiteh::config::{Mode, ReadConfig, paths::DataPaths};
use benitoiteh::safe_fs::ScopeRoot;
use benitoiteh::tools::{ToolContext, ToolSet, exclude::ExcludeRules};
use fixture::{Fixture, Protocol};
use serde_json::json;
use std::error::Error;
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};
use support::mock_provider::{Delivery, MockProvider, ResponseScript};

type MeasureResult<T = ()> = Result<T, Box<dyn Error>>;
const POLL: Duration = Duration::from_millis(1);
const LIMIT: Duration = Duration::from_secs(15);

fn macos() -> bool {
    if cfg!(target_os = "macos") {
        true
    } else {
        eprintln!("SKIP: performance measurements require macOS (ps and /usr/bin/time -l).");
        false
    }
}

fn runs() -> MeasureResult<usize> {
    let value = match std::env::var("BENITOITEH_PERF_RUNS") {
        Ok(value) => value,
        Err(std::env::VarError::NotPresent) => "10".into(),
        Err(std::env::VarError::NotUnicode(_)) => {
            return Err("BENITOITEH_PERF_RUNS must be a positive integer".into());
        }
    };
    let count = value
        .parse::<usize>()
        .map_err(|_| "BENITOITEH_PERF_RUNS must be a positive integer")?;
    if count == 0 {
        return Err("BENITOITEH_PERF_RUNS must be at least 1".into());
    }
    Ok(count)
}

fn binary() -> PathBuf {
    std::env::var_os("BENITOITEH_PERF_BINARY")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_BIN_EXE_benitoiteh")))
}

fn emit(item: &str, unit: &str, samples: impl serde::Serialize, note: &str) {
    println!(
        "\nPERF {}",
        json!({"item":item,"unit":unit,"samples":samples,
        "note":format!("{note}; binary={}", binary().display())})
    );
}

fn ms(duration: Duration) -> f64 {
    duration.as_secs_f64() * 1000.0
}

struct Workspace(PathBuf);
impl Workspace {
    fn new() -> MeasureResult<Self> {
        let mut random = [0; 16];
        getrandom::fill(&mut random)?;
        let root = std::env::temp_dir().join(format!(
            "benitoiteh-perf-{:032x}",
            u128::from_ne_bytes(random)
        ));
        fs::create_dir(&root)?;
        let workspace = Self(fs::canonicalize(root)?);
        for directory in [
            "project",
            "home",
            "config/benitoiteh",
            "state",
            "tmp",
            "spill",
        ] {
            fs::create_dir_all(workspace.0.join(directory))?;
        }
        Ok(workspace)
    }

    fn configure(&self, url: &str, restricted: bool) -> MeasureResult {
        let mode = if restricted {
            ""
        } else {
            "default_mode = 'normal'\n"
        };
        let processor = if restricted {
            support::benitoite()?
        } else {
            self.0.join("missing-benitoite")
        };
        fs::write(
            self.0.join("config/benitoiteh/config.toml"),
            format!(
                "{mode}default_provider = 'mock'\n[providers.mock]\nkind = 'openai-chat'\nbase_url = {url:?}\nmodel = 'mock-model'\napi_key = {{ env = 'BENITOITEH_EXEC_TEST_KEY' }}\ncapabilities = {{ reasoning_replay = 'same-destination' }}\n[benitoite]\npath = {}\n",
                serde_json::to_string(&processor)?
            ),
        )?;
        Ok(())
    }
}
impl Drop for Workspace {
    fn drop(&mut self) {
        let _removed = fs::remove_dir_all(&self.0);
    }
}

fn command(root: &Path, program: &Path) -> Command {
    let mut command = Command::new(program);
    command
        .current_dir(root.join("project"))
        .env_clear()
        .env("PATH", "")
        .env("HOME", root.join("home"))
        .env("XDG_CONFIG_HOME", root.join("config"))
        .env("XDG_STATE_HOME", root.join("state"))
        .env("TMPDIR", root.join("tmp"))
        .env("BENITOITEH_EXEC_TEST_KEY", fixture::KEY)
        .process_group(0)
        .stdin(Stdio::null());
    command
}

struct Running {
    child: Child,
    reaped: bool,
}
impl Running {
    fn spawn(command: &mut Command) -> io::Result<Self> {
        Ok(Self {
            child: command.spawn()?,
            reaped: false,
        })
    }
    fn status(&mut self) -> io::Result<Option<ExitStatus>> {
        let status = self.child.try_wait()?;
        self.reaped = status.is_some();
        Ok(status)
    }
    fn finish(&mut self, limit: Duration) -> MeasureResult<ExitStatus> {
        let start = Instant::now();
        loop {
            if let Some(status) = self.status()? {
                return Ok(status);
            }
            if start.elapsed() >= limit {
                return Err("Measurement child timed out".into());
            }
            thread::sleep(POLL);
        }
    }
}
impl Drop for Running {
    fn drop(&mut self) {
        if !self.reaped {
            let _group = rustix::process::kill_process_group(
                rustix::process::Pid::from_child(&self.child),
                rustix::process::Signal::KILL,
            );
            let _killed = self.child.kill();
            let _waited = self.child.wait();
        }
    }
}

fn logs(command: &mut Command, root: &Path) -> io::Result<()> {
    // 200 番の出力がパイプを満たして子を止めないよう、ファイルへ排出する。
    command
        .stdout(File::create(root.join("stdout"))?)
        .stderr(File::create(root.join("stderr"))?);
    Ok(())
}
fn success(status: ExitStatus, root: &Path) -> MeasureResult {
    if !status.success() {
        return Err(format!(
            "Measurement child exited {status}; stderr: {}",
            fs::read_to_string(root.join("stderr"))?
        )
        .into());
    }
    Ok(())
}

// 作成時の関門: 公開の --version の終了時間を測る道具であり、時間の閾値で判定しない。
#[test]
#[ignore = "manual release performance measurement"]
fn version_ms() -> MeasureResult {
    if !macos() {
        return Ok(());
    }
    let mut samples = Vec::new();
    for index in 0..=runs()? {
        let workspace = Workspace::new()?;
        let mut command = command(&workspace.0, &binary());
        command.arg("--version");
        logs(&mut command, &workspace.0)?;
        let start = Instant::now();
        let status = command.status()?;
        let elapsed = ms(start.elapsed());
        success(status, &workspace.0)?;
        if index != 0 {
            samples.push(elapsed);
        }
    }
    emit(
        "version_ms",
        "ms",
        samples,
        "spawn to exit; one warm-up; fresh isolated directories per run",
    );
    Ok(())
}

// 5 ms の accept ポーリングによる誤差を避ける専用の一本の待ち受け。
struct OneRequest {
    address: SocketAddr,
    accepted: Receiver<Instant>,
    received: Receiver<Instant>,
    worker: Option<JoinHandle<io::Result<()>>>,
}
impl OneRequest {
    fn start() -> io::Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let address = listener.local_addr()?;
        let (accepted_tx, accepted) = mpsc::channel();
        let (received_tx, received) = mpsc::channel();
        let worker = thread::spawn(move || {
            let (mut stream, _) = listener.accept()?;
            let at = Instant::now();
            accepted_tx.send(at).map_err(io::Error::other)?;
            stream.set_read_timeout(Some(Duration::from_secs(5)))?;
            stream.set_write_timeout(Some(Duration::from_secs(5)))?;
            stream.set_nodelay(true)?;
            receive_request(&mut stream)?;
            received_tx.send(Instant::now()).map_err(io::Error::other)?;
            let body = Protocol::Chat.response(None, "Done.", false);
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            )?;
            stream.write_all(&body)
        });
        Ok(Self {
            address,
            accepted,
            received,
            worker: Some(worker),
        })
    }
    fn finish(&mut self) -> MeasureResult {
        self.worker
            .take()
            .ok_or("Missing accept worker")?
            .join()
            .map_err(|_| "Accept worker panicked")??;
        Ok(())
    }
}
impl Drop for OneRequest {
    fn drop(&mut self) {
        // 起動失敗でも blocking accept とそのスレッドを残さない。
        if let Some(worker) = self.worker.take() {
            let _wake = TcpStream::connect(self.address);
            let _joined = worker.join();
        }
    }
}
fn receive_request(stream: &mut TcpStream) -> io::Result<()> {
    let mut bytes = Vec::new();
    let mut buffer = [0; 8192];
    loop {
        let count = stream.read(&mut buffer)?;
        if count == 0 {
            return Err(io::ErrorKind::UnexpectedEof.into());
        }
        bytes.extend_from_slice(buffer.get(..count).ok_or(io::ErrorKind::InvalidData)?);
        if bytes.len() > 16 * 1024 * 1024 {
            return Err(io::ErrorKind::InvalidData.into());
        }
        if let Some(head) = bytes.windows(4).position(|v| v == b"\r\n\r\n") {
            let end = head.saturating_add(4);
            let headers = std::str::from_utf8(bytes.get(..head).ok_or(io::ErrorKind::InvalidData)?)
                .map_err(io::Error::other)?;
            let length = headers
                .lines()
                .filter_map(|line| line.split_once(':'))
                .find(|(name, _)| name.eq_ignore_ascii_case("content-length"))
                .and_then(|(_, length)| length.trim().parse::<usize>().ok())
                .ok_or(io::ErrorKind::InvalidData)?;
            if bytes.len() >= end.saturating_add(length) {
                return Ok(());
            }
        }
    }
}
fn first_request(restricted: bool) -> MeasureResult {
    let mut samples = Vec::new();
    let mut received_samples = Vec::new();
    for index in 0..=runs()? {
        let workspace = Workspace::new()?;
        let mut server = OneRequest::start()?;
        workspace.configure(&format!("http://{}", server.address), restricted)?;
        let mut command = command(&workspace.0, &binary());
        command.args(["exec", "Reply Done."]);
        if !restricted {
            command.arg("--no-sandbox");
        }
        logs(&mut command, &workspace.0)?;
        let start = Instant::now();
        let mut child = Running::spawn(&mut command)?;
        let status = child.finish(LIMIT)?;
        success(status, &workspace.0)?;
        let accepted = server.accepted.recv_timeout(LIMIT)?;
        let received = server.received.recv_timeout(LIMIT)?;
        server.finish()?;
        if index != 0 {
            samples.push(ms(accepted.duration_since(start)));
            received_samples.push(ms(received.duration_since(start)));
        }
    }
    let condition = if restricted {
        "restricted; includes three sandbox-exec interpreter launches: --version=json, skill list, skill show"
    } else {
        "normal; no interpreter; --no-sandbox"
    };
    emit(
        if restricted {
            "exec_first_request_restricted_ms"
        } else {
            "exec_first_request_ms"
        },
        "ms",
        samples,
        &format!(
            "spawn to blocking accept; {condition}; request fully received ms={received_samples:?}; one warm-up"
        ),
    );
    Ok(())
}
// 作成時の関門: exec の最初の接続を入口で測る道具であり、時間の閾値で判定しない。
#[test]
#[ignore = "manual release performance measurement"]
fn exec_first_request_ms() -> MeasureResult {
    if !macos() {
        return Ok(());
    }
    first_request(false)
}
// 作成時の関門: 本物の処理系を含む起動条件を測る道具であり、時間の閾値で判定しない。
#[test]
#[ignore = "manual release performance measurement"]
fn exec_first_request_restricted_ms() -> MeasureResult {
    if !macos() {
        return Ok(());
    }
    first_request(true)
}

struct Terminal {
    child: Running,
    // 子の終了直後も出力を排出できるよう、親の slave を保持する。
    _pty: tty::Pty,
    input: File,
    output: Vec<u8>,
    started: Instant,
}
impl Terminal {
    fn start(root: &Path, restricted: bool) -> MeasureResult<Self> {
        let pty = tty::Pty::new()?;
        let input = pty.input()?;
        let mut command = command(root, &binary());
        if !restricted {
            command.args([
                "--mode",
                "normal",
                "--sandbox",
                "danger-full-access",
                "--no-sandbox",
                "--approval",
                "never",
            ]);
        }
        command
            .stdin(pty.slave()?)
            .stdout(pty.slave()?)
            .stderr(pty.slave()?);
        let started = Instant::now();
        let child = Running::spawn(&mut command)?;
        Ok(Self {
            child,
            _pty: pty,
            input,
            output: Vec::new(),
            started,
        })
    }
    fn read(&mut self) -> MeasureResult<Instant> {
        let mut buffer = [0; 65536];
        for _ in 0..32 {
            match self.input.read(&mut buffer) {
                Ok(0) => break,
                Ok(n) => self
                    .output
                    .extend_from_slice(buffer.get(..n).ok_or("Invalid pty read length")?),
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => break,
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(error) => return Err(error.into()),
            }
        }
        Ok(Instant::now())
    }
    fn wait_for(
        &mut self,
        mut observe: impl FnMut(&str, Instant) -> bool,
    ) -> MeasureResult<Instant> {
        let start = Instant::now();
        loop {
            let at = self.read()?;
            let visible = String::from_utf8_lossy(&self.output);
            if observe(&visible, at) {
                return Ok(at);
            }
            if self.child.status()?.is_some() || start.elapsed() >= LIMIT {
                return Err(
                    format!("TUI exited before expected output or timed out: {visible}").into(),
                );
            }
            thread::sleep(POLL);
        }
    }
    fn ready(&mut self) -> MeasureResult<Instant> {
        self.wait_for(|visible, _| visible.contains("Sandbox:") && visible.contains("> "))
    }
    fn quit(&mut self) -> MeasureResult {
        self.input.write_all(b"/quit\r")?;
        let start = Instant::now();
        loop {
            self.read()?;
            if let Some(status) = self.child.status()? {
                if !status.success() {
                    return Err(format!("TUI exited {status}").into());
                }
                return Ok(());
            }
            if start.elapsed() >= Duration::from_secs(3) {
                return Err("TUI did not quit; killing its process group".into());
            }
            thread::sleep(POLL);
        }
    }
}
// 作成時の関門: 本物の端末の表示と ps の RSS を測る道具であり、時間の閾値で判定しない。
#[test]
#[ignore = "manual release performance measurement"]
fn tui_ready_and_idle_rss() -> MeasureResult {
    if !macos() {
        return Ok(());
    }
    let mut ready_samples = Vec::new();
    let mut rss_samples = Vec::new();
    for index in 0..=runs()? {
        let workspace = Workspace::new()?;
        workspace.configure("http://127.0.0.1:1/v1", true)?;
        let mut terminal = Terminal::start(&workspace.0, true)?;
        let ready = terminal.ready()?;
        let elapsed = ms(ready.duration_since(terminal.started));
        thread::sleep(Duration::from_secs(1));
        let output = Command::new("/bin/ps")
            .args(["-o", "rss=", "-p", &terminal.child.child.id().to_string()])
            .output()?;
        if !output.status.success() {
            return Err("ps failed to read idle RSS".into());
        }
        let rss = std::str::from_utf8(&output.stdout)?.trim().parse::<u64>()?;
        terminal.quit()?;
        if index != 0 {
            ready_samples.push(elapsed);
            rss_samples.push(rss);
        }
    }
    let note = "restricted; includes three sandbox-exec interpreter launches: --version=json, skill list, skill show; ready=Sandbox: and > ; one warm-up";
    emit("tui_ready_ms", "ms", ready_samples, note);
    emit(
        "tui_idle_rss_kib",
        "KiB",
        rss_samples,
        &format!("{note}; ps -o rss= one second after ready"),
    );
    Ok(())
}

// 作成時の関門: 199 回の read と最後の応答を実際の exec で測る道具であり、時間の閾値で判定しない。
#[test]
#[ignore = "manual release performance measurement"]
fn session_200_max_rss_kib() -> MeasureResult {
    if !macos() {
        return Ok(());
    }
    let mut samples = Vec::new();
    for _ in 0..runs()?.min(3) {
        let mut scripts = Vec::new();
        for index in 1..=199 {
            scripts.push(ResponseScript::sse(Protocol::Chat.response(
                Some((
                    &format!("call-{index:03}"),
                    "read",
                    json!({"path":format!("f{index:03}.txt")}),
                )),
                "",
                false,
            )));
        }
        scripts.push(ResponseScript::sse(
            Protocol::Chat.response(None, "Done.", false),
        ));
        let mut server = MockProvider::start(scripts)?;
        let fixture = Fixture::new(Protocol::Chat, &server)?;
        for index in 1..=199 {
            fs::write(
                fixture.root.join(format!("project/f{index:03}.txt")),
                "x".repeat(2047) + "\n",
            )?;
        }
        let mut command = command(&fixture.root, Path::new("/usr/bin/time"));
        command.arg("-l").arg(binary()).args([
            "exec",
            "Read all 199 files.",
            "--max-turns",
            "200",
            "--no-sandbox",
        ]);
        logs(&mut command, &fixture.root)?;
        let mut child = Running::spawn(&mut command)?;
        success(child.finish(Duration::from_secs(120))?, &fixture.root)?;
        let requests = server.requests()?;
        if requests.len() != 200 {
            return Err(format!("Expected 200 requests, got {}", requests.len()).into());
        }
        server.shutdown()?;
        let stderr = fs::read_to_string(fixture.root.join("stderr"))?;
        let bytes = stderr
            .lines()
            .find(|line| line.contains("maximum resident set size"))
            .and_then(|line| line.split_whitespace().next())
            .ok_or("Missing maximum RSS")?
            .parse::<u64>()?;
        samples.push(bytes / 1024);
    }
    emit(
        "session_200_max_rss_kib",
        "KiB",
        samples,
        "normal; no interpreter or tool child processes; 199 distinct 2 KiB reads + final response; exactly 200 requests; /usr/bin/time -l bytes / 1024; no warm-up; at most three runs",
    );
    Ok(())
}

fn stream_once() -> MeasureResult<Vec<f64>> {
    let events: Vec<String> = (1..=20).map(|index| format!("data: {}\n\n", json!({"choices":[{"delta":{"content":format!("M{index:02}\n")},"finish_reason":null}]}))).collect();
    let size = events.first().ok_or("Missing event")?.len();
    if events.iter().any(|event| event.len() != size) {
        return Err("Unequal SSE event lengths".into());
    }
    let tail = Protocol::Chat.response(None, "", false);
    let mut body = events.concat().into_bytes();
    body.extend(tail);
    let mut script = ResponseScript::sse(body);
    script.delivery = Delivery::Chunks {
        size,
        delay: Duration::from_millis(100),
    };
    let mut server = MockProvider::start(vec![script])?;
    let fixture = Fixture::new(Protocol::Chat, &server)?;
    let mut terminal = Terminal::start(&fixture.root, false)?;
    terminal.ready()?;
    terminal.input.write_all(b"Stream the markers\r")?;
    let mut seen = vec![None; 20];
    terminal.wait_for(|visible, at| {
        for (index, entry) in seen.iter_mut().enumerate() {
            if entry.is_none() && visible.contains(&format!("M{:02}", index.saturating_add(1))) {
                *entry = Some(at);
            }
        }
        seen.iter().all(Option::is_some)
    })?;
    // 最後の finish_reason と [DONE] も消費させてから /quit を送る。
    terminal.wait_for(|visible, _| visible.contains("The run completed."))?;
    terminal.quit()?;
    let written = server.chunk_times()?;
    let chunks = written.first().ok_or("Missing chunk times")?;
    let mut samples = Vec::new();
    for (index, entry) in seen.iter().enumerate() {
        let observed = entry.ok_or("Missing displayed marker")?;
        let sent = chunks.get(index).ok_or("Missing marker chunk")?;
        samples.push(ms(observed
            .checked_duration_since(*sent)
            .ok_or("Marker observed before write completed")?));
    }
    if server.requests()?.len() != 1 {
        return Err("Expected one streaming request".into());
    }
    server.shutdown()?;
    Ok(samples)
}
// 作成時の関門: SSE の書き終わりから本物の端末の印までを測る道具であり、時間の閾値で判定しない。
#[test]
#[ignore = "manual release performance measurement"]
fn stream_delta_ms() -> MeasureResult {
    if !macos() {
        return Ok(());
    }
    let count = runs()?;
    stream_once()?;
    let mut samples = Vec::new();
    for _ in 0..count {
        samples.extend(stream_once()?);
    }
    emit(
        "stream_delta_ms",
        "ms",
        samples,
        "normal; no interpreter; danger-full-access, --no-sandbox, approval never; 20 unique newline markers per run at 100 ms intervals; one warm-up run; pty polling <= 1 ms",
    );
    Ok(())
}

// 作成時の関門: 公開の grep の結果と時間を同じ木で測る道具であり、時間の閾値で判定しない。
#[test]
#[ignore = "manual release performance measurement"]
fn grep_10k_ms() -> MeasureResult {
    if !macos() {
        return Ok(());
    }
    let count = runs()?;
    let workspace = Workspace::new()?;
    let project = workspace.0.join("project");
    let temporary = workspace.0.join("tmp");
    let spill = workspace.0.join("spill");
    let paths = DataPaths::from_values(
        Some(&workspace.0.join("home")),
        Some(&workspace.0.join("config")),
        Some(&workspace.0.join("state")),
        Some(&temporary),
    )?;
    let context = ToolContext::new(
        Mode::Restricted,
        ScopeRoot::open(&project)?,
        ScopeRoot::open(&temporary)?,
        ScopeRoot::open(&spill)?,
        ExcludeRules::new(
            &project,
            &paths,
            &spill,
            &ReadConfig::default(),
            &ReadConfig::default(),
        )?,
    );
    let tools = ToolSet::new(Mode::Restricted);
    let mut total = 0_usize;
    for directory in 0_usize..16 {
        let parent = project.join(format!("d{directory:02}"));
        fs::create_dir(&parent)?;
        for file in 0_usize..625 {
            let number = directory.saturating_mul(625).saturating_add(file);
            let prefix = if number < 100 { "needle\n" } else { "hay\n" };
            let body = format!(
                "{prefix}{}\n",
                "x".repeat(1023_usize.saturating_sub(prefix.len()))
            );
            total = total.saturating_add(body.len());
            fs::write(parent.join(format!("f{file:03}.txt")), body)?;
        }
    }
    let measure = || -> MeasureResult<f64> {
        let start = Instant::now();
        let result = tools.execute(&context, "grep", json!({"path":".","pattern":"needle"}));
        let elapsed = ms(start.elapsed());
        if result.is_error
            || result.text.contains("Search stopped")
            || result.text.contains("truncated")
        {
            return Err(format!("Search failed or stopped: {}", result.text).into());
        }
        if result
            .text
            .lines()
            .filter(|line| line.contains("needle"))
            .count()
            != 100
        {
            return Err(format!("Expected exactly 100 matching lines: {}", result.text).into());
        }
        Ok(elapsed)
    };
    let note = format!(
        "10000 files, 16 directories x 625; total_bytes={total}; excluded=0; exactly 100 matching lines; 作成の直後（ページキャッシュに残る）; grep runs inside test binary={}",
        std::env::current_exe()?.display()
    );
    emit("grep_10k_first_ms", "ms", vec![measure()?], &note);
    measure()?;
    let mut samples = Vec::new();
    for _ in 0..count {
        samples.push(measure()?);
    }
    emit(
        "grep_10k_ms",
        "ms",
        samples,
        &format!("{note}; repeated after first traversal and one warm-up"),
    );
    Ok(())
}
