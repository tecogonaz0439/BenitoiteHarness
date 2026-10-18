//! safe_fs の公開の契約を、本物の FS で確かめる（設計書 01-03「道具の定義の共通の規則」「書く道具のパスの規則」、01-05「巻き戻し」）。

use std::error::Error;
use std::ffi::OsString;
use std::fs;
use std::io::{self, Read};
use std::os::unix::ffi::OsStringExt;
use std::os::unix::fs::{MetadataExt, PermissionsExt, symlink};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use benitoiteh::cli::args::ExecArgs;
use benitoiteh::config::{ProjectConfig, UserConfig};
use benitoiteh::safe_fs::{
    DirectoryEntry, EntryKind, SafeFsError, ScopeRelPath, ScopeRoot, resolved_path,
};

type TestResult = Result<(), Box<dyn Error>>;

struct Fixture(PathBuf);

impl Fixture {
    fn new(name: &str) -> Result<Self, Box<dyn Error>> {
        // コンテナでは共有された作業ツリーでなく Linux 側の一時領域を使う。
        // macOS では /var の別名を解き、実のパスで操作する（実装プラン 00-common「実装の規約の補足」）。
        let parent = fs::canonicalize(std::env::temp_dir())?;
        let nonce = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
        let path = parent.join(format!(
            "benitoiteh-safe-fs-{name}-{}-{nonce}",
            std::process::id()
        ));
        fs::create_dir(&path)?;
        let fixture = Self(path);
        fs::create_dir(fixture.root())?;
        Ok(fixture)
    }

    fn root(&self) -> PathBuf {
        self.0.join("root")
    }

    fn scope(&self) -> Result<ScopeRoot, SafeFsError> {
        ScopeRoot::open(self.root())
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        if let Err(error) = fs::remove_dir_all(&self.0) {
            eprintln!(
                "Could not remove test directory {}: {error}",
                self.0.display()
            );
        }
    }
}

fn rel(path: impl AsRef<Path>) -> Result<ScopeRelPath, SafeFsError> {
    ScopeRelPath::new(path)
}

// 作成時の関門: OS の印が実際に働く起動条件を、本物の木と公開の検査で確かめる。
// Rosetta で除く既存の safe_fs の境界に置き、別の層では繰り返さない（実装プラン P1-04）。
#[test]
fn startup_checks_verify_platform_flags_and_remove_the_probe_tree() -> TestResult {
    let fixture = Fixture::new("startup")?;
    let before = fs::read_dir(&fixture.0)?.count();
    benitoiteh::safe_fs::verify_platform_support(&fixture.0)?;
    assert_eq!(fs::read_dir(&fixture.0)?.count(), before);
    Ok(())
}

// 作成時の関門: 信頼しない設定の禁止欄の部分採用、範囲外の読み取り、除外の削減を捕まえる。
// openat2 を使うので、Rosetta で除くファイルに置く（実装プラン 00-common「検査」）。
#[test]
fn project_configuration_is_confined_and_is_discarded_as_a_whole_on_error() -> TestResult {
    let fixture = Fixture::new("project-config")?;
    let project_root = fixture.root();
    let project = project_root.join(".benitoiteh");
    fs::create_dir(&project)?;
    let root = fixture.scope()?;
    assert!(!ProjectConfig::load(&root).read);
    let path = project.join("config.toml");
    fs::write(
        &path,
        "model='project-model'\n[read]\nexclude=['project-secret']",
    )?;
    let loaded = ProjectConfig::load(&root);
    assert!(loaded.read);
    assert!(loaded.error.is_none());
    let user = UserConfig::parse(
        "default_provider='p'\n[providers.p]\nkind='openai-chat'\nmodel='user-model'\n[read]\nexclude=['user-secret']",
    )?;
    let args = ExecArgs::parse(["request"])?;
    let resolved = user.resolve_exec(&args, &loaded.config, &project_root)?;
    assert_eq!(resolved.destination.provider, "p");
    assert_eq!(resolved.destination.model, "project-model");
    assert_eq!(resolved.read.exclude, ["user-secret", "project-secret"]);
    for forbidden in [
        "base_url='https://attacker.example'",
        "default_provider='evil'",
        "default_mode='normal'",
        "api_key={env='KEY'}",
        "[restricted]\nscratch_auto_effects=['File.Read']",
    ] {
        fs::write(&path, format!("model='discard-me'\n{forbidden}"))?;
        let rejected = ProjectConfig::load(&root);
        assert!(rejected.read);
        assert!(rejected.error.is_some());
        assert!(rejected.config.model.is_none());
        assert_eq!(
            user.resolve_exec(&args, &rejected.config, &project_root)?
                .destination
                .model,
            "user-model"
        );
    }
    fs::remove_file(&path)?;
    fs::write(fixture.0.join("outside.toml"), "model='outside-model'")?;
    symlink("../../outside.toml", &path)?;
    let rejected = ProjectConfig::load(&root);
    assert!(rejected.error.is_some());
    assert!(rejected.config.model.is_none());
    Ok(())
}

