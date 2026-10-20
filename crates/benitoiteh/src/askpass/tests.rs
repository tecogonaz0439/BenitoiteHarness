//! 窓口の認証と要求の上限を、本物のソケットで確かめる（設計書 03-04）。
use super::*;
use protocol::Request;

pub(crate) struct Fixture {
    pub root: PathBuf,
    pub broker: Broker,
    pub call: CallGuard,
    _root: FixtureRoot,
}
struct FixtureRoot(PathBuf);
impl FixtureRoot {
    fn create(path: PathBuf) -> io::Result<Self> {
        fs::create_dir(&path)?;
        Ok(Self(path))
    }
}
impl Drop for FixtureRoot {
    fn drop(&mut self) {
        let _removed = fs::remove_dir_all(&self.0);
    }
}
impl Fixture {
    pub fn new() -> io::Result<Self> {
        let mut random = [0; 16];
        getrandom::fill(&mut random).map_err(io::Error::other)?;
        let root =
            std::env::temp_dir().join(format!("bha-test-{:032x}", u128::from_ne_bytes(random)));
        let root = FixtureRoot::create(root)?;
        Self::at(root, &std::env::current_exe()?, true)
    }
    fn at(cleanup: FixtureRoot, executable: &Path, interactive: bool) -> io::Result<Self> {
        let root = cleanup.0.canonicalize()?;
        let id = crate::session::SessionId::generate(std::time::SystemTime::now())
            .map_err(io::Error::other)?;
        let broker = Broker::create(&root, id.as_str(), executable, &[], interactive)
            .map_err(io::Error::other)?;
        let call = broker
            .begin(Facts {
                call_id: "call-17".into(),
                command: "git fetch".into(),
                cwd: root.clone(),
                mode: "normal (escalated)",
            })
            .map_err(io::Error::other)?
            .ok_or(io::ErrorKind::Other)?;
        Ok(Self {
            root,
            broker,
            call,
            _root: cleanup,
        })
    }
    pub fn request(&self, ssh_prompt: Option<&str>, prompt: &str) -> io::Result<UnixStream> {
        let mut stream = UnixStream::connect(&self.call.socket)?;
        stream.set_read_timeout(Some(Duration::from_secs(6)))?;
        protocol::write_request(
            &mut stream,
            &Request {
                v: 1,
                token: self.call.call.token.to_string(),
                kind: Kind::Ssh,
                ssh_prompt: ssh_prompt.map(str::to_owned),
                prompt: prompt.into(),
                pid: 42,
                ppid: 41,
            },
        )?;
        Ok(stream)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.call.finish();
        self.broker.shutdown();
    }
}

// 置き場所そのものを調べるテストは、利用者の runtime を変えず子だけで外す。
fn rerun_without_runtime(name: &str) -> io::Result<bool> {
    if runtime_directory().is_none() {
        return Ok(false);
    }
    let output = std::process::Command::new(std::env::current_exe()?)
        .args(["--exact", &format!("askpass::tests::{name}"), "--nocapture"])
        .env_remove("XDG_RUNTIME_DIR")
        .output()?;
    assert!(
        output.status.success(),
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(true)
}

fn status(mut stream: UnixStream) -> io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    stream.read_to_end(&mut bytes)?;
    Ok(bytes)
}

// 関門: 認証、回数、失効を通らず画面へ届く退行を、窓口の実際の wire の境界で捕まえる。
#[test]
fn broker_refuses_unknown_expired_tokens_fourth_request_and_malformed_frames() -> io::Result<()> {
    let f = Fixture::new()?;
    let receiver = f.broker.take_requests().ok_or(io::ErrorKind::Other)?;
    for bytes in [vec![0, 0, 32, 1], vec![0, 0, 0, 1, b'{']] {
        let mut stream = UnixStream::connect(&f.call.socket)?;
        stream.set_read_timeout(Some(Duration::from_secs(6)))?;
        stream.write_all(&bytes)?;
        assert_eq!(status(stream)?, b"N");
    }
    for (token, ssh_prompt) in [
        ("00".repeat(32), None),
        (f.call.call.token.to_string(), Some("unknown")),
    ] {
        let mut stream = UnixStream::connect(&f.call.socket)?;
        protocol::write_request(
            &mut stream,
            &Request {
                v: 1,
                token,
                kind: Kind::Ssh,
                ssh_prompt: ssh_prompt.map(str::to_owned),
                prompt: "untrusted".into(),
                pid: 42,
                ppid: 41,
            },
        )?;
        assert_eq!(status(stream)?, b"N");
    }
    assert!(receiver.try_recv().is_err());
    for attempt in 0..4 {
        let stream = f.request(Some("confirm"), "allow?")?;
        if attempt < 3 {
            let prompt = receiver
                .recv_timeout(Duration::from_secs(5))
                .map_err(io::Error::other)?;
            prompt
                .reply
                .send(Answer::Yes)
                .map_err(|_| io::ErrorKind::BrokenPipe)?;
            assert_eq!(status(stream)?, b"Y");
        } else {
            assert_eq!(status(stream)?, b"N");
        }
    }
    assert!(receiver.try_recv().is_err());
    f.call.finish();
    assert_eq!(status(f.request(None, "expired")?)?, b"N");
    assert!(receiver.try_recv().is_err());
    Ok(())
}

