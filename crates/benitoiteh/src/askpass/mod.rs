//! 呼び出しごとの askpass の認証、画面との受け渡し、秘密の除去（設計書 03-04「段 2」）。
mod error;
mod output;
pub use error::AskpassError;
pub(crate) use output::Redactor;
pub mod helper;
mod protocol;
pub mod text;

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fs::{self, DirBuilder};
use std::io::{self, Read, Write};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt, symlink};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError, mpsc};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};
use zeroize::Zeroizing;

pub use protocol::Kind;
pub(crate) use protocol::MAX_SECRET;
pub const SOCKET_ENV: &str = "BENITOITEH_ASKPASS_SOCKET";
pub const TOKEN_ENV: &str = "BENITOITEH_ASKPASS_TOKEN";

/// 画面だけが作る回答。値は複製せず、補助への送信の後に消す。
pub enum Answer {
    Secret(Zeroizing<String>),
    Yes,
    Declined,
    Cancelled,
}

/// ハーネスが保持した呼び出しの事実。補助が送った値とは別にする。
#[derive(Clone)]
pub struct Facts {
    pub call_id: String,
    pub command: String,
    pub cwd: PathBuf,
    pub mode: &'static str,
}

/// 画面に一度だけ渡す求め。報告は未検証であり、記録へ写さない。
pub struct Prompt {
    pub facts: Facts,
    pub kind: Kind,
    pub ssh_prompt: Option<String>,
    pub prompt: String,
    pub pid: u32,
    pub ppid: u32,
    pub reply: mpsc::SyncSender<Answer>,
    call: Arc<CallState>,
    open: Arc<AtomicBool>,
    outcome: Arc<Mutex<Option<&'static str>>>,
}
impl Prompt {
    /// 失効と補助の切断は、画面側で値を送らずに閉じる。
    pub fn is_open(&self) -> bool {
        self.open.load(Ordering::Acquire) && self.call.active.load(Ordering::Acquire)
    }
    /// 補助への書き込みが確定した結果だけを、画面と道具で共有する。
    pub(crate) fn outcome(&self) -> Option<&'static str> {
        *self.outcome.lock().unwrap_or_else(PoisonError::into_inner)
    }
    /// 回答を返さず、接続の寿命だけを示すタッチの通知か。
    pub fn touch(&self) -> bool {
        self.ssh_prompt.as_deref() == Some("none")
    }
    /// 秘密の値ではなく、はい・いいえを求める確認か。
    pub fn confirm(&self) -> bool {
        self.ssh_prompt.as_deref() == Some("confirm")
    }
}

#[derive(Default)]
struct State {
    requests: usize,
    hashes: Vec<output::SecretHash>,
    events: Vec<String>,
    wait_start: Option<Instant>,
    waited: Duration,
}
struct CallState {
    active: AtomicBool,
    token: Zeroizing<String>,
    facts: Facts,
    rolling_base: Zeroizing<u64>,
    state: Mutex<State>,
}

/// 実行の間だけ有効な合言葉と、平文を保持しない結果の処理。
pub struct CallGuard {
    call: Arc<CallState>,
    socket: PathBuf,
}
impl CallGuard {
    pub(crate) fn environment(&self, env: &mut BTreeMap<OsString, OsString>) {
        env.insert(SOCKET_ENV.into(), self.socket.as_os_str().to_owned());
        env.insert(TOKEN_ENV.into(), self.call.token.as_str().into());
    }
    pub(crate) fn finish(&self) {
        crate::sandbox::run::RunPause::stop(self);
        // 窓口が最後の応答と記録を終えてから、結果を作る。
        while self
            .call
            .state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .wait_start
            .is_some()
        {
            thread::sleep(Duration::from_millis(5));
        }
    }
    pub(crate) fn redactor(&self) -> Redactor {
        Redactor::new(Arc::clone(&self.call))
    }
    pub(crate) fn events(&self) -> Vec<String> {
        self.call
            .state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .events
            .clone()
    }
}
impl crate::sandbox::run::RunPause for CallGuard {
    fn paused(&self) -> Duration {
        let state = self
            .call
            .state
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        state.waited.saturating_add(
            state
                .wait_start
                .map_or(Duration::ZERO, |start| start.elapsed()),
        )
    }
    fn stop(&self) {
        let _state = self
            .call
            .state
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        self.call.active.store(false, Ordering::Release);
    }
}
impl Drop for CallGuard {
    fn drop(&mut self) {
        crate::sandbox::run::RunPause::stop(self);
    }
}

