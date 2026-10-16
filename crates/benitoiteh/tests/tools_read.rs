//! JSON の道具呼び出しで読み取りの契約を守る（設計書 07-01「持ち主の境界」）。

use std::error::Error;
use std::fs;
use std::os::unix::fs::symlink;
use std::path::PathBuf;

use benitoiteh::config::{Mode, ReadConfig, paths::DataPaths};
use benitoiteh::provider::{ArgumentSpec, ArgumentType, ToolSpec};
use benitoiteh::safe_fs::ScopeRoot;
use benitoiteh::tools::exclude::ExcludeRules;
use benitoiteh::tools::truncate::{Side, cap_middle, truncate};
use benitoiteh::tools::{FileVersion, ToolContext, ToolSet, repair_and_validate};
use serde_json::json;

type TestResult = Result<(), Box<dyn Error>>;

struct Fixture {
    directory: PathBuf,
    project: PathBuf,
    temporary: PathBuf,
    spill: PathBuf,
    paths: DataPaths,
}

impl Fixture {
    fn new() -> Result<Self, Box<dyn Error>> {
        let mut random = [0u8; 16];
        getrandom::fill(&mut random)?;
        let directory = std::env::temp_dir().join(format!(
            "benitoiteh-tools-{:032x}",
            u128::from_ne_bytes(random)
        ));
        fs::create_dir(&directory)?;
        let directory = fs::canonicalize(directory)?;
        let project = directory.join("project");
        let temporary = directory.join("temporary");
        let paths = DataPaths::from_values(Some(&directory), None, None, Some(&directory))?;
        let spill = paths.data_dir.join("sessions/current/spill");
        for path in [&project, &temporary, &spill, &paths.config_dir] {
            fs::create_dir_all(path)?;
        }
        Ok(Self {
            directory,
            project,
            temporary,
            spill,
            paths,
        })
    }

    fn context(&self, mode: Mode) -> Result<ToolContext, Box<dyn Error>> {
        self.with_rules(mode, &ReadConfig::default(), &ReadConfig::default())
    }

    fn with_rules(
        &self,
        mode: Mode,
        user: &ReadConfig,
        project: &ReadConfig,
    ) -> Result<ToolContext, Box<dyn Error>> {
        Ok(ToolContext::new(
            mode,
            ScopeRoot::open(&self.project)?,
            ScopeRoot::open(&self.temporary)?,
            ScopeRoot::open(&self.spill)?,
            ExcludeRules::new(&self.project, &self.paths, &self.spill, user, project)?,
        ))
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _removed = fs::remove_dir_all(&self.directory);
    }
}

#[test]
fn read_pages_number_lines_and_record_the_opened_version() -> TestResult {
    let fixture = Fixture::new()?;
    let file = fixture.project.join("source.bnt");
    fs::write(&file, "one\ntwo\nthree\nfour\n")?;
    let context = fixture.context(Mode::Restricted)?;
    let tools = ToolSet::new(Mode::Restricted);
    let result = tools.execute(
        &context,
        "read",
        json!({"path":"source.bnt", "offset":2, "limit":2}),
    );
    assert!(!result.is_error, "{}", result.text);
    assert_eq!(
        result.text,
        "     2\ttwo\n     3\tthree\n[Showing lines 2-3 of 4. Use offset=4 to continue.]"
    );
    assert_eq!(
        context.read_version(&file)?,
        Some(FileVersion::from_metadata(&fs::metadata(&file)?))
    );
    let result = tools.execute(
        &context,
        "read",
        json!({"path":"source.bnt", "offset":4, "limit":null}),
    );
    assert_eq!(result.text, "     4\tfour\n");
    let result = tools.execute(&context, "read", json!({"path":"source.bnt", "offset":5}));
    assert!(result.is_error);
    assert_eq!(
        result.text,
        "Offset 5 is beyond end of file (4 lines total)."
    );
    for args in [
        json!({"path":"source.bnt", "offset":0}),
        json!({"path":"source.bnt", "limit":-1}),
    ] {
        let result = tools.execute(&context, "read", args);
        assert!(result.is_error);
        assert!(result.text.contains("positive integer"));
    }
    fs::write(fixture.project.join("empty"), "")?;
    let result = tools.execute(&context, "read", json!({"path":"empty"}));
    assert!(!result.is_error);
    assert_eq!(result.text, "");
    Ok(())
}

