//! 作成の確認とパスの安全を、道具の入口とファイルシステムで確かめる（設計書 01-03、03-02）。
#[allow(dead_code)]
mod support;
use benitoiteh::approval::ApprovalKind;
use benitoiteh::config::{Mode, ReadConfig, paths::DataPaths};
use benitoiteh::safe_fs::ScopeRoot;
use benitoiteh::session::SessionId;
use benitoiteh::tools::{
    ApprovalStatus, Invocation, ToolContext, ToolOutput, ToolSet, exclude::ExcludeRules,
};
use serde_json::json;
use std::fs;
#[cfg(target_os = "linux")]
use std::os::unix::ffi::OsStringExt;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt, symlink};
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::time::SystemTime;
type TestResult = Result<(), Box<dyn std::error::Error>>;
struct Fixture {
    root: PathBuf,
    paths: DataPaths,
    context: ToolContext,
}
impl Fixture {
    fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let root = std::env::temp_dir().join(format!(
            "bh-create-{}",
            SessionId::generate(SystemTime::now())?
        ));
        fs::create_dir(&root)?;
        let root = root.canonicalize()?;
        for name in ["home", "project", "temporary", "spill", "config", "data"] {
            fs::create_dir(root.join(name))?;
        }
        let paths = DataPaths::from_values(
            Some(&root.join("home")),
            Some(&root.join("config")),
            Some(&root.join("data")),
            Some(&root.join("temporary")),
        )?;
        let context = ToolContext::new(
            Mode::Restricted,
            ScopeRoot::open(root.join("project"))?,
            ScopeRoot::open(root.join("temporary"))?,
            ScopeRoot::open(root.join("spill"))?,
            ExcludeRules::new(
                &root.join("project"),
                &paths,
                &root.join("spill"),
                &ReadConfig::default(),
                &ReadConfig::default(),
            )?,
        )
        .with_project_creation(
            paths.clone(),
            fs::canonicalize(env!("CARGO_BIN_EXE_benitoiteh"))?,
        );
        Ok(Self {
            root,
            paths,
            context,
        })
    }
    fn call(&self, path: &str, approved: bool, id: &str) -> ToolOutput {
        // 処理系がないときにも残る道具を、本番と同じ定義から使う。
        let tools = ToolSet::without_benitoite(Mode::Restricted);
        let call = tools.validate("create_project", json!({"path":path}));
        match call {
            Ok(call) => tools.execute_validated(
                &self.context,
                &call,
                if approved {
                    ApprovalStatus::Approved
                } else {
                    ApprovalStatus::Unapproved
                },
                Invocation {
                    call_id: id,
                    cancelled: &AtomicBool::new(false),
                    confirmed_effects: &Default::default(),
                    session_domains: &Default::default(),
                },
            ),
            Err(error) => ToolOutput::error(error.to_string()),
        }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _removed = fs::remove_dir_all(&self.root);
    }
}

// 関門: 新しい道具の作成境界。未確認の作成、権限・雛形の誤りは既存の write のテストでは届かない。
#[test]
fn approval_is_required_and_template_runs_with_the_real_processor() -> TestResult {
    let f = Fixture::new()?;
    let path = f.root.join("created");
    let path = path.to_str().ok_or("path")?;
    assert!(f.call(path, true, "forged").is_error);
    assert!(!PathBuf::from(path).exists());
    let out = f.call(path, false, "one");
    let request = out.approval.ok_or("approval")?;
    assert_eq!(request.kind, ApprovalKind::CreateProject);
    assert_eq!(request.facts.path, Some(path.into()));
    assert!(!PathBuf::from(path).exists());
    let out = f.call(path, true, "one");
    assert!(!out.is_error && out.processor_launches == 0, "{out:?}");
    assert!(out.text.contains(path) && out.text.contains("previous project"));
    let main = PathBuf::from(path).join("main.bnt");
    assert_eq!(
        fs::read_to_string(&main)?,
        "import Benitoite.Unofficial.IO.Console\n\nfunction main() -> Unit uses Console.Write\n  Console.writeLine(\"hello, world.\")\nend function\n"
    );
    let control_dir = f.root.join("mode-directory");
    fs::DirBuilder::new().mode(0o755).create(&control_dir)?;
    let control_file = f.root.join("mode-file");
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o644)
        .open(&control_file)?;
    for (actual, control) in [
        (PathBuf::from(path), control_dir),
        (main.clone(), control_file),
    ] {
        assert_eq!(
            fs::metadata(actual)?.permissions().mode() & 0o777,
            fs::metadata(control)?.permissions().mode() & 0o777
        );
    }
    let processor = support::benitoite()?;
    let check = std::process::Command::new(&processor)
        .arg("check")
        .arg(&main)
        .current_dir(path)
        .output()?;
    assert!(
        check.status.success(),
        "{}",
        String::from_utf8_lossy(&check.stderr)
    );
    let run = std::process::Command::new(&processor)
        .args(["run", "--allow-effects=Console.Write"])
        .arg(&main)
        .current_dir(path)
        .output()?;
    assert!(
        run.status.success(),
        "{}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(run.stdout, b"hello, world.\n");
    assert!(
        ToolSet::new(Mode::Normal)
            .validate("create_project", json!({"path":path}))
            .is_err()
    );
    Ok(())
}