// 要求の途中でも止められるよう、読み取りの区切りで同じ合図を見る。
#[derive(Default)]
struct StopSignal {
    stopped: AtomicBool,
    #[cfg(test)]
    read_observer: Mutex<Option<mpsc::Sender<bool>>>,
}
impl StopSignal {
    fn load(&self, order: Ordering) -> bool {
        self.stopped.load(order)
    }
    fn store(&self, value: bool, order: Ordering) {
        self.stopped.store(value, order);
    }
    fn read_stopped(&self) -> bool {
        let stopped = self.load(Ordering::Acquire);
        #[cfg(test)]
        if let Some(observer) = self
            .read_observer
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .as_ref()
        {
            let _sent = observer.send(stopped);
        }
        stopped
    }
}

/// セッション単位の補助と、対話の実行だけが持つ窓口。
pub struct Broker {
    directory: PathBuf,
    parent_identity: (u64, u64),
    socket: Option<PathBuf>,
    identity: (u64, u64),
    diagnostic: Option<&'static str>,
    current: Arc<Mutex<Option<Arc<CallState>>>>,
    receiver: Mutex<Option<mpsc::Receiver<Prompt>>>,
    stop: Arc<StopSignal>,
    thread: Mutex<Option<JoinHandle<()>>>,
}
impl std::fmt::Debug for Broker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Broker").finish_non_exhaustive()
    }
}
impl Broker {
    /// 書ける場所の外に専用の領域を作る。失敗時は部分的な準備も消す。
    pub fn create(
        temporary_root: &Path,
        id: &str,
        executable: &Path,
        writable: &[PathBuf],
        interactive: bool,
    ) -> Result<Self, AskpassError> {
        if !matches!(
            (
                Path::new(id).components().next(),
                Path::new(id).components().count()
            ),
            (Some(std::path::Component::Normal(_)), 1)
        ) {
            return Err(io::ErrorKind::InvalidInput.into());
        }
        let name = helper_name(id);
        // Linux の利用者専用の runtime を先に試す。共有 /tmp は最後の逃がし先。
        let mut roots = Vec::new();
        if let Some(runtime) = runtime_directory() {
            roots.push(runtime);
        }
        roots.push(temporary_root.canonicalize()?);
        roots.push(Path::new("/tmp").canonicalize()?);
        let directory = roots
            .into_iter()
            .map(|root| parent_directory(&root).join(name))
            .find(|directory| {
                directory.join("sock").as_os_str().as_bytes().len() <= 103
                    && !writable.iter().any(|root| directory.starts_with(root))
            })
            .ok_or(io::ErrorKind::InvalidInput)?;
        if directory.join("sock").as_os_str().as_bytes().len() > 103 {
            return Err(io::ErrorKind::InvalidInput.into());
        }
        if writable.iter().any(|root| directory.starts_with(root)) {
            return Err(io::ErrorKind::PermissionDenied.into());
        }
        let parent = directory.parent().ok_or(io::ErrorKind::InvalidInput)?;
        match DirBuilder::new().mode(0o700).create(parent) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error.into()),
        }
        if fs::symlink_metadata(parent)
            .is_ok_and(|metadata| metadata.uid() != rustix::process::geteuid().as_raw())
        {
            return Err(AskpassError::UnownedParent(parent.to_path_buf()));
        }
        if !valid_parent(parent) {
            return Err(AskpassError::ChangedDirectory);
        }
        let parent_metadata = fs::symlink_metadata(parent)?;
        let parent_identity = (parent_metadata.dev(), parent_metadata.ino());
        match DirBuilder::new().mode(0o700).create(&directory) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                // 呼び出し側はセッションの錠を持つ。落ちた前回の補助を再利用しない。
                let metadata = fs::symlink_metadata(&directory)?;
                if !metadata.is_dir()
                    || metadata.permissions().mode() & 0o7777 != 0o700
                    || metadata.uid() != rustix::process::geteuid().as_raw()
                {
                    return Err(io::ErrorKind::PermissionDenied.into());
                }
                match fs::remove_file(directory.join("sock")) {
                    Ok(()) => {}
                    Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                    Err(error) => return Err(error.into()),
                }
                crate::session::remove::directory(&directory)?;
                DirBuilder::new().mode(0o700).create(&directory)?;
            }
            Err(error) => return Err(error.into()),
        }
        let metadata = fs::symlink_metadata(&directory)?;
        if !metadata.is_dir()
            || metadata.permissions().mode() & 0o7777 != 0o700
            || metadata.uid() != rustix::process::geteuid().as_raw()
        {
            return Err(io::ErrorKind::PermissionDenied.into());
        }
        let mut broker = Self {
            directory,
            parent_identity,
            socket: None,
            identity: (metadata.dev(), metadata.ino()),
            diagnostic: None,
            current: Arc::new(Mutex::new(None)),
            receiver: Mutex::new(None),
            stop: Arc::new(StopSignal::default()),
            thread: Mutex::new(None),
        };
        let executable = executable.canonicalize()?;
        for kind in [Kind::Ssh, Kind::Git, Kind::Sudo] {
            symlink(&executable, broker.directory.join(kind.helper()))?;
        }
        if interactive {
            broker.enable_interactive();
        }
        Ok(broker)
    }
    fn enable_interactive(&mut self) {
        if self.listen().is_err() {
            self.diagnostic = Some(text::LISTENER_DISABLED);
            self.socket = None;
            self.receiver = Mutex::new(None);
        }
    }
    fn listen(&mut self) -> io::Result<()> {
        let socket = self.directory.join("sock");
        let listener = UnixListener::bind(&socket)?;
        self.socket = Some(socket);
        listener.set_nonblocking(true)?;
        let stop = Arc::clone(&self.stop);
        let current = Arc::clone(&self.current);
        let (sender, receiver) = mpsc::channel();
        self.receiver = Mutex::new(Some(receiver));
        let thread = thread::Builder::new()
            .name("askpass".into())
            .spawn(move || {
                accept_loop(
                    || listener.accept().map(|(stream, _)| stream),
                    &current,
                    &sender,
                    &stop,
                );
            })?;
        *self.thread.lock().unwrap_or_else(PoisonError::into_inner) = Some(thread);
        Ok(())
    }
    /// 補助のリンクは使えるが窓口が使えないときの診断。
    pub(crate) fn diagnostic(&self) -> Option<&'static str> {
        self.diagnostic
    }
    /// 作った領域が同じ実体であり、他の利用者から書き換えられないことを確かめる。
    pub(crate) fn valid_directory(&self) -> bool {
        let Some(parent) = self.directory.parent() else {
            return false;
        };
        if !valid_parent(parent)
            || !fs::symlink_metadata(parent)
                .is_ok_and(|metadata| (metadata.dev(), metadata.ino()) == self.parent_identity)
        {
            return false;
        }
        fs::symlink_metadata(&self.directory).is_ok_and(|metadata| {
            metadata.is_dir()
                && !metadata.is_symlink()
                && metadata.uid() == rustix::process::geteuid().as_raw()
                && metadata.permissions().mode() & 0o7777 == 0o700
                && (metadata.dev(), metadata.ino()) == self.identity
        })
    }
    /// Worker の異常な終了でも、Agent のスレッドの寿命を待たず窓口を閉じて消す。
    pub(crate) fn shutdown(&self) {
        self.stop.store(true, Ordering::Release);
        if let Some(call) = self
            .current
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take()
        {
            let _state = call.state.lock().unwrap_or_else(PoisonError::into_inner);
            call.active.store(false, Ordering::Release);
        }
        if let Some(thread) = self
            .thread
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take()
        {
            let _joined = thread.join();
        }
        if self.valid_directory() {
            let _unlinked = fs::remove_file(self.directory.join("sock"));
            let _removed = crate::session::remove::directory(&self.directory);
        }
    }
    /// 補助に向ける五つの変数。呼び出しの権限はここでは渡さない。
    pub fn environment(&self, env: &mut BTreeMap<OsString, OsString>) {
        for (key, kind) in [
            ("SSH_ASKPASS", Kind::Ssh),
            ("GIT_ASKPASS", Kind::Git),
            ("SUDO_ASKPASS", Kind::Sudo),
        ] {
            env.insert(
                key.into(),
                self.directory.join(kind.helper()).into_os_string(),
            );
        }
        env.insert("SSH_ASKPASS_REQUIRE".into(), "force".into());
        env.insert("GIT_TERMINAL_PROMPT".into(), "0".into());
    }
    /// 一つの画面だけに受け口を渡す。セッションを作り直すと別の窓口になる。
    pub fn take_requests(&self) -> Option<mpsc::Receiver<Prompt>> {
        self.receiver
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take()
    }
    pub(crate) fn begin(&self, facts: Facts) -> Result<Option<CallGuard>, AskpassError> {
        if !self.valid_directory() || self.stop.load(Ordering::Acquire) {
            return Err(AskpassError::ChangedDirectory);
        }
        let Some(socket) = &self.socket else {
            return Ok(None);
        };
        let mut random = Zeroizing::new([0u8; 32]);
        getrandom::fill(random.as_mut()).map_err(io::Error::other)?;
        let mut token = Zeroizing::new(String::with_capacity(64));
        use std::fmt::Write as _;
        for byte in random.iter() {
            write!(token, "{byte:02x}").map_err(io::Error::other)?;
        }
        let mut rolling_key = Zeroizing::new([0u8; 8]);
        getrandom::fill(rolling_key.as_mut()).map_err(io::Error::other)?;
        let rolling_base = Zeroizing::new(u64::from_ne_bytes(*rolling_key) | 257);
        let call = Arc::new(CallState {
            active: AtomicBool::new(true),
            token,
            facts,
            rolling_base,
            state: Mutex::new(State::default()),
        });
        let mut current = self.current.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(previous) = current.replace(Arc::clone(&call)) {
            previous.active.store(false, Ordering::Release);
        }
        Ok(Some(CallGuard {
            call,
            socket: socket.clone(),
        }))
    }
}
impl Drop for Broker {
    fn drop(&mut self) {
        self.shutdown();
    }
}

