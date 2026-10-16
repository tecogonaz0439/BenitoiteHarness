//! 本物の Linux の補助・Landlock・seccomp を子プロセスで検証する（設計書 03-02、07-01、実装プラン P0-07）。

#![cfg(target_os = "linux")]

use std::collections::BTreeMap;
use std::error::Error;
use std::ffi::{OsStr, OsString};
use std::fs::{self, File};
use std::io::{BufRead, BufReader};
use std::net::TcpListener;
use std::os::fd::AsRawFd;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::os::unix::net::{UnixDatagram, UnixListener};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use benitoiteh::sandbox::{
    CapabilityStatus, PolicyConfig, PreparedSandbox, ProgramAccess, ReadAccess, SandboxCommand,
    SandboxError, SandboxPolicy, SandboxStdin, probe_linux,
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
        self.command_for(config, &self.probe, args)
    }

    fn command_for(
        &self,
        config: PolicyConfig,
        program: &Path,
        args: &[&OsStr],
    ) -> Result<PreparedSandbox, Box<dyn Error>> {
        Ok(SandboxCommand {
            policy: SandboxPolicy::new(config)?,
            program: program.to_path_buf(),
            args: args.iter().map(|arg| arg.to_os_string()).collect(),
            cwd: self.root.clone(),
            env: BTreeMap::new(),
            stdin: SandboxStdin::Null,
        }
        .prepare(&self.helper, &self.root)?)
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

// 作成時の関門: 書き込み権の不足と、守るマウントの欠落を、外の FS の状態でも検出する。
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
    let outside = f.root.join("outside");
    fs::write(&outside, b"unchanged")?;
    f.run(f.config(), &[OsStr::new("write"), outside.as_os_str()], 11)?;
    assert_eq!(fs::read(&outside)?, b"unchanged");
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
    let sandbox = f.command(config, &[OsStr::new("wait-stdin")])?;
    let mut command = Command::new(sandbox.get_program());
    command
        .args(sandbox.get_args())
        .env_clear()
        .current_dir(&f.root)
        .process_group(0)
        .stdin(Stdio::piped())
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
    drop(child.child_mut().stdin.take());
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
    .prepare(&f.helper, &f.root)?
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
        command.prepare(&writable_helper, &f.root),
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
