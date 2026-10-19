//! 検査の道具、書いた後の検査、診断による読み取りの安全の契約（設計書 01-03、04-01）。

#[allow(dead_code)]
mod support;
use benitoiteh::benitoite::{BenitoiteError, StderrLimit};
use benitoiteh::config::{Mode, ReadConfig, paths::DataPaths};
use benitoiteh::safe_fs::ScopeRoot;
use benitoiteh::session::SessionId;
use benitoiteh::tools::{CheckLaunch, ToolContext, ToolOutput, ToolSet, exclude::ExcludeRules};
use serde_json::{Value, json};
use std::error::Error;
use std::fs;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, SystemTime};

type TestResult = Result<(), Box<dyn Error>>;
const GOOD: &str = "function main() -> Unit\n  return ()\nend function\n";
const BAD: &str = "function main() -> Unit\n  return UNKNOWN_NAME\nend function\n";
const WARNING: &str =
    "function main() -> Unit\n  bind unused <- 1 div 0\n  return ()\nend function\n";
struct Fixture {
    root: PathBuf,
    project: PathBuf,
    temporary: PathBuf,
    context: ToolContext,
    tools: ToolSet,
}
impl Fixture {
    fn new(mode: Mode) -> Result<Self, Box<dyn Error>> {
        let root = std::env::temp_dir().join(format!(
            "bh-check-{}",
            SessionId::generate(SystemTime::now())?
        ));
        fs::create_dir(&root)?;
        let root = fs::canonicalize(root)?;
        let project = root.join("project");
        let temporary = root.join("temporary");
        let paths = DataPaths::from_values(Some(&root), None, None, Some(&root))?;
        let spill = paths.data_dir.join("sessions/current/spill");
        for path in [&project, &temporary, &spill] {
            fs::create_dir_all(path)?;
        }
        let context = ToolContext::new(
            mode,
            ScopeRoot::open(&project)?,
            ScopeRoot::open(&temporary)?,
            ScopeRoot::open(&spill)?,
            ExcludeRules::new(
                &project,
                &paths,
                &spill,
                &ReadConfig::default(),
                &ReadConfig::default(),
            )?,
        )
        .with_check_query(support::checks(project.clone())?);
        Ok(Self {
            root,
            project,
            temporary,
            context,
            tools: ToolSet::new(mode),
        })
    }
    fn call(&self, tool: &str, args: Value) -> ToolOutput {
        self.tools.execute(&self.context, tool, args)
    }
    fn check(&self, path: &str, tests: bool) -> ToolOutput {
        self.call("benitoite_check", json!({"path":path,"tests":tests}))
    }
    fn write(&self, path: &str, content: &str) -> ToolOutput {
        self.call("write", json!({"path":path,"content":content}))
    }
    fn read(&self, path: &str) {
        let output = self.call("read", json!({"path":path}));
        assert!(!output.is_error, "{}", output.text);
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _removed = fs::remove_dir_all(&self.root);
    }
}

// 関門: Skill の検査の診断を、読める実のファイルの行と絶対パスで表示する。
#[test]
fn loaded_skill_scripts_and_directories_can_be_checked_with_absolute_diagnostics() -> TestResult {
    let f = Fixture::new(Mode::Restricted)?;
    let directory = f.root.join(".agents/skills/sample");
    fs::create_dir_all(&directory)?;
    fs::write(
        directory.join("SKILL.md"),
        "---\nname: sample\ndescription: test\n---\n",
    )?;
    fs::write(directory.join("main.bnt"), GOOD)?;
    let (_, roots) = benitoiteh::context::skills::collect(
        &f.project,
        &f.root,
        &benitoiteh::config::UserSkillsConfig::default(),
        &f.context.exclude,
    );
    let context = ToolContext::new(
        Mode::Restricted,
        ScopeRoot::open(&f.project)?,
        ScopeRoot::open(&f.temporary)?,
        ScopeRoot::open(
            f.root
                .join(".local/state/benitoiteh/sessions/current/spill"),
        )?,
        f.context.exclude.clone(),
    )
    .with_skill_roots(roots)
    .with_check_query(support::checks(f.project.clone())?);
    for (path, tests) in [
        (directory.join("main.bnt"), false),
        (directory.clone(), true),
    ] {
        let output = f.tools.execute(
            &context,
            "benitoite_check",
            json!({"path":path,"tests":tests}),
        );
        assert!(!output.is_error && output.text == "ok", "{output:?}");
    }
    fs::write(directory.join("main.bnt"), BAD)?;
    let output = f.tools.execute(
        &context,
        "benitoite_check",
        json!({"path":directory.join("main.bnt")}),
    );
    assert!(
        output.is_error && output.text.contains("UNKNOWN_NAME"),
        "{output:?}"
    );
    assert!(
        output
            .text
            .contains(directory.join("main.bnt").to_str().ok_or("path")?)
    );
    Ok(())
}