fn read(scope: &ScopeRoot, path: &ScopeRelPath) -> Result<String, Box<dyn Error>> {
    let mut text = String::new();
    scope.open_read(path)?.read_to_string(&mut text)?;
    Ok(text)
}

// 作成時の関門: 正規化で危険な要素が消える退行を、ScopeRelPath の入口だけで捕まえる。
#[test]
fn relative_paths_reject_invalid_components_before_normalization() -> TestResult {
    for path in [
        "",
        "/absolute",
        ".",
        "..",
        "dir/../file",
        "dir/./file",
        "dir//file",
        "dir/",
        "nul\0file",
    ] {
        assert!(
            matches!(rel(path), Err(SafeFsError::InvalidPath)),
            "{path:?}"
        );
    }
    let non_utf8 = OsString::from_vec(vec![b'a', 0xff]);
    assert_eq!(rel(Path::new(&non_utf8))?.as_path(), Path::new(&non_utf8));
    assert_eq!(rel("日本語/file")?.as_path(), Path::new("日本語/file"));
    Ok(())
}

// 作成時の関門: 実のリンクと open 系呼び出しの誤りで、根の外を読める退行を捕まえる。
#[test]
fn reads_follow_only_relative_links_confined_to_the_root() -> TestResult {
    let fixture = Fixture::new("read")?;
    fs::create_dir(fixture.root().join("dir"))?;
    fs::write(fixture.root().join("dir/actual.txt"), "inside")?;
    fs::write(fixture.0.join("outside"), "outside")?;
    symlink("dir", fixture.root().join("inside-dir"))?;
    symlink("dir/actual.txt", fixture.root().join("alias.bnt"))?;
    symlink("..", fixture.root().join("outside-dir"))?;
    symlink("../outside", fixture.root().join("outside-file"))?;
    symlink(
        fixture.0.join("outside"),
        fixture.root().join("absolute-outside"),
    )?;
    symlink(
        fixture.root().join("dir/actual.txt"),
        fixture.root().join("absolute-inside"),
    )?;
    let scope = fixture.scope()?;
    for path in ["dir/actual.txt", "inside-dir/actual.txt", "alias.bnt"] {
        let relative = rel(path)?;
        assert_eq!(read(&scope, &relative)?, "inside");
        assert_eq!(
            resolved_path(&scope.open_read(&relative)?)?,
            fs::canonicalize(fixture.root().join("dir/actual.txt"))?
        );
    }
    for path in [
        "outside-dir/outside",
        "outside-file",
        "absolute-outside",
        "absolute-inside",
    ] {
        assert!(
            matches!(scope.open_read(&rel(path)?), Err(SafeFsError::OutsideScope)),
            "{path}"
        );
    }
    assert!(
        matches!(scope.open_read(&rel("missing")?), Err(SafeFsError::NotFound(error)) if error.kind() == io::ErrorKind::NotFound)
    );
    Ok(())
}