// 関門: 各応答の wire 契約とタッチの接続の寿命を、画面とは独立に保つ。
#[test]
fn broker_answers_once_cancels_and_closes_touch_on_disconnect_or_invalidation() -> io::Result<()> {
    for (ssh_prompt, secret, expected) in [
        (None, true, b"Ssynthetic".as_slice()),
        (Some("confirm"), false, b"N".as_slice()),
    ] {
        let f = Fixture::new()?;
        let receiver = f.broker.take_requests().ok_or(io::ErrorKind::Other)?;
        let stream = f.request(ssh_prompt, "untrusted")?;
        let prompt = receiver
            .recv_timeout(Duration::from_secs(5))
            .map_err(io::Error::other)?;
        prompt
            .reply
            .send(if secret {
                Answer::Secret(Zeroizing::new("synthetic".into()))
            } else {
                Answer::Cancelled
            })
            .map_err(|_| io::ErrorKind::BrokenPipe)?;
        assert_eq!(status(stream)?, expected);
        f.call.finish();
        assert!(!prompt.is_open());
        if secret {
            assert_eq!(
                f.call.redactor().push(b"synthetic/synthetic", true),
                b"[secret]/[secret]"
            );
        }
    }
    for disconnect in [true, false] {
        let f = Fixture::new()?;
        let receiver = f.broker.take_requests().ok_or(io::ErrorKind::Other)?;
        let stream = f.request(Some("none"), "touch")?;
        let prompt = receiver
            .recv_timeout(Duration::from_secs(5))
            .map_err(io::Error::other)?;
        assert!(prompt.touch() && prompt.is_open());
        if disconnect {
            drop(stream);
        } else {
            f.call.finish();
            assert!(status(stream)?.is_empty());
        }
        let deadline = Instant::now();
        while prompt.is_open() && deadline.elapsed() < Duration::from_secs(2) {
            thread::sleep(Duration::from_millis(5));
        }
        assert!(!prompt.is_open());
    }
    Ok(())
}

// 関門: 正規化したパス長、書ける場所、落ちた後の再開とリンクを辿らない後始末を保つ。
#[test]
fn helper_directories_fall_back_reject_writable_locations_and_recover_stale_entries()
-> io::Result<()> {
    if rerun_without_runtime(
        "helper_directories_fall_back_reject_writable_locations_and_recover_stale_entries",
    )? {
        return Ok(());
    }
    let f = Fixture::new()?;
    let long = f.root.join("long".repeat(30));
    fs::create_dir(&long)?;
    let id = crate::session::SessionId::generate(std::time::SystemTime::now())
        .map_err(io::Error::other)?;
    let broker = Broker::create(&long, id.as_str(), &std::env::current_exe()?, &[], false)
        .map_err(io::Error::other)?;
    let directory = broker.directory.clone();
    assert_eq!(
        directory.parent(),
        Some(parent_directory(&Path::new("/tmp").canonicalize()?).as_path())
    );
    assert!(directory.join("sock").as_os_str().as_bytes().len() <= 103);
    assert_eq!(
        fs::metadata(&directory)?.permissions().mode() & 0o7777,
        0o700
    );
    assert!(!directory.join("sock").exists());
    drop(broker);
    assert!(!directory.exists());
    let target = f.root.join("keep");
    fs::write(&target, b"keep")?;
    DirBuilder::new().mode(0o700).create(&directory)?;
    place_id(&directory, format!("{id}\n").as_bytes())?;
    fs::write(directory.join("sock"), b"stale")?;
    symlink(&target, directory.join("stale-link"))?;
    let resumed = Broker::create(&long, id.as_str(), &std::env::current_exe()?, &[], true)
        .map_err(io::Error::other)?;
    assert!(directory.join("sock").exists());
    assert!(!directory.join("stale-link").exists());
    assert_eq!(fs::read(&target)?, b"keep");
    drop(resumed);
    assert!(!directory.exists());
    assert!(
        Broker::create(
            &long,
            id.as_str(),
            &std::env::current_exe()?,
            &[Path::new("/tmp").canonicalize()?],
            true
        )
        .is_err()
    );
    assert!(!directory.exists());
    Ok(())
}

// 落ちた前回の子を模して、`id` を 0600 で置く。
fn place_id(directory: &Path, contents: &[u8]) -> io::Result<()> {
    use std::os::unix::fs::OpenOptionsExt;
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(directory.join("id"))?;
    file.set_permissions(fs::Permissions::from_mode(0o600))?;
    file.write_all(contents)
}