#[test]
fn read_limits_lines_bytes_and_long_unicode_lines_and_replaces_invalid_utf8() -> TestResult {
    let fixture = Fixture::new()?;
    let tools = ToolSet::new(Mode::Restricted);
    let context = fixture.context(Mode::Restricted)?;
    fs::write(fixture.project.join("many"), "short\n".repeat(2001))?;
    let result = tools.execute(&context, "read", json!({"path":"many", "limit":9000}));
    assert!(!result.is_error);
    assert!(
        result
            .text
            .ends_with("[Showing lines 1-2000 of 2001. Use offset=2001 to continue.]")
    );
    assert!(!result.text.contains("  2001\t"));
    fs::write(
        fixture.project.join("wide"),
        format!("{}\n", "界".repeat(2001)).repeat(20),
    )?;
    let result = tools.execute(&context, "read", json!({"path":"wide"}));
    assert!(!result.is_error);
    assert!(
        result
            .text
            .contains("[line 1 truncated: 2001 chars. Use grep to find text in this line.]")
    );
    assert!(result.text.contains("Use offset=9 to continue."));
    assert!(result.text.len() <= 50 * 1024);
    assert!(!result.text.contains('\u{fffd}'));
    fs::write(fixture.project.join("lossy"), [b'a', 0xff, b'b', b'\n'])?;
    let result = tools.execute(&context, "read", json!({"path":"lossy"}));
    assert_eq!(result.text, "     1\ta\u{fffd}b\n");
    for (name, bytes) in [
        ("binary", b"a\0b".as_slice()),
        ("picture.PNG", b"not a text image".as_slice()),
    ] {
        fs::write(fixture.project.join(name), bytes)?;
        let result = tools.execute(&context, "read", json!({"path":name}));
        assert!(result.is_error);
        assert!(result.text.contains("binary or image"));
        assert_eq!(context.read_version(&fixture.project.join(name))?, None);
    }
    Ok(())
}

#[test]
fn restricted_scope_rejects_escape_and_allows_only_read_in_temporary_and_spill() -> TestResult {
    let fixture = Fixture::new()?;
    let outside = fixture.directory.join("outside");
    fs::write(&outside, "outside sentinel\n")?;
    fs::write(fixture.temporary.join("scratch.bnt"), "temporary\n")?;
    fs::write(fixture.spill.join("output"), "spill\n")?;
    fs::write(fixture.project.join("inside"), "inside\n")?;
    symlink(&outside, fixture.project.join("escape"))?;
    symlink("inside", fixture.project.join("alias"))?;
    let tools = ToolSet::new(Mode::Restricted);
    let context = fixture.context(Mode::Restricted)?;
    for path in [outside.to_string_lossy().as_ref(), "../outside", "escape"] {
        let result = tools.execute(&context, "read", json!({"path":path}));
        assert!(result.is_error, "{path}: {}", result.text);
        assert!(!result.text.contains("outside sentinel"));
    }
    let result = tools.execute(&context, "read", json!({"path":"alias"}));
    assert_eq!(result.text, "     1\tinside\n");
    for path in [
        fixture.temporary.join("scratch.bnt"),
        fixture.spill.join("output"),
    ] {
        let result = tools.execute(&context, "read", json!({"path":path}));
        assert!(!result.is_error, "{}", result.text);
        assert!(context.read_version(&path)?.is_some());
    }
    for path in [&fixture.temporary, &fixture.spill, &fixture.directory] {
        for name in ["glob", "grep"] {
            assert!(
                tools
                    .execute(&context, name, json!({"path":path, "pattern":"*"}))
                    .is_error
            );
        }
    }
    let normal_context = fixture.context(Mode::Normal)?;
    let normal_tools = ToolSet::new(Mode::Normal);
    for path in [outside.to_string_lossy().as_ref(), "../outside", "escape"] {
        let result = normal_tools.execute(&normal_context, "read", json!({"path":path}));
        assert_eq!(result.text, "     1\toutside sentinel\n");
    }
    let result = normal_tools.execute(
        &normal_context,
        "grep",
        json!({"path":&outside, "pattern":"sentinel"}),
    );
    assert!(!result.is_error, "{}", result.text);
    assert!(result.text.contains("outside sentinel"));
    Ok(())
}