// 作成時の関門: /var のような根の外側の別名と、未作成の末尾を誤拒否する退行を捕まえる。
// 最後のリンクを変換で消して書き込みの拒否を迂回しないことも、同じ入口で確かめる。
#[test]
fn request_conversion_normalizes_existing_prefix_but_preserves_final_links() -> TestResult {
    let fixture = Fixture::new("resolve")?;
    symlink(fixture.root(), fixture.0.join("alias"))?;
    fs::write(fixture.root().join("target"), "original")?;
    symlink("target", fixture.root().join("link"))?;
    symlink("../outside", fixture.root().join("dangling"))?;
    fs::write(fixture.0.join("outside-existing"), "outside")?;
    symlink("../outside-existing", fixture.root().join("escape"))?;
    let scope = fixture.scope()?;
    assert_eq!(scope.real_path(), fs::canonicalize(fixture.root())?);
    for path in [
        fixture.root().join("new/deep/file"),
        fixture.0.join("alias/new/deep/file"),
    ] {
        assert_eq!(
            scope.resolve_path(&fixture.root(), &path)?,
            rel("new/deep/file")?
        );
    }
    assert_eq!(
        scope.resolve_path(&fixture.0.join("alias"), Path::new("new/file"))?,
        rel("new/file")?
    );
    for path in ["link", "dangling"] {
        let relative = scope.resolve_path(&fixture.root(), Path::new(path))?;
        assert_eq!(relative, rel(path)?);
        assert!(matches!(
            scope.write(&relative, b"replaced", false),
            Err(SafeFsError::FinalSymlink)
        ));
    }
    assert!(!fixture.0.join("outside").exists());
    assert_eq!(fs::read(fixture.root().join("target"))?, b"original");
    for path in [
        fixture.0.join("outside-existing"),
        fixture.root().join("escape"),
    ] {
        assert!(matches!(
            scope.resolve_path(&fixture.root(), &path),
            Err(SafeFsError::OutsideScope)
        ));
    }
    #[cfg(target_os = "macos")]
    {
        // /var の下には書かず、本物の /var -> /private/var の対応を入口で確かめる。
        let var_scope = ScopeRoot::open("/private/var")?;
        let missing = fixture
            .0
            .file_name()
            .ok_or_else(|| io::Error::from(io::ErrorKind::InvalidInput))?;
        let relative = Path::new(missing).join("missing/file");
        let request = Path::new("/var").join(&relative);
        assert_eq!(
            var_scope.resolve_path(Path::new("/"), &request)?,
            rel(relative)?
        );
    }
    Ok(())
}

// 作成時の関門: 破壊的な書き込みにリンクを通す退行を、先の中身を含めて捕まえる。
#[test]
fn writes_reject_final_symlinks_hard_links_and_escaping_parents() -> TestResult {
    let fixture = Fixture::new("write-links")?;
    fs::write(fixture.root().join("single"), "original")?;
    symlink("single", fixture.root().join("symlink"))?;
    symlink("../not-created", fixture.root().join("dangling"))?;
    fs::hard_link(fixture.root().join("single"), fixture.root().join("hard"))?;
    symlink("..", fixture.root().join("parent"))?;
    let scope = fixture.scope()?;
    for path in ["symlink", "dangling"] {
        assert!(
            matches!(
                scope.write(&rel(path)?, b"new", false),
                Err(SafeFsError::FinalSymlink)
            ),
            "{path}"
        );
    }
    for path in ["single", "hard"] {
        assert!(
            matches!(
                scope.write(&rel(path)?, b"new", false),
                Err(SafeFsError::MultipleHardLinks)
            ),
            "{path}"
        );
    }
    for create_parents in [false, true] {
        assert!(matches!(
            scope.write(&rel("parent/new/file")?, b"new", create_parents),
            Err(SafeFsError::OutsideScope)
        ));
    }
    assert_eq!(fs::read(fixture.root().join("single"))?, b"original");
    assert_eq!(fs::read(fixture.root().join("hard"))?, b"original");
    assert!(!fixture.0.join("not-created").exists());
    assert!(!fixture.0.join("new").exists());
    assert_eq!(scope.read_dir(None)?.len(), 5);
    Ok(())
}