// 関門: macOS の子の名前は乱数の部分だけなので、ほかのセッションの子と重なりうる。
// `id` が自分のものでない子（ない・違う・確かめに通らない）を作り直しで消す退行を捕まえる。
#[test]
fn helper_recreation_requires_its_own_id_file() -> io::Result<()> {
    if rerun_without_runtime("helper_recreation_requires_its_own_id_file")? {
        return Ok(());
    }
    let f = Fixture::new()?;
    let id = crate::session::SessionId::generate(std::time::SystemTime::now())
        .map_err(io::Error::other)?;
    let other = crate::session::SessionId::generate(std::time::SystemTime::now())
        .map_err(io::Error::other)?;
    let exe = std::env::current_exe()?;
    let broker =
        Broker::create(&f.root, id.as_str(), &exe, &[], false).map_err(io::Error::other)?;
    let directory = broker.directory.clone();
    assert_eq!(
        fs::read(directory.join("id"))?,
        format!("{id}\n").into_bytes()
    );
    assert_eq!(
        fs::symlink_metadata(directory.join("id"))?.mode() & 0o7777,
        0o600
    );
    drop(broker);
    assert!(!directory.exists());
    let mut cases = vec!["foreign", "missing", "link", "mode", "fifo", "long"];
    // 持ち主の違う `id` は root でないと作れない。
    if rustix::process::geteuid().is_root() {
        cases.push("owner");
    }
    for case in cases {
        DirBuilder::new().mode(0o700).create(&directory)?;
        fs::write(directory.join("keep"), b"keep")?;
        let own = format!("{id}\n");
        match case {
            "foreign" => place_id(&directory, format!("{other}\n").as_bytes())?,
            "missing" => {}
            "link" => {
                let target = f.root.join("id-target");
                fs::write(&target, own.as_bytes())?;
                fs::set_permissions(&target, fs::Permissions::from_mode(0o600))?;
                symlink(&target, directory.join("id"))?;
            }
            "mode" => {
                place_id(&directory, own.as_bytes())?;
                fs::set_permissions(directory.join("id"), fs::Permissions::from_mode(0o644))?;
            }
            "fifo" => {
                // rustix は macOS で mknodat を持たないので、OS の mkfifo を使う。
                let made = std::process::Command::new("mkfifo")
                    .args(["-m", "600"])
                    .arg(directory.join("id"))
                    .status()?;
                assert!(made.success());
            }
            "long" => place_id(&directory, format!("{}\n", "x".repeat(64)).as_bytes())?,
            _ => {
                place_id(&directory, own.as_bytes())?;
                rustix::fs::chown(
                    directory.join("id").as_path(),
                    Some(rustix::process::Uid::from_raw(65534)),
                    None,
                )?;
            }
        }
        assert!(
            Broker::create(&f.root, id.as_str(), &exe, &[], false).is_err(),
            "{case}"
        );
        assert_eq!(fs::read(directory.join("keep"))?, b"keep", "{case}");
        fs::remove_dir_all(&directory)?;
    }
    DirBuilder::new().mode(0o700).create(&directory)?;
    fs::write(directory.join("keep"), b"stale")?;
    place_id(&directory, format!("{id}\n").as_bytes())?;
    let resumed =
        Broker::create(&f.root, id.as_str(), &exe, &[], false).map_err(io::Error::other)?;
    assert!(!directory.join("keep").exists());
    drop(resumed);
    Ok(())
}

// 関門: read ごとの上限へ戻る退行は、断片を途中で追加しても要求全体が 5 秒で閉じるかで捕まえる。
#[test]
fn partial_requests_cannot_extend_the_total_read_deadline() -> io::Result<()> {
    let f = Fixture::new()?;
    let mut stream = UnixStream::connect(&f.call.socket)?;
    stream.set_read_timeout(Some(Duration::from_secs(4)))?;
    stream.write_all(&[0])?;
    thread::sleep(Duration::from_secs(3));
    stream.write_all(&[0])?;
    let mut status = [0];
    stream.read_exact(&mut status)?;
    assert_eq!(status, [b'N']);
    Ok(())
}

// 関門: 同じ名前の別の実体・リンク・権限の変更を、次の起動と Drop が信用しない。
#[test]
fn replaced_or_unprotected_helper_directory_is_neither_used_nor_removed() -> io::Result<()> {
    let f = Fixture::new()?;
    for change in ["replace", "symlink", "mode"] {
        let id = crate::session::SessionId::generate(std::time::SystemTime::now())
            .map_err(io::Error::other)?;
        let broker = Broker::create(&f.root, id.as_str(), &std::env::current_exe()?, &[], true)
            .map_err(io::Error::other)?;
        let directory = broker.directory.clone();
        let saved = f.root.join(format!("saved-{change}"));
        if change == "mode" {
            fs::set_permissions(&directory, fs::Permissions::from_mode(0o755))?;
        } else {
            fs::rename(&directory, &saved)?;
            if change == "symlink" {
                symlink(&saved, &directory)?;
            } else {
                DirBuilder::new().mode(0o700).create(&directory)?;
                fs::write(directory.join("keep"), b"keep")?;
            }
        }
        assert!(matches!(
            broker.begin(f.call.call.facts.clone()),
            Err(AskpassError::ChangedDirectory)
        ));
        drop(broker);
        assert!(fs::symlink_metadata(&directory).is_ok());
        if change == "replace" {
            assert_eq!(fs::read(directory.join("keep"))?, b"keep");
        }
        if change == "symlink" {
            fs::remove_file(&directory)?;
        } else {
            fs::remove_dir_all(&directory)?;
        }
    }
    Ok(())
}