// 関門: 実際の処理系の引数と診断を受け渡す契約。模擬の JSON だけでは、
// check と test --check の取り違え、main の要否、警告の扱いを検出できない。
#[test]
fn real_checks_classify_results_and_check_tests_without_running_them() -> TestResult {
    let f = Fixture::new(Mode::Restricted)?;
    for (name, contents, error, expected) in [
        ("-good.bnt", GOOD, false, "ok"),
        ("bad.bnt", BAD, true, "error[E0301]"),
        ("warning.bnt", WARNING, false, "warning["),
    ] {
        fs::write(f.project.join(name), contents)?;
        let output = f.check(name, false);
        assert_eq!(output.is_error, error, "{}", output.text);
        assert!(output.text.starts_with(expected), "{}", output.text);
        assert_eq!(output.check_launch, Some(CheckLaunch::Tool));
        assert_eq!(
            output
                .check
                .as_ref()
                .map(|check| (&check.key, check.failed)),
            Some((&f.project.join(name), error))
        );
    }
    fs::create_dir(f.project.join("empty"))?;
    let output = f.check("empty", false);
    assert!(output.is_error && output.check.is_none());
    assert!(
        output
            .text
            .starts_with("Could not check empty with the Benitoite processor: error: directory"),
        "{}",
        output.text
    );
    assert_eq!(f.check("empty", true).text, "ok");
    fs::create_dir(f.project.join("tests"))?;
    let marker = f.temporary.join("test-was-run");
    let script = format!(
        "import Benitoite.Unofficial.IO.File\n@test(\"must not run\")\nfunction example() -> Unit uses File.Write\n  bind _ <- File.writeText(path: {}, text: \"ran\")\n  return ()\nend function\n",
        json!(marker)
    );
    fs::write(f.project.join("tests/example.bnt"), script)?;
    for path in ["tests/example.bnt", "tests"] {
        let output = f.check(path, true);
        assert_eq!(output.text, "ok");
        assert!(!output.is_error);
        assert!(!marker.exists());
    }
    if rustix::process::geteuid().is_root() {
        eprintln!(
            "Skipped unreadable-directory discovery check: root can read mode 000 directories."
        );
    } else {
        let mut permissions = fs::metadata(f.project.join("empty"))?.permissions();
        permissions.set_mode(0o0);
        fs::set_permissions(f.project.join("empty"), permissions)?;
        let output = f.check(".", true);
        fs::set_permissions(f.project.join("empty"), fs::Permissions::from_mode(0o700))?;
        assert!(
            output.is_error && output.text.contains("error[E0101]"),
            "{}",
            output.text
        );
        assert!(output.check.is_some_and(|check| check.failed));
    }
    Ok(())
}