// 作成時の関門: atomic replace の中身、元の権限、新規 0644、親の作成を同じ境界で確認する。
#[test]
fn writes_replace_atomically_preserve_permissions_and_create_parents() -> TestResult {
    let fixture = Fixture::new("write")?;
    fs::write(fixture.root().join("existing"), "old")?;
    fs::set_permissions(
        fixture.root().join("existing"),
        fs::Permissions::from_mode(0o751),
    )?;
    let scope = fixture.scope()?;
    let old_file = scope.open_read(&rel("existing")?)?;
    scope.write(&rel("existing")?, b"new", false)?;
    let metadata = scope.metadata(&rel("existing")?)?;
    assert_eq!(metadata.permissions().mode() & 0o7777, 0o751);
    assert_ne!(metadata.ino(), old_file.metadata()?.ino());
    assert_eq!(read(&scope, &rel("existing")?)?, "new");
    let mut old_contents = String::new();
    (&old_file).read_to_string(&mut old_contents)?;
    assert_eq!(old_contents, "old");
    assert!(matches!(
        scope.write(&rel("new/dir/file")?, b"content", false),
        Err(SafeFsError::NotFound(_))
    ));
    scope.write(&rel("new/dir/file")?, b"content", true)?;
    assert_eq!(read(&scope, &rel("new/dir/file")?)?, "content");
    assert_eq!(
        scope.metadata(&rel("new/dir/file")?)?.permissions().mode() & 0o7777,
        0o644
    );
    // 範囲内の親のリンクは、Linux と macOS のどちらでも辿れる。
    scope.symlink(Path::new("new"), &rel("parent-alias")?)?;
    scope.write(&rel("parent-alias/deeper/file")?, b"linked", true)?;
    assert_eq!(read(&scope, &rel("new/deeper/file")?)?, "linked");
    assert_eq!(
        scope.read_dir(Some(&rel("new/dir")?))?,
        vec![DirectoryEntry {
            name: "file".into(),
            kind: EntryKind::File
        }]
    );
    Ok(())
}

// 作成時の関門: P1 が実の親で場所を判定した後、同じ親へ書けることを確かめる。
// 親を開き直す退行は、外を指すリンクへの差し替えで失敗する。テスト用の公開は増やさない。
#[test]
fn opened_directory_keeps_the_parent_for_checked_writes() -> TestResult {
    let fixture = Fixture::new("parent-handle")?;
    fs::create_dir(fixture.root().join("actual"))?;
    fs::create_dir(fixture.0.join("outside"))?;
    symlink("actual", fixture.root().join("alias"))?;
    let scope = fixture.scope()?;
    let parent = scope.open_dir(&rel("alias")?)?;
    assert_eq!(
        parent.real_path(),
        fs::canonicalize(fixture.root().join("actual"))?
    );
    fs::remove_file(fixture.root().join("alias"))?;
    symlink("../outside", fixture.root().join("alias"))?;
    parent.write(&rel("file")?, b"inside", false)?;
    assert_eq!(fs::read(fixture.root().join("actual/file"))?, b"inside");
    assert!(!fixture.0.join("outside/file").exists());
    assert!(matches!(
        scope.open_dir(&rel("alias")?),
        Err(SafeFsError::OutsideScope)
    ));
    let names = vec![OsString::from("日本語")];
    // macOS の FS は不正な UTF-8 の名前を拒むため、実際の操作は Linux で確かめる。
    #[cfg(target_os = "linux")]
    let names = {
        let mut names = names;
        names.push(OsString::from_vec(vec![b'f', 0xff]));
        names
    };
    for name in names {
        let relative = rel(Path::new(&name))?;
        parent
            .write(&relative, b"bytes", false)
            .map_err(|error| format!("write file {name:?}: {error}"))?;
        assert_eq!(
            read(&parent, &relative).map_err(|error| format!("read file {name:?}: {error}"))?,
            "bytes"
        );
        assert!(
            parent
                .read_dir(None)
                .map_err(|error| format!("list directory containing {name:?}: {error}"))?
                .iter()
                .any(|entry| entry.name == name && entry.kind == EntryKind::File)
        );
        parent
            .remove_file(&relative)
            .map_err(|error| format!("remove file {name:?}: {error}"))?;
    }
    Ok(())
}

