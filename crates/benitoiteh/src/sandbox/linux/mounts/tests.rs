//! マウントの優先順と起動前の拒否を、ホストの機能に依存せず検証する（実装プラン P0-07）。

use std::error::Error;
use std::os::unix::fs::symlink;

use super::*;
use crate::sandbox::PolicyConfig;

type TestResult = Result<(), Box<dyn Error>>;

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Result<Self, Box<dyn Error>> {
        let mut nonce = [0; 16];
        getrandom::fill(&mut nonce)?;
        let root = fs::canonicalize(std::env::temp_dir())?
            .join(format!("bh-mounts-{:032x}", u128::from_ne_bytes(nonce)));
        fs::create_dir(&root)?;
        fs::create_dir(root.join("write"))?;
        Ok(Self(root))
    }

    fn config(&self) -> PolicyConfig {
        PolicyConfig::new(vec![self.0.join("write")])
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        if let Err(error) = fs::remove_dir_all(&self.0) {
            eprintln!("Could not remove {}: {error}", self.0.display());
        }
    }
}

// 作成時の関門: 後のマウントによる保護の上書き、親子順の逆転、過深の探索を捕まえる。
// 引数列の契約だけを比較する。OS の拒否そのものは sandbox_linux が確かめる。
#[test]
fn mount_order_keeps_masks_after_writable_roots() -> TestResult {
    let f = Fixture::new()?;
    let w = f.0.join("write");
    let sub = w.join("sub");
    for path in [
        w.join(".GIT"),
        w.join("a/.git"),
        w.join("a/b/.BENITOITEH"),
        w.join("a/b/c/.git"),
        sub.clone(),
    ] {
        fs::create_dir_all(path)?;
    }
    let secret = f.0.join("secret");
    fs::create_dir(&secret)?;
    fs::write(secret.join("key"), b"secret")?;
    let denied_file = w.join("secret-file");
    fs::write(&denied_file, b"secret")?;
    for allow_ip in [false, true] {
        let mut config = f.config();
        config.writable = vec![sub.clone(), w.clone()];
        config.denied_read = vec![
            secret.join("key"),
            denied_file.clone(),
            secret.clone(),
            secret.clone(),
        ];
        config.allow_ip = allow_ip;
        let mut plan = build(
            &SandboxPolicy::new(config)?,
            &f.0,
            &f.0,
            &f.0.join("placeholders"),
        )?;
        let cover = plan.cleanup.cover.as_ref().ok_or("Missing cover")?;
        let mut expected: Vec<OsString> = [
            "--unshare-user",
            "--unshare-pid",
            "--as-pid-1",
            "--unshare-ipc",
            "--disable-userns",
            "--new-session",
            "--die-with-parent",
            "--cap-drop",
            "ALL",
        ]
        .map(OsString::from)
        .to_vec();
        if !allow_ip {
            expected.push("--unshare-net".into());
        }
        expected.extend(
            ["--ro-bind", "/", "/", "--dev", "/dev", "--proc", "/proc"].map(OsString::from),
        );
        for path in [&w, &sub] {
            expected.extend([
                OsString::from("--bind"),
                path.as_os_str().to_owned(),
                path.as_os_str().to_owned(),
            ]);
        }
        for path in [w.join(".GIT"), w.join("a/.git"), w.join("a/b/.BENITOITEH")] {
            expected.extend([
                OsString::from("--ro-bind"),
                path.as_os_str().to_owned(),
                path.as_os_str().to_owned(),
            ]);
        }
        let missing = [
            w.join(".git"),
            w.join(".benitoiteh"),
            sub.join(".git"),
            sub.join(".benitoiteh"),
        ];
        for path in &missing {
            expected.extend([
                OsString::from("--perms"),
                OsString::from("555"),
                OsString::from("--tmpfs"),
                path.as_os_str().to_owned(),
                OsString::from("--remount-ro"),
                path.as_os_str().to_owned(),
            ]);
        }
        expected.extend([
            OsString::from("--perms"),
            OsString::from("000"),
            OsString::from("--tmpfs"),
            secret.as_os_str().to_owned(),
            OsString::from("--remount-ro"),
            secret.as_os_str().to_owned(),
        ]);
        expected.extend([
            OsString::from("--ro-bind"),
            cover.as_os_str().to_owned(),
            denied_file.as_os_str().to_owned(),
            OsString::from("--chdir"),
            f.0.as_os_str().to_owned(),
        ]);
        assert_eq!(plan.args, expected);
        assert_eq!(plan.cleanup.directories, missing);
        assert!(!cover.starts_with(&w));
        let metadata = fs::symlink_metadata(cover)?;
        assert!(metadata.is_file());
        assert_eq!(metadata.len(), 0);
        assert_eq!(metadata.permissions().mode() & 0o7777, 0);
        let cover = cover.clone();
        plan.cleanup.finish()?;
        assert!(!cover.exists());
    }
    Ok(())
}

