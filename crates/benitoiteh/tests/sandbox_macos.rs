//! 本物の Seatbelt と子孫の終了状態で安全の性質を確かめる（設計書 03-02、07-01「安全の性質のテスト」）。

#![cfg(target_os = "macos")]

use std::collections::BTreeMap;
use std::error::Error;
use std::ffi::{OsStr, OsString};
use std::fs::{self, File};
use std::io;
use std::net::TcpListener;
use std::os::fd::AsRawFd;
use std::os::unix::net::{UnixDatagram, UnixListener};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use benitoiteh::sandbox::{
    PolicyConfig, ProgramAccess, SandboxCommand, SandboxPolicy, SandboxStdin,
};

type TestResult = Result<(), Box<dyn Error>>;

static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

fn io_at<T>(operation: &str, path: &Path, result: io::Result<T>) -> io::Result<T> {
    result.map_err(|error| {
        io::Error::new(
            error.kind(),
            format!("{operation} at {}: {error}", path.display()),
        )
    })
}

fn copy(from: &Path, to: &Path) -> io::Result<u64> {
    io_at(
        &format!("fs::copy from {}", from.display()),
        to,
        fs::copy(from, to),
    )
}

struct Fixture {
    root: PathBuf,
    writable: PathBuf,
    probe: PathBuf,
}

fn artifact(name: &str) -> Result<PathBuf, Box<dyn Error>> {
    let executable =
        std::env::current_exe().map_err(|error| format!("std::env::current_exe: {error}"))?;
    let directory = executable
        .parent()
        .and_then(Path::parent)
        .ok_or("Missing target directory")?;
    let path = directory.join(name);
    if !path.is_file() {
        return Err(format!(
            "Missing {}; run cargo build -p sandbox-probe before testing",
            path.display()
        )
        .into());
    }
    Ok(io_at(
        "fs::canonicalize artifact",
        &path,
        fs::canonicalize(&path),
    )?)
}

impl Fixture {
    fn new(short: bool) -> Result<Self, Box<dyn Error>> {
        // Unix のソケットのパスは 104 バイトまでなので、この試験だけ短い /tmp の下に置く。
        let parent = if short {
            PathBuf::from("/tmp")
        } else {
            std::env::temp_dir()
        };
        let parent = io_at(
            "fs::canonicalize temporary parent",
            &parent,
            fs::canonicalize(&parent),
        )?;
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|error| format!("SystemTime::duration_since UNIX_EPOCH: {error}"))?
            .as_nanos();
        // 時刻の分解能だけに頼ると並列のテストが同じ名前を選ぶので、プロセス内の連番も使う。
        let sequence = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
        let root = parent.join(format!("bh-{}-{nonce:x}-{sequence:x}", std::process::id()));
        io_at("fs::create_dir fixture root", &root, fs::create_dir(&root))?;
        let fixture = Self {
            writable: root.join("write"),
            probe: root.join("probe"),
            root,
        };
        io_at(
            "fs::create_dir writable root",
            &fixture.writable,
            fs::create_dir(&fixture.writable),
        )?;
        copy(&artifact("sandbox-probe")?, &fixture.probe)?;
        Ok(fixture)
    }

    fn config(&self) -> PolicyConfig {
        PolicyConfig::new(vec![self.writable.clone()])
    }

    fn command(&self, config: PolicyConfig, args: &[&OsStr]) -> Result<Command, Box<dyn Error>> {
        self.command_for(config, &self.probe, args)
    }

    fn command_for(
        &self,
        config: PolicyConfig,
        program: &Path,
        args: &[&OsStr],
    ) -> Result<Command, Box<dyn Error>> {
        let policy = SandboxPolicy::new(config).map_err(|error| {
            format!(
                "SandboxPolicy::new for {} in {}: {error}",
                program.display(),
                self.root.display()
            )
        })?;
        Ok(SandboxCommand {
            policy,
            program: program.to_path_buf(),
            args: args.iter().map(|value| value.to_os_string()).collect(),
            cwd: self.root.clone(),
            env: BTreeMap::new(),
            stdin: SandboxStdin::Null,
        }
        .to_command(&self.probe)
        .map_err(|error| {
            format!(
                "SandboxCommand::to_command for {} in {}: {error}",
                program.display(),
                self.root.display()
            )
        })?)
    }

    fn run(&self, config: PolicyConfig, args: &[&OsStr], expected: i32) -> TestResult {
        let output = io_at(
            &format!("Command::output sandbox-probe {args:?}"),
            &self.probe,
            self.command(config, args)?.output(),
        )?;
        check(&output, expected);
        Ok(())
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        if let Err(error) = fs::remove_dir_all(&self.root) {
            eprintln!("fs::remove_dir_all at {}: {error}", self.root.display());
        }
    }
}