// 読み取り元での現実の失敗を起こす。safe_fs の内部には差し込み口を作らない。
struct FailingSource(bool);

impl Read for FailingSource {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        if self.0 {
            return Err(io::Error::from(io::ErrorKind::BrokenPipe));
        }
        let Some(first) = buffer.first_mut() else {
            return Ok(0);
        };
        *first = b'x';
        self.0 = true;
        Ok(1)
    }
}

// 作成時の関門: TRUNC で元を壊す、失敗時に一時ファイルを残す退行を捕まえる。
#[test]
fn failed_streaming_write_preserves_original_and_removes_temporary_file() -> TestResult {
    let fixture = Fixture::new("failure")?;
    fs::write(fixture.root().join("existing"), "original")?;
    let scope = fixture.scope()?;
    let before = scope.metadata(&rel("existing")?)?;
    for path in ["existing", "new"] {
        let error = scope.write_from(&rel(path)?, &mut FailingSource(false), false);
        assert!(
            matches!(error, Err(SafeFsError::Io(error)) if error.kind() == io::ErrorKind::BrokenPipe)
        );
    }
    assert_eq!(scope.metadata(&rel("existing")?)?.ino(), before.ino());
    assert_eq!(read(&scope, &rel("existing")?)?, "original");
    assert_eq!(
        scope.read_dir(None)?,
        vec![DirectoryEntry {
            name: "existing".into(),
            kind: EntryKind::File
        }]
    );
    Ok(())
}

// 作成時の関門: 記録・巻き戻しの操作でリンク先を触る退行を、公開操作と FS の結果で捕まえる。
#[test]
fn metadata_listing_link_operations_and_deletion_do_not_follow_final_links() -> TestResult {
    let fixture = Fixture::new("entries")?;
    fs::write(fixture.0.join("outside"), "keep")?;
    fs::create_dir(fixture.0.join("outside-dir"))?;
    fs::write(fixture.0.join("outside-dir/file"), "keep-directory")?;
    let scope = fixture.scope()?;
    scope.create_dir(&rel("dir")?)?;
    scope.write(&rel("dir/file")?, b"inside", false)?;
    scope.symlink(Path::new("../outside"), &rel("link")?)?;
    scope.symlink(Path::new("../outside-dir"), &rel("dir-link")?)?;
    scope.symlink(Path::new("../missing"), &rel("dangling")?)?;
    for path in ["link", "dir-link", "dangling"] {
        assert!(scope.metadata(&rel(path)?)?.file_type().is_symlink());
    }
    let metadata = scope.metadata(&rel("dir/file")?)?;
    let actual = fs::metadata(fixture.root().join("dir/file"))?;
    assert_eq!(
        (
            metadata.dev(),
            metadata.ino(),
            metadata.size(),
            metadata.nlink(),
            metadata.mode(),
            metadata.mtime(),
            metadata.mtime_nsec(),
            metadata.ctime(),
            metadata.ctime_nsec()
        ),
        (
            actual.dev(),
            actual.ino(),
            actual.size(),
            actual.nlink(),
            actual.mode(),
            actual.mtime(),
            actual.mtime_nsec(),
            actual.ctime(),
            actual.ctime_nsec()
        )
    );
    assert_eq!(scope.read_link(&rel("link")?)?, Path::new("../outside"));
    assert_eq!(
        scope.read_dir(None)?,
        vec![
            DirectoryEntry {
                name: "dangling".into(),
                kind: EntryKind::Symlink
            },
            DirectoryEntry {
                name: "dir".into(),
                kind: EntryKind::Directory
            },
            DirectoryEntry {
                name: "dir-link".into(),
                kind: EntryKind::Symlink
            },
            DirectoryEntry {
                name: "link".into(),
                kind: EntryKind::Symlink
            },
        ]
    );
    assert!(
        matches!(scope.create_dir(&rel("dir")?), Err(SafeFsError::AlreadyExists(error)) if error.kind() == io::ErrorKind::AlreadyExists)
    );
    assert!(matches!(
        scope.symlink(Path::new("dir"), &rel("link")?),
        Err(SafeFsError::AlreadyExists(_))
    ));
    assert!(scope.read_dir(Some(&rel("dir-link")?)).is_err());
    assert!(scope.remove_dir(&rel("dir-link")?).is_err());
    assert!(scope.remove_dir(&rel("dir")?).is_err());
    scope.remove_file(&rel("link")?)?;
    scope.remove_file(&rel("dir-link")?)?;
    scope.remove_file(&rel("dangling")?)?;
    assert_eq!(fs::read(fixture.0.join("outside"))?, b"keep");
    assert_eq!(
        fs::read(fixture.0.join("outside-dir/file"))?,
        b"keep-directory"
    );
    scope.remove_file(&rel("dir/file")?)?;
    scope.remove_dir(&rel("dir")?)?;
    assert!(scope.read_dir(None)?.is_empty());
    assert!(matches!(
        scope.remove_file(&rel("missing")?),
        Err(SafeFsError::NotFound(_))
    ));
    Ok(())
}

