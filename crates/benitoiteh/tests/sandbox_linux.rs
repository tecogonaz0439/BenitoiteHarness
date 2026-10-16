//! 本物の Linux の補助・Landlock・seccomp を子プロセスで検証する（設計書 03-02、07-01、実装プラン P0-05）。

#![cfg(target_os = "linux")]

use std::collections::BTreeMap;
use std::error::Error;
use std::ffi::{OsStr, OsString};
use std::fs::{self, File};
use std::net::TcpListener;
use std::os::fd::AsRawFd;
use std::os::unix::fs::symlink;
use std::os::unix::net::{UnixDatagram, UnixListener};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use benitoiteh::sandbox::{
    PolicyConfig, ProgramAccess, ReadAccess, SandboxCommand, SandboxPolicy, SandboxStdin,
};

type TestResult = Result<(), Box<dyn Error>>;
static SEQUENCE: AtomicU64 = AtomicU64::new(0);

struct Fixture {
    root: PathBuf,
    writable: PathBuf,
    probe: PathBuf,
    helper: PathBuf,
}

impl Fixture {
    fn new() -> Result<Self, Box<dyn Error>> {
        let parent = fs::canonicalize(std::env::temp_dir())?;
        let nonce = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
        let sequence = SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let root = parent.join(format!(
            "bh-linux-{}-{nonce:x}-{sequence:x}",
            std::process::id()
        ));
        fs::create_dir(&root)?;
        let f = Self {
            writable: root.join("write"),
            probe: root.join("probe"),
            helper: fs::canonicalize(env!("CARGO_BIN_EXE_benitoiteh"))?,
            root,
        };
        fs::create_dir(&f.writable)?;
        let executable = std::env::current_exe()?;
        let source = executable
            .parent()
            .and_then(Path::parent)
            .ok_or("Missing target directory")?
            .join("sandbox-probe");
        if !source.is_file() {
            return Err(format!(
                "Missing {}; run cargo build -p sandbox-probe for this target",
                source.display()
            )
            .into());
        }
        fs::copy(source, &f.probe)?;
        Ok(f)
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
        Ok(SandboxCommand {
            policy: SandboxPolicy::new(config)?,
            program: program.to_path_buf(),
            args: args.iter().map(|arg| arg.to_os_string()).collect(),
            cwd: self.root.clone(),
            env: BTreeMap::new(),
            stdin: SandboxStdin::Null,
        }
        .to_command(&self.helper)?)
    }

    fn run(&self, config: PolicyConfig, args: &[&OsStr], expected: i32) -> TestResult {
        self.run_case("probe", config, args, expected)
    }

    fn run_case(
        &self,
        phase: &str,
        config: PolicyConfig,
        args: &[&OsStr],
        expected: i32,
    ) -> TestResult {
        let context = format!(
            "phase={phase} program={} cwd={} args={args:?} policy={config:?}",
            self.probe.display(),
            self.root.display()
        );
        let output = self
            .command(config, args)?
            .output()
            .map_err(|error| format!("{context}: {error}"))?;
        check(&output, expected, &context);
        Ok(())
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        if let Err(error) = fs::remove_dir_all(&self.root) {
            eprintln!("Could not remove {}: {error}", self.root.display());
        }
    }
}