fn check(output: &Output, expected: i32) {
    assert_eq!(
        output.status.code(),
        Some(expected),
        "status={}\n{}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
}

// 作成時の関門: 読ませない場所の設定漏れを、実際の read と正の対照で捕まえる。
#[test]
fn reading_excludes_denied_paths() -> TestResult {
    let f = Fixture::new(false)?;
    let secret = f.root.join("secret");
    let public = f.root.join("public");
    fs::create_dir(&secret)?;
    fs::write(secret.join("key"), b"secret")?;
    fs::write(&public, b"public")?;
    let mut config = f.config();
    config.denied_read.push(secret.clone());
    f.run(config.clone(), &[OsStr::new("read"), public.as_os_str()], 0)?;
    f.run(
        config,
        &[OsStr::new("read"), secret.join("key").as_os_str()],
        10,
    )
}

// 作成時の関門: 作成・上書き・付け替えの許可漏れを、FS の変化を伴う同じ境界で捕まえる。
#[test]
fn writing_is_confined_and_protected_names_cannot_be_created_or_replaced() -> TestResult {
    let f = Fixture::new(false)?;
    f.run(
        f.config(),
        &[OsStr::new("write"), f.writable.join("ok").as_os_str()],
        0,
    )?;
    let written = f.writable.join("ok");
    assert_eq!(
        io_at("fs::read written file", &written, fs::read(&written))?,
        b"sandbox-probe\n"
    );
    f.run(
        f.config(),
        &[OsStr::new("write"), f.root.join("outside").as_os_str()],
        10,
    )?;
    for name in [".git", ".benitoiteh"] {
        // 末尾の名前だけでなく入れ子も、将来の git の使用時に危険になるので守る。
        for parent in [&f.writable, &f.writable.join("nested")] {
            io_at(
                "fs::create_dir_all test parent",
                parent,
                fs::create_dir_all(parent),
            )?;
            let protected = parent.join(name);
            f.run(
                f.config(),
                &[OsStr::new("mkdir"), protected.as_os_str()],
                10,
            )?;
            f.run(
                f.config(),
                &[OsStr::new("write"), protected.as_os_str()],
                10,
            )?;
            // 拒否された rename の元は残るので、保護する名前ごとに別の準備用の名前を使う。
            let source = parent.join(format!("replacement-{name}"));
            io_at(
                "fs::create_dir rename source",
                &source,
                fs::create_dir(&source),
            )?;
            f.run(
                f.config(),
                &[
                    OsStr::new("rename"),
                    source.as_os_str(),
                    protected.as_os_str(),
                ],
                10,
            )?;
            assert!(source.is_dir());
            assert!(!protected.exists());
            io_at(
                "fs::create_dir existing protected directory",
                &protected,
                fs::create_dir(&protected),
            )?;
            let contents = protected.join("config");
            io_at(
                "fs::write protected fixture contents",
                &contents,
                fs::write(&contents, b"unchanged"),
            )?;
            f.run(f.config(), &[OsStr::new("write"), contents.as_os_str()], 10)?;
            f.run(
                f.config(),
                &[
                    OsStr::new("rename"),
                    protected.as_os_str(),
                    parent.join(format!("moved-{name}")).as_os_str(),
                ],
                10,
            )?;
            assert_eq!(
                io_at(
                    "fs::read protected fixture contents",
                    &contents,
                    fs::read(&contents)
                )?,
                b"unchanged"
            );
        }
    }
    Ok(())
}

// 作成時の関門: 大文字の別名で保護を回り込む退行と、通常モードでの保護の欠落を捕まえる。
#[test]
fn protected_names_match_ascii_case_insensitively() -> TestResult {
    let f = Fixture::new(false)?;
    let mut config = f.config();
    config.deny_exec_in_writable = false;
    config.protected_names.push(".Cache-2_data".into());
    // 既定の FS では綴りの違う名前も同じ場所になるので、試験ごとに別の親を使う。
    for (i, name) in [
        ".GIT",
        ".Git",
        ".BENITOITEH",
        ".Benitoiteh",
        ".CACHE-2_DATA",
    ]
    .iter()
    .enumerate()
    {
        let parent = f.writable.join(format!("case-{i}"));
        io_at(
            "fs::create_dir case parent",
            &parent,
            fs::create_dir(&parent),
        )?;
        let protected = parent.join(name);
        f.run(
            config.clone(),
            &[OsStr::new("mkdir"), protected.as_os_str()],
            10,
        )?;
        assert!(!protected.exists(), "{}", protected.display());

        let hooks = protected.join("hooks");
        io_at(
            "fs::create_dir_all existing mixed-case hooks",
            &hooks,
            fs::create_dir_all(&hooks),
        )?;
        let contents = hooks.join("x");
        f.run(
            config.clone(),
            &[OsStr::new("write"), contents.as_os_str()],
            10,
        )?;
        assert!(!contents.exists(), "{}", contents.display());
    }
    // '.' と名前の末尾を正しく照合しないと、これらの正の対照まで拒否される。
    for name in ["aGIT", ".GIT-extra"] {
        let path = f.writable.join(name);
        f.run(config.clone(), &[OsStr::new("mkdir"), path.as_os_str()], 0)?;
        assert!(path.is_dir(), "{}", path.display());
    }
    Ok(())
}

// 作成時の関門: 通常モードでも書ける場所の実行を常に拒む退行を、本物の bash からの起動で捕まえる。
#[test]
fn writable_executables_can_run_when_policy_allows_them() -> TestResult {
    let f = Fixture::new(false)?;
    let writable_probe = f.writable.join("probe");
    copy(&f.probe, &writable_probe)?;
    let mut config = f.config();
    config.deny_exec_in_writable = false;
    let shell_path = Path::new("/bin/bash");
    let shell = io_at(
        "fs::canonicalize bash",
        shell_path,
        fs::canonicalize(shell_path),
    )?;
    let mut command = f.command_for(
        config,
        &shell,
        &[
            OsStr::new("-c"),
            OsStr::new("exec \"$1\" socketpair"),
            OsStr::new("probe-workspace-exec"),
            writable_probe.as_os_str(),
        ],
    )?;
    let output = io_at(
        "Command::output writable executable from bash",
        &writable_probe,
        command.output(),
    )?;
    check(&output, 0);
    Ok(())
}

// 作成時の関門: 子孫の規則の継承と、許可一覧より書ける場所の拒否が優先することを確かめる。
#[test]
fn execution_allowlist_applies_to_descendants_and_rejects_writable_executables() -> TestResult {
    let f = Fixture::new(false)?;
    let writable_probe = f.writable.join("probe");
    fs::copy(&f.probe, &writable_probe)?;
    let mut config = f.config();
    config.programs = ProgramAccess::Only(vec![f.probe.clone(), writable_probe.clone()]);
    f.run(
        config.clone(),
        &[
            OsStr::new("exec"),
            f.probe.as_os_str(),
            OsStr::new("socketpair"),
        ],
        0,
    )?;
    f.run(
        config.clone(),
        &[
            OsStr::new("exec"),
            OsStr::new("/bin/sh"),
            OsStr::new("-c"),
            OsStr::new("exit 0"),
        ],
        10,
    )?;
    f.run(
        config.clone(),
        &[
            OsStr::new("exec"),
            f.probe.as_os_str(),
            OsStr::new("exec"),
            OsStr::new("/bin/sh"),
            OsStr::new("-c"),
            OsStr::new("exit 0"),
        ],
        12,
    )?;
    f.run(
        config,
        &[
            OsStr::new("exec"),
            writable_probe.as_os_str(),
            OsStr::new("socketpair"),
        ],
        10,
    )
}

// 作成時の関門: IP の許可で Unix IPC まで許す退行を、ストリームと宛先付き送信の両方で捕まえる。
#[test]
fn external_unix_ipc_is_denied_with_and_without_ip_permission() -> TestResult {
    let f = Fixture::new(true)?;
    let stream_path = f.root.join("s");
    let datagram_path = f.root.join("d");
    let _stream = UnixListener::bind(&stream_path)?;
    let _datagram = UnixDatagram::bind(&datagram_path)?;
    for allow_ip in [false, true] {
        let mut config = f.config();
        config.allow_ip = allow_ip;
        f.run(
            config.clone(),
            &[OsStr::new("unix-connect"), stream_path.as_os_str()],
            10,
        )?;
        f.run(
            config.clone(),
            &[OsStr::new("unix-dgram-send"), datagram_path.as_os_str()],
            10,
        )?;
        f.run(config, &[OsStr::new("socketpair")], 0)?;
    }
    Ok(())
}

// 作成時の関門: 通信の許可の有無を、同じ数値のループバック宛先で確かめる。DNS は使わない。
#[test]
fn ip_connections_follow_policy() -> TestResult {
    let f = Fixture::new(false)?;
    let server = TcpListener::bind("127.0.0.1:0")?;
    let address = server.local_addr()?.to_string();
    f.run(
        f.config(),
        &[OsStr::new("tcp-connect"), OsStr::new(&address)],
        10,
    )?;
    let mut config = f.config();
    config.allow_ip = true;
    f.run(
        config,
        &[OsStr::new("tcp-connect"), OsStr::new(&address)],
        0,
    )
}

// 作成時の関門: Seatbelt 自体は記述子を閉じないという限界と、標準の close-on-exec の対照を記録する。
#[test]
fn inherited_descriptors_require_close_on_exec_on_macos() -> TestResult {
    let f = Fixture::new(false)?;
    let path = f.root.join("fd");
    fs::write(&path, b"descriptor")?;
    let file = File::open(&path)?;
    let number = file.as_raw_fd().to_string();
    f.run(
        f.config(),
        &[OsStr::new("fd-open"), OsStr::new(&number)],
        13,
    )?;

    let sandbox = f.command_for(
        f.config(),
        &fs::canonicalize("/bin/sh")?,
        &[
            OsStr::new("-c"),
            OsStr::new("exec \"$1\" fd-open \"$PROBE_FD\""),
            OsStr::new("probe-fd"),
            f.probe.as_os_str(),
        ],
    )?;
    // sandbox-exec による環境の引き継ぎ後、番号を引数にする。親のテストの記述子の印は変えない。
    let args: Vec<OsString> = sandbox.get_args().map(OsStr::to_os_string).collect();
    let output = Command::new(&f.probe)
        .arg("spawn-with-fd")
        .arg(&path)
        .arg("--")
        .arg(sandbox.get_program())
        .args(args)
        .env_clear()
        .current_dir(&f.root)
        .process_group(0)
        .output()?;
    check(&output, 0);
    // file を最初の子の終了まで保持して、記述子を閉じたための成功を避ける。
    drop(file);
    Ok(())
}

// 作成時の関門: 方針の文の検査ではなく、dyld が実際に同じライブラリを写せるかを正負で比べる。
#[test]
fn dyld_cannot_map_libraries_from_writable_directories() -> TestResult {
    let f = Fixture::new(false)?;
    let trusted = f.root.join("trusted.dylib");
    let writable = f.writable.join("untrusted.dylib");
    copy(&artifact("libsandbox_probe.dylib")?, &trusted)?;
    copy(&trusted, &writable)?;
    let shell_path = Path::new("/bin/sh");
    let shell = io_at(
        "fs::canonicalize dyld shell",
        shell_path,
        fs::canonicalize(shell_path),
    )?;
    for (library, allowed) in [(&trusted, true), (&writable, false)] {
        // sandbox-exec は SIP の対象なので、DYLD_* はサンドボックスを掛けた後に設定する。
        let mut command = f.command_for(f.config(), &shell, &[
            OsStr::new("-c"),
            OsStr::new("export DYLD_INSERT_LIBRARIES=\"$1\" DYLD_PRINT_LIBRARIES=1; exec \"$2\" socketpair"),
            OsStr::new("probe-dyld"), library.as_os_str(), f.probe.as_os_str(),
        ])?;
        let output = io_at("Command::output dyld insertion", library, command.output())?;
        let stderr = String::from_utf8_lossy(&output.stderr);
        if allowed {
            check(&output, 0);
            assert!(
                stderr.lines().any(|line| line.starts_with("dyld[")
                    && line.contains("<")
                    && line.contains(trusted.to_string_lossy().as_ref())),
                "{stderr}"
            );
        } else {
            assert!(
                !output.status.success(),
                "The writable library was accepted: {stderr}"
            );
            assert!(stderr.contains("untrusted.dylib"), "{stderr}");
        }
    }
    Ok(())
}