// 作成時の関門: FIFO の相手を待つ退行を、safe_fs の公開操作で時間内の完了として検出する。
// exec 側だけでは write_target の待ちを捕まえられない。内部の関数の公開は増やさない
// （設計書 01-03「道具の定義の共通の規則」「書く道具のパスの規則」）。
fn check_fifo_without_waiting(write: bool) -> TestResult {
    use std::os::unix::fs::FileTypeExt;
    use std::sync::mpsc;
    use std::thread;
    use std::time::Duration;

    let fixture = Fixture::new(if write { "fifo-write" } else { "fifo-metadata" })?;
    let path = fixture.root().join("pipe");
    #[cfg(target_os = "linux")]
    rustix::fs::mkfifoat(rustix::fs::CWD, &path, rustix::fs::Mode::RWXU)?;
    #[cfg(target_os = "macos")]
    {
        // rustix 1.1.5 は Apple 向けに mkfifoat を公開しないため、OS の道具で作る。
        assert!(
            std::process::Command::new("mkfifo")
                .arg(&path)
                .current_dir(&fixture.0)
                .status()?
                .success()
        );
    }
    let scope = fixture.scope()?;
    let relative = rel("pipe")?;
    let (sender, receiver) = mpsc::channel();
    let worker = thread::spawn(move || {
        let result = if write {
            match scope.write(&relative, b"must not replace FIFO", false) {
                Err(SafeFsError::NotRegularFile) => Ok(()),
                other => Err(format!("Expected NotRegularFile, got {other:?}")),
            }
        } else {
            scope
                .metadata(&relative)
                .map_err(|error| error.to_string())
                .and_then(|metadata| {
                    if metadata.file_type().is_fifo() {
                        Ok(())
                    } else {
                        Err("Expected FIFO metadata".to_owned())
                    }
                })
        };
        let _sent = sender.send(result);
    });
    // scoped thread の終了待ちは使わず、待った場合もテストを時間内に失敗させる。
    // 相手を開いて待ちを解消すると、NONBLOCK がなくても通ってしまうため開かない。
    let result = receiver.recv_timeout(Duration::from_secs(5))?;
    worker
        .join()
        .map_err(|_| io::Error::other("FIFO worker panicked"))?;
    result.map_err(io::Error::other)?;
    assert!(fs::symlink_metadata(&path)?.file_type().is_fifo());
    Ok(())
}

#[test]
fn fifo_metadata_returns_without_waiting_for_a_peer() -> TestResult {
    check_fifo_without_waiting(false)
}

#[test]
fn fifo_write_rejection_returns_without_waiting_for_a_peer() -> TestResult {
    check_fifo_without_waiting(true)
}