#[test]
fn exclusions_apply_to_real_paths_and_both_modes_and_settings_only_add_rules() -> TestResult {
    let fixture = Fixture::new()?;
    let names = [
        ".env",
        ".env.local",
        "cert.pem",
        "key.key",
        "key.p12",
        "key.pfx",
        "id_rsa.pub",
        "id_ecdsa",
        "id_ed25519_test",
        ".ssh/config",
        ".gnupg/key",
        ".aws/credentials",
        ".netrc",
        ".git-credentials",
        ".git/config",
        "user.private",
        "project.private",
    ];
    for name in names {
        let path = fixture.project.join(name);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, "secret sentinel\n")?;
    }
    for name in [".env.example", ".env.sample", "public.txt"] {
        fs::write(fixture.project.join(name), "public sentinel\n")?;
    }
    symlink(".env", fixture.project.join("innocent.txt"))?;
    let config = fixture.paths.config_dir.join("config.toml");
    let data = fixture.paths.data_dir.join("private.json");
    fs::write(&config, "secret config\n")?;
    fs::write(&data, "secret data\n")?;
    fs::write(fixture.spill.join("output"), "saved output\n")?;
    let user = ReadConfig {
        exclude: vec!["user.*".to_owned()],
    };
    let project = ReadConfig {
        exclude: vec!["project.*".to_owned()],
    };
    for mode in [Mode::Restricted, Mode::Normal] {
        let tools = ToolSet::new(mode);
        let context = fixture.with_rules(mode, &user, &project)?;
        for name in names.into_iter().chain(["innocent.txt"]) {
            let result = tools.execute(&context, "read", json!({"path":name}));
            assert!(result.is_error, "{name}: {}", result.text);
            assert!(
                result.text.contains("excluded by read rule"),
                "{}",
                result.text
            );
            assert!(!result.text.contains("secret sentinel"));
        }
        for path in [&config, &data] {
            let result = tools.execute(&context, "read", json!({"path":path}));
            assert!(result.is_error);
        }
        let result = tools.execute(
            &context,
            "read",
            json!({"path":fixture.spill.join("output")}),
        );
        assert!(!result.is_error, "{}", result.text);
        for name in ["glob", "grep"] {
            let pattern = if name == "glob" { "**/*" } else { "sentinel" };
            let result = tools.execute(&context, name, json!({"pattern":pattern}));
            assert!(!result.is_error, "{}", result.text);
            assert!(!result.text.contains("secret"));
            assert!(!result.text.contains("innocent"));
            assert!(result.text.contains(".env.example"));
            assert!(result.text.contains(".env.sample"));
            assert!(result.text.contains("public.txt"));
            assert!(!result.text.contains("private"));
        }
    }
    Ok(())
}

#[test]
fn searches_honor_local_gitignore_include_hidden_files_and_do_not_follow_links() -> TestResult {
    let fixture = Fixture::new()?;
    fs::create_dir_all(fixture.project.join("nested"))?;
    fs::create_dir_all(fixture.directory.join("outside-dir"))?;
    fs::write(fixture.directory.join(".gitignore"), "*.rs\n")?;
    fs::write(fixture.project.join(".gitignore"), "ignored.rs\n")?;
    fs::write(
        fixture.project.join("nested/.gitignore"),
        "nested-ignored.rs\n",
    )?;
    for name in [
        "visible.rs",
        "ignored.rs",
        ".hidden.rs",
        "nested/visible.rs",
        "nested/nested-ignored.rs",
    ] {
        fs::write(fixture.project.join(name), "needle\n")?;
    }
    fs::write(
        fixture.directory.join("outside-dir/external.rs"),
        "needle\n",
    )?;
    symlink(
        fixture.directory.join("outside-dir"),
        fixture.project.join("link"),
    )?;
    symlink("visible.rs", fixture.project.join("alias.rs"))?;
    let context = fixture.context(Mode::Restricted)?;
    let tools = ToolSet::new(Mode::Restricted);
    for name in ["glob", "grep"] {
        let args = if name == "glob" {
            json!({"pattern":"**/*.rs"})
        } else {
            json!({"pattern":"needle", "glob":"**/*.rs", "files_only":true})
        };
        let result = tools.execute(&context, name, args);
        assert!(!result.is_error, "{}", result.text);
        let paths: std::collections::BTreeSet<_> = result.text.lines().collect();
        assert_eq!(
            paths,
            std::collections::BTreeSet::from(["visible.rs", ".hidden.rs", "nested/visible.rs"])
        );
    }
    Ok(())
}

