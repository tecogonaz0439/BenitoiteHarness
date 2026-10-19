//! 本物の Linux の補助・Landlock・seccomp を子プロセスで検証する（設計書 03-02、07-01、実装プラン P0-07）。

#![cfg(target_os = "linux")]

#[path = "support/temporary_cleanup.rs"]
#[cfg(target_arch = "x86_64")]
mod temporary_cleanup;

#[path = "support/bash_sandbox.rs"]
mod bash_sandbox;
#[path = "support/socket_directory.rs"]
mod socket_directory;

use std::collections::BTreeMap;
use std::error::Error;
use std::ffi::{OsStr, OsString};
use std::fs::{self, File};
use std::io::{self, BufRead, BufReader, Write};
use std::net::TcpListener;
use std::os::fd::AsRawFd;
use std::os::unix::fs::{MetadataExt, PermissionsExt, symlink};
use std::os::unix::net::{UnixDatagram, UnixListener};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use benitoiteh::sandbox::{
    CapabilityStatus, PolicyConfig, PreparedSandbox, ProgramAccess, ReadAccess, SandboxChild,
    SandboxCommand, SandboxError, SandboxPolicy, SandboxStdin, probe_linux,
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

    fn command(
        &self,
        config: PolicyConfig,
        args: &[&OsStr],
    ) -> Result<PreparedSandbox, Box<dyn Error>> {
        self.command_with_stdin(config, &self.probe, args, SandboxStdin::Null)
    }

    fn command_with_stdin(
        &self,
        config: PolicyConfig,
        program: &Path,
        args: &[&OsStr],
        stdin: SandboxStdin,
    ) -> Result<PreparedSandbox, Box<dyn Error>> {
        Ok(SandboxCommand {
            policy: SandboxPolicy::new(config)?,
            program: program.to_path_buf(),
            args: args.iter().map(|arg| arg.to_os_string()).collect(),
            cwd: self.root.clone(),
            env: BTreeMap::new(),
            stdin,
        }
        .prepare(&self.helper, &self.root, &self.root.join("placeholders"))?)
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
    let availability = probe_linux(&f.helper, &f.root)?;
    assert_eq!(availability.bubblewrap, CapabilityStatus::Available);
    assert_eq!(availability.user_namespaces, CapabilityStatus::Available);
    assert_eq!(availability.landlock_v3, CapabilityStatus::Available);
    assert_eq!(availability.seccomp, CapabilityStatus::Available);
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

// 作成時の関門: 引数の並びだけでは bubblewrap が特権を本当に外したか確かめられない。
// 同じテストを podman の root でも走らせ、--cap-drop ALL の欠落を検出する。
#[test]
fn sandbox_program_has_no_capabilities() -> TestResult {
    let f = Fixture::new()?;
    let output = f.command(f.config(), &[OsStr::new("caps")])?.output()?;
    check(&output, 0, "capability sets after helper setup");
    let stdout = std::str::from_utf8(&output.stdout)?;
    println!("{stdout}");
    let fields: BTreeMap<_, _> = stdout
        .lines()
        .filter_map(|line| line.split_once(':'))
        .map(|(key, value)| (key, value.trim()))
        .collect();
    for field in ["CapInh", "CapPrm", "CapEff", "CapAmb"] {
        assert_eq!(fields.get(field), Some(&"0000000000000000"), "{stdout}");
    }
    assert!(fields.contains_key("CapBnd"), "{stdout}");
    Ok(())
}

// 作成時の関門: 内容とディレクトリの一覧の両方を拒む契約。兄弟と symlink の別名も試す。
#[test]
fn reading_excludes_secrets_and_confines_readable_roots() -> TestResult {
    let f = Fixture::new()?;
    let secret = f.root.join("secret");
    let public = f.root.join("public");
    let secret_file = f.writable.join("secret-file");
    fs::create_dir(&secret)?;
    fs::write(secret.join("key"), b"secret")?;
    fs::write(&public, b"public")?;
    fs::write(&secret_file, b"secret-file")?;
    let alias = f.root.join("alias");
    symlink(secret.join("key"), &alias)?;
    let mut config = f.config();
    config.denied_read.push(secret.clone());
    config.denied_read.push(secret.join("key"));
    config.denied_read.push(secret_file.clone());
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
            &[OsStr::new("read"), secret_file.as_os_str()],
            10,
        )?;
        f.run(
            config.clone(),
            &[OsStr::new("read-dir"), secret.as_os_str()],
            10,
        )?;
        // 秘密は bubblewrap で覆うため、祖先の一覧は読める（設計書 03-02）。
        f.run(
            config.clone(),
            &[OsStr::new("read-dir"), f.root.as_os_str()],
            0,
        )?;
    }
    f.run(
        config,
        &[OsStr::new("read"), OsStr::new("/etc/alpine-release")],
        10,
    )?;
    assert_eq!(fs::read(secret.join("key"))?, b"secret");
    assert_eq!(fs::read(secret_file)?, b"secret-file");
    Ok(())
}