/// セッションが終わっても残す、利用者だけが書ける親のパス。
pub(crate) fn parent_directory(root: &Path) -> PathBuf {
    root.join(format!("bha-{}", rustix::process::geteuid().as_raw()))
}
/// macOS の TMPDIR に収まるよう、正しい ID は乱数の部分だけを子の名前にする。
pub(crate) fn helper_name(id: &str) -> &str {
    #[cfg(target_os = "macos")]
    if id.parse::<crate::session::SessionId>().is_ok() {
        return id.rsplit_once('-').map_or(id, |(_, random)| random);
    }
    id
}

/// 有効な Linux の runtime だけを使い、共有場所やリンクは候補にしない。
pub(crate) fn runtime_directory() -> Option<PathBuf> {
    #[cfg(target_os = "linux")]
    {
        let runtime = PathBuf::from(std::env::var_os("XDG_RUNTIME_DIR")?);
        if runtime.is_absolute() && valid_parent(&runtime) {
            return runtime.canonicalize().ok();
        }
    }
    None
}

/// 親はリンクをたどらず、自分の 0700 のディレクトリだけを受け付ける。
pub(crate) fn valid_parent(parent: &Path) -> bool {
    fs::symlink_metadata(parent).is_ok_and(|metadata| {
        metadata.is_dir()
            && !metadata.is_symlink()
            && metadata.uid() == rustix::process::geteuid().as_raw()
            && metadata.permissions().mode() & 0o7777 == 0o700
    })
}