// 作成時の関門: gitdir の相対パス、追加の守る名前、消えた秘密の扱いを固定する。
#[test]
fn gitdir_and_custom_protection_use_the_same_mount_precedence() -> TestResult {
    let f = Fixture::new()?;
    let w = f.0.join("write");
    let gitdir = w.join("metadata");
    fs::create_dir(&gitdir)?;
    fs::create_dir(w.join(".benitoiteh"))?;
    fs::write(w.join(".git"), b"gitdir: metadata\n")?;
    let removed = f.0.join("removed");
    fs::write(&removed, b"secret")?;
    let mut config = f.config();
    config.protected_names.push(".extra".into());
    config.denied_read.push(removed.clone());
    let policy = SandboxPolicy::new(config)?;
    fs::remove_file(removed)?;
    let mut plan = build(&policy, &f.0, &f.0, &f.0.join("placeholders"))?;
    let mut expected: Vec<OsString> = [
        "--unshare-user",
        "--unshare-pid",
        "--as-pid-1",
        "--unshare-ipc",
        "--disable-userns",
        "--new-session",
        "--die-with-parent",
        "--cap-drop",
        "ALL",
        "--unshare-net",
        "--ro-bind",
        "/",
        "/",
        "--dev",
        "/dev",
        "--proc",
        "/proc",
    ]
    .map(OsString::from)
    .to_vec();
    expected.extend([
        OsString::from("--bind"),
        w.as_os_str().to_owned(),
        w.as_os_str().to_owned(),
    ]);
    for path in [w.join(".benitoiteh"), w.join(".git")] {
        expected.extend([
            OsString::from("--ro-bind"),
            path.as_os_str().to_owned(),
            path.as_os_str().to_owned(),
        ]);
    }
    let extra = w.join(".extra");
    expected.extend([
        OsString::from("--perms"),
        OsString::from("555"),
        OsString::from("--tmpfs"),
        extra.as_os_str().to_owned(),
        OsString::from("--remount-ro"),
        extra.as_os_str().to_owned(),
        OsString::from("--ro-bind"),
        gitdir.as_os_str().to_owned(),
        gitdir.as_os_str().to_owned(),
        OsString::from("--chdir"),
        f.0.as_os_str().to_owned(),
    ]);
    assert_eq!(plan.args, expected);
    assert_eq!(plan.cleanup.directories, [extra]);
    assert!(plan.cleanup.cover.is_none());
    plan.cleanup.finish()?;
    Ok(())
}

// 作成時の関門: 方針を作った後のリンクの出現と、不正な gitdir を起動前に拒否する。
#[test]
fn mutable_links_and_invalid_gitdir_fail_before_launch() -> TestResult {
    let f = Fixture::new()?;
    let w = f.0.join("write");
    let policy = SandboxPolicy::new(f.config())?;
    for path in [w.join(".git"), w.join("a/.git")] {
        fs::create_dir_all(path.parent().ok_or("Missing parent")?)?;
        symlink(&f.0, &path)?;
        assert!(matches!(
            build(&policy, &f.0, &f.0, &f.0.join("placeholders")),
            Err(SandboxError::InvalidPolicy(_))
        ));
        fs::remove_file(path)?;
    }
    symlink(&f.0, w.join("link"))?;
    for contents in [
        "wrong",
        "gitdir: missing",
        "gitdir: link",
        "gitdir:\n",
        "gitdir: metadata\nextra",
    ] {
        fs::write(w.join(".git"), contents)?;
        assert!(
            build(&policy, &f.0, &f.0, &f.0.join("placeholders")).is_err(),
            "{contents}"
        );
    }
    fs::remove_file(w.join(".git"))?;
    let denied = w.join("secret");
    fs::create_dir(&denied)?;
    let mut config = f.config();
    config.denied_read.push(denied.clone());
    let policy = SandboxPolicy::new(config)?;
    fs::remove_dir(&denied)?;
    symlink(&f.0, &denied)?;
    assert!(matches!(
        build(&policy, &f.0, &f.0, &f.0.join("placeholders")),
        Err(SandboxError::InvalidPolicy(_))
    ));
    Ok(())
}

// 作成時の関門: パスを UTF-8 に置き換えたり、ハッシュの名前が変わると別の錠になる。
// 記録のバイト列と既知の SHA-256 を確かめ、相対パスと不一致の記録が削除だけになる境界を守る。
#[test]
fn placeholder_records_preserve_path_bytes_and_validate_record_identity() -> TestResult {
    use std::os::unix::ffi::OsStringExt;
    let f = Fixture::new()?;
    let directory = f.0.join("placeholders");
    let store = Store::prepare(&directory, &[])?;
    let bytes = b"/placeholder-\xff/.benitoiteh";
    let mut placeholder = store.lock(PathBuf::from(OsString::from_vec(bytes.to_vec())))?;
    placeholder.record_missing()?;
    let record =
        directory.join("6b976ac00bf2e3b6ff5f6296fa087deb25c7ced255066765ede6c58ed3cac9f5.path");
    assert_eq!(fs::read(&record)?, bytes);
    // 固定の絶対パスを片付ける処理には渡さず、記録の生成だけを確かめる。
    fs::remove_file(record)?;
    drop(placeholder);
    for (name, bytes) in [
        (
            "d2d9e1f13413d3e07064d629ac3341a6e1d6a5eb0895e76a3c66df19307a56f0",
            b"relative".as_slice(),
        ),
        (
            "0e16e3abe8b3c19aad4caa4c6442c20df7de439fa94aa276b494a92e13545011",
            b"/different".as_slice(),
        ),
    ] {
        let record = directory.join(format!("{name}.path"));
        fs::write(&record, bytes)?;
        Store::prepare(&directory, &[])?;
        assert!(!record.exists());
    }
    let malformed = directory.join(OsString::from_vec(b"\xff.path".to_vec()));
    fs::write(&malformed, b"invalid hash")?;
    Store::prepare(&directory, &[])?;
    assert!(!malformed.exists());
    Ok(())
}