// 作成時の関門: 書ける場所の外・秘密・守る場所からのリンクの拒否漏れを、
// 本物の子の終了状態と外から見たリンク数で捕まえる。既存の書き込み検査はリンクを作らない。
// （設計書 01-03「書く道具のパスの規則」、03-01「保証しないこと」、BH-092）
#[test]
fn hardlinks_stay_within_unprotected_writable_paths() -> TestResult {
    let f = Fixture::new()?;
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

    // 同じディレクトリ内のリンクは REFER を要さないため、別のディレクトリを先にする。
    f.run_case(
        "hardlink positive control",
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

    // outside は --ro-bind / /、先は --bind の別マウントなので EXDEV を見込む。
    // secret/key は権限 000 の tmpfs を辿れず、secret-file は覆いのファイルの
    // マウント、.git は --ro-bind の別マウントになる（設計書 03-02「Linux のファイルシステムの見え方」）。
    // Landlock の REFER の拒否も EXDEV を返しうるので、終了状態から拒否の仕組みは断定しない。
    for (name, source, expected) in [
        ("outside", outside, 14),
        ("secret-key", secret_key, 10),
        ("secret-file", secret_file, 14),
        ("protected", protected, 14),
    ] {
        let destination = f.writable.join(format!("link-{name}"));
        f.run_case(
            name,
            config.clone(),
            &[
                OsStr::new("hardlink"),
                source.as_os_str(),
                destination.as_os_str(),
            ],
            expected,
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

// 作成時の関門: 書き込み権の不足と、守るマウントの欠落を、外の FS の状態でも検出する。
// 同じディレクトリ内の rename だけでは捕まらない、追加の Landlock 層の REFER の許可漏れも試す。
#[test]
fn writing_creates_removes_and_atomically_replaces_files_only_in_writable_roots() -> TestResult {
    let f = Fixture::new()?;
    let nested = f.writable.join("nested");
    fs::create_dir(&nested)?;
    for parent in [&f.writable, &nested] {
        let file = parent.join("new");
        let temp = parent.join("temporary");
        let directory = parent.join("directory");
        for path in [&file, &temp] {
            f.run(f.config(), &[OsStr::new("write"), path.as_os_str()], 0)?;
            assert_eq!(fs::read(path)?, b"sandbox-probe\n");
        }
        fs::write(&file, b"old")?;
        f.run(
            f.config(),
            &[OsStr::new("rename"), temp.as_os_str(), file.as_os_str()],
            0,
        )?;
        assert_eq!(fs::read(&file)?, b"sandbox-probe\n");
        assert!(!temp.exists());
        f.run(
            f.config(),
            &[OsStr::new("remove-file"), file.as_os_str()],
            0,
        )?;
        assert!(!file.exists());
        f.run(f.config(), &[OsStr::new("mkdir"), directory.as_os_str()], 0)?;
        assert!(directory.is_dir());
        f.run(
            f.config(),
            &[OsStr::new("remove-dir"), directory.as_os_str()],
            0,
        )?;
        assert!(!directory.exists());
    }
    let source = f.writable.join("a/f");
    let destination = f.writable.join("b/f");
    for directory in ["a", "b"] {
        fs::create_dir(f.writable.join(directory))?;
    }
    fs::write(&source, b"moved between directories")?;
    f.run_case(
        "rename across writable directories",
        f.config(),
        &[
            OsStr::new("rename"),
            source.as_os_str(),
            destination.as_os_str(),
        ],
        0,
    )?;
    let metadata = fs::symlink_metadata(&source);
    assert!(
        matches!(&metadata, Err(error) if error.kind() == io::ErrorKind::NotFound),
        "rename source must be absent: {metadata:?}"
    );
    assert_eq!(fs::read(&destination)?, b"moved between directories");
    let outside = f.root.join("outside");
    fs::write(&outside, b"unchanged")?;
    f.run(f.config(), &[OsStr::new("write"), outside.as_os_str()], 11)?;
    assert_eq!(fs::read(&outside)?, b"unchanged");
    Ok(())
}

// 作成時の関門: /dev/null の書き込み権の欠落と、例外を全体へ広げる退行を
// 本物の子で区別する。プロジェクトに書ける場合も、一時の場所だけの場合も同じ契約。
#[test]
fn dev_null_accepts_writes_without_allowing_other_outside_files() -> TestResult {
    let f = Fixture::new()?;
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
        f.run(config, &[OsStr::new("write"), outside.as_os_str()], 11)?;
        assert_eq!(fs::read(&outside)?, b"unchanged");
    }
    Ok(())
}

// 作成時の関門: すでにあるマウントと、まだない名前の塞ぎの両方を本物の OS で確かめる。
#[test]
fn root_protection_blocks_creation_writing_removal_and_replacement() -> TestResult {
    let f = Fixture::new()?;
    for name in [".git", ".benitoiteh"] {
        let target = f.writable.join(name);
        for operation in ["write", "mkdir", "remove-dir"] {
            f.run(f.config(), &[OsStr::new(operation), target.as_os_str()], 11)?;
            assert!(!target.exists(), "placeholder was not cleaned up");
        }
        let replacement = f.writable.join("replacement");
        fs::create_dir(&replacement)?;
        f.run(
            f.config(),
            &[
                OsStr::new("rename"),
                replacement.as_os_str(),
                target.as_os_str(),
            ],
            11,
        )?;
        assert!(replacement.is_dir());
        assert!(!target.exists());
        fs::remove_dir(&replacement)?;
        fs::create_dir(&target)?;
        let contents = target.join("config");
        fs::write(&contents, b"protected")?;
        for operation in ["write", "remove-file"] {
            f.run(
                f.config(),
                &[OsStr::new(operation), contents.as_os_str()],
                11,
            )?;
            assert_eq!(fs::read(&contents)?, b"protected");
        }
        let moved = f.writable.join("moved");
        f.run(
            f.config(),
            &[OsStr::new("rename"), target.as_os_str(), moved.as_os_str()],
            11,
        )?;
        assert!(!moved.exists());
        assert_eq!(fs::read(&contents)?, b"protected");
    }
    Ok(())
}

// 作成時の関門: 外部 gitdir の重ね忘れと、大小文字・深さの境界を区別する。
#[test]
fn gitdir_targets_and_existing_nested_metadata_are_read_only() -> TestResult {
    let f = Fixture::new()?;
    let gitdir = f.writable.join("metadata");
    fs::create_dir(&gitdir)?;
    let gitfile = f.writable.join(".git");
    fs::write(&gitfile, b"gitdir: metadata\n")?;
    let data = gitdir.join("config");
    fs::write(&data, b"protected")?;
    for path in [&gitfile, &data] {
        f.run(f.config(), &[OsStr::new("write"), path.as_os_str()], 11)?;
    }
    let moved = f.writable.join("moved");
    for path in [&gitfile, &gitdir] {
        f.run(
            f.config(),
            &[OsStr::new("rename"), path.as_os_str(), moved.as_os_str()],
            11,
        )?;
        assert!(!moved.exists());
    }
    assert_eq!(fs::read(&gitfile)?, b"gitdir: metadata\n");
    assert_eq!(fs::read(&data)?, b"protected");
    for name in [".GIT", "a/.git", "a/b/.git", "a/b/.GIT", "a/.benitoiteh"] {
        let target = f.writable.join(name);
        fs::create_dir_all(&target)?;
        let data = target.join("config");
        fs::write(&data, b"protected")?;
        f.run(f.config(), &[OsStr::new("write"), data.as_os_str()], 11)?;
        f.run(
            f.config(),
            &[OsStr::new("rename"), target.as_os_str(), moved.as_os_str()],
            11,
        )?;
        assert_eq!(fs::read(&data)?, b"protected");
        assert!(!moved.exists());
    }
    Ok(())
}

// 作成時の関門: 保証の範囲を広く見せない。未作成の入れ子と深さ 4 以上は範囲外（設計書 03-02）。
#[test]
fn uncreated_nested_and_deep_metadata_and_uncreated_uppercase_root_are_outside_guarantee()
-> TestResult {
    let f = Fixture::new()?;
    for name in ["a/.git", "a/b/c/.git", ".GIT"] {
        let target = f.writable.join(name);
        fs::create_dir_all(target.parent().ok_or("Missing parent")?)?;
        // 根の直下でも、まだない .GIT は塞がない（設計書 03-02「Linux のファイルシステムの見え方」）。
        f.run(f.config(), &[OsStr::new("mkdir"), target.as_os_str()], 0)?;
        assert!(target.is_dir());
    }
    let deep = f.writable.join("a/b/c/.git/config");
    f.run(f.config(), &[OsStr::new("write"), deep.as_os_str()], 0)?;
    assert_eq!(fs::read(&deep)?, b"sandbox-probe\n");
    Ok(())
}

// 作成時の関門: 読み取り専用の覆いを中から外す・上書きする操作を、実際の mount で試す。
#[test]
fn protected_and_denied_mounts_cannot_be_removed_or_overmounted() -> TestResult {
    let f = Fixture::new()?;
    let secret = f.root.join("secret");
    let file = f.root.join("secret-file");
    let source = f.writable.join("source");
    fs::create_dir(&secret)?;
    fs::create_dir(&source)?;
    fs::write(secret.join("key"), b"secret")?;
    fs::write(&file, b"secret-file")?;
    let mut config = f.config();
    config.denied_read = vec![secret.clone(), file.clone()];
    for target in [
        f.writable.join(".git"),
        f.writable.join(".benitoiteh"),
        secret.clone(),
        file.clone(),
    ] {
        f.run(
            config.clone(),
            &[OsStr::new("umount"), target.as_os_str()],
            10,
        )?;
        let from = if target == file { &file } else { &source };
        f.run(
            config.clone(),
            &[
                OsStr::new("bind-mount"),
                from.as_os_str(),
                target.as_os_str(),
            ],
            10,
        )?;
        assert_eq!(fs::read(secret.join("key"))?, b"secret");
        assert_eq!(fs::read(&file)?, b"secret-file");
        assert!(!f.writable.join(".git").exists());
        assert!(!f.writable.join(".benitoiteh").exists());
    }
    Ok(())
}

// 作成時の関門: 待機前の rmdir と、起動前からあった空のディレクトリの削除を捕まえる。
#[test]
fn cleanup_waits_for_exit_and_preserves_preexisting_empty_directories() -> TestResult {
    let f = Fixture::new()?;
    let existing = f.writable.join(".git");
    let created = f.writable.join(".benitoiteh");
    let occupied = f.writable.join(".keep");
    fs::create_dir(&existing)?;
    let mut config = f.config();
    config.protected_names.push(".keep".into());
    let sandbox = f.command_with_stdin(
        config,
        &f.probe,
        &[OsStr::new("wait-stdin")],
        SandboxStdin::Piped,
    )?;
    let mut command = Command::new(sandbox.get_program());
    command
        .args(sandbox.get_args())
        .env_clear()
        .current_dir(&f.root)
        .process_group(0)
        .stdin(Stdio::inherit())
        .stdout(Stdio::piped());
    let mut child = sandbox.spawn_wrapped(command)?;
    let stdout = child.child_mut().stdout.take().ok_or("Missing stdout")?;
    let mut ready = String::new();
    BufReader::new(stdout).read_line(&mut ready)?;
    // 先に EOF を渡しておき、検査の失敗でも子を待てるようにする。
    let present_while_running = created.is_dir();
    let occupied_result = (|| -> std::io::Result<()> {
        // 外の利用者が置いた内容を、終了後の後始末が消してはいけない。
        fs::set_permissions(&occupied, fs::Permissions::from_mode(0o755))?;
        fs::write(occupied.join("outside-data"), b"keep")
    })();
    drop(child.take_stdin());
    let status = child.wait()?;
    occupied_result?;
    assert!(status.success(), "{status}");
    assert_eq!(ready.trim(), "ready");
    assert!(present_while_running);
    assert!(existing.is_dir());
    assert_eq!(fs::read_dir(&existing)?.count(), 0);
    assert!(!created.exists());
    assert_eq!(fs::read(occupied.join("outside-data"))?, b"keep");

    let hidden_cwd = f.root.join("hidden-cwd");
    fs::create_dir(&hidden_cwd)?;
    let mut config = f.config();
    config.denied_read.push(hidden_cwd.clone());
    let output = SandboxCommand {
        policy: SandboxPolicy::new(config)?,
        program: f.probe.clone(),
        args: vec!["socketpair".into()],
        cwd: hidden_cwd,
        env: BTreeMap::new(),
        stdin: SandboxStdin::Null,
    }
    .prepare(&f.helper, &f.root, &f.root.join("placeholders"))?
    .output()?;
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).starts_with("bwrap:"));
    assert!(existing.is_dir());
    assert!(!created.exists());
    Ok(())
}

// 作成時の関門: 起動の失敗で覆いが残る退行を、本物の spawn の ENOENT で捕まえる。
#[test]
fn failed_spawn_removes_the_exclusive_file_cover() -> TestResult {
    let f = Fixture::new()?;
    let secret = f.root.join("secret-file");
    fs::write(&secret, b"secret")?;
    let mut config = f.config();
    config.denied_read.push(secret.clone());
    drop(f.command(f.config(), &[OsStr::new("socketpair")])?);
    let before = fs::read_dir(&f.root)?.count();
    let sandbox = f.command(config, &[OsStr::new("socketpair")])?;
    assert_eq!(fs::read_dir(&f.root)?.count(), before.saturating_add(1));
    let result = sandbox.spawn_wrapped(Command::new(f.root.join("missing-wrapper")));
    assert!(
        matches!(result, Err(SandboxError::Io(error)) if error.kind() == std::io::ErrorKind::NotFound)
    );
    assert_eq!(fs::read_dir(&f.root)?.count(), before);
    assert_eq!(fs::read(secret)?, b"secret");
    Ok(())
}

// 作成時の関門: 変更可能なリンクと補助の上書きを、子を起動する前に拒む契約。
#[test]
fn writable_symlinks_and_writable_helper_fail_before_spawn() -> TestResult {
    let f = Fixture::new()?;
    for name in [".git", "a/.git"] {
        let link = f.writable.join(name);
        fs::create_dir_all(link.parent().ok_or("Missing parent")?)?;
        symlink(&f.root, &link)?;
        assert!(f.command(f.config(), &[OsStr::new("socketpair")]).is_err());
        fs::remove_file(link)?;
    }
    let link = f.writable.join("link");
    symlink(&f.root, &link)?;
    fs::write(f.writable.join(".git"), b"gitdir: link\n")?;
    assert!(f.command(f.config(), &[OsStr::new("socketpair")]).is_err());
    fs::remove_file(f.writable.join(".git"))?;
    let mut config = f.config();
    config.denied_read.push(link);
    assert!(matches!(
        SandboxPolicy::new(config),
        Err(SandboxError::InvalidPolicy(_))
    ));
    let writable_helper = f.writable.join("helper");
    fs::copy(&f.helper, &writable_helper)?;
    let command = SandboxCommand {
        policy: SandboxPolicy::new(f.config())?,
        program: f.probe.clone(),
        args: vec!["socketpair".into()],
        cwd: f.root.clone(),
        env: BTreeMap::new(),
        stdin: SandboxStdin::Null,
    };
    assert!(matches!(
        command.prepare(&writable_helper, &f.root, &f.root.join("placeholders")),
        Err(SandboxError::InvalidPolicy(_))
    ));
    let mut config = f.config();
    config.denied_read.push(f.root.clone());
    assert!(matches!(
        SandboxPolicy::new(config),
        Err(SandboxError::InvalidPolicy(_))
    ));
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
    let sockets = socket_directory::SocketDirectory::new()?;
    let stream = sockets.path().join("s");
    let datagram = sockets.path().join("d");
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
    for inherited in [marker.as_path(), Path::new("/proc/self/fd")] {
        let sandbox = f.command(f.config(), &[OsStr::new("write"), marker.as_os_str()])?;
        let args: Vec<OsString> = sandbox.get_args().map(OsStr::to_os_string).collect();
        let mut wrapper = Command::new(&f.probe);
        wrapper
            .arg("spawn-with-fd")
            .arg(inherited)
            .arg("--")
            .arg(sandbox.get_program())
            .args(&args)
            .env_clear()
            .current_dir(&f.root)
            .process_group(0);
        let output = sandbox.output_wrapped(wrapper)?;
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
        let mut command = Command::new(&f.helper);
        command
            .arg("__sandbox")
            .arg(&json)
            .arg("--")
            .arg(&f.probe)
            .arg("write")
            .arg(&marker)
            .env_clear()
            .current_dir(&f.root);
        let _control = attach_control(&mut command, 1)?;
        let output = command.output()?;
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

fn attach_control(
    command: &mut Command,
    count: usize,
) -> Result<std::os::unix::net::UnixStream, Box<dyn Error>> {
    use rustix::net::{SendAncillaryBuffer, SendAncillaryMessage, SendFlags, sendmsg};
    use std::io::IoSlice;
    use std::mem::MaybeUninit;
    use std::os::fd::{AsFd, OwnedFd};
    let (parent, inherited) = std::os::unix::net::UnixStream::pair()?;
    command.stdin(Stdio::from(OwnedFd::from(inherited)));
    let file = File::open("/dev/null")?;
    let descriptors = vec![file.as_fd(); count];
    let mut space = [MaybeUninit::uninit(); rustix::cmsg_space!(ScmRights(2))];
    let mut ancillary = SendAncillaryBuffer::new(&mut space);
    if count > 0 {
        assert!(ancillary.push(SendAncillaryMessage::ScmRights(&descriptors)));
    }
    sendmsg(
        &parent,
        &[IoSlice::new(b"S")],
        &mut ancillary,
        SendFlags::NOSIGNAL,
    )?;
    Ok(parent)
}

fn run_limits() -> benitoiteh::sandbox::run::RunLimits {
    use benitoiteh::sandbox::run::RunLimits;
    RunLimits {
        timeout: std::time::Duration::from_secs(5),
        grace: std::time::Duration::from_millis(100),
        pipe_close_wait: std::time::Duration::from_secs(1),
        ..RunLimits::default()
    }
}

// 番号は名前空間の内外で違うので、目印の引数だけで外側のプロセスを照合する。
fn marked_processes(marker: &Path) -> Result<Vec<rustix::process::Pid>, Box<dyn Error>> {
    let mut result = Vec::new();
    for entry in fs::read_dir("/proc")? {
        let entry = entry?;
        let Some(pid) = entry
            .file_name()
            .to_str()
            .and_then(|name| name.parse::<i32>().ok())
            .and_then(rustix::process::Pid::from_raw)
        else {
            continue;
        };
        let cmdline = match fs::read(entry.path().join("cmdline")) {
            Ok(bytes) => bytes,
            Err(error)
                if error.kind() == std::io::ErrorKind::NotFound
                    || error.raw_os_error() == Some(rustix::io::Errno::SRCH.raw_os_error()) =>
            {
                continue;
            }
            Err(error) => return Err(error.into()),
        };
        if cmdline
            .split(|byte| *byte == 0)
            .any(|arg| arg == marker.as_os_str().as_encoded_bytes())
        {
            result.push(pid);
        }
    }
    Ok(result)
}
struct MarkedCleanup(PathBuf);
impl Drop for MarkedCleanup {
    fn drop(&mut self) {
        if let Ok(pids) = marked_processes(&self.0) {
            for pid in pids {
                // 送る直前にもう一度照合し、番号の使い回しで別のプロセスを止めない。
                if marked_processes(&self.0).is_ok_and(|current| current.contains(&pid)) {
                    let _sent = rustix::process::kill_process(pid, rustix::process::Signal::KILL);
                }
            }
        }
    }
}

// 作成時の関門: BH-090 の「返った直後」を、一度の /proc の走査で確かめる。
// パイプを閉じた setsid の子は、出力の EOF だけでは見つけられない。
#[test]
fn namespace_exit_leaves_no_descendants_even_after_setsid() -> TestResult {
    use benitoiteh::sandbox::run::{Leftovers, RunEnd, run};
    use std::sync::atomic::AtomicBool;
    use std::sync::{Arc, Mutex};
    let f = Fixture::new()?;
    for (session, pipes) in [
        ("session", "open"),
        ("session", "closed"),
        ("group", "open"),
    ] {
        let marker = f.writable.join(format!("descendant-{session}-{pipes}"));
        let _cleanup = MarkedCleanup(marker.clone());
        let stdout = Arc::new(Mutex::new(Vec::new()));
        let stderr = Arc::new(Mutex::new(Vec::new()));
        let sandbox = f.command(
            f.config(),
            &[
                OsStr::new("descendant"),
                OsStr::new(session),
                OsStr::new(pipes),
                marker.as_os_str(),
            ],
        )?;
        let result = run(
            sandbox,
            vec![],
            stdout,
            Arc::clone(&stderr),
            run_limits(),
            &AtomicBool::new(false),
        )?;
        let remaining = marked_processes(&marker)?;
        assert!(
            remaining.is_empty(),
            "Processes remaining immediately after run: {remaining:?}"
        );
        assert_eq!(result.end, RunEnd::Exited);
        assert!(
            result.status.success(),
            "{:?}: {:?}",
            result.status,
            stderr.lock()
        );
        assert_eq!(result.leftovers, Leftovers::None);
        assert!(marker.is_file(), "The descendant must have started");
        assert!(!f.writable.join(".git").exists());
        assert!(!f.writable.join(".benitoiteh").exists());
    }
    Ok(())
}

// 作成時の関門: 外側の bubblewrap だけを止め、目的のプログラムへ合図が届かない退行を捕まえる。
#[test]
fn namespace_stop_signals_and_exit_statuses_reach_the_caller() -> TestResult {
    use benitoiteh::sandbox::run::{Leftovers, RunEnd, StopSignal, run};
    use std::sync::atomic::AtomicBool;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;
    let f = Fixture::new()?;
    for (signal, expected) in [
        (StopSignal::Interrupt, "SIGINT"),
        (StopSignal::Terminate, "SIGTERM"),
    ] {
        for mode in ["handle", "ignore"] {
            let marker = f.writable.join(format!("signal-{signal:?}-{mode}"));
            let _cleanup = MarkedCleanup(marker.clone());
            let sandbox = f.command(
                f.config(),
                &[OsStr::new("signal"), OsStr::new(mode), marker.as_os_str()],
            )?;
            let mut limits = run_limits();
            limits.timeout = Duration::from_secs(2);
            limits.stop_signal = signal;
            let start = std::time::Instant::now();
            let result = run(
                sandbox,
                vec![],
                Arc::new(Mutex::new(Vec::new())),
                Arc::new(Mutex::new(Vec::new())),
                limits,
                &AtomicBool::new(false),
            )?;
            assert!(marked_processes(&marker)?.is_empty());
            assert_eq!(result.end, RunEnd::TimedOut);
            assert_eq!(result.leftovers, Leftovers::None);
            if mode == "handle" {
                assert_eq!(fs::read_to_string(marker)?, expected);
                assert_eq!(result.status.code(), Some(0));
            } else {
                assert_eq!(fs::read_to_string(marker)?, "ready");
                assert_eq!(result.status.code(), Some(137));
                assert!(start.elapsed() >= limits.timeout.saturating_add(limits.grace));
            }
            assert!(!f.writable.join(".git").exists());
            assert!(!f.writable.join(".benitoiteh").exists());
        }
    }
    // 通常の終了値と、合図による終了値（128+番号）を bubblewrap の外で受け取る。
    for code in ["0", "23"] {
        let result = run(
            f.command(f.config(), &[OsStr::new("exit"), OsStr::new(code)])?,
            vec![],
            Arc::new(Mutex::new(Vec::new())),
            Arc::new(Mutex::new(Vec::new())),
            run_limits(),
            &AtomicBool::new(false),
        )?;
        assert_eq!(result.status.code(), Some(code.parse()?));
    }
    let mut limits = run_limits();
    limits.timeout = Duration::from_millis(500);
    limits.stop_signal = StopSignal::Terminate;
    let result = run(
        f.command(f.config(), &[OsStr::new("sleep"), OsStr::new("60000")])?,
        vec![],
        Arc::new(Mutex::new(Vec::new())),
        Arc::new(Mutex::new(Vec::new())),
        limits,
        &AtomicBool::new(false),
    )?;
    assert_eq!(result.status.code(), Some(143));
    Ok(())
}

// 作成時の関門: 制御の fd の漏れと /proc/1/fd 経由の取得を、目的のプログラムから試す。
#[test]
fn target_cannot_inherit_or_open_control_descriptors_and_stdin_modes_work() -> TestResult {
    use std::io::Write;
    let f = Fixture::new()?;
    f.run(f.config(), &[OsStr::new("check-control-fds")], 0)?;
    for stdin in [SandboxStdin::Null, SandboxStdin::Piped] {
        let sandbox =
            f.command_with_stdin(f.config(), &f.probe, &[OsStr::new("echo-stdin")], stdin)?;
        let output = if stdin == SandboxStdin::Piped {
            let mut wrapper = Command::new(sandbox.get_program());
            wrapper
                .args(sandbox.get_args())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped());
            let mut child = sandbox.spawn_wrapped(wrapper)?;
            let mut writer = child.take_stdin().ok_or("Missing stdin writer")?;
            writer.write_all(b"piped bytes")?;
            drop(writer);
            child.wait_with_output()?
        } else {
            sandbox.output()?
        };
        assert!(output.status.success(), "{output:?}");
        assert_eq!(
            output.stdout,
            if stdin == SandboxStdin::Piped {
                b"piped bytes".as_slice()
            } else {
                b""
            }
        );
    }

    // 実行の公開の入口も、bubblewrap の stdin を差し替えずに書き口を使う。
    let stdout = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let output = benitoiteh::sandbox::run::run(
        f.command_with_stdin(
            f.config(),
            &f.probe,
            &[OsStr::new("echo-stdin")],
            SandboxStdin::Piped,
        )?,
        vec![b'x'; 1024 * 1024],
        std::sync::Arc::clone(&stdout),
        std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
        run_limits(),
        &std::sync::atomic::AtomicBool::new(false),
    )?;
    assert!(output.status.success());
    assert_eq!(
        *stdout
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner),
        vec![b'x'; 1024 * 1024]
    );
    Ok(())
}

// 作成時の関門: 準備が失敗しているのに目的のプログラムを動かす退行を、内容の不変で確かめる。
#[test]
fn malformed_control_input_fails_before_target_execution() -> TestResult {
    let f = Fixture::new()?;
    let marker = f.writable.join("not-executed");
    fs::write(&marker, b"unchanged")?;
    let json = serde_json::to_string(&SandboxPolicy::new(f.config())?)?;
    for count in [None, Some(0), Some(2)] {
        let mut command = Command::new(&f.helper);
        command.args([
            OsStr::new("__sandbox"),
            OsStr::new(&json),
            OsStr::new("--"),
            f.probe.as_os_str(),
            OsStr::new("write"),
            marker.as_os_str(),
        ]);
        let _control = match count {
            Some(count) => Some(attach_control(&mut command, count)?),
            None => {
                command.stdin(Stdio::null());
                None
            }
        };
        let output = command.output()?;
        check(&output, 125, "malformed control socket");
        assert!(String::from_utf8_lossy(&output.stderr).contains("descriptors"));
        assert_eq!(fs::read(&marker)?, b"unchanged");
    }
    Ok(())
}

// 作成時の関門: 親の stdin の複製を、並列テストの stdin を変更せずに本物の子で試す。
#[test]
fn inherited_stdin_is_forwarded_from_parent_process() -> TestResult {
    use std::io::Write;
    let f = Fixture::new()?;
    if std::env::var_os("BH_TEST_INHERIT_STDIN").is_some() {
        let output = f
            .command_with_stdin(
                f.config(),
                &f.probe,
                &[OsStr::new("echo-stdin")],
                SandboxStdin::Inherit,
            )?
            .output()?;
        check(&output, 0, "inherited stdin");
        assert_eq!(output.stdout, b"inherited bytes");
    } else {
        let mut child = Command::new(std::env::current_exe()?)
            .args([
                "--exact",
                "inherited_stdin_is_forwarded_from_parent_process",
                "--nocapture",
            ])
            .env("BH_TEST_INHERIT_STDIN", "1")
            .current_dir(&f.root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        let mut writer = child.stdin.take().ok_or("Missing stdin")?;
        writer.write_all(b"inherited bytes")?;
        drop(writer);
        let output = child.wait_with_output()?;
        check(&output, 0, "isolated parent with inherited stdin");
    }
    Ok(())
}

// 作成時の関門: 共有錠の欠落と、既存の場所に錠を掛け忘れる退行は、単独の子のテストでは見えない。
// 2 通りの起動順で、A の終了後にも B のマウントが拒み、最後の B が記録を片付ける契約を守る。
fn start_waiting(sandbox: PreparedSandbox) -> Result<SandboxChild, Box<dyn Error>> {
    let mut wrapper = Command::new(sandbox.get_program());
    wrapper
        .args(sandbox.get_args())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = sandbox.spawn_wrapped(wrapper)?;
    let stdout = child.child_mut().stdout.take().ok_or("Missing stdout")?;
    let mut ready = String::new();
    BufReader::new(stdout).read_line(&mut ready)?;
    if ready.trim() != "ready" {
        let output = child.wait_with_output()?;
        return Err(format!("Missing readiness: {ready:?}, {output:?}").into());
    }
    Ok(child)
}

fn waiting_command(f: &Fixture, operation: &[&OsStr]) -> Result<PreparedSandbox, Box<dyn Error>> {
    let mut args = vec![OsStr::new("wait-line")];
    args.extend_from_slice(operation);
    f.command_with_stdin(f.config(), &f.probe, &args, SandboxStdin::Piped)
}

fn release_waiting(mut child: SandboxChild) -> Result<Output, Box<dyn Error>> {
    let mut stdin = child.take_stdin().ok_or("Missing stdin")?;
    stdin.write_all(b"continue\n")?;
    drop(stdin);
    Ok(child.wait_with_output()?)
}

fn path_records(f: &Fixture) -> Result<Vec<PathBuf>, Box<dyn Error>> {
    let mut paths = Vec::new();
    for entry in fs::read_dir(f.root.join("placeholders"))? {
        let path = entry?.path();
        if path
            .extension()
            .is_some_and(|extension| extension == "path")
        {
            paths.push(path);
        }
    }
    Ok(paths)
}

fn concurrent_placeholders(
    prepare_b_after_a_starts: bool,
    remove_without_lock: bool,
) -> TestResult {
    for operation in ["write", "rename"] {
        if remove_without_lock && operation == "rename" {
            continue;
        }
        let f = Fixture::new()?;
        let protected = f.writable.join(".benitoiteh");
        let target = protected.join("x");
        let renamed = f.writable.join("renamed");
        let a = waiting_command(&f, &[OsStr::new("socketpair")])?;
        let args = if operation == "write" {
            vec![
                OsStr::new(if remove_without_lock {
                    "write-with-parents"
                } else {
                    "write"
                }),
                target.as_os_str(),
            ]
        } else {
            vec![
                OsStr::new("rename"),
                protected.as_os_str(),
                renamed.as_os_str(),
            ]
        };
        let (a, b) = if prepare_b_after_a_starts {
            let a = start_waiting(a)?;
            let b = start_waiting(waiting_command(&f, &args)?)?;
            (a, b)
        } else {
            let b = waiting_command(&f, &args)?;
            (start_waiting(a)?, start_waiting(b)?)
        };
        if remove_without_lock {
            // 故意に錠を無視して消す。BH-095 のカーネルの挙動を確かめる。
            fs::remove_dir(&protected)?;
        }
        let a_output = release_waiting(a)?;
        check(&a_output, 0, "concurrent/A");
        if !remove_without_lock {
            assert!(protected.is_dir());
            assert!(!path_records(&f)?.is_empty());
        }
        let b_output = release_waiting(b)?;
        if remove_without_lock {
            check(&b_output, 0, "unlocked/B/write");
            assert_eq!(fs::read(target)?, b"sandbox-probe\n");
        } else {
            assert!(
                !b_output.status.success(),
                "B unexpectedly succeeded: {b_output:?}"
            );
            let stderr = String::from_utf8_lossy(&b_output.stderr);
            assert!(
                stderr.contains(if operation == "write" {
                    "ReadOnlyFilesystem"
                } else {
                    "ResourceBusy"
                }),
                "{b_output:?}"
            );
            assert!(!target.exists());
            assert!(!renamed.exists());
            assert!(!protected.exists());
        }
        assert!(!f.writable.join(".git").exists());
        assert!(path_records(&f)?.is_empty());
    }
    Ok(())
}

#[test]
fn concurrent_placeholders_prepared_before_either_child_starts() -> TestResult {
    concurrent_placeholders(false, false)
}

#[test]
fn concurrent_placeholders_existing_mountpoint_is_locked() -> TestResult {
    concurrent_placeholders(true, false)
}

#[test]
#[ignore = "BH-095: deliberately removes an active mountpoint without its lock"]
fn unlocked_placeholder_removal_detaches_other_namespace_mount() -> TestResult {
    concurrent_placeholders(false, true)
}

fn record_for(f: &Fixture, path: &Path) -> Result<(PathBuf, PathBuf), Box<dyn Error>> {
    use sha2::{Digest, Sha256};
    use std::os::unix::ffi::OsStrExt;
    let hash: String = Sha256::digest(path.as_os_str().as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    let directory = f.root.join("placeholders");
    let record = directory.join(format!("{hash}.path"));
    fs::write(&record, path.as_os_str().as_bytes())?;
    Ok((record, directory.join(format!("{hash}.lock"))))
}

// 作成時の関門: 強制終了で残った記録を入力にし、利用者の内容と他の子の共有錠を守る。
// 模擬の後始末を差し込まず、prepare の入口から実際のファイルと flock で確かめる。
#[test]
fn stale_placeholder_records_are_reaped_without_removing_user_data_or_locked_paths() -> TestResult {
    use rustix::fs::{FlockOperation, flock};
    let f = Fixture::new()?;
    drop(f.command(f.config(), &[OsStr::new("socketpair")])?);
    // 次の prepare が直ちに新しい記録を書かないよう、回収対象は別の書ける場所に置く。
    let stale = f.root.join("stale");
    fs::create_dir(&stale)?;
    let empty = stale.join("empty");
    let occupied = stale.join("occupied");
    let regular = stale.join("file");
    let link = stale.join("link");
    let locked = stale.join("locked");
    for path in [&empty, &occupied, &locked] {
        fs::create_dir(path)?;
    }
    fs::write(occupied.join("keep"), b"keep")?;
    fs::write(&regular, b"keep")?;
    symlink(&empty, &link)?;
    let mut records = Vec::new();
    for path in [&empty, &occupied, &regular, &link, &stale.join("missing")] {
        records.push(record_for(&f, path)?.0);
    }
    let (locked_record, lock) = record_for(&f, &locked)?;
    let lock = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(lock)?;
    flock(&lock, FlockOperation::LockShared)?;
    let relative_record = f
        .root
        .join("placeholders/d2d9e1f13413d3e07064d629ac3341a6e1d6a5eb0895e76a3c66df19307a56f0.path");
    fs::write(&relative_record, b"relative")?;
    let mismatch = stale.join("mismatch");
    fs::create_dir(&mismatch)?;
    let (mismatched_record, _) = record_for(&f, &mismatch)?;
    fs::write(&mismatched_record, empty.as_os_str().as_encoded_bytes())?;
    let prepared = f.command(f.config(), &[OsStr::new("socketpair")])?;
    assert!(!empty.exists());
    assert_eq!(fs::read(occupied.join("keep"))?, b"keep");
    assert_eq!(fs::read(&regular)?, b"keep");
    assert!(fs::symlink_metadata(link)?.file_type().is_symlink());
    assert!(locked.is_dir());
    assert!(locked_record.exists());
    assert!(mismatch.is_dir());
    for record in records
        .into_iter()
        .chain([relative_record, mismatched_record])
    {
        assert!(!record.exists(), "{}", record.display());
    }
    drop(prepared);
    drop(lock);
    drop(f.command(f.config(), &[OsStr::new("socketpair")])?);
    assert!(!locked.exists());
    assert!(!locked_record.exists());
    Ok(())
}

// 作成時の関門: 錠の差し替えを可能にする配置と権限を、準備の入口で拒む。
#[test]
fn placeholders_directory_is_private_real_and_outside_writable_roots() -> TestResult {
    let f = Fixture::new()?;
    let command = SandboxCommand {
        policy: SandboxPolicy::new(f.config())?,
        program: f.probe.clone(),
        args: vec!["socketpair".into()],
        cwd: f.root.clone(),
        env: BTreeMap::new(),
        stdin: SandboxStdin::Null,
    };
    let directory = f.root.join("private/placeholders");
    drop(command.prepare(&f.helper, &f.root, &directory)?);
    assert_eq!(fs::symlink_metadata(&directory)?.mode() & 0o777, 0o700);
    let link = f.root.join("link-placeholders");
    symlink(&directory, &link)?;
    let inside = f.writable.join("placeholders");
    let regular = f.root.join("file-placeholders");
    fs::write(&regular, b"file")?;
    for invalid in [&link, &inside, &regular] {
        assert!(matches!(
            command.prepare(&f.helper, &f.root, invalid),
            Err(SandboxError::InvalidPolicy(_))
        ));
    }
    fs::set_permissions(&directory, fs::Permissions::from_mode(0o750))?;
    assert!(matches!(
        command.prepare(&f.helper, &f.root, &directory),
        Err(SandboxError::InvalidPolicy(_))
    ));
    assert_eq!(fs::metadata(&directory)?.mode() & 0o777, 0o750);
    Ok(())
}

// 作成時の関門: BH-095 の探索コストを本番の prepare で測り、計測用の公開口を作らない。
#[test]
#[ignore = "BH-095: creates a large tree and reports prepare latency"]
fn nested_protected_scan_timing() -> TestResult {
    use std::time::{Duration, Instant};
    for large in [false, true] {
        let f = Fixture::new()?;
        if large {
            let mut level = vec![f.writable.clone()];
            for depth in 0..3 {
                let mut next = Vec::new();
                for parent in level {
                    for number in 0..30 {
                        let directory = parent.join(format!("d-{depth}-{number}"));
                        fs::create_dir(&directory)?;
                        fs::write(parent.join(format!("f-{depth}-{number}")), b"file")?;
                        next.push(directory);
                    }
                }
                level = next;
            }
        } else {
            for directory in 0..10 {
                let path = f.writable.join(format!("d-{directory}"));
                fs::create_dir(&path)?;
                for file in 0..10 {
                    fs::write(path.join(format!("f-{file}")), b"file")?;
                }
            }
        }
        let mountinfo = fs::read_to_string("/proc/self/mountinfo")?;
        // 空白などを含む一時領域でも、mountinfo のエスケープに合わせて照合する。
        let mount_line = mountinfo
            .lines()
            .filter_map(|line| {
                let encoded = line.split_whitespace().nth(4)?;
                let decoded = encoded
                    .replace("\\040", " ")
                    .replace("\\011", "\t")
                    .replace("\\012", "\n")
                    .replace("\\134", "\\");
                f.writable
                    .starts_with(&decoded)
                    .then_some((decoded.len(), line))
            })
            .max_by_key(|(length, _)| *length)
            .ok_or("Missing mountinfo entry")?
            .1;
        let command = SandboxCommand {
            policy: SandboxPolicy::new(f.config())?,
            program: f.probe.clone(),
            args: vec!["socketpair".into()],
            cwd: f.root.clone(),
            env: BTreeMap::new(),
            stdin: SandboxStdin::Null,
        };
        let placeholders = f.root.join("placeholders");
        let mut times = Vec::new();
        for _ in 0..20 {
            let start = Instant::now();
            let prepared = command.prepare(&f.helper, &f.root, &placeholders)?;
            let elapsed = start.elapsed();
            drop(prepared);
            times.push(elapsed);
        }
        let first = *times.first().ok_or("Missing first timing")?;
        times.sort();
        let median: Duration = times
            .get(9)
            .ok_or("Missing lower median")?
            .checked_add(*times.get(10).ok_or("Missing upper median")?)
            .ok_or("Timing overflow")?
            / 2;
        println!(
            "tree={} directories={} files={} traversed_entries={} samples=20 first={first:?} median={median:?} root={} mountinfo={mount_line}",
            if large { "large" } else { "small" },
            if large { 27_930 } else { 10 },
            if large { 27_930 } else { 100 },
            if large { 55_860 } else { 110 },
            f.writable.display()
        );
    }
    Ok(())
}

// 作成時の関門: 回収せずに落とした子の錠を解放すると、他の prepare がマウントの場所を消せる。
// 錠を意図的に保持するテストなので、専用の親プロセスを終了してから残りの回収も確かめる。
#[test]
fn dropped_unreaped_child_keeps_placeholder_locks_until_parent_exit() -> TestResult {
    use rustix::fs::{FlockOperation, flock};
    if let Some(root) = std::env::var_os("BH_TEST_DROPPED_PLACEHOLDERS") {
        let root = PathBuf::from(root);
        let f = Fixture {
            writable: root.join("write"),
            probe: root.join("probe"),
            helper: fs::canonicalize(env!("CARGO_BIN_EXE_benitoiteh"))?,
            root,
        };
        let mut child = start_waiting(waiting_command(&f, &[OsStr::new("socketpair")])?)?;
        let id = i32::try_from(child.child_mut().id())?;
        let pid = rustix::process::Pid::from_raw(id).ok_or("Invalid child PID")?;
        let record = path_records(&f)?
            .into_iter()
            .find(|record| {
                fs::read(record).ok().as_deref()
                    == Some(
                        f.writable
                            .join(".benitoiteh")
                            .as_os_str()
                            .as_encoded_bytes(),
                    )
            })
            .ok_or("Missing record")?;
        let lock = File::open(record.with_extension("lock"))?;
        drop(child);
        assert_eq!(
            flock(&lock, FlockOperation::NonBlockingLockExclusive),
            Err(rustix::io::Errno::WOULDBLOCK)
        );
        // Drop の制御ソケットの閉鎖で停止する。安全の錠の保持とは別に、子は回収しておく。
        loop {
            match rustix::process::waitpid(Some(pid), rustix::process::WaitOptions::empty()) {
                Ok(Some(_)) => break,
                Err(rustix::io::Errno::INTR) => continue,
                result => return Err(format!("Could not reap dropped child: {result:?}").into()),
            }
        }
        assert!(record.exists());
        // フィクスチャのディレクトリは、外側の親が回収するまで残す。
        std::mem::forget(f);
        return Ok(());
    }
    let f = Fixture::new()?;
    let output = Command::new(std::env::current_exe()?)
        .args([
            "--exact",
            "dropped_unreaped_child_keeps_placeholder_locks_until_parent_exit",
            "--nocapture",
        ])
        .env("BH_TEST_DROPPED_PLACEHOLDERS", &f.root)
        .current_dir(&f.root)
        .output()?;
    check(&output, 0, "isolated dropped-child parent");
    assert!(!path_records(&f)?.is_empty());
    // 専用の親が終了すると OS が錠を外し、次の prepare が残りを回収できる。
    let prepared = f.command(f.config(), &[OsStr::new("socketpair")])?;
    assert!(!f.writable.join(".benitoiteh").exists());
    drop(prepared);
    Ok(())
}

// 作成時の関門: 錠を開けないときに保護なしで続ける退行を、準備の公開の入口で拒む。
#[test]
fn placeholder_lock_io_failure_aborts_prepare_with_its_stage() -> TestResult {
    let f = Fixture::new()?;
    drop(f.command(f.config(), &[OsStr::new("socketpair")])?);
    let record = path_records(&f)?
        .into_iter()
        .next()
        .ok_or("Missing record")?;
    let lock = record.with_extension("lock");
    fs::remove_file(&lock)?;
    fs::create_dir(&lock)?;
    assert!(matches!(
        f.command(f.config(), &[OsStr::new("socketpair")]),
        Err(error) if matches!(error.downcast_ref::<SandboxError>(), Some(SandboxError::Setup { stage: "placeholders", .. }))
    ));
    assert!(record.exists());
    Ok(())
}

#[path = "support/processor.rs"]
#[cfg(target_arch = "x86_64")]
mod processor;

#[path = "support/doctor.rs"]
#[cfg(target_arch = "x86_64")]
mod doctor_support;

// 作成時の関門: 正負の本物の起動を CLI につなぎ、機能の一覧だけから
// 制限モードを使えると表示する退行を捕まえる。Codex 内では走らせない。
#[cfg(target_arch = "x86_64")]
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

// 作成時の関門: --new-session が制御端末を切り離すことを実行で確かめる。
#[test]
fn normal_sandbox_has_no_controlling_terminal() -> TestResult {
    let f = Fixture::new()?;
    let mut pty = tty::Pty::new()?;
    pty.control(&f.probe)?;
    for operation in ["tty-open", "tty-write", "tty-ioctl"] {
        let command = f.command(f.config(), &[OsStr::new(operation)])?;
        let mut wrapper = pty.wrapper(&f.probe);
        wrapper.arg(command.get_program()).args(command.get_args());
        let mut child = command.spawn_wrapped(wrapper)?;
        let (output, terminal) = pty.capture(child.child_mut())?;
        child.wait()?;
        if operation == "tty-open" {
            check(&output, 15, operation);
        } else {
            // Landlock が先に open を拒む場合と、端末がない場合をともに認める。
            assert!(
                matches!(output.status.code(), Some(10 | 11)),
                "{operation}: {output:?}"
            );
        }
        assert!(
            terminal.is_empty(),
            "sandbox child wrote to the controlling terminal"
        );
    }
    Ok(())
}