#[track_caller]
fn check(output: &Output, expected: i32, context: &str) {
    assert_eq!(
        output.status.code(),
        Some(expected),
        "{context}\nexpected={expected} status={}\nstdout:\n{}\nstderr:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

// 作成時の関門: 拒否を別の初期化の失敗で満たさず、同じ方針の正の対照を先に通す。
// 環境を調べる処理も別のプロセスに置き、テストの親には制限を掛けない。
#[test]
fn kernel_landlock_and_seccomp_are_available() -> TestResult {
    if std::env::var_os("BENITOITEH_TEST_LANDLOCK_INFO").as_deref() == Some(OsStr::new("1")) {
        use landlock::{
            ABI, Access, AccessFs, CompatLevel, Compatible, Ruleset, RulesetAttr, RulesetStatus,
        };
        let status = Ruleset::default()
            .set_compatibility(CompatLevel::HardRequirement)
            .handle_access(AccessFs::from_all(ABI::V3))?
            .create()?
            .restrict_self()?;
        println!(
            "kernel={} arch={} Landlock={:?}",
            rustix::system::uname().release().to_string_lossy(),
            std::env::consts::ARCH,
            status.landlock
        );
        assert_eq!(status.ruleset, RulesetStatus::FullyEnforced);
        return Ok(());
    }
    let f = Fixture::new()?;
    let info = Command::new(std::env::current_exe()?)
        .args([
            "--exact",
            "kernel_landlock_and_seccomp_are_available",
            "--nocapture",
        ])
        .env("BENITOITEH_TEST_LANDLOCK_INFO", "1")
        .current_dir(&f.root)
        .output()?;
    println!(
        "{}{}",
        String::from_utf8_lossy(&info.stdout),
        String::from_utf8_lossy(&info.stderr)
    );
    check(&info, 0, "environment/Landlock");
    let output = f
        .command(f.config(), &[OsStr::new("socketpair")])?
        .output()?;
    println!("{}", String::from_utf8_lossy(&output.stderr));
    check(&output, 0, "environment/seccomp");
    println!("seccomp=enabled (sandbox-probe socketpair succeeded after helper setup)");
    Ok(())
}

// 作成時の関門: 内容とディレクトリの一覧の両方を拒む契約。兄弟と symlink の別名も試す。
#[test]
fn reading_excludes_secrets_and_confines_readable_roots() -> TestResult {
    let f = Fixture::new()?;
    let secret = f.root.join("secret");
    let public = f.root.join("public");
    fs::create_dir(&secret)?;
    fs::write(secret.join("key"), b"secret")?;
    fs::write(&public, b"public")?;
    let alias = f.root.join("alias");
    symlink(secret.join("key"), &alias)?;
    let mut config = f.config();
    config.denied_read.push(secret.clone());
    for readable in [ReadAccess::All, ReadAccess::Roots(vec![f.root.clone()])] {
        config.readable = readable;
        f.run(config.clone(), &[OsStr::new("read"), public.as_os_str()], 0)?;
        f.run(
            config.clone(),
            &[OsStr::new("read-dir"), f.writable.as_os_str()],
            0,
        )?;
        for path in [&secret.join("key"), &alias] {
            f.run(config.clone(), &[OsStr::new("read"), path.as_os_str()], 10)?;
        }
        f.run(
            config.clone(),
            &[OsStr::new("read-dir"), secret.as_os_str()],
            10,
        )?;
        // 部分木への許可は祖先に付けられないので、祖先の一覧も拒否される。
        f.run(
            config.clone(),
            &[OsStr::new("read-dir"), f.root.as_os_str()],
            10,
        )?;
    }
    f.run(
        config,
        &[OsStr::new("read"), OsStr::new("/etc/alpine-release")],
        10,
    )?;
    Ok(())
}

// 作成時の関門: BH-049 の狭い書き込みの契約を OS の結果で固定する。
// MakeReg/MakeDir や rename を許す退行は、未作成の保護名を作る否定の行で失敗する。
#[test]
fn writing_updates_existing_files_but_cannot_create_or_replace_protected_names() -> TestResult {
    let f = Fixture::new()?;
    let existing = f.writable.join("existing");
    fs::write(&existing, b"old")?;
    f.run(f.config(), &[OsStr::new("write"), existing.as_os_str()], 0)?;
    assert_eq!(fs::read(&existing)?, b"sandbox-probe\n");
    let outside = f.root.join("outside");
    fs::write(&outside, b"unchanged")?;
    f.run(f.config(), &[OsStr::new("write"), outside.as_os_str()], 10)?;
    assert_eq!(fs::read(&outside)?, b"unchanged");
    let nested = f.writable.join("nested");
    fs::create_dir(&nested)?;
    for parent in [&f.writable, &nested] {
        for ordinary in ["new-file", "new-directory"] {
            let path = parent.join(ordinary);
            f.run(f.config(), &[OsStr::new("write"), path.as_os_str()], 10)?;
            f.run(f.config(), &[OsStr::new("mkdir"), path.as_os_str()], 10)?;
            assert!(!path.exists());
        }
        for name in [".git", ".benitoiteh", ".GIT"] {
            let target = parent.join(name);
            f.run(f.config(), &[OsStr::new("write"), target.as_os_str()], 10)?;
            f.run(f.config(), &[OsStr::new("mkdir"), target.as_os_str()], 10)?;
            let replacement = parent.join(format!("replacement-{name}"));
            fs::create_dir(&replacement)?;
            f.run(
                f.config(),
                &[
                    OsStr::new("rename"),
                    replacement.as_os_str(),
                    target.as_os_str(),
                ],
                10,
            )?;
            assert!(replacement.is_dir());
            assert!(!target.exists());
            fs::create_dir(&target)?;
            let file = target.join("config");
            fs::write(&file, b"protected")?;
            f.run(f.config(), &[OsStr::new("write"), file.as_os_str()], 10)?;
            f.run(
                f.config(),
                &[
                    OsStr::new("rename"),
                    target.as_os_str(),
                    parent.join(format!("moved-{name}")).as_os_str(),
                ],
                10,
            )?;
            assert_eq!(fs::read(file)?, b"protected");
            // worktree の .git ファイルの形でも、既存のファイルに与える権利から除く。
            fs::remove_dir_all(&target)?;
            fs::write(&target, b"gitdir: /outside")?;
            f.run(f.config(), &[OsStr::new("write"), target.as_os_str()], 10)?;
            assert_eq!(fs::read(target)?, b"gitdir: /outside");
        }
    }
    Ok(())
}

// 作成時の関門: 実行の許可一覧と、子孫への継承・書ける場所での実行拒否を同じ境界で試す。
#[test]
fn executable_allowlist_is_inherited_and_writable_programs_are_denied() -> TestResult {
    let f = Fixture::new()?;
    let writable = f.writable.join("probe");
    fs::copy(&f.probe, &writable)?;
    let unlisted = f.root.join("unlisted-probe");
    fs::copy(&f.probe, &unlisted)?;
    let mut config = f.config();
    config.programs = ProgramAccess::Only(vec![f.probe.clone(), writable.clone()]);
    // 読める静的な実行ファイルにも、読み取りの規則から実行権が漏れないことを試す。
    // 動的ローダの拒否だけで通る否定のテストにしない（実装プラン P0-05「テスト」）。
    for readable in [ReadAccess::All, ReadAccess::Roots(vec![f.root.clone()])] {
        config.readable = readable;
        f.run_case(
            "execution/unlisted-readable",
            config.clone(),
            &[OsStr::new("read"), unlisted.as_os_str()],
            0,
        )?;
        f.run_case(
            "execution/unlisted-static",
            config.clone(),
            &[
                OsStr::new("exec"),
                unlisted.as_os_str(),
                OsStr::new("socketpair"),
            ],
            10,
        )?;
    }
    config.readable = ReadAccess::All;
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
        config.clone(),
        &[
            OsStr::new("exec"),
            writable.as_os_str(),
            OsStr::new("socketpair"),
        ],
        10,
    )?;
    config.programs = ProgramAccess::Unrestricted;
    f.run(
        config.clone(),
        &[
            OsStr::new("exec"),
            writable.as_os_str(),
            OsStr::new("socketpair"),
        ],
        10,
    )?;
    config.deny_exec_in_writable = false;
    f.run(
        config,
        &[
            OsStr::new("exec"),
            writable.as_os_str(),
            OsStr::new("socketpair"),
        ],
        0,
    )
}

// 作成時の関門: BH-043 のローダの拒否と、許すと一覧を回り込む懸念を正負で比べる。
#[test]
fn dynamic_loader_must_be_denied_to_confine_execution() -> TestResult {
    let f = Fixture::new()?;
    let shell = fs::canonicalize("/bin/sh")?;
    let loader = fs::read_dir("/lib")?
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .find(|entry| {
            entry
                .file_name()
                .to_str()
                .is_some_and(|name| name.starts_with("ld-musl-") && name.ends_with(".so.1"))
        })
        .ok_or("The Alpine musl loader was not found")?;
    let loader = fs::canonicalize(loader.path())?;
    // 正規化した実のパスは許可一覧に使い、起動には /bin/sh の別名を保つ。
    // busybox を argv[0]=busybox で起動すると、次の -c を applet 名と解釈するためである。
    // サンドボックスなしの対照で、ローダへの引数の誤りを拒否と取り違えない。
    let output = Command::new(&loader)
        .args(["/bin/sh", "-c", "exit 0"])
        .env_clear()
        .current_dir(&f.root)
        .output()?;
    check(
        &output,
        0,
        &format!(
            "dynamic/unsandboxed-loader loader={} shell={}",
            loader.display(),
            shell.display()
        ),
    );
    let mut config = f.config();
    config.programs = ProgramAccess::Only(vec![f.probe.clone()]);
    f.run_case(
        "dynamic/loader-denied",
        config.clone(),
        &[
            OsStr::new("exec"),
            loader.as_os_str(),
            OsStr::new("/bin/sh"),
            OsStr::new("-c"),
            OsStr::new("exit 0"),
        ],
        10,
    )?;
    // shell 自身の実行を許しても、ELF の interpreter の実行権がなければ拒まれる。
    config.programs = ProgramAccess::Only(vec![f.probe.clone(), shell.clone()]);
    f.run_case(
        "dynamic/interpreter-denied",
        config.clone(),
        &[
            OsStr::new("exec"),
            OsStr::new("/bin/sh"),
            OsStr::new("-c"),
            OsStr::new("exit 0"),
        ],
        10,
    )?;
    // ローダにだけ実行権を与えると、shell は実行権なしでも読み取って写せてしまう。
    config.programs = ProgramAccess::Only(vec![f.probe.clone(), loader.clone()]);
    f.run_case(
        "dynamic/program-denied",
        config.clone(),
        &[
            OsStr::new("exec"),
            OsStr::new("/bin/sh"),
            OsStr::new("-c"),
            OsStr::new("exit 0"),
        ],
        10,
    )?;
    f.run_case(
        "dynamic/loader-reads-unlisted-program",
        config,
        &[
            OsStr::new("exec"),
            loader.as_os_str(),
            OsStr::new("/bin/sh"),
            OsStr::new("-c"),
            OsStr::new("exit 0"),
        ],
        0,
    )
}

// 作成時の関門: IP を許したときに Unix の socket/sendto まで通る退行を捕まえる。
#[test]
fn external_unix_ipc_is_denied_but_stream_socketpairs_work() -> TestResult {
    let f = Fixture::new()?;
    let stream = f.root.join("s");
    let datagram = f.root.join("d");
    let _listener = UnixListener::bind(&stream)?;
    let _datagram = UnixDatagram::bind(&datagram)?;
    for allow_ip in [false, true] {
        let mut config = f.config();
        config.allow_ip = allow_ip;
        f.run(
            config.clone(),
            &[OsStr::new("unix-connect"), stream.as_os_str()],
            10,
        )?;
        f.run(
            config.clone(),
            &[OsStr::new("unix-dgram-send"), datagram.as_os_str()],
            10,
        )?;
        f.run(config, &[OsStr::new("socketpair")], 0)?;
    }
    Ok(())
}

// 作成時の関門: 同じループバックの数値の宛先で IP の可否だけを変えて試す。
#[test]
fn ip_connections_follow_policy() -> TestResult {
    let f = Fixture::new()?;
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let address = listener.local_addr()?.to_string();
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

// 作成時の関門: fd 検査・JSON・必須 ABI の失敗で exec に進まない契約を、
// 実行されたら既存の目印を書き換える本物のプログラムで試す。規則を外す注入口は設けない。
#[test]
fn setup_failures_do_not_execute_the_program_and_cloexec_descriptors_do_not_leak() -> TestResult {
    let f = Fixture::new()?;
    let marker = f.writable.join("marker");
    fs::write(&marker, b"unchanged")?;
    let descriptor = File::open(&marker)?;
    let number = descriptor.as_raw_fd().to_string();
    f.run(
        f.config(),
        &[OsStr::new("fd-open"), OsStr::new(&number)],
        13,
    )?;
    let sandbox = f.command(f.config(), &[OsStr::new("write"), marker.as_os_str()])?;
    let args: Vec<OsString> = sandbox.get_args().map(OsStr::to_os_string).collect();
    for inherited in [marker.as_path(), Path::new("/proc/self/fd")] {
        let output = Command::new(&f.probe)
            .arg("spawn-with-fd")
            .arg(inherited)
            .arg("--")
            .arg(sandbox.get_program())
            .args(&args)
            .env_clear()
            .current_dir(&f.root)
            .process_group(0)
            .output()?;
        check(
            &output,
            125,
            &format!("setup/descriptors inherited={}", inherited.display()),
        );
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains("descriptors"), "{stderr}");
        assert_eq!(fs::read(&marker)?, b"unchanged");
    }
    drop(descriptor);
    let mut raised_abi = serde_json::to_value(SandboxPolicy::new(f.config())?)?;
    raised_abi
        .as_object_mut()
        .ok_or("Missing policy object")?
        .insert("required_landlock_abi".into(), serde_json::json!(u64::MAX));
    for (json, stage) in [
        ("{\nbroken".to_owned(), "policy"),
        (serde_json::to_string(&raised_abi)?, "Landlock"),
    ] {
        let output = Command::new(&f.helper)
            .arg("__sandbox")
            .arg(&json)
            .arg("--")
            .arg(&f.probe)
            .arg("write")
            .arg(&marker)
            .env_clear()
            .current_dir(&f.root)
            .output()?;
        check(&output, 125, &format!("setup/{stage} policy={json}"));
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert_eq!(stderr.lines().count(), 1, "{stderr}");
        assert!(stderr.contains(stage), "{stderr}");
        assert_eq!(fs::read(&marker)?, b"unchanged");
    }
    let mut config = f.config();
    config.programs = ProgramAccess::Only(Vec::new());
    let output = f
        .command(config, &[OsStr::new("write"), marker.as_os_str()])?
        .output()?;
    check(&output, 125, "setup/exec: empty executable allowlist");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(stderr.lines().count(), 1, "{stderr}");
    assert!(stderr.contains("exec"), "{stderr}");
    assert_eq!(fs::read(&marker)?, b"unchanged");
    Ok(())
}