// 関門: bind の失敗がリンクを失わせて利用者の askpass を使う退行を、本物の名前の競合で捕まえる。
#[test]
fn listener_failure_keeps_helpers_and_diagnoses_that_they_refuse_requests() -> io::Result<()> {
    let f = Fixture::new()?;
    let id = crate::session::SessionId::generate(std::time::SystemTime::now())
        .map_err(io::Error::other)?;
    let mut broker = Broker::create(&f.root, id.as_str(), &std::env::current_exe()?, &[], false)
        .map_err(io::Error::other)?;
    fs::write(broker.directory.join("sock"), b"occupied")?;
    broker.enable_interactive();
    assert_eq!(broker.diagnostic(), Some(text::LISTENER_DISABLED));
    assert!(broker.take_requests().is_none());
    let mut env = BTreeMap::new();
    broker.environment(&mut env);
    for kind in [Kind::Ssh, Kind::Git, Kind::Sudo] {
        assert!(broker.directory.join(kind.helper()).is_symlink());
    }
    assert_eq!(env.len(), 5);
    assert!(
        broker
            .begin(f.call.call.facts.clone())
            .map_err(io::Error::other)?
            .is_none()
    );
    Ok(())
}

// 関門: accept が実際に EMFILE になった後にも、同じ窓口が次の要求を受ける。
// 他のテストの記述子の上限を変えないよう、この試験だけを子プロセスに隔離する。
#[test]
fn accept_recovers_from_descriptor_exhaustion_without_losing_the_listener() -> io::Result<()> {
    const CHILD: &str = "BENITOITEH_ASKPASS_FD_TEST";
    if std::env::var_os(CHILD).is_none() {
        let output = std::process::Command::new(std::env::current_exe()?)
            .args(["--exact", "askpass::tests::accept_recovers_from_descriptor_exhaustion_without_losing_the_listener", "--nocapture"])
            .env(CHILD, "1").output()?;
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        return Ok(());
    }
    let f = Fixture::new()?;
    struct SocketCleanup(PathBuf);
    impl Drop for SocketCleanup {
        fn drop(&mut self) {
            let _removed = fs::remove_file(&self.0);
        }
    }
    let _cleanup = SocketCleanup(f.broker.directory.join("accept"));
    let listener = UnixListener::bind(f.broker.directory.join("accept"))?;
    listener.set_nonblocking(true)?;
    let initial_client = UnixStream::connect(f.broker.directory.join("accept"))?;
    let limit = rustix::process::getrlimit(rustix::process::Resource::Nofile);
    rustix::process::setrlimit(
        rustix::process::Resource::Nofile,
        rustix::process::Rlimit {
            current: Some(64),
            maximum: limit.maximum,
        },
    )?;
    let mut files = Vec::new();
    loop {
        match fs::File::open("/dev/null") {
            Ok(file) => files.push(file),
            Err(error) if error.raw_os_error() == Some(rustix::io::Errno::MFILE.raw_os_error()) => {
                break;
            }
            Err(error) => return Err(error),
        }
    }
    let (errors, observed) = mpsc::channel();
    let (sender, prompts) = mpsc::channel();
    thread::scope(|scope| -> io::Result<()> {
        let current = &f.broker.current;
        let stop = &f.broker.stop;
        let worker = scope.spawn(move || {
            accept_loop(
                || {
                    let result = listener.accept().map(|(stream, _)| stream);
                    if let Err(error) = &result
                        && error.raw_os_error() == Some(rustix::io::Errno::MFILE.raw_os_error())
                    {
                        let _sent = errors.send(());
                    }
                    result
                },
                current,
                &sender,
                stop,
            )
        });
        observed.recv().map_err(io::Error::other)?;
        drop(initial_client);
        // 記述子を解放する順番は、OS の EMFILE を受けた通知で決める。
        files.clear();
        rustix::process::setrlimit(rustix::process::Resource::Nofile, limit)?;
        // macOS は EMFILE の accept で接続も失わせるので、回復後の新しい要求を使う。
        let mut client = UnixStream::connect(f.broker.directory.join("accept"))?;
        protocol::write_request(
            &mut client,
            &Request {
                v: 1,
                token: f.call.call.token.to_string(),
                kind: Kind::Ssh,
                ssh_prompt: Some("confirm".into()),
                prompt: "confirm".into(),
                pid: 42,
                ppid: 41,
            },
        )?;
        let prompt = prompts.recv().map_err(io::Error::other)?;
        prompt
            .reply
            .send(Answer::Yes)
            .map_err(|_| io::ErrorKind::BrokenPipe)?;
        let mut answer = [0];
        client.read_exact(&mut answer)?;
        assert_eq!(answer, [b'Y']);
        f.broker.stop.store(true, Ordering::Release);
        worker.join().map_err(|_| io::ErrorKind::Other)?;
        fs::remove_file(f.broker.directory.join("accept"))?;
        Ok(())
    })?;
    Ok(())
}