// 関門: BH-029 の架空のパスの重複と、.env の message の漏れを本物の処理系で守る。
#[test]
fn real_diagnostics_hide_secrets_and_deduplicate_imported_modules_before_counting() -> TestResult {
    let f = Fixture::new(Mode::Restricted)?;
    fs::create_dir(f.project.join("Lib"))?;
    let other = "public function value() -> Integer\n  return MISSING_VALUE\nend function\n";
    fs::write(f.project.join("Lib/Other.bnt"), other)?;
    fs::write(
        f.project.join("Lib/Text.bnt"),
        "import Lib.Other\npublic function value() -> Integer\n  return Other.value()\nend function\n",
    )?;
    let output = f.check(".", true);
    assert!(output.is_error, "{}", output.text);
    assert_eq!(
        output.text.matches("error[E0301]").count(),
        1,
        "{}",
        output.text
    );
    for hidden in ["Lib/Lib", "does not read", "other files", "another file"] {
        assert!(!output.text.contains(hidden), "{}", output.text);
    }
    f.read("Lib/Other.bnt");
    let output = f.write("Lib/Other.bnt", other);
    assert!(
        !output.is_error
            && output
                .text
                .contains("Check of Lib/Other.bnt after this change:\n")
    );
    assert_eq!(
        output.text.matches("error[E0301]").count(),
        1,
        "{}",
        output.text
    );
    for hidden in ["Lib/Lib", "does not read", "other files", "another file"] {
        assert!(!output.text.contains(hidden), "{}", output.text);
    }
    f.read("Lib/Other.bnt");
    let fixed = "public function value() -> Integer\n  return 1\nend function\n";
    assert!(
        f.write("Lib/Other.bnt", fixed)
            .text
            .ends_with("Check of Lib/Other.bnt after this change: ok")
    );
    fs::write(
        f.project.join(".env.bnt"),
        "function secret() -> Unit\n  return SECRET_TOKEN_abc\nend function\n",
    )?;
    let output = f.check(".", true);
    assert!(output.is_error);
    assert_eq!(
        output.text,
        "1 diagnostic in a file that benitoiteh does not read is not shown."
    );
    let output = f.write("main.bnt", GOOD);
    assert_eq!(
        output.text,
        "Wrote main.bnt (3 lines).\nCheck of main.bnt after this change: ok\n1 diagnostic in a file that benitoiteh does not read is not shown."
    );
    assert!(!output.is_error && output.check.is_some_and(|check| !check.failed));
    let output = f.write(".env.other.bnt", BAD);
    assert!(!output.is_error);
    assert!(output.text.contains("Check of .env.other.bnt after this change:\n2 diagnostics in files that benitoiteh does not read are not shown."), "{}", output.text);
    assert!(!output.text.contains("UNKNOWN_NAME"));
    Ok(())
}

// 関門: 書く操作の成功に検査の is_error を移す退行、下位のパッケージを検査済みと
// 示す退行、edit だけ自動検査を忘れる退行を、書く道具の境界で守る。
#[test]
fn after_write_filters_diagnostics_and_skips_nested_packages_temporary_and_normal_mode()
-> TestResult {
    let f = Fixture::new(Mode::Restricted)?;
    fs::write(f.project.join("other.bnt"), BAD)?;
    let output = f.write("main.bnt", BAD);
    assert!(!output.is_error && output.text.starts_with("Wrote main.bnt (3 lines).\nCheck of main.bnt after this change:\nerror[E0301] main.bnt:2:10:"), "{}", output.text);
    assert!(output.text.ends_with("1 diagnostic in another file. Use benitoite_check with tests=true on the project directory to see it."));
    assert!(output.check.is_some_and(|check| check.failed));
    fs::write(f.project.join("second.bnt"), BAD)?;
    f.read("main.bnt");
    let output = f.call(
        "edit",
        json!({"path":"main.bnt","old_string":"UNKNOWN_NAME","new_string":"()"}),
    );
    assert!(
        !output.is_error
            && output
                .text
                .contains("Check of main.bnt after this change: ok\n2 diagnostics in other files."),
        "{}",
        output.text
    );
    assert_eq!(output.check_launch, Some(CheckLaunch::AfterWrite));
    assert!(output.check.is_some_and(|check| !check.failed));
    let output = f.write("warning.bnt", WARNING);
    assert!(!output.is_error && output.text.contains("warning["));
    assert!(output.check.is_some_and(|check| !check.failed));
    fs::create_dir_all(f.project.join("nested/deep"))?;
    fs::write(f.project.join("nested/package.bnt"), "package ()\n")?;
    let output = f.write("nested/deep/new.bnt", BAD);
    assert_eq!(
        output.text,
        "Wrote nested/deep/new.bnt (3 lines).\nNot checked automatically: nested/deep/new.bnt is in a nested package."
    );
    assert!(output.check.is_none() && output.check_launch.is_none());
    let path = f
        .temporary
        .join("scratch.bnt")
        .to_string_lossy()
        .into_owned();
    let output = f.write(&path, BAD);
    assert!(output.text.ends_with(&format!(
        "Not checked automatically: {path} is outside the project directory."
    )));
    assert!(output.check.is_none() && output.check_launch.is_none());
    let normal = Fixture::new(Mode::Normal)?;
    let output = normal.write("main.bnt", BAD);
    assert_eq!(output.text, "Wrote main.bnt (3 lines).");
    assert!(output.check.is_none() && output.check_launch.is_none());
    Ok(())
}