// 関門: 新しい根を作る操作には、write の拡張子・現在の範囲の検査と異なる規則が要る。
#[test]
fn invalid_existing_and_protected_destinations_never_create_anything() -> TestResult {
    let f = Fixture::new()?;
    fs::create_dir(f.root.join("existing"))?;
    fs::write(f.root.join("file"), "keep")?;
    symlink(f.root.join("existing"), f.root.join("link"))?;
    symlink(f.root.join("absent"), f.root.join("dangling"))?;
    let mut paths = vec![
        "existing".to_owned(),
        "file".into(),
        "link".into(),
        "dangling".into(),
        "missing/child".into(),
        "file/child".into(),
    ];
    for name in [".git", ".GIT", ".benitoiteh", ".BeNiToItEh"] {
        fs::create_dir_all(f.root.join(name))?;
        fs::write(f.root.join(name).join("existing"), "keep")?;
        paths.extend([format!("{name}/child"), format!("{name}/existing")]);
    }
    paths.extend([
        f.paths.home.to_string_lossy().into_owned(),
        "home/.config/new".into(),
        "home/.ssh/id_ed25519".into(),
        "home/.ssh/absent".into(),
        "data/benitoiteh/new".into(),
        "config/benitoiteh/new".into(),
        "temporary/new".into(),
    ]);
    fs::create_dir(f.paths.home.join(".config"))?;
    fs::create_dir(f.paths.home.join(".ssh"))?;
    fs::write(f.paths.home.join(".ssh/id_ed25519"), "secret")?;
    fs::create_dir_all(&f.paths.data_dir)?;
    fs::create_dir_all(&f.paths.config_dir)?;
    // 同じ要求の字面でも、リンク先の保護規則・存在・種類を誤りから区別できない。
    symlink(f.paths.home.join(".ssh"), f.root.join("alias"))?;
    paths.extend(["alias/id_ed25519".into(), "alias/absent".into()]);
    for suffix in paths {
        let path = if suffix.starts_with('/') {
            PathBuf::from(suffix)
        } else {
            f.root.join(suffix)
        };
        let before = path.exists();
        let requested = path.to_str().ok_or("path")?;
        let out = f.call(requested, false, "invalid");
        assert!(out.is_error && out.approval.is_none(), "{path:?}: {out:?}");
        assert_eq!(
            out.text,
            format!(
                "The project cannot be created at {}: the parent directory must exist and the location must be permitted. Choose another destination, or stop and report.",
                json!(requested)
            )
        );
        assert_eq!(path.exists(), before);
    }
    let out = f.call("missing\"\u{202e}/new", false, "escaped");
    assert_eq!(
        out.text,
        "The project cannot be created at \"missing\\\"\\u202e/new\": the parent directory must exist and the location must be permitted. Choose another destination, or stop and report."
    );
    for requested in [
        "",
        ".",
        "..",
        "a//b",
        "./new",
        "../new",
        "new/",
        "new\nname",
        "new\tname",
        "~/new",
        "~user/new",
    ] {
        let out = f.call(requested, false, "invalid");
        assert!(
            out.is_error && out.approval.is_none(),
            "{requested:?}: {out:?}"
        );
        assert!(
            out.text.contains(if requested.starts_with('~') {
                "do not expand ~"
            } else {
                "Invalid project path"
            }),
            "{out:?}"
        );
    }
    // macOS の FS は不正な UTF-8 の名前自体を拒む。実パスの拒否は Linux で確かめる。
    #[cfg(target_os = "linux")]
    {
        let raw = f.root.join(std::ffi::OsString::from_vec(vec![b'n', 0xff]));
        fs::create_dir(&raw)?;
        symlink(&raw, f.root.join("utf8-alias"))?;
        let out = f.call(
            f.root.join("utf8-alias/new").to_str().ok_or("alias")?,
            false,
            "utf8",
        );
        assert!(
            out.is_error
                && out.text
                    == format!(
                        "The project cannot be created at {}: the parent directory must exist and the location must be permitted. Choose another destination, or stop and report.",
                        json!(f.root.join("utf8-alias/new"))
                    ),
            "{out:?}"
        );
        assert!(!raw.join("new").exists());
    }
    assert_eq!(fs::read_to_string(f.root.join("file"))?, "keep");
    Ok(())
}

// 関門: call_id だけの承認、親リンクの差し替え、同じ実パスの別ディレクトリへの再作成を拒む。
#[test]
fn approved_retries_reject_changed_arguments_and_replaced_parents() -> TestResult {
    for change in ["arguments", "symlink", "identity", "occupied"] {
        let f = Fixture::new()?;
        fs::create_dir(f.root.join("parent"))?;
        fs::create_dir(f.root.join("other"))?;
        symlink(f.root.join("parent"), f.root.join("alias"))?;
        let requested = f.root.join("alias/new");
        assert!(
            f.call(requested.to_str().ok_or("path")?, false, "one")
                .approval
                .is_some()
        );
        let mut retry = requested.clone();
        match change {
            "arguments" => retry = f.root.join("alias/different"),
            "symlink" => {
                fs::remove_file(f.root.join("alias"))?;
                symlink(f.root.join("other"), f.root.join("alias"))?;
            }
            "identity" => {
                fs::rename(f.root.join("parent"), f.root.join("moved"))?;
                fs::create_dir(f.root.join("parent"))?;
            }
            "occupied" => {
                symlink(f.root.join("other"), f.root.join("parent/new"))?;
            }
            _ => return Err("change".into()),
        }
        let out = f.call(retry.to_str().ok_or("retry")?, true, "one");
        assert!(out.is_error, "{change}: {out:?}");
        if change == "occupied" {
            assert_eq!(
                out.text,
                "The project destination already exists (possibly as a symbolic link). Choose a new directory name."
            );
        }
        assert!(!f.root.join("other/main.bnt").exists());
        assert!(!f.root.join("parent/new/main.bnt").exists());
        assert!(!f.root.join("parent/different").exists());
    }
    Ok(())
}