pub(crate) fn clear_environment(env: &mut BTreeMap<OsString, OsString>) {
    for key in [
        "SSH_ASKPASS",
        "GIT_ASKPASS",
        "SUDO_ASKPASS",
        "SSH_ASKPASS_REQUIRE",
        "GIT_TERMINAL_PROMPT",
        SOCKET_ENV,
        TOKEN_ENV,
    ] {
        env.remove(std::ffi::OsStr::new(key));
    }
}

// accept の一時の失敗で窓口を終えない。資源不足では忙しい再試行を避ける。
fn accept_loop(
    mut accept: impl FnMut() -> io::Result<UnixStream>,
    current: &Mutex<Option<Arc<CallState>>>,
    sender: &mpsc::Sender<Prompt>,
    stop: &StopSignal,
) {
    while !stop.load(Ordering::Acquire) {
        match accept() {
            Ok(mut stream) => {
                let _handled = serve(&mut stream, current, sender, stop);
            }
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::Interrupted
                        | io::ErrorKind::ConnectionAborted
                        | io::ErrorKind::ConnectionReset
                ) => {}
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(10))
            }
            Err(error)
                if [
                    rustix::io::Errno::MFILE,
                    rustix::io::Errno::NFILE,
                    rustix::io::Errno::NOBUFS,
                    rustix::io::Errno::NOMEM,
                ]
                .iter()
                .any(|errno| error.raw_os_error() == Some(errno.raw_os_error())) =>
            {
                thread::sleep(Duration::from_millis(25))
            }
            Err(_) => break,
        }
    }
}