#[test]
fn grep_handles_regex_case_glob_context_binary_and_long_line_character_positions() -> TestResult {
    let fixture = Fixture::new()?;
    fs::write(
        fixture.project.join("source.rs"),
        "before\nNeedle 42\nafter\n",
    )?;
    fs::write(fixture.project.join("skip.txt"), "needle 43\n")?;
    fs::write(fixture.project.join("binary.rs"), b"needle 44\0\n")?;
    let context = fixture.context(Mode::Restricted)?;
    let tools = ToolSet::new(Mode::Restricted);
    let result = tools.execute(
        &context,
        "grep",
        json!({"pattern":"needle [0-9]+", "glob":"*.rs", "ignore_case":true, "context":1}),
    );
    assert!(!result.is_error, "{}", result.text);
    assert_eq!(
        result.text,
        "source.rs:1:before\nsource.rs:2:Needle 42\nsource.rs:3:after\n"
    );
    fs::write(
        fixture.project.join("long.txt"),
        format!("{}TARGET{}\n", "界".repeat(2000), "後".repeat(2000)),
    )?;
    let result = tools.execute(
        &context,
        "grep",
        json!({"pattern":"TARGET", "path":"long.txt"}),
    );
    assert!(!result.is_error, "{}", result.text);
    assert!(result.text.contains("TARGET"));
    assert!(result.text.contains("[chars 1501-2500 of 4006]"));
    assert_eq!(result.text.matches('界').count(), 500);
    assert!(!result.text.contains('\u{fffd}'));
    for args in [
        json!({"pattern":"["}),
        json!({"pattern":"needle", "glob":"["}),
        json!({"pattern":"needle", "context":-1}),
    ] {
        assert!(tools.execute(&context, "grep", args).is_error);
    }
    Ok(())
}

#[test]
fn search_limits_report_counts_and_bytes_and_glob_sorts_by_modification_time() -> TestResult {
    let fixture = Fixture::new()?;
    for index in 0..501 {
        fs::write(
            fixture.project.join(format!("file-{index:03}.txt")),
            "hit\n",
        )?;
    }
    let latest = fixture.project.join("file-000.txt");
    fs::File::options()
        .write(true)
        .open(&latest)?
        .set_modified(std::time::SystemTime::now() + std::time::Duration::from_secs(10))?;
    let context = fixture.context(Mode::Restricted)?;
    let tools = ToolSet::new(Mode::Restricted);
    let result = tools.execute(&context, "glob", json!({"pattern":"*.txt"}));
    assert!(!result.is_error, "{}", result.text);
    assert_eq!(result.text.lines().next(), Some("file-000.txt"));
    assert_eq!(result.text.lines().count(), 501);
    assert!(result.text.contains("[Showing 500 of 501 files."));
    let result = tools.execute(
        &context,
        "grep",
        json!({"pattern":"hit", "files_only":true}),
    );
    assert_eq!(result.text.lines().count(), 101);
    assert!(result.text.contains("Results truncated"));
    fs::write(
        fixture.project.join("wide"),
        format!("hit{}\n", "界".repeat(500)).repeat(101),
    )?;
    let result = tools.execute(&context, "grep", json!({"pattern":"hit", "path":"wide"}));
    assert!(!result.is_error, "{}", result.text);
    assert!(result.text.len() <= 50 * 1024);
    assert!(result.text.contains("Results truncated"));
    assert!(result.text.lines().count() < 100);
    assert!(!result.text.contains('\u{fffd}'));
    Ok(())
}