// 関門: OS の read の分割に左右されず、重なりと切り詰めの境を渡る一致を覆う。
#[test]
fn redaction_covers_read_boundaries_before_the_output_tail_is_limited() -> io::Result<()> {
    let f = Fixture::new()?;
    let receiver = f.broker.take_requests().ok_or(io::ErrorKind::Other)?;
    let stream = f.request(None, "secret")?;
    receiver
        .recv()
        .map_err(io::Error::other)?
        .reply
        .send(Answer::Secret(Zeroizing::new("abab".into())))
        .map_err(|_| io::ErrorKind::BrokenPipe)?;
    assert_eq!(status(stream)?, b"Sabab");
    for input in [
        format!("{}abababTAIL", "x".repeat(4093)),
        format!("{}TAIL", "ab".repeat(9000)),
        // 区切りの手前まで渡す形でも、区切りの前後の一致を読み取りの境目によらず伏せる。
        format!(
            "{}\nabab\rxabab\0{}\nababTAIL",
            "x".repeat(4090),
            "x".repeat(8190)
        ),
    ] {
        let mut tails = Vec::new();
        for chunk_size in [1, 4095, 8192] {
            let mut output =
                crate::tools::process_output::Output::mediated(12, 2, f.call.redactor());
            for chunk in input.as_bytes().chunks(chunk_size) {
                output.write_all(chunk)?;
            }
            output.finish_redaction()?;
            assert!(
                output.tail().0.ends_with("[secret]TAIL"),
                "{}",
                output.tail().0
            );
            assert!(!output.tail().0.contains("ab"));
            let mut whole = crate::tools::process_output::Output::mediated(
                usize::MAX,
                usize::MAX,
                f.call.redactor(),
            );
            for chunk in input.as_bytes().chunks(chunk_size) {
                whole.write_all(chunk)?;
            }
            whole.finish_redaction()?;
            tails.push(whole.tail().0);
        }
        assert!(tails.windows(2).all(|pair| pair.first() == pair.last()));
        assert!(tails.iter().all(|tail| !tail.contains("ab")));
    }
    let sink = Arc::new(Mutex::new(crate::tools::process_output::Output::mediated(
        100,
        10,
        f.call.redactor(),
    )));
    let stdout = Arc::clone(&sink);
    let stderr = Arc::clone(&sink);
    stdout
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .write_all(b"ab")?;
    stderr
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .write_all(b"ababTAIL")?;
    let mut output = sink.lock().unwrap_or_else(PoisonError::into_inner);
    output.finish_redaction()?;
    assert_eq!(output.tail().0, "[secret]TAIL");

    Ok(())
}

// 関門: 親はセッションの終了後にも残し、共有の根から同名の補助を置けなくする。
#[test]
fn private_parent_survives_session_and_invalid_parent_disables_mediation() -> io::Result<()> {
    if rerun_without_runtime(
        "private_parent_survives_session_and_invalid_parent_disables_mediation",
    )? {
        return Ok(());
    }
    let f = Fixture::new()?;
    let short = Path::new("/tmp").join(format!(
        "bhp-{}",
        crate::session::SessionId::generate(std::time::SystemTime::now())
            .map_err(io::Error::other)?
    ));
    fs::create_dir(&short)?;
    struct Cleanup(PathBuf);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _removed = fs::remove_dir_all(&self.0);
        }
    }
    let _cleanup = Cleanup(short.clone());
    let parent = parent_directory(&short.canonicalize()?);
    let broker = Broker::create(&short, "session", &std::env::current_exe()?, &[], false)
        .map_err(io::Error::other)?;
    assert_eq!(broker.directory, parent.join("session"));
    assert!(valid_parent(&parent));
    let identity = fs::symlink_metadata(&parent)?;
    drop(broker);
    assert!(valid_parent(&parent));
    assert_eq!((identity.dev(), identity.ino()), {
        let metadata = fs::symlink_metadata(&parent)?;
        (metadata.dev(), metadata.ino())
    });
    for change in ["mode", "link", "replace"] {
        let broker = Broker::create(&short, "session", &std::env::current_exe()?, &[], true)
            .map_err(io::Error::other)?;
        let saved = short.join("saved");
        if change == "mode" {
            fs::set_permissions(&parent, fs::Permissions::from_mode(0o755))?;
        } else {
            fs::rename(&parent, &saved)?;
            if change == "link" {
                symlink(&saved, &parent)?;
            } else {
                DirBuilder::new().mode(0o700).create(&parent)?;
            }
        }
        assert!(matches!(
            broker.begin(f.call.call.facts.clone()),
            Err(AskpassError::ChangedDirectory)
        ));
        if change != "replace" {
            assert!(matches!(
                Broker::create(&short, "other", &std::env::current_exe()?, &[], true),
                Err(AskpassError::ChangedDirectory)
            ));
        }
        drop(broker);
        if change == "mode" {
            fs::set_permissions(&parent, fs::Permissions::from_mode(0o700))?;
            fs::remove_file(parent.join("session/sock"))?;
            crate::session::remove::directory(&parent.join("session")).map_err(io::Error::other)?;
        } else {
            if change == "link" {
                fs::remove_file(&parent)?;
            } else {
                fs::remove_dir(&parent)?;
            }
            fs::rename(&saved, &parent)?;
            fs::remove_file(parent.join("session/sock"))?;
            crate::session::remove::directory(&parent.join("session")).map_err(io::Error::other)?;
        }
    }
    Ok(())
}