// 固定長の全バイトを比較する。早期 return で合言葉の一致位置を漏らさない。
fn equal_token(left: &str, right: &str) -> bool {
    if left.len() != 64 || right.len() != 64 {
        return false;
    }
    left.bytes()
        .zip(right.bytes())
        .fold(0u8, |difference, (a, b)| difference | (a ^ b))
        == 0
}
fn serve(
    stream: &mut UnixStream,
    current: &Mutex<Option<Arc<CallState>>>,
    sender: &mpsc::Sender<Prompt>,
    stop: &StopSignal,
) -> io::Result<()> {
    stream.set_nonblocking(false)?;
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    stream.set_write_timeout(Some(Duration::from_secs(5)))?;
    #[cfg(target_os = "linux")]
    if rustix::net::sockopt::socket_peercred(&*stream)?.uid != rustix::process::getuid() {
        return stream.write_all(b"N");
    }
    let request = match protocol::read_request(stream, || stop.read_stopped()) {
        Ok(request) => request,
        Err(_) => return stream.write_all(b"N"),
    };
    let call = current
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone();
    let Some(call) = call.filter(|call| {
        equal_token(&call.token, &request.token) && call.active.load(Ordering::Acquire)
    }) else {
        return stream.write_all(b"N");
    };
    stream.set_read_timeout(Some(Duration::from_millis(50)))?;
    {
        let mut state = call.state.lock().unwrap_or_else(PoisonError::into_inner);
        if !call.active.load(Ordering::Acquire) {
            return stream.write_all(b"N");
        }
        state.requests = state.requests.saturating_add(1);
        if state.requests > 3 {
            if state.requests == 4 {
                state
                    .events
                    .push(text::event(request.kind.name(), "refused"));
            }
            return stream.write_all(b"N");
        }
        state.wait_start = Some(Instant::now());
    }
    let (reply, response) = mpsc::sync_channel(1);
    let open = Arc::new(AtomicBool::new(true));
    let touch = request.ssh_prompt.as_deref() == Some("none");
    let result = Arc::new(Mutex::new(None));
    let sent = sender
        .send(Prompt {
            facts: call.facts.clone(),
            kind: request.kind,
            ssh_prompt: request.ssh_prompt.clone(),
            prompt: request.prompt.clone(),
            pid: request.pid,
            ppid: request.ppid,
            reply,
            call: Arc::clone(&call),
            open: Arc::clone(&open),
            outcome: Arc::clone(&result),
        })
        .is_ok();
    let mut outcome = "cancelled";
    let mut answer = None;
    while sent && call.active.load(Ordering::Acquire) && !stop.load(Ordering::Acquire) {
        {
            match response.try_recv() {
                Ok(value) => {
                    answer = Some(value);
                    break;
                }
                Err(mpsc::TryRecvError::Disconnected) => break,
                Err(mpsc::TryRecvError::Empty) => {}
            }
        }
        let mut byte = [0];
        match stream.read(&mut byte) {
            Ok(0) => break,
            Ok(_) => {
                outcome = "refused";
                break;
            }
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::WouldBlock
                        | io::ErrorKind::TimedOut
                        | io::ErrorKind::Interrupted
                ) => {}
            Err(_) => break,
        }
    }
    let mut state = call.state.lock().unwrap_or_else(PoisonError::into_inner);
    if let Some(start) = state.wait_start.take() {
        state.waited = state.waited.saturating_add(start.elapsed());
    }
    // 失効後に画面から届いた答えは、補助へ書かない（設計書 03-04「段 2」）。
    if call.active.load(Ordering::Acquire) && !stop.load(Ordering::Acquire) {
        match answer {
            Some(Answer::Secret(secret))
                if !touch
                    && request.ssh_prompt.is_none()
                    && secret.len() <= protocol::MAX_SECRET
                    && !secret.bytes().any(|b| matches!(b, 0 | b'\r' | b'\n')) =>
            {
                state.hashes.push(output::SecretHash::new(
                    secret.as_bytes(),
                    *call.rolling_base,
                ));
                outcome = if stream
                    .write_all(b"S")
                    .and_then(|()| stream.write_all(secret.as_bytes()))
                    .is_ok()
                {
                    "answered"
                } else {
                    "refused"
                };
            }
            Some(Answer::Yes) if request.ssh_prompt.as_deref() == Some("confirm") => {
                outcome = if stream.write_all(b"Y").is_ok() {
                    "answered"
                } else {
                    "refused"
                };
            }
            Some(Answer::Declined) => {
                let _written = stream.write_all(b"N");
                outcome = "declined";
            }
            Some(Answer::Cancelled) | None => {
                if !touch {
                    let _written = stream.write_all(b"N");
                }
            }
            Some(Answer::Secret(_) | Answer::Yes) => {
                let _written = stream.write_all(b"N");
                outcome = "refused";
            }
        }
    } else if !touch {
        let _written = stream.write_all(b"N");
    }
    state.events.push(text::event(request.kind.name(), outcome));
    *result.lock().unwrap_or_else(PoisonError::into_inner) = Some(outcome);
    open.store(false, Ordering::Release);
    Ok(())
}

#[cfg(test)]
pub(crate) mod tests;