fn fake_diagnostic(file: &str, line: u64, start: u64, end: u64) -> Value {
    json!({"kind":"check","severity":"error","code":"E0301","message":"SECRET_MESSAGE", "primary":{"file":file,"start":{"line":line,"column":start,"offset":0},"end":{"line":line,"column":end,"offset":0},"label":"SECRET_LABEL"},"secondary":[],"notes":[],"helps":[]})
}
fn fake_query(stderr: Vec<u8>) -> Arc<benitoiteh::benitoite::CheckQuery> {
    Arc::new(move |_, _, _| {
        Err(BenitoiteError::Exit {
            stdout: Vec::new(),
            code: Some(2),
            stderr: stderr.clone(),
        })
    })
}

// 関門: 実の除外規則を通さなければ、後ろの部分と祖先の照合の増大を検出できない。
// 既存の長いパスの検査と同じ別プロセスの期限で、new と render の共有予算も守る。
#[test]
fn adversarial_diagnostics_finish_within_ten_seconds() -> TestResult {
    use std::sync::atomic::AtomicBool;
    use std::time::Instant;
    const CHILD: &str = "BH_DIAGNOSTIC_STRESS_CHILD";
    const READY: &str = "BH_DIAGNOSTIC_STRESS_READY";
    if std::env::var_os(CHILD).is_some() {
        let mut f = Fixture::new(Mode::Restricted)?;
        fs::write(f.project.join("main.bnt"), GOOD)?;
        let parent = "a/".repeat(31);
        fs::create_dir_all(f.project.join(&parent))?;
        for i in 0..1000 {
            fs::write(f.project.join(format!("{parent}file-{i}.bnt")), GOOD)?;
        }
        // ファイルの準備の速度を、診断の後処理の十秒に含めない。
        let processing_started = Instant::now();
        fs::write(std::env::var_os(READY).ok_or("ready path")?, b"ready")?;
        let paths = DataPaths::from_values(Some(&f.root), None, None, Some(&f.root))?;
        f.context.exclude = ExcludeRules::new(
            &f.project,
            &paths,
            f.context.spill.real_path(),
            &ReadConfig {
                exclude: vec!["**/node_modules".into(), "*.secret".into()],
            },
            &ReadConfig::default(),
        )?;
        let stop = AtomicBool::new(false);
        use benitoiteh::benitoite::diagnostics::{PreparedDiagnostics, parse};
        let path = "a/".repeat(40_000) + "a.bnt";
        let line = fake_diagnostic(&path, 1, 1, 2).to_string() + "\n";
        let count = (16_usize * 1024 * 1024) / line.len();
        let mut bytes = line.repeat(count).into_bytes();
        bytes.resize(16 * 1024 * 1024, b' ');
        let rendered = PreparedDiagnostics::new(parse(&bytes).diagnostics, &f.context, &stop)
            .render(&f.context, None, &stop);
        assert_eq!(rendered.unread, count);
        // 以前の 4096 バイト・256 要素の上限には収まる、千件の異なるパス。
        let lines = (0..1000)
            .map(|i| {
                fake_diagnostic(&("a/".repeat(254) + &format!("file-{i}.bnt")), 1, 1, 2).to_string()
            })
            .collect::<Vec<_>>()
            .join("\n");
        let rendered =
            PreparedDiagnostics::new(parse(lines.as_bytes()).diagnostics, &f.context, &stop)
                .render(&f.context, None, &stop);
        assert_eq!(rendered.unread, 1000);
        // 三十二要素の読める位置を千件解決すると予算が尽きる。次の呼び出しへは持ち越さない。
        let mut primary_lines = Vec::new();
        for i in 0..1000 {
            let path = format!("{parent}file-{i}.bnt");
            let mut diagnostic = fake_diagnostic(&path, 1, 1, 2);
            *diagnostic.get_mut("message").ok_or("message")? = json!(format!("primary-{i}"));
            primary_lines.push(diagnostic.to_string());
        }
        let rendered = PreparedDiagnostics::new(
            parse(primary_lines.join("\n").as_bytes()).diagnostics,
            &f.context,
            &stop,
        )
        .render(&f.context, None, &stop);
        assert!(
            rendered.unread > 0 && rendered.unread < 1000,
            "{rendered:?}"
        );
        let primary_unread = rendered.unread;
        // primary がない二十件から、補助位置と置き換えで計八百の異なるパスを解決する。
        let mut lines = Vec::new();
        for i in 0..20 {
            let mut value = fake_diagnostic("main.bnt", 1, 1, 2);
            *value.get_mut("primary").ok_or("primary")? = Value::Null;
            *value.get_mut("message").ok_or("message")? = json!(format!("diagnostic-{i}"));
            let mut secondary = Vec::new();
            let mut edits = Vec::new();
            for j in 0..40 {
                let path = format!("{parent}aux-{i}-{j}.bnt");
                let span = fake_diagnostic(&path, 1, 1, 2)
                    .get("primary")
                    .ok_or("primary")?
                    .clone();
                if j < 20 {
                    secondary.push(span);
                } else {
                    let mut edit = span;
                    edit.as_object_mut()
                        .ok_or("edit")?
                        .insert("replacement".into(), json!("x"));
                    edits.push(edit);
                }
            }
            *value.get_mut("secondary").ok_or("secondary")? = json!(secondary);
            *value.get_mut("helps").ok_or("helps")? = json!([{"message":"help", "edits":edits}]);
            lines.push(value.to_string());
        }
        let rendered = PreparedDiagnostics::new(
            parse(lines.join("\n").as_bytes()).diagnostics,
            &f.context,
            &stop,
        )
        .render(&f.context, None, &stop);
        assert!(rendered.unread > 0, "{rendered:?}");
        // new で使い切った予算を render で初期化すると、先頭の補助位置が表示されてしまう。
        let mixed = lines
            .iter()
            .chain(primary_lines.iter())
            .cloned()
            .collect::<Vec<_>>()
            .join("\n");
        let rendered =
            PreparedDiagnostics::new(parse(mixed.as_bytes()).diagnostics, &f.context, &stop)
                .render(&f.context, None, &stop);
        assert_eq!(rendered.unread, primary_unread);
        assert!(!rendered.text.contains("help:"));
        let deep = format!("{parent}a/deep.bnt");
        fs::create_dir_all(f.project.join(format!("{parent}a")))?;
        fs::write(f.project.join(&deep), GOOD)?;
        let rendered = PreparedDiagnostics::new(
            parse(fake_diagnostic(&deep, 1, 1, 2).to_string().as_bytes()).diagnostics,
            &f.context,
            &stop,
        )
        .render(&f.context, None, &stop);
        assert_eq!(rendered.unread, 1);
        let rendered = PreparedDiagnostics::new(
            parse(fake_diagnostic("main.bnt", 1, 1, 2).to_string().as_bytes()).diagnostics,
            &f.context,
            &stop,
        )
        .render(&f.context, None, &stop);
        assert_eq!(rendered.unread, 0);
        assert!(rendered.text.contains("main.bnt"));
        println!(
            "Diagnostic processing elapsed: {:.3} seconds",
            processing_started.elapsed().as_secs_f64()
        );
        return Ok(());
    }
    let directory = Fixture::new(Mode::Restricted)?;
    let ready = directory.root.join("ready");
    let mut child = std::process::Command::new(std::env::current_exe()?)
        .args([
            "--exact",
            "adversarial_diagnostics_finish_within_ten_seconds",
            "--nocapture",
        ])
        .env(CHILD, "1")
        .env(READY, &ready)
        .current_dir(&directory.root)
        .spawn()?;
    let start = Instant::now();
    let mut processing = None;
    loop {
        if let Some(status) = child.try_wait()? {
            assert!(status.success(), "diagnostic worker failed: {status}");
            break;
        }
        if processing.is_none() && ready.exists() {
            processing = Some(Instant::now());
        }
        let timed_out =
            processing.is_some_and(|started: Instant| started.elapsed() >= Duration::from_secs(10));
        if timed_out || (processing.is_none() && start.elapsed() >= Duration::from_secs(5)) {
            child.kill()?;
            let _status = child.wait()?;
            return Err("diagnostic processing exceeded ten seconds".into());
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    Ok(())
}

// 関門: 診断の file を信用した親による代理読み取りと、全文を読んだことにする
// 退行は本物の診断だけでは作れない。道具の口に偽の診断を渡して守る（P2-07）。
#[test]
fn forged_diagnostics_cannot_read_outside_scope_secrets_pipes_or_record_a_read() -> TestResult {
    let mut f = Fixture::new(Mode::Restricted)?;
    fs::write(f.project.join("main.bnt"), GOOD)?;
    fs::write(f.root.join("outside.bnt"), "SECRET_SOURCE")?;
    fs::write(f.project.join(".env.bnt"), "SECRET_SOURCE")?;
    fs::create_dir(f.project.join("sub"))?;
    symlink(f.root.join("outside.bnt"), f.project.join("link.bnt"))?;
    #[cfg(target_os = "linux")]
    rustix::fs::mkfifoat(
        rustix::fs::CWD,
        f.project.join("pipe.bnt"),
        rustix::fs::Mode::RWXU,
    )?;
    #[cfg(target_os = "macos")]
    assert!(
        std::process::Command::new("/usr/bin/mkfifo")
            .arg(f.project.join("pipe.bnt"))
            .status()?
            .success()
    );
    for path in [
        f.root.join("outside.bnt").to_string_lossy().into_owned(),
        "sub/../main.bnt".into(),
        "../outside.bnt".into(),
        ".env.bnt".into(),
        "link.bnt".into(),
        "pipe.bnt".into(),
    ] {
        f.context.check_query = Some(fake_query(
            fake_diagnostic(&path, 1, 1, 3).to_string().into_bytes(),
        ));
        let output = f.check("main.bnt", false);
        assert!(output.is_error, "{}", output.text);
        assert_eq!(
            output.text, "1 diagnostic in a file that benitoiteh does not read is not shown.",
            "{path}"
        );
    }
    let mut diagnostic = fake_diagnostic("main.bnt", 2, 3, 9);
    *diagnostic.get_mut("secondary").ok_or("secondary")? = json!([{"file":".env.bnt","start":{"line":1,"column":1,"offset":0},"end":{"line":1,"column":2,"offset":1},"label":"HIDDEN_SECONDARY"}]);
    *diagnostic.get_mut("helps").ok_or("helps")? = json!([{"message":"a visible help","edits":[{"file":".env.bnt","start":{"line":1,"column":1,"offset":0},"end":{"line":1,"column":2,"offset":1},"replacement":"HIDDEN_REPLACEMENT"}]}]);
    f.context.check_query = Some(fake_query(diagnostic.to_string().into_bytes()));
    let output = f.check("main.bnt", false);
    for hidden in [
        "HIDDEN_SECONDARY",
        "HIDDEN_REPLACEMENT",
        "SECRET_SOURCE",
        ".env.bnt",
    ] {
        assert!(!output.text.contains(hidden), "{}", output.text);
    }
    assert!(output.text.ends_with("  help: a visible help"));
    f.context.check_query = Some(fake_query(
        fake_diagnostic("main.bnt", 2, 3, 9)
            .to_string()
            .into_bytes(),
    ));
    let output = f.check("main.bnt", false);
    assert!(output.text.contains("  2 |   return ()\n"));
    let output = f.write("main.bnt", GOOD);
    assert!(
        output.is_error
            && output
                .text
                .starts_with("File has not been read in this session.")
    );
    let long = format!("{}SECRET_TAIL\n", "a".repeat(2100));
    fs::write(f.project.join("long.bnt"), long)?;
    for (line, start, end, source, marker) in [
        (1, 1999, 2100, true, true),
        (1, 2001, 2100, true, false),
        (2, 1, 2, false, false),
    ] {
        f.context.check_query = Some(fake_query(
            fake_diagnostic("long.bnt", line, start, end)
                .to_string()
                .into_bytes(),
        ));
        let output = f.check("main.bnt", false);
        assert_eq!(output.text.contains(" | aaaa"), source);
        assert_eq!(output.text.contains('^'), marker);
        assert!(!output.text.contains("SECRET_TAIL"));
        assert!(!output.text.contains("^^^"));
    }
    Ok(())
}

// 関門: 子の停止と出力制限は問い合わせのテストが守る。ここでは、道具が 30 秒と
// FailOver を指定し、書いた行と成功の印を保持する独立した契約を守る。
#[test]
fn incomplete_checks_preserve_write_success_and_limits_and_launch_metadata() -> TestResult {
    let mut f = Fixture::new(Mode::Restricted)?;
    let calls = Arc::new(AtomicUsize::new(0));
    for (overflow, reason) in [
        (false, "The benitoite query timed out"),
        (true, "the diagnostics from benitoite exceeded 16 MiB"),
    ] {
        let calls = Arc::clone(&calls);
        f.context.check_query = Some(Arc::new(move |_, limits, _| {
            calls.fetch_add(1, Ordering::AcqRel);
            assert_eq!(limits.timeout, Duration::from_secs(30));
            assert_eq!(limits.stderr, StderrLimit::FailOver(16 * 1024 * 1024));
            if overflow {
                Err(BenitoiteError::DiagnosticsLimit)
            } else {
                Err(BenitoiteError::TimedOut)
            }
        }));
        let path = if overflow {
            "overflow.bnt"
        } else {
            "timeout.bnt"
        };
        let output = f.write(path, GOOD);
        assert_eq!(
            output.text,
            format!(
                "Wrote {path} (3 lines).\nCheck of {path} after this change did not complete: {reason}."
            )
        );
        assert!(!output.is_error && output.check.is_none());
        assert_eq!(output.check_launch, Some(CheckLaunch::AfterWrite));
        let output = f.check(path, false);
        assert!(output.is_error && output.check.is_none());
        if overflow {
            assert_eq!(
                output.text,
                "The diagnostics from benitoite exceeded 16 MiB and were not read. Check fewer or narrower paths."
            );
        }
    }
    assert_eq!(calls.load(Ordering::Acquire), 4);
    f.context.check_query = Some(Arc::new(|_, _, _| Err(BenitoiteError::Cancelled)));
    let output = f.write("cancelled.bnt", GOOD);
    assert_eq!(
        output.text,
        "Wrote cancelled.bnt (3 lines).\nCheck of cancelled.bnt after this change did not complete: the check was cancelled."
    );
    assert!(!output.is_error && output.check.is_none());
    f.context.check_query = None;
    let output = f.write("no-processor.bnt", GOOD);
    assert_eq!(output.text, "Wrote no-processor.bnt (3 lines).");
    assert!(output.check_launch.is_none());
    let output = f.check("no-processor.bnt", false);
    assert_eq!(
        output.text,
        "Could not check no-processor.bnt with the Benitoite processor: the processor is not available in this session."
    );
    assert!(output.is_error && output.check.is_none() && output.check_launch.is_none());
    Ok(())
}

// 関門: 実在する偽の表示先が read.exclude を迂回する BH-029 の退行を、
// 本物の処理系と write/check の境界で捕まえる。以前の存在しないパスの試験では届かない。
#[test]
fn existing_display_path_cannot_reveal_excluded_import_diagnostics() -> TestResult {
    let mut f = Fixture::new(Mode::Restricted)?;
    fs::create_dir(f.project.join("Secrets"))?;
    fs::write(
        f.project.join("Secrets/Keys.bnt"),
        "public function value() -> Integer\n  return SECRET_TOKEN_abc\nend function\n",
    )?;
    fs::create_dir(f.project.join(".benitoiteh"))?;
    fs::write(
        f.project.join(".benitoiteh/config.toml"),
        "[read]\nexclude = [\"Secrets/**\"]\n",
    )?;
    let loaded = benitoiteh::config::ProjectConfig::load(&f.context.project);
    assert!(loaded.read && loaded.error.is_none());
    let paths = DataPaths::from_values(Some(&f.root), None, None, Some(&f.root))?;
    f.context.exclude = ExcludeRules::new(
        &f.project,
        &paths,
        f.context.spill.real_path(),
        &ReadConfig::default(),
        &loaded.config.read,
    )?;
    assert!(f.call("read", json!({"path":"Secrets/Keys.bnt"})).is_error);
    let outputs = [
        f.write("A/X.bnt", "import Secrets.Keys\npublic function value() -> Integer\n  return Keys.value()\nend function\n"),
        f.write("A/Secrets/Keys.bnt", "public function value() -> Integer\n  return 0\nend function\n"),
        f.check(".", true),
    ];
    f.read("A/Secrets/Keys.bnt");
    for output in outputs {
        assert!(!output.text.contains("SECRET_TOKEN_abc"), "{}", output.text);
        assert!(
            output
                .text
                .contains("1 diagnostic in a file that benitoiteh does not read is not shown."),
            "{}",
            output.text
        );
    }
    let mut diagnostic = fake_diagnostic("A/X.bnt", 1, 1, 2);
    *diagnostic.get_mut("secondary").ok_or("secondary")? = json!([{"file":"A/Secrets/Keys.bnt","start":{"line":1,"column":1,"offset":0},"end":{"line":1,"column":2,"offset":1},"label":"SECRET_TOKEN_abc secondary"}]);
    *diagnostic.get_mut("helps").ok_or("helps")? = json!([{"message":"change","edits":[{"file":"A/Secrets/Keys.bnt","start":{"line":1,"column":1,"offset":0},"end":{"line":1,"column":2,"offset":1},"replacement":"SECRET_TOKEN_abc replacement"}]}]);
    f.context.check_query = Some(fake_query(diagnostic.to_string().into_bytes()));
    let output = f.check(".", true);
    assert!(
        !output.text.contains("SECRET_TOKEN_abc") && !output.text.contains("A/Secrets/Keys.bnt"),
        "{}",
        output.text
    );
    Ok(())
}

// 関門: 診断が read の画像・バイナリ・大きさの拒否を迂回して行を添える退行を守る。
// 疎な大きいファイルと大きい一行を用い、道具の結果を境界にする。
#[test]
fn diagnostic_sources_reject_nontext_and_large_files_and_bound_long_lines() -> TestResult {
    let mut f = Fixture::new(Mode::Restricted)?;
    fs::write(f.project.join("main.bnt"), GOOD)?;
    fs::write(f.project.join("image.svg"), "SECRET_SOURCE")?;
    fs::write(f.project.join("binary.bnt"), b"SECRET_SOURCE\0bytes")?;
    use std::io::Write;
    let mut large = fs::File::create(f.project.join("large.bnt"))?;
    large.set_len(64 * 1024 * 1024 + 1)?;
    large.write_all(&[b'x'; 8192])?;
    large.write_all(b"\n")?;
    fs::write(
        f.project.join("late-binary.bnt"),
        b"safe first line\nmore\0bytes",
    )?;
    let mut long = fs::File::create(f.project.join("long.bnt"))?;
    long.write_all("界".repeat(2000).as_bytes())?;
    for _ in 0..1024 {
        long.write_all(&[b'x'; 8192])?;
    }
    long.write_all(b"SECRET_SOURCE\nlast\n")?;
    for path in ["image.svg", "binary.bnt", "late-binary.bnt", "large.bnt"] {
        f.context.check_query = Some(fake_query(
            fake_diagnostic(path, 1, 1, 2).to_string().into_bytes(),
        ));
        let output = f.check("main.bnt", false);
        assert!(
            !output.text.contains("SECRET_SOURCE") && !output.text.contains(" | "),
            "{}",
            output.text
        );
    }
    let source = benitoiteh::benitoite::diagnostics::DiagnosticSources::line(
        &f.context,
        &f.project.join("long.bnt"),
        1,
    )
    .ok_or("bounded line")?;
    assert_eq!(source.text.chars().count(), 2000);
    assert!(source.truncated);
    f.context.check_query = Some(fake_query(
        fake_diagnostic("long.bnt", 1, 2001, 2002)
            .to_string()
            .into_bytes(),
    ));
    let output = f.check("main.bnt", false);
    assert!(output.text.contains(&"界".repeat(2000)));
    assert!(!output.text.contains("SECRET_SOURCE") && !output.text.contains('^'));
    f.context.check_query = Some(fake_query(
        fake_diagnostic("long.bnt", 2, 1, 5)
            .to_string()
            .into_bytes(),
    ));
    assert!(f.check("main.bnt", false).text.contains("  2 | last\n"));
    Ok(())
}