// 読み始めの通知で途中の要求が受理済みと確かめ、停止を見た回数で終了を確かめる。
#[test]
fn shutdown_observes_stop_while_a_partial_request_is_connected() -> io::Result<()> {
    let f = Fixture::new()?;
    let (checks, observed) = mpsc::channel();
    *f.broker
        .stop
        .read_observer
        .lock()
        .unwrap_or_else(PoisonError::into_inner) = Some(checks);
    let mut client = UnixStream::connect(&f.call.socket)?;
    client.write_all(&[0, 0])?;
    assert!(!observed.recv().map_err(io::Error::other)?);
    assert!(!observed.recv().map_err(io::Error::other)?);
    f.broker.shutdown();
    let observations = observed.try_iter().collect::<Vec<_>>();
    assert_eq!(observations.iter().filter(|stopped| **stopped).count(), 1);
    assert_eq!(observations.last(), Some(&true));
    assert!(!f.broker.directory.exists());
    Ok(())
}

// 関門: stderr の割り込みで stdout 自体の連続した秘密が漏れる退行を、sink の書く順で再現する。
#[test]
fn interleaved_stream_redaction_preserves_each_stream_secret() -> io::Result<()> {
    let f = Fixture::new()?;
    let requests = f.broker.take_requests().ok_or(io::ErrorKind::Other)?;
    let stream = f.request(None, "synthetic")?;
    requests
        .recv()
        .map_err(io::Error::other)?
        .reply
        .send(Answer::Secret(Zeroizing::new("Zq9-secret-Value".into())))
        .map_err(|_| io::ErrorKind::BrokenPipe)?;
    assert_eq!(status(stream)?, b"SZq9-secret-Value");
    let sink = Arc::new(Mutex::new(crate::tools::process_output::Output::mediated(
        100,
        10,
        f.call.redactor(),
    )));
    let stdout = Arc::new(Mutex::new(crate::tools::process_output::StreamOutput::new(
        Arc::clone(&sink),
        Some(f.call.redactor()),
    )));
    let stderr = Arc::new(Mutex::new(crate::tools::process_output::StreamOutput::new(
        Arc::clone(&sink),
        Some(f.call.redactor()),
    )));
    stdout
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .write_all(b"pw=Zq9-sec")?;
    stderr
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .write_all(b"progress 50%\n")?;
    stdout
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .write_all(b"ret-Value\n")?;
    stdout
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .finish()?;
    stderr
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .finish()?;
    let mut output = sink.lock().unwrap_or_else(PoisonError::into_inner);
    output.finish_redaction()?;
    let result = output.tail().0;
    assert!(result.contains("[secret]"), "{result}");
    assert!(result.contains("progress 50%\n"), "{result}");
    assert!(!result.contains("Zq9-sec"), "{result}");
    assert!(!result.contains("ret-Value"), "{result}");
    Ok(())
}

// 関門: 秘密の長さだけ出力を留める退行は、改行まで届いた行を次の出力まで示さず、
// 混ぜた並びの順も書いた順と変える。区切りの手前まで渡しても、読み取りをまたぐ秘密は伏せる。
#[test]
fn line_separators_release_output_in_written_order_without_missing_secrets() -> io::Result<()> {
    let f = Fixture::new()?;
    let requests = f.broker.take_requests().ok_or(io::ErrorKind::Other)?;
    let secret = "Zq9-secret-Value0123456789abcdef";
    assert_eq!(secret.len(), 32);
    let stream = f.request(None, "synthetic")?;
    requests
        .recv()
        .map_err(io::Error::other)?
        .reply
        .send(Answer::Secret(Zeroizing::new(secret.into())))
        .map_err(|_| io::ErrorKind::BrokenPipe)?;
    status(stream)?;
    let sink = Arc::new(Mutex::new(crate::tools::process_output::Output::mediated(
        100,
        10,
        f.call.redactor(),
    )));
    let stdout = Arc::new(Mutex::new(crate::tools::process_output::StreamOutput::new(
        Arc::clone(&sink),
        Some(f.call.redactor()),
    )));
    let stderr = Arc::new(Mutex::new(crate::tools::process_output::StreamOutput::new(
        Arc::clone(&sink),
        Some(f.call.redactor()),
    )));
    for (stream, line) in [(&stdout, b"a\n"), (&stderr, b"b\n"), (&stdout, b"c\n")] {
        stream
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .write_all(line)?;
    }
    // 最後を流す前に、書いた順のまま届いている。
    assert_eq!(
        sink.lock().unwrap_or_else(PoisonError::into_inner).tail().0,
        "a\nb\nc\n"
    );
    let mut redactor = f.call.redactor();
    let line = b"progress 50% of five\n";
    assert_eq!(line.len(), 21);
    assert_eq!(redactor.push(line, false), line);
    assert_eq!(redactor.push(b"x\r", false), b"x\r");
    assert_eq!(redactor.push(b"y\0", false), b"y\0");
    let mut output = redactor.push(b"pw=Zq9-secret-Va", false);
    output.extend(redactor.push(b"lue0123456789abcdef\n", false));
    assert_eq!(output, b"pw=[secret]\n");
    assert_eq!(redactor.push(&[], true), b"");
    Ok(())
}