#[test]
fn invalid_calls_return_corrective_errors_without_reading_and_array_arguments_are_repaired()
-> TestResult {
    let fixture = Fixture::new()?;
    fs::write(fixture.project.join("file"), "sentinel\n")?;
    let context = fixture.context(Mode::Restricted)?;
    let tools = ToolSet::new(Mode::Restricted);
    for (name, args, detail) in [
        ("missing", json!({"path":"file"}), "Unknown tool"),
        ("read", json!({}), "Missing required argument"),
        ("read", json!({"path":3}), "must be a string"),
        (
            "read",
            json!({"path":"file", "offset":1.5}),
            "must be an integer",
        ),
        (
            "read",
            json!({"path":"file", "approved":true}),
            "Unknown argument",
        ),
        (
            "grep",
            json!({"pattern":"sentinel", "ignore_case":"true"}),
            "must be a boolean",
        ),
        ("read", json!([]), "JSON object"),
        ("read", json!({"path":null}), "must be a string"),
    ] {
        let result = tools.execute(&context, name, args);
        assert!(result.is_error);
        assert!(result.text.contains(detail), "{}", result.text);
        if name != "missing" {
            assert!(result.text.contains("Expected arguments:"));
        }
        assert_eq!(context.read_version(&fixture.project.join("file"))?, None);
    }
    // P1 の五つには配列がないので、型の持ち主の公開の検証器で同じ契約を確かめる。
    let spec = ToolSpec {
        name: "paths".into(),
        description: "Paths".into(),
        arguments: vec![ArgumentSpec {
            name: "paths".into(),
            kind: ArgumentType::StringArray,
            required: true,
            description: "Paths".into(),
        }],
    };
    let mut arguments = json!({"paths":"[\"first.bnt\",\"second.bnt\"]"});
    repair_and_validate(&spec, &mut arguments)?;
    assert_eq!(arguments, json!({"paths":["first.bnt", "second.bnt"]}));
    for value in [json!("not JSON"), json!("[1]"), json!([false]), json!(null)] {
        assert!(repair_and_validate(&spec, &mut json!({"paths":value})).is_err());
    }
    Ok(())
}

#[test]
fn shared_context_records_concurrent_reads_without_losing_versions() -> TestResult {
    let fixture = Fixture::new()?;
    for index in 0..8 {
        fs::write(fixture.project.join(format!("file-{index}")), "contents\n")?;
    }
    let context = fixture.context(Mode::Restricted)?;
    let tools = ToolSet::new(Mode::Restricted);
    std::thread::scope(|scope| {
        for index in 0..8 {
            let context = &context;
            let tools = &tools;
            scope.spawn(move || {
                let result =
                    tools.execute(context, "read", json!({"path":format!("file-{index}")}));
                assert_eq!(result.text, "     1\tcontents\n");
                assert!(!result.is_error);
            });
        }
    });
    for index in 0..8 {
        assert!(
            context
                .read_version(&fixture.project.join(format!("file-{index}")))?
                .is_some()
        );
    }
    Ok(())
}

#[test]
fn common_truncation_preserves_head_tail_and_utf8_and_reports_omitted_bytes() {
    let result = truncate("first\nsecond\nthird\n", Side::Head, 2, 100, "[more]");
    assert_eq!(result.text, "first\nsecond\n[more]");
    assert_eq!(result.omitted_bytes, 6);
    let result = truncate("first\nsecond\nthird\n", Side::Tail, 2, 100, "[more]");
    assert_eq!(result.text, "second\nthird\n[more]");
    assert_eq!(result.omitted_bytes, 6);
    for side in [Side::Head, Side::Tail] {
        let result = truncate("あいうえお", side, 10, 12, "[more]");
        assert!(result.truncated);
        assert!(result.text.len() <= 12);
        assert!(!result.text.contains('\u{fffd}'));
        assert!(result.text.ends_with("[more]"));
    }
    let result = cap_middle(&format!("START{}END", "界".repeat(30000)));
    assert!(result.len() <= 64 * 1024);
    assert!(result.starts_with("START"));
    assert!(result.ends_with("END"));
    assert!(result.contains("bytes omitted from the middle"));
    assert!(!result.contains('\u{fffd}'));
}
