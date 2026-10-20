//! 本物の Seatbelt と子孫の終了状態で安全の性質を確かめる（設計書 03-02、07-01「安全の性質のテスト」）。

#![cfg(target_os = "macos")]

#[path = "support/temporary_cleanup.rs"]
mod temporary_cleanup;

#[path = "support/bash_sandbox.rs"]
mod bash_sandbox;

#[path = "support/macos_process_info.rs"]
mod macos_process_info;

use std::collections::BTreeMap;
use std::error::Error;
use std::ffi::{OsStr, OsString};
use std::fs::{self, File};
use std::io;
use std::net::TcpListener;
use std::os::fd::AsRawFd;
use std::os::unix::fs::MetadataExt;
use std::os::unix::net::{UnixDatagram, UnixListener};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use benitoiteh::config::{Sandbox, writable::WritableLocations};
use benitoiteh::sandbox::{
    PolicyConfig, ProgramAccess, SandboxCommand, SandboxPolicy, SandboxStdin, normal_policy,
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
        .to_command(Path::new(env!("CARGO_BIN_EXE_benitoiteh")))
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

// 関門: 判定だけを直して製品の通常モードの sort が警告を出す退行を捕まえる。
// 対照は同じ方針から実測した二名の行だけを除き、別の SBPL を手で書かない。
#[test]
fn normal_sort_is_warning_free_with_the_measured_sysctls() -> TestResult {
    let sort = fs::canonicalize("/usr/bin/sort")?;
    let helper = Path::new(env!("CARGO_BIN_EXE_benitoiteh"));
    for (input, expected) in [("", ""), ("z.txt\na space.txt\n", "a space.txt\nz.txt\n")] {
        let f = Fixture::new(true)?;
        let temporary = f.root.join("temporary");
        fs::create_dir(&temporary)?;
        let mut env = BTreeMap::new();
        for (name, child) in [
            ("HOME", "home"),
            ("XDG_CONFIG_HOME", "config"),
            ("XDG_STATE_HOME", "state"),
        ] {
            let path = f.root.join(child);
            fs::create_dir(&path)?;
            env.insert(OsString::from(name), path.into_os_string());
        }
        env.insert("TMPDIR".into(), temporary.as_os_str().to_owned());
        env.insert("PATH".into(), "/usr/bin:/bin".into());
        env.insert("LC_ALL".into(), "C".into());
        let locations = WritableLocations {
            project: f.writable.clone(),
            temporary,
            additional: Vec::new(),
        };
        let policy = normal_policy(Sandbox::WorkspaceWrite, &locations, false, &[])?
            .ok_or("Missing normal sandbox policy")?;
        let input_path = f.writable.join("input.txt");
        fs::write(&input_path, input)?;
        let sandbox = SandboxCommand {
            policy,
            program: sort.clone(),
            args: vec![input_path.into_os_string()],
            cwd: f.writable.clone(),
            env,
            stdin: SandboxStdin::Null,
        };
        let output = sandbox.to_command(helper)?.output()?;
        assert_eq!(output.status.code(), Some(0), "{output:?}");
        assert_eq!(output.stdout, expected.as_bytes(), "{output:?}");
        assert!(output.stderr.is_empty(), "{output:?}");

        let mut profile = sandbox.policy.seatbelt_profile();
        for rule in [
            "(allow sysctl-read (sysctl-name \"hw.memsize\"))\n",
            "(allow sysctl-read (sysctl-name \"hw.pagesize\"))\n",
        ] {
            assert!(profile.source.contains(rule), "{}", profile.source);
            profile.source = profile.source.replace(rule, "");
        }
        let mut control =
            benitoiteh::sandbox::session::command(helper, Path::new("/usr/bin/sandbox-exec"));
        control.arg("-p").arg(profile.source);
        for (name, value) in profile.parameters {
            let mut argument = OsString::from(format!("-D{name}="));
            argument.push(value);
            control.arg(argument);
        }
        let control = control
            .arg("--")
            .arg(&sandbox.program)
            .args(&sandbox.args)
            .current_dir(&sandbox.cwd)
            .env_clear()
            .envs(&sandbox.env)
            .stdin(Stdio::null())
            .output()?;
        let stderr = String::from_utf8_lossy(&control.stderr);
        assert!(stderr.contains("sysconf pages"), "{control:?}");
        assert!(stderr.contains("Operation not permitted"), "{control:?}");
    }
    Ok(())
}

// 関門: file-read の制限とは別の sysctl 経由の環境漏れを、本物の囮と自己の正の対照で守る。
#[test]
fn process_info_denies_parent_and_unrelated_environments_but_allows_self() -> TestResult {
    let f = Fixture::new(true)?;
    let probe = macos_process_info::ProcessInfoProbe::new(&f.root)?;
    let args = probe.args("denied");
    let args: Vec<&OsStr> = args.iter().map(OsString::as_os_str).collect();
    let command = f.command_for(
        PolicyConfig::new(vec![f.writable.clone()]),
        probe.python(),
        &args,
    )?;
    let output = probe.wrap(&command).output()?;
    assert!(output.status.success(), "{output:?}");
    assert_eq!(output.stdout, b"process-info: denied\n");
    Ok(())
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

// 作成時の関門: 書ける場所の外・秘密・守る場所からのリンクの拒否漏れを、
// 本物の子の終了状態と外から見たリンク数で捕まえる。既存の書き込み検査はリンクを作らない。
// （設計書 01-03「書く道具のパスの規則」、03-01「保証しないこと」、BH-092）
#[test]
fn hardlinks_stay_within_unprotected_writable_paths() -> TestResult {
    let f = Fixture::new(false)?;
    let secret = f.root.join("secret");
    let secret_key = secret.join("key");
    let secret_file = f.root.join("secret-file");
    let outside = f.root.join("outside");
    let protected = f.writable.join(".git/config");
    let source = f.writable.join("a/f");
    let destination = f.writable.join("b/g");
    fs::create_dir(&secret)?;
    for directory in [".git", "a", "b"] {
        fs::create_dir(f.writable.join(directory))?;
    }
    for path in [&secret_key, &secret_file, &outside, &protected, &source] {
        fs::write(path, b"hardlink fixture")?;
    }
    let mut config = f.config();
    config.denied_read.extend([secret, secret_file.clone()]);

    // 同じディレクトリ内だけでは、Linux の Landlock の REFER の許可漏れを見落とす。
    f.run(
        config.clone(),
        &[
            OsStr::new("hardlink"),
            source.as_os_str(),
            destination.as_os_str(),
        ],
        0,
    )?;
    let source_metadata = fs::metadata(&source)?;
    let destination_metadata = fs::metadata(&destination)?;
    assert_eq!(source_metadata.nlink(), 2);
    assert_eq!(destination_metadata.nlink(), 2);
    assert_eq!(source_metadata.dev(), destination_metadata.dev());
    assert_eq!(source_metadata.ino(), destination_metadata.ino());

    for (name, source) in [
        ("outside", outside),
        ("secret-key", secret_key),
        ("secret-file", secret_file),
        ("protected", protected),
    ] {
        let destination = f.writable.join(format!("link-{name}"));
        f.run(
            config.clone(),
            &[
                OsStr::new("hardlink"),
                source.as_os_str(),
                destination.as_os_str(),
            ],
            10,
        )?;
        let metadata = fs::symlink_metadata(&destination);
        assert!(
            matches!(&metadata, Err(error) if error.kind() == io::ErrorKind::NotFound),
            "{name}: destination must be absent: {metadata:?}"
        );
        assert_eq!(fs::metadata(&source)?.nlink(), 1, "{name}");
    }
    Ok(())
}

// 作成時の関門: /dev/null の書き込み権の欠落と、例外を全体へ広げる退行を
// 本物の子で区別する。プロジェクトに書ける場合も、一時の場所だけの場合も同じ契約。
#[test]
fn dev_null_accepts_writes_without_allowing_other_outside_files() -> TestResult {
    let f = Fixture::new(false)?;
    let temporary = f.root.join("temporary-only");
    fs::create_dir(&temporary)?;
    let outside = f.root.join("outside");
    fs::write(&outside, b"unchanged")?;
    for writable in [&f.writable, &temporary] {
        let config = PolicyConfig::new(vec![writable.clone()]);
        f.run(
            config.clone(),
            &[OsStr::new("write"), OsStr::new("/dev/null")],
            0,
        )?;
        f.run(config, &[OsStr::new("write"), outside.as_os_str()], 10)?;
        assert_eq!(fs::read(&outside)?, b"unchanged");
    }
    Ok(())
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

#[path = "support/process.rs"]
mod process_support;

// 作成時の関門: sandbox-exec に包んだ場合も、新しいグループの後始末と
// setsid で逃げてパイプを保持する子の検出が働くことを、本物の Seatbelt で試す。
#[test]
fn sandboxed_groups_are_cleaned_and_escaped_pipe_holders_are_reported() -> TestResult {
    use benitoiteh::sandbox::run::{Leftovers, RunEnd, RunLimits, run};
    use std::sync::atomic::AtomicBool;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;
    for session in ["group", "session"] {
        let f = process_support::ProcessFixture::new()?;
        let sandbox = SandboxCommand {
            policy: SandboxPolicy::new(PolicyConfig::new(vec![f.root.clone()]))?,
            program: fs::canonicalize(&f.probe)?,
            args: vec![
                "descendant".into(),
                session.into(),
                "open".into(),
                f.marker.as_os_str().to_owned(),
            ],
            cwd: f.root.clone(),
            env: BTreeMap::new(),
            stdin: SandboxStdin::Null,
        }
        .to_command(Path::new(env!("CARGO_BIN_EXE_benitoiteh")))?;
        let limits = RunLimits {
            timeout: Duration::from_secs(5),
            pipe_close_wait: Duration::from_millis(100),
            ..RunLimits::default()
        };
        let result = run(
            sandbox,
            vec![],
            Arc::new(Mutex::new(Vec::new())),
            Arc::new(Mutex::new(Vec::new())),
            limits,
            &AtomicBool::new(false),
        )?;
        assert!(result.status.success());
        assert_eq!(result.end, RunEnd::Exited);
        let pid = f.pid()?;
        if session == "group" {
            assert_eq!(result.leftovers, Leftovers::None);
            f.wait_until(|| f.stopped(pid))?;
        } else {
            assert_eq!(result.leftovers, Leftovers::PossiblyRemaining);
            assert!(!f.stopped(pid));
        }
    }
    Ok(())
}

// 作成時の関門: sandbox-exec へ送った合図が、目的のプログラムへ実際に届く契約。
#[test]
fn sandboxed_timeout_delivers_the_requested_signal() -> TestResult {
    use benitoiteh::sandbox::run::{Leftovers, RunEnd, RunLimits, StopSignal, run};
    use std::sync::atomic::AtomicBool;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;
    for (signal, expected) in [
        (StopSignal::Interrupt, "SIGINT"),
        (StopSignal::Terminate, "SIGTERM"),
    ] {
        let f = process_support::ProcessFixture::new()?;
        let sandbox = SandboxCommand {
            policy: SandboxPolicy::new(PolicyConfig::new(vec![f.root.clone()]))?,
            program: fs::canonicalize(&f.probe)?,
            args: vec![
                "signal".into(),
                "handle".into(),
                f.marker.as_os_str().to_owned(),
            ],
            cwd: f.root.clone(),
            env: BTreeMap::new(),
            stdin: SandboxStdin::Null,
        }
        .to_command(Path::new(env!("CARGO_BIN_EXE_benitoiteh")))?;
        let limits = RunLimits {
            timeout: Duration::from_secs(2),
            stop_signal: signal,
            ..RunLimits::default()
        };
        let result = run(
            sandbox,
            vec![],
            Arc::new(Mutex::new(Vec::new())),
            Arc::new(Mutex::new(Vec::new())),
            limits,
            &AtomicBool::new(false),
        )?;
        assert!(result.status.success());
        assert_eq!(result.end, RunEnd::TimedOut);
        assert_eq!(result.leftovers, Leftovers::None);
        assert_eq!(fs::read_to_string(f.marker.clone())?, expected);
    }
    Ok(())
}

#[path = "support/processor.rs"]
mod processor;

#[path = "support/doctor.rs"]
mod doctor_support;

// 作成時の関門: 本物のサンドボックスと CLI を通し、選んだ実行ファイルの
// 無限の標準出力を上限で止め、使えないと報告することを確かめる。
#[test]
fn doctor_stops_a_processor_that_exceeds_the_stdout_limit() -> TestResult {
    let fixture = doctor_support::Fixture::new()?;
    fixture.write_config("default_provider = 'local'\n[providers.local]\nkind = 'openai-chat'\nbase_url = 'http://127.0.0.1:1/v1'\nmodel = 'local'\ncontext_window = 4096\n[benitoite]\npath = '/usr/bin/yes'\n")?;
    let started = std::time::Instant::now();
    let output = fixture.command().arg("--json").output()?;
    assert!(started.elapsed() < std::time::Duration::from_secs(5));
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert!(output.stderr.is_empty());
    let rows = String::from_utf8(output.stdout)?
        .lines()
        .map(serde_json::from_str::<serde_json::Value>)
        .collect::<Result<Vec<_>, _>>()?;
    for (name, status) in [
        ("benitoite.discovery", "ok"),
        ("benitoite.executable", "ok"),
        ("sandbox.restricted_rule", "unavailable"),
        ("benitoite.version", "unchecked"),
        ("benitoite.skill", "unchecked"),
    ] {
        let row = rows
            .iter()
            .find(|row| row.get("name").and_then(serde_json::Value::as_str) == Some(name))
            .ok_or("Missing doctor item")?;
        assert_eq!(
            row.get("status"),
            Some(&serde_json::Value::from(status)),
            "{row}"
        );
        if name == "sandbox.restricted_rule" {
            assert_eq!(
                row.get("detail").and_then(serde_json::Value::as_str),
                Some(
                    "The sandbox rule check exceeded the 1 MiB stdout limit; the child was stopped."
                )
            );
        }
    }
    assert_eq!(
        rows.last().and_then(|row| row.get("restricted_sandbox")),
        Some(&serde_json::Value::from(false))
    );
    assert_eq!(fs::read_dir(&fixture.project)?.count(), 0);
    assert_eq!(fs::read_dir(fixture.root.join("state"))?.count(), 0);
    temporary_cleanup::check_temporary_root(&fixture.root.join("tmp"))?;
    Ok(())
}

// 作成時の関門: 正負の本物の起動を CLI につなぎ、機能の一覧だけから
// 制限モードを使えると表示する退行を捕まえる。Codex 内では走らせない。
#[test]
fn doctor_verifies_restricted_and_normal_sandboxes() -> TestResult {
    let fixture = doctor_support::Fixture::new()?;
    fixture.write_config(&format!("default_provider = 'local'\n[providers.local]\nkind = 'openai-chat'\nbase_url = 'http://127.0.0.1:1/v1'\nmodel = 'local'\ncontext_window = 4096\n[benitoite]\npath = '{}'\n", processor::benitoite()?.display()))?;
    let output = fixture.command().arg("--json").output()?;
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    assert!(output.stderr.is_empty(), "{output:?}");
    let rows = String::from_utf8(output.stdout)?
        .lines()
        .map(serde_json::from_str::<serde_json::Value>)
        .collect::<Result<Vec<_>, _>>()?;
    let (summary, items) = rows.split_last().ok_or("Missing doctor summary")?;
    assert_eq!(
        summary.get("restricted_sandbox"),
        Some(&serde_json::Value::from(true))
    );
    assert_eq!(
        summary.get("normal_sandbox"),
        Some(&serde_json::Value::from("enabled"))
    );
    assert_eq!(
        summary.get("exit_status"),
        Some(&serde_json::Value::from(0))
    );
    for name in [
        "benitoite.discovery",
        "benitoite.executable",
        "benitoite.version",
        "benitoite.skill",
    ] {
        assert!(
            items
                .iter()
                .any(|item| item.get("name").and_then(serde_json::Value::as_str) == Some(name)),
            "Missing {name}"
        );
    }
    assert!(
        items
            .iter()
            .any(|item| item.get("name").and_then(serde_json::Value::as_str)
                == Some("sandbox.restricted_rule")
                && item.get("detail").and_then(serde_json::Value::as_str)
                    == Some("benitoite --version=json succeeded and /bin/sh was denied."))
    );
    for item in items {
        let name = item
            .get("name")
            .and_then(serde_json::Value::as_str)
            .ok_or("Missing doctor item name")?;
        let expected = if name == "sandbox.linux_scope" {
            "warning"
        } else {
            "ok"
        };
        assert_eq!(
            item.get("status"),
            Some(&serde_json::Value::from(expected)),
            "{item}"
        );
    }
    assert_eq!(fs::read_dir(&fixture.project)?.count(), 0);
    assert_eq!(fs::read_dir(fixture.root.join("state"))?.count(), 0);
    temporary_cleanup::check_temporary_root(&fixture.root.join("tmp"))?;
    Ok(())
}

#[path = "support/tty.rs"]
#[allow(dead_code)]
mod tty;

// 作成時の関門: 方針で ioctl を塞ぐと仮定せず、本物の起動の切り離しを対照付きで守る。
#[test]
fn sandboxes_detach_the_controlling_terminal_for_both_modes() -> TestResult {
    let f = Fixture::new(false)?;
    let mut pty = tty::Pty::new()?;
    pty.control(&f.probe)?;
    for restricted in [false, true] {
        for operation in ["tty-open", "tty-write", "tty-ioctl"] {
            let mut config = f.config();
            if restricted {
                config.programs = ProgramAccess::Only(vec![f.probe.clone()]);
            }
            let command = f.command(config, &[OsStr::new(operation)])?;
            let mut child = pty
                .wrapper(&f.probe)
                .arg(command.get_program())
                .args(command.get_args())
                .current_dir(command.get_current_dir().ok_or("missing cwd")?)
                .envs(
                    command
                        .get_envs()
                        .filter_map(|(key, value)| value.map(|value| (key, value))),
                )
                .spawn()?;
            let (output, terminal) = pty.capture(&mut child)?;
            if operation == "tty-open" {
                // 15 は ENXIO、16 は別の open の誤り。Seatbelt の EPERM も区別して報告する。
                assert!(
                    matches!(output.status.code(), Some(15 | 16)),
                    "{restricted} {output:?}"
                );
                eprintln!("restricted={restricted} tty-open: {output:?}");
            } else {
                assert!(matches!(output.status.code(), Some(10 | 11)), "{output:?}");
            }
            assert!(
                terminal.is_empty(),
                "sandbox child wrote to the controlling terminal"
            );
        }
    }
    Ok(())
}