// 関門: 呼び出しの状態（照合の値を含む）を次の呼び出しまで持ち続ける退行と、
// 先の守りの後始末が後の呼び出しを窓口から外す退行を捕まえる（設計書 03-04「秘密の扱い」）。
#[test]
fn finished_calls_release_their_state_without_disturbing_the_next_call() -> io::Result<()> {
    let f = Fixture::new()?;
    let requests = f.broker.take_requests().ok_or(io::ErrorKind::Other)?;
    let facts = f.call.call.facts.clone();
    let first = f
        .broker
        .begin(facts.clone())
        .map_err(io::Error::other)?
        .ok_or(io::ErrorKind::Other)?;
    let weak = Arc::downgrade(&first.call);
    let redactor = first.redactor();
    let second = f
        .broker
        .begin(facts)
        .map_err(io::Error::other)?
        .ok_or(io::ErrorKind::Other)?;
    drop(first);
    assert!(weak.upgrade().is_some());
    drop(redactor);
    assert!(weak.upgrade().is_none());
    let mut client = UnixStream::connect(&second.socket)?;
    client.set_read_timeout(Some(Duration::from_secs(6)))?;
    protocol::write_request(
        &mut client,
        &Request {
            v: 1,
            token: second.call.token.to_string(),
            kind: Kind::Ssh,
            ssh_prompt: Some("confirm".into()),
            prompt: "second".into(),
            pid: 42,
            ppid: 41,
        },
    )?;
    let prompt = requests
        .recv_timeout(Duration::from_secs(5))
        .map_err(io::Error::other)?;
    assert_eq!(prompt.prompt, "second");
    prompt
        .reply
        .send(Answer::Yes)
        .map_err(|_| io::ErrorKind::BrokenPipe)?;
    assert_eq!(status(client)?, b"Y");
    // 後の呼び出しも、守りを手放せば窓口から外れる。
    let weak = Arc::downgrade(&second.call);
    drop(prompt);
    drop(second);
    assert!(weak.upgrade().is_none());
    Ok(())
}

// 関門: Broker の準備・begin の早期失敗でも、Fixture が作った根を残さない。
#[test]
fn fixture_removes_root_when_broker_or_begin_fails() -> io::Result<()> {
    let outer = Fixture::new()?;
    for (name, executable, interactive) in [
        ("broker", outer.root.join("missing-executable"), true),
        ("begin", std::env::current_exe()?, false),
    ] {
        let root = outer.root.join(name);
        let cleanup = FixtureRoot::create(root.clone())?;
        assert!(Fixture::at(cleanup, &executable, interactive).is_err());
        assert!(!root.exists(), "{}", root.display());
    }
    Ok(())
}

// 関門: 通常の macOS TMPDIR で、短い子ならソケットの上限に収まる配置を逃がさない。
#[cfg(target_os = "macos")]
#[test]
fn macos_short_helper_name_keeps_private_temporary_root() -> io::Result<()> {
    let f = Fixture::new()?;
    let root = Path::new("/tmp").canonicalize()?.join(format!(
        "bh-mac-{}",
        crate::session::SessionId::generate(std::time::SystemTime::now())
            .map_err(io::Error::other)?
    ));
    fs::create_dir(&root)?;
    struct Remove(PathBuf);
    impl Drop for Remove {
        fn drop(&mut self) {
            let _removed = fs::remove_dir_all(&self.0);
        }
    }
    let _remove = Remove(root.clone());
    let root = root.join(
        "x".repeat(
            70usize
                .saturating_sub(root.as_os_str().as_bytes().len())
                .saturating_sub(1),
        ),
    );
    fs::create_dir(&root)?;
    let id = crate::session::SessionId::generate(std::time::SystemTime::now())
        .map_err(io::Error::other)?;
    let broker = Broker::create(&root, id.as_str(), &std::env::current_exe()?, &[], true)
        .map_err(io::Error::other)?;
    assert!(
        broker.directory.starts_with(&root),
        "{}",
        broker.directory.display()
    );
    assert!(broker.directory.join("sock").as_os_str().as_bytes().len() <= 103);
    let receiver = broker.take_requests().ok_or(io::ErrorKind::Other)?;
    let call = broker
        .begin(f.call.call.facts.clone())
        .map_err(io::Error::other)?
        .ok_or(io::ErrorKind::Other)?;
    let mut client = UnixStream::connect(&call.socket)?;
    protocol::write_request(
        &mut client,
        &Request {
            v: 1,
            token: call.call.token.to_string(),
            kind: Kind::Ssh,
            ssh_prompt: Some("confirm".into()),
            prompt: "synthetic".into(),
            pid: 42,
            ppid: 41,
        },
    )?;
    receiver
        .recv()
        .map_err(io::Error::other)?
        .reply
        .send(Answer::Yes)
        .map_err(|_| io::ErrorKind::BrokenPipe)?;
    assert_eq!(status(client)?, b"Y");
    Ok(())
}

// 関門: 環境は子ごとに固定し、runtime の有効性と選択順を本物の Broker で確かめる。
#[cfg(target_os = "linux")]
#[test]
fn linux_runtime_directory_precedes_temporary_root_only_when_private() -> io::Result<()> {
    const CHILD_ROOT: &str = "BENITOITEH_TEST_RUNTIME_ROOT";
    const CHILD_CASE: &str = "BENITOITEH_TEST_RUNTIME_CASE";
    if let Some(root) = std::env::var_os(CHILD_ROOT) {
        let root = PathBuf::from(root);
        let case = std::env::var(CHILD_CASE).map_err(io::Error::other)?;
        let result = Broker::create(
            &root.join("tmp"),
            "runtime-test",
            &std::env::current_exe()?,
            &[],
            true,
        );
        if case == "unowned-parent" {
            let error = result.err().ok_or(io::ErrorKind::Other)?;
            assert!(matches!(error, AskpassError::UnownedParent(_)));
            let message = error.to_string();
            assert!(message.contains(&parent_directory(&root.join("tmp")).display().to_string()));
            assert!(message.contains("owned by another user"));
            assert!(message.contains("Set TMPDIR to a private directory owned by you"));
        } else {
            let broker = result.map_err(io::Error::other)?;
            let expected = root.join(if case == "valid" { "run" } else { "tmp" });
            assert!(
                broker.directory.starts_with(expected),
                "{}",
                broker.directory.display()
            );
            assert!(broker.take_requests().is_some());
            assert!(
                broker
                    .begin(Facts {
                        call_id: "runtime".into(),
                        command: "synthetic".into(),
                        cwd: root,
                        mode: "normal (danger-full-access)"
                    })
                    .map_err(io::Error::other)?
                    .is_some()
            );
        }
        return Ok(());
    }
    let mut random = [0; 8];
    getrandom::fill(&mut random).map_err(io::Error::other)?;
    let root = FixtureRoot::create(
        Path::new("/tmp")
            .canonicalize()?
            .join(format!("br-{:016x}", u64::from_ne_bytes(random))),
    )?;
    let temporary = root.0.join("tmp");
    let runtime = root.0.join("run");
    DirBuilder::new().mode(0o700).create(&temporary)?;
    let parent = parent_directory(&temporary);
    DirBuilder::new().mode(0o755).create(&parent)?;
    for case in ["valid", "mode", "link", "foreign", "unowned-parent"] {
        if matches!(case, "foreign" | "unowned-parent") && !rustix::process::geteuid().is_root() {
            continue;
        }
        DirBuilder::new().mode(0o700).create(&runtime)?;
        if case == "mode" {
            fs::set_permissions(&runtime, fs::Permissions::from_mode(0o755))?;
        } else if case == "link" {
            fs::rename(&runtime, root.0.join("target"))?;
            symlink(root.0.join("target"), &runtime)?;
        } else if case == "foreign" {
            rustix::fs::chown(&runtime, Some(rustix::process::Uid::from_raw(65534)), None)?;
        } else if case == "unowned-parent" {
            fs::set_permissions(&runtime, fs::Permissions::from_mode(0o755))?;
            rustix::fs::chown(&parent, Some(rustix::process::Uid::from_raw(65534)), None)?;
        }
        let output = std::process::Command::new(std::env::current_exe()?)
            .args([
                "--exact",
                "askpass::tests::linux_runtime_directory_precedes_temporary_root_only_when_private",
                "--nocapture",
            ])
            .env(CHILD_ROOT, &root.0)
            .env(CHILD_CASE, case)
            .env("XDG_RUNTIME_DIR", &runtime)
            .output()?;
        assert!(
            output.status.success(),
            "{case}: {}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        if case == "foreign" {
            rustix::fs::chown(&runtime, Some(rustix::process::geteuid()), None)?;
        }
        if case == "unowned-parent" {
            rustix::fs::chown(&parent, Some(rustix::process::geteuid()), None)?;
        }
        if case == "link" {
            fs::remove_file(&runtime)?;
        } else {
            fs::remove_dir_all(&runtime)?;
        }
        fs::set_permissions(&parent, fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}
