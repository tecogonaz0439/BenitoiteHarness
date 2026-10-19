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

// 作成時の関門: 既存の通常ファイルのテストでは捕まらない FIFO の待機と巨大入力を、
// 道具の公開の境界で検出する。時間内の完了と普通のファイルの取得をともに要求する
// （設計書 01-03「glob」「grep」「read」）。
fn watched(operation: impl FnOnce() -> TestResult + Send + 'static) -> TestResult {
    let (sender, receiver) = std::sync::mpsc::channel();
    let worker = std::thread::spawn(move || {
        let _sent = sender.send(operation().map_err(|error| error.to_string()));
    });
    receiver
        .recv_timeout(std::time::Duration::from_secs(5))?
        .map_err(std::io::Error::other)?;
    worker
        .join()
        .map_err(|_| std::io::Error::other("worker failed"))?;
    Ok(())
}

fn fifo(path: &std::path::Path) -> TestResult {
    #[cfg(target_os = "linux")]
    rustix::fs::mkfifoat(rustix::fs::CWD, path, rustix::fs::Mode::RWXU)?;
    #[cfg(target_os = "macos")]
    assert!(
        std::process::Command::new("mkfifo")
            .arg(path)
            .status()?
            .success()
    );
    Ok(())
}

#[test]
fn searches_skip_hostile_gitignores_without_waiting() -> TestResult {
    watched(|| {
        let fixture = Fixture::new()?;
        fs::create_dir(fixture.project.join("sub"))?;
        fs::write(fixture.project.join("sub/ordinary.txt"), "needle\n")?;
        let ignore = fixture.project.join("sub/.gitignore");
        let tools = ToolSet::new(Mode::Restricted);
        let context = fixture.context(Mode::Restricted)?;
        for kind in ["fifo", "link", "large", "utf8"] {
            match kind {
                "fifo" => fifo(&ignore)?,
                "link" => symlink("/dev/zero", &ignore)?,
                "large" => fs::write(&ignore, vec![b'x'; 10 * 1024 * 1024])?,
                "utf8" => fs::write(&ignore, [0xff])?,
                _ => return Err("unknown fixture".into()),
            }
            for tool in ["glob", "grep"] {
                let pattern = if tool == "glob" { "*.txt" } else { "needle" };
                let result = tools.execute(&context, tool, json!({"pattern":pattern}));
                assert!(!result.is_error, "{kind}/{tool}: {}", result.text);
                assert!(result.text.contains("sub/ordinary.txt"), "{}", result.text);
                assert!(
                    result.text.contains("Skipped .gitignore"),
                    "{}",
                    result.text
                );
                assert!(result.text.contains("sub/.gitignore"));
            }
            fs::remove_file(&ignore)?;
        }
        Ok(())
    })
}

// 作成時の関門: 起点の親の規則と不正な行の後の有効な規則が失われる退行を、
// 通常の検索では覆わない起点の形と組み合わせて確かめる。内部の判定は呼ばない。
#[test]
fn search_roots_inherit_valid_ignore_rules_and_preserve_depth_first_order() -> TestResult {
    let fixture = Fixture::new()?;
    fs::create_dir_all(fixture.project.join("sub/a"))?;
    fs::create_dir(fixture.project.join("sub/.git"))?;
    fs::write(
        fixture.project.join(".gitignore"),
        "\u{feff}*.skip\n!keep.skip\n",
    )?;
    fs::write(
        fixture.project.join("sub/.gitignore"),
        "[z-a]\nhidden.txt\n",
    )?;
    fs::write(fixture.project.join("sub/.ignore"), "*.txt\n")?;
    for name in [
        "a/x.txt",
        "b.txt",
        ".hidden.txt",
        "hidden.txt",
        "bad.skip",
        "keep.skip",
    ] {
        fs::write(fixture.project.join("sub").join(name), "needle\n")?;
    }
    symlink("sub", fixture.project.join("alias"))?;
    symlink("sub/bad.skip", fixture.project.join("file-alias"))?;
    let context = fixture.context(Mode::Restricted)?;
    let tools = ToolSet::new(Mode::Restricted);
    for path in ["sub", "alias"] {
        let result = tools.execute(
            &context,
            "grep",
            json!({"path":path,"pattern":"needle","files_only":true}),
        );
        assert!(!result.is_error, "{}", result.text);
        assert_eq!(
            result.text,
            "sub/.hidden.txt\nsub/a/x.txt\nsub/b.txt\nsub/keep.skip\n"
        );
        let result = tools.execute(&context, "glob", json!({"path":path,"pattern":"a/*.txt"}));
        assert_eq!(result.text, "sub/a/x.txt");
    }
    for path in ["sub/bad.skip", "file-alias"] {
        for tool in ["glob", "grep"] {
            let pattern = if tool == "glob" { "*.skip" } else { "needle" };
            let result = tools.execute(&context, tool, json!({"path":path,"pattern":pattern}));
            assert!(!result.is_error, "{}", result.text);
            assert!(result.text.contains("sub/bad.skip"));
            assert!(!result.text.contains("file-alias"));
        }
    }
    Ok(())
}

#[test]
fn hostile_file_inputs_are_rejected_or_skipped_without_waiting() -> TestResult {
    watched(|| {
        let fixture = Fixture::new()?;
        fs::write(fixture.project.join("ordinary.txt"), "needle\n")?;
        fifo(&fixture.project.join("pipe"))?;
        symlink("/dev/zero", fixture.project.join("zero"))?;
        fs::File::create(fixture.project.join("huge"))?.set_len(64 * 1024 * 1024 * 1024)?;
        let context = fixture.context(Mode::Normal)?;
        let tools = ToolSet::new(Mode::Normal);
        for path in ["pipe", "zero", "huge"] {
            let result = tools.execute(&context, "read", json!({"path":path}));
            assert!(result.is_error, "{path}: {}", result.text);
            if path == "huge" {
                assert!(result.text.contains("Use grep"));
            }
            assert_eq!(context.read_version(&fixture.project.join(path))?, None);
            let result = tools.execute(&context, "grep", json!({"path":path,"pattern":"needle"}));
            assert!(!result.is_error, "{path}: {}", result.text);
            assert!(result.text.contains("Skipped"), "{}", result.text);
        }
        let result = tools.execute(&context, "grep", json!({"pattern":"needle"}));
        assert!(!result.is_error, "{}", result.text);
        assert!(result.text.contains("ordinary.txt:1:needle"));
        assert!(result.text.contains("huge (too large)"));
        assert!(result.text.contains("pipe (non-regular)"));
        Ok(())
    })
}

// 作成時の関門: 一つのディレクトリの権限の誤りで、他の結果まで失う退行を検出する。
// 通常ファイルと範囲外のリンクの既存テストでは、この継続の契約を確かめられない。
#[test]
fn unreadable_directories_are_reported_without_losing_other_results() -> TestResult {
    use std::os::unix::fs::PermissionsExt;
    if rustix::process::geteuid().is_root() {
        eprintln!("Skipped unreadable-directory search check: root can read mode 000 directories.");
        return Ok(());
    }
    let fixture = Fixture::new()?;
    let directory = fixture.project.join("blocked");
    fs::create_dir(&directory)?;
    fs::write(directory.join("hidden.txt"), "needle\n")?;
    fs::write(fixture.project.join("ordinary.txt"), "needle\n")?;
    fs::set_permissions(&directory, fs::Permissions::from_mode(0o000))?;
    let tools = ToolSet::new(Mode::Restricted);
    let context = fixture.context(Mode::Restricted)?;
    let results = [
        tools.execute(&context, "glob", json!({"pattern":"*.txt"})),
        tools.execute(&context, "grep", json!({"pattern":"needle"})),
    ];
    fs::set_permissions(&directory, fs::Permissions::from_mode(0o700))?;
    for result in results {
        assert!(!result.is_error, "{}", result.text);
        assert!(result.text.contains("ordinary.txt"));
        assert!(!result.text.contains("hidden.txt"));
        assert!(
            result
                .text
                .contains("Skipped unreadable directories: 1; first 5 paths: blocked")
        );
    }
    Ok(())
}

// 作成時の関門: 一つのファイルの open/F_GETPATH の失敗で、後のファイルまで
// 検索不能になる退行を、本物のファイルと道具の公開の境界で検出する。
#[test]
fn grep_continues_after_unreadable_files_and_long_macos_file_paths() -> TestResult {
    watched(|| {
        use std::os::unix::fs::PermissionsExt;
        let fixture = Fixture::new()?;
        fs::write(fixture.project.join("z-ordinary.txt"), "needle\n")?;
        let mut directory = fixture.project.join("a");
        fs::create_dir(&directory)?;
        while directory.as_os_str().len() < 850 {
            directory.push("d".repeat(100));
            fs::create_dir(&directory)?;
        }
        let name = format!("{}.txt", "x".repeat(240));
        ScopeRoot::open(&directory)?.write(
            &benitoiteh::safe_fs::ScopeRelPath::new(&name)?,
            b"needle\n",
            false,
        )?;
        let blocked = fixture.project.join("b-unreadable.txt");
        fs::write(&blocked, "needle\n")?;
        fs::set_permissions(&blocked, fs::Permissions::from_mode(0o000))?;
        let tools = ToolSet::new(Mode::Restricted);
        let context = fixture.context(Mode::Restricted)?;
        let result = tools.execute(&context, "grep", json!({"pattern":"needle"}));
        fs::set_permissions(&blocked, fs::Permissions::from_mode(0o600))?;
        assert!(!result.is_error, "{}", result.text);
        assert!(
            result.text.contains("z-ordinary.txt:1:needle"),
            "{}",
            result.text
        );
        #[cfg(target_os = "macos")]
        assert!(result.text.contains("unreadable"), "{}", result.text);
        if rustix::process::geteuid().is_root() {
            eprintln!("Skipped unreadable-file search check: root can read mode 000 files.");
        } else {
            assert!(
                result.text.contains("b-unreadable.txt (unreadable)"),
                "{}",
                result.text
            );
        }
        Ok(())
    })
}

// 作成時の関門: 通知の洪水で照合した行が cap_middle に消される退行と、
// ファイル名の改行で偽の結果が作られる退行を、結果の境界で検出する。
#[test]
fn search_notices_are_bounded_summaries_after_all_matching_rows() -> TestResult {
    use benitoiteh::tools::SearchLimits;
    let fixture = Fixture::new()?;
    fs::write(
        fixture.project.join("a-matches.txt"),
        format!("needle{}\n", "x".repeat(440)).repeat(100),
    )?;
    for index in 0..512 {
        let path = fixture
            .project
            .join(format!("b-large-{index:04}-{}.txt", "x".repeat(80)));
        fs::File::create(path)?.set_len(65537)?;
    }
    for index in 0..7 {
        let directory = fixture.project.join(format!("ignore-{index}"));
        fs::create_dir(&directory)?;
        fs::write(directory.join(".gitignore"), [0xff])?;
    }
    let context = fixture
        .context(Mode::Restricted)?
        .with_search_limits(SearchLimits {
            max_file_bytes: 65536,
            ..SearchLimits::default()
        });
    let result =
        ToolSet::new(Mode::Restricted).execute(&context, "grep", json!({"pattern":"needle"}));
    assert!(!result.is_error, "{}", result.text);
    assert_eq!(result.text.matches("a-matches.txt:").count(), 100);
    assert!(result.text.contains("a-matches.txt:50:needle"));
    assert!(result.text.contains("a-matches.txt:100:needle"));
    assert!(!result.text.contains("omitted from the middle"));
    assert!(result.text.contains("Skipped files: 512; first 5 paths:"));
    assert!(
        result
            .text
            .contains("Skipped .gitignore files: 7; first 5 paths:")
    );
    assert!(result.text.contains("b-large-0004-"));
    assert!(!result.text.contains("b-large-0005-"));
    assert_eq!(result.text.lines().count(), 102);
    assert!(result.text.len() <= 64 * 1024);
    let (_, notices) = result
        .text
        .split_once("[Skipped")
        .ok_or("missing notices")?;
    assert!(notices.len().saturating_add("[Skipped".len()) <= 4096);
    Ok(())
}

#[test]
fn search_paths_cannot_inject_control_characters_into_rows_or_notices() -> TestResult {
    let fixture = Fixture::new()?;
    let raw_directory = "dir\nFAKE\u{1b}\u{85}\u{202e}";
    let raw_file = "file\t\r\u{7f}\u{2066}.txt";
    let directory = fixture.project.join(raw_directory);
    fs::create_dir(&directory)?;
    fs::write(directory.join(raw_file), "needle\n")?;
    symlink("/dev/zero", directory.join(".gitignore"))?;
    let context = fixture.context(Mode::Restricted)?;
    let tools = ToolSet::new(Mode::Restricted);
    let visible_directory = "dir FAKE\\x1b\\x85\\u{202e}";
    let visible_file = "file \\x0d\\x7f\\u{2066}.txt";
    for name in ["glob", "grep"] {
        let pattern = if name == "glob" { "*.txt" } else { "needle" };
        let result = tools.execute(&context, name, json!({"pattern":pattern}));
        assert!(!result.is_error, "{}", result.text);
        assert!(
            result
                .text
                .starts_with(&format!("{visible_directory}/{visible_file}")),
            "{}",
            result.text
        );
        assert!(
            result
                .text
                .contains(&format!("{visible_directory}/.gitignore"))
        );
        assert_eq!(result.text.lines().count(), 2);
        assert!(
            !result
                .text
                .contains(['\r', '\t', '\u{1b}', '\u{85}', '\u{202e}', '\u{2066}'])
        );
    }
    Ok(())
}

// 作成時の関門: 起点を開けても、その祖先を列挙できない場合がある。
// 上の「読めない子」の検査では起点を作る途中の失敗を捕まえられない。
#[test]
fn unreadable_ancestors_do_not_prevent_searching_an_accessible_start() -> TestResult {
    use std::os::unix::fs::PermissionsExt;
    if rustix::process::geteuid().is_root() {
        eprintln!("Skipped unreadable-ancestor search check: root can list mode 111 directories.");
        return Ok(());
    }
    let fixture = Fixture::new()?;
    let ancestor = fixture.project.join("ancestor");
    fs::create_dir_all(ancestor.join("start"))?;
    fs::write(ancestor.join(".gitignore"), "ignored.txt\n")?;
    fs::write(ancestor.join("start/ignored.txt"), "needle\n")?;
    fs::write(ancestor.join("start/ordinary.txt"), "needle\n")?;
    let context = fixture.context(Mode::Restricted)?;
    let tools = ToolSet::new(Mode::Restricted);
    fs::set_permissions(&ancestor, fs::Permissions::from_mode(0o111))?;
    let results = [
        tools.execute(
            &context,
            "glob",
            json!({"path":"ancestor/start", "pattern":"*.txt"}),
        ),
        tools.execute(
            &context,
            "grep",
            json!({"path":"ancestor/start", "pattern":"needle"}),
        ),
    ];
    fs::set_permissions(&ancestor, fs::Permissions::from_mode(0o700))?;
    for result in results {
        assert!(!result.is_error, "{}", result.text);
        assert!(result.text.contains("start/ordinary.txt"));
        assert!(result.text.contains("start/ignored.txt"));
        assert!(
            result
                .text
                .contains("Skipped unreadable directories: 1; first 5 paths: ancestor")
        );
    }
    Ok(())
}

// 作成時の関門: 走査の停止と取り消しは、結果を返す道具の境界の契約である。
// 既存の 500/100 件の出力上限は、探索の量と時間の上限を検証しない。
#[test]
fn traversal_limits_return_partial_results_and_cancellation() -> TestResult {
    use benitoiteh::tools::{ApprovalStatus, Invocation, SearchLimits};
    use std::sync::atomic::AtomicBool;
    use std::time::Duration;
    let fixture = Fixture::new()?;
    for number in 0..502 {
        fs::write(
            fixture.project.join(format!("file-{number:03}")),
            "needle\n",
        )?;
    }
    let tools = ToolSet::new(Mode::Restricted);
    let context = fixture
        .context(Mode::Restricted)?
        .with_search_limits(SearchLimits {
            max_entries: 501,
            ..SearchLimits::default()
        });
    let result = tools.execute(&context, "glob", json!({"pattern":"*"}));
    assert!(!result.is_error, "{}", result.text);
    assert!(result.text.contains("Showing 500 of at least 501 files"));
    assert!(result.text.contains("entry limit"));
    let context = fixture
        .context(Mode::Restricted)?
        .with_search_limits(SearchLimits {
            max_entries: 2,
            ..SearchLimits::default()
        });
    let result = tools.execute(&context, "grep", json!({"pattern":"needle"}));
    assert_eq!(result.text.matches(":1:needle").count(), 2);
    assert!(result.text.contains("entry limit"));
    for name in ["glob", "grep"] {
        let pattern = if name == "glob" { "*" } else { "needle" };
        let call = tools.validate(name, json!({"pattern":pattern}))?;
        let context = fixture
            .context(Mode::Restricted)?
            .with_search_limits(SearchLimits {
                max_duration: Duration::ZERO,
                ..SearchLimits::default()
            });
        let result = tools.execute(&context, name, call.arguments().clone());
        assert!(!result.is_error);
        assert!(result.text.contains("time limit"), "{}", result.text);
        assert!(!result.text.contains("Skipped .gitignore"));
        let context = fixture.context(Mode::Restricted)?;
        let result = tools.execute_validated(
            &context,
            &call,
            ApprovalStatus::Unapproved,
            Invocation {
                call_id: "cancel-search",
                cancelled: &AtomicBool::new(true),
                confirmed_effects: &std::collections::BTreeMap::new(),
                session_domains: &std::collections::BTreeSet::new(),
            },
        );
        assert!(!result.is_error);
        assert!(result.text.contains("cancelled"));
        assert!(!result.text.contains("Skipped .gitignore"));
    }
    let context = fixture
        .context(Mode::Restricted)?
        .with_search_limits(SearchLimits {
            max_file_bytes: 2,
            ..SearchLimits::default()
        });
    let result = tools.execute(
        &context,
        "grep",
        json!({"path":"file-000","pattern":"needle"}),
    );
    assert!(!result.is_error);
    assert!(result.text.contains("file-000 (too large)"));
    let tree = Fixture::new()?;
    fs::create_dir(tree.project.join("a"))?;
    fs::write(tree.project.join("a/x"), "needle\n")?;
    fs::write(tree.project.join("b"), "needle\n")?;
    let context = tree
        .context(Mode::Restricted)?
        .with_search_limits(SearchLimits {
            max_entries: 2,
            ..SearchLimits::default()
        });
    let result = tools.execute(&context, "grep", json!({"pattern":"needle"}));
    assert!(result.text.starts_with("a/x:1:needle\n"), "{}", result.text);
    assert!(!result.text.contains("b:1:"));
    assert!(result.text.contains("entry limit"));
    Ok(())
}

// 作成時の関門: 塊の境で UTF-8/CRLF を壊すこと、長い行を切って照合すること、
// context の重なる行を重複させることは、既存の小さな入力では捕まらない。
#[test]
fn chunked_read_and_grep_preserve_lines_and_regex_across_buffers() -> TestResult {
    watched(|| {
        let fixture = Fixture::new()?;
        let tools = ToolSet::new(Mode::Restricted);
        let context = fixture.context(Mode::Restricted)?;
        fs::write(
            fixture.project.join("wide"),
            format!("{}界\r\nlast\r\n", "a".repeat(8191)),
        )?;
        let result = tools.execute(&context, "read", json!({"path":"wide","offset":2}));
        assert_eq!(result.text, "     2\tlast\n");
        let result = tools.execute(&context, "read", json!({"path":"wide","limit":1}));
        assert!(result.text.contains("8192 chars"), "{}", result.text);
        assert!(
            result
                .text
                .ends_with("[Showing lines 1-1 of 2. Use offset=2 to continue.]")
        );
        for (position, binary) in [(8191, true), (8192, false)] {
            let mut bytes = b"needle".to_vec();
            bytes.resize(position, b'a');
            bytes.push(0);
            fs::write(fixture.project.join("binary-boundary"), bytes)?;
            let result = tools.execute(
                &context,
                "grep",
                json!({"path":"binary-boundary", "pattern":"needle"}),
            );
            assert!(!result.is_error, "{}", result.text);
            assert_eq!(
                result.text.contains("binary-boundary:1:needle"),
                !binary,
                "{}",
                result.text
            );
        }
        fs::write(
            fixture.project.join("long"),
            format!("before\nBEGIN{}END\nafter\n", "界".repeat(30_000)),
        )?;
        let result = tools.execute(
            &context,
            "grep",
            json!({"path":"long","pattern":"BEGIN.*END$","context":1}),
        );
        assert!(!result.is_error, "{}", result.text);
        assert!(result.text.starts_with("long:1:before\nlong:2:BEGIN"));
        assert!(result.text.ends_with("long:3:after\n"));
        assert!(result.text.contains("of 30008]"));
        assert!(!result.text.contains('\u{fffd}'));
        fs::write(fixture.project.join("context"), "first\nhit\nhit\nlast\n")?;
        let result = tools.execute(
            &context,
            "grep",
            json!({"path":"context","pattern":"hit","context":1}),
        );
        assert_eq!(
            result.text,
            "context:1:first\ncontext:2:hit\ncontext:3:hit\ncontext:4:last\n"
        );
        let result = tools.execute(
            &context,
            "grep",
            json!({"path":"context","pattern":"hit","context":u64::MAX}),
        );
        assert_eq!(
            result.text,
            "context:1:first\ncontext:2:hit\ncontext:3:hit\ncontext:4:last\n"
        );
        Ok(())
    })
}

#[test]
fn deep_search_finishes_with_a_small_descriptor_limit() -> TestResult {
    const CHILD: &str = "BENITOITEH_TEST_DEEP_SEARCH";
    if std::env::var_os(CHILD).is_some() {
        let fixture = Fixture::new()?;
        struct RestoreLimit(rustix::process::Rlimit);
        impl Drop for RestoreLimit {
            fn drop(&mut self) {
                let _restored =
                    rustix::process::setrlimit(rustix::process::Resource::Nofile, self.0);
            }
        }
        let limit = RestoreLimit(rustix::process::getrlimit(
            rustix::process::Resource::Nofile,
        ));
        rustix::process::setrlimit(
            rustix::process::Resource::Nofile,
            rustix::process::Rlimit {
                current: Some(64),
                maximum: limit.0.maximum,
            },
        )?;
        let mut directory = fixture.project.clone();
        for _ in 0..240 {
            directory.push("d");
            fs::create_dir(&directory)?;
            fs::write(directory.join("s"), "needle\n")?;
        }
        fs::write(directory.join("last.txt"), "needle\n")?;
        let context = fixture.context(Mode::Restricted)?;
        let tools = ToolSet::new(Mode::Restricted);
        for name in ["glob", "grep"] {
            let pattern = if name == "glob" { "*.txt" } else { "needle" };
            let args = if name == "glob" {
                json!({"pattern":pattern})
            } else {
                json!({"pattern":pattern,"glob":"*.txt"})
            };
            let result = tools.execute(&context, name, args);
            assert!(!result.is_error, "{}", result.text);
            assert!(result.text.contains("last.txt"), "{}", result.text);
            assert!(!result.text.contains("Skipped"));
            assert!(!result.text.contains("stopped"));
        }
        return Ok(());
    }
    // 子だけの記述子の上限を下げ、他の並行テストへ影響させない。
    let mut child = std::process::Command::new(std::env::current_exe()?)
        .args([
            "--exact",
            "deep_search_finishes_with_a_small_descriptor_limit",
            "--nocapture",
        ])
        .env(CHILD, "1")
        .spawn()?;
    let watcher = std::thread::spawn(move || -> Result<bool, std::io::Error> {
        let start = std::time::Instant::now();
        loop {
            if let Some(status) = child.try_wait()? {
                return Ok(status.success());
            }
            if start.elapsed() >= std::time::Duration::from_secs(15) {
                child.kill()?;
                child.wait()?;
                return Ok(false);
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    });
    assert!(
        watcher
            .join()
            .map_err(|_| std::io::Error::other("watcher failed"))??
    );
    Ok(())
}

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

// 関門: 行番号だけを外し、範囲・長い行の印・表示の件数・読んだ版を失わない契約。
// Reference と誤って共用する退行は、既存の番号付き read だけでは捕まらない。
#[test]
fn plain_read_preserves_ranges_truncation_and_screen_lines_in_both_modes() -> TestResult {
    for mode in [Mode::Restricted, Mode::Normal] {
        let f = Fixture::new()?;
        let context = f.context(mode)?.with_eval_switches(true, false);
        let tools = ToolSet::new(mode).plain_read();
        let file = f.project.join("plain.bnt");
        fs::write(&file, "one\ntwo\nthree\nfour\n")?;
        let result = tools.execute(
            &context,
            "read",
            json!({"path":"plain.bnt","offset":2,"limit":2}),
        );
        assert!(!result.is_error, "{}", result.text);
        assert_eq!(
            result.text,
            "two\nthree\n[Showing lines 2-3 of 4. Use offset=4 to continue.]"
        );
        assert_eq!(result.screen.as_deref(), Some("2 lines"));
        assert_eq!(
            context.read_version(&file)?,
            Some(FileVersion::from_metadata(&fs::metadata(&file)?))
        );
        let result = tools.execute(&context, "read", json!({"path":"plain.bnt","offset":4}));
        assert_eq!(result.text, "four\n");
        assert_eq!(result.screen.as_deref(), Some("1 lines"));
        let result = tools.execute(&context, "read", json!({"path":"plain.bnt","offset":5}));
        assert!(result.is_error);
        assert_eq!(
            result.text,
            "Offset 5 is beyond end of file (4 lines total)."
        );
        fs::write(&file, format!("{}\r\n", "界".repeat(2001)).repeat(20))?;
        let result = tools.execute(&context, "read", json!({"path":"plain.bnt"}));
        assert!(!result.is_error);
        assert!(result.text.starts_with(&"界".repeat(2000)));
        assert!(
            result
                .text
                .contains("[line 1 truncated: 2001 chars. Use grep to find text in this line.]")
        );
        assert!(
            result
                .text
                .ends_with("[Showing lines 1-8 of 20. Use offset=9 to continue.]")
        );
        assert_eq!(result.screen.as_deref(), Some("8 lines"));
        assert!(result.text.len() <= 50 * 1024);
        assert!(!result.text.contains('\t'));
        fs::write(&file, "short\n".repeat(2001))?;
        let result = tools.execute(&context, "read", json!({"path":"plain.bnt","limit":9000}));
        assert!(
            result
                .text
                .ends_with("[Showing lines 1-2000 of 2001. Use offset=2001 to continue.]")
        );
        assert_eq!(result.screen.as_deref(), Some("2000 lines"));
    }
    Ok(())
}

// 作成時の関門: テキストだけの既存の検査では、形式の判定・寸法・画像の拒否順序・
// 保存の失敗を捕まえられない。公開の道具の結果と objects/、読んだ版で確かめる。
fn png_header(width: u32, height: u32) -> Vec<u8> {
    let mut bytes = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
    bytes.extend(width.to_be_bytes());
    bytes.extend(height.to_be_bytes());
    bytes.extend([8, 2, 0, 0, 0, 0, 0, 0, 0]);
    bytes.extend(b"\0\0\0\0IDAT\0\0\0\0");
    bytes
}

fn webp_header(kind: &[u8; 4], chunk: &[u8]) -> Result<Vec<u8>, Box<dyn Error>> {
    let mut bytes = b"RIFF".to_vec();
    let padding = chunk.len() % 2;
    bytes.extend(
        u32::try_from(chunk.len().saturating_add(padding).saturating_add(12))?.to_le_bytes(),
    );
    bytes.extend(b"WEBP");
    bytes.extend(kind);
    bytes.extend(u32::try_from(chunk.len())?.to_le_bytes());
    bytes.extend(chunk);
    bytes.extend(std::iter::repeat_n(0, padding));
    Ok(bytes)
}

type ImageHeader = (&'static str, Vec<u8>);

fn image_headers() -> Result<Vec<ImageHeader>, Box<dyn Error>> {
    Ok(vec![
        ("image/png", png_header(321, 123)),
        (
            "image/jpeg",
            b"\xff\xd8\xff\xff\xe0\0\x04ab\xff\xd0\xff\x01\xff\xc0\0\x08\x08\0\x7b\x01\x41\0"
                .to_vec(),
        ),
        (
            "image/jpeg",
            b"\xff\xd8\xff\xc2\0\x08\x08\0\x7b\x01\x41\0".to_vec(),
        ),
        ("image/gif", b"GIF87a\x41\x01\x7b\0".to_vec()),
        ("image/gif", b"GIF89a\x41\x01\x7b\0".to_vec()),
        (
            "image/webp",
            webp_header(b"VP8 ", b"\0\0\0\x9d\x01\x2a\x41\x41\x7b\x40")?,
        ),
        (
            "image/webp",
            webp_header(b"VP8L", &[0x2f, 0x40, 0x81, 0x1e, 0])?,
        ),
        // VP8X の動きの印は受け付け、画面の寸法を使う。
        (
            "image/webp",
            webp_header(b"VP8X", &[2, 0, 0, 0, 0x40, 1, 0, 0x7a, 0, 0])?,
        ),
    ])
}

#[test]
fn read_image_formats_by_signature_and_record_versions_in_both_modes() -> TestResult {
    let fixture = Fixture::new()?;
    for mode in [Mode::Normal, Mode::Restricted] {
        let tools = ToolSet::new(mode);
        let context = fixture.context(mode)?.with_image_input(true);
        for (index, (mime, bytes)) in image_headers()?.into_iter().enumerate() {
            let name = if index % 2 == 0 { "image" } else { "image.txt" };
            let path = fixture.project.join(name);
            fs::write(&path, &bytes)?;
            let result = tools.execute(&context, "read", json!({"path":name}));
            assert!(!result.is_error, "{mime}/{index}: {}", result.text);
            assert_eq!(result.images.len(), 1);
            let image = result.images.first().ok_or("missing image")?;
            assert_eq!(image.media_type, mime);
            assert_eq!((image.width, image.height), (321, 123));
            assert_eq!(image.data.as_deref(), Some(bytes.as_slice()));
            assert_eq!(image.bytes, u64::try_from(bytes.len())?);
            assert_eq!(image.sha256.len(), 64);
            assert_eq!(
                result.text,
                format!("Read image file {mime} (321x123, {} bytes)", bytes.len())
            );
            assert_eq!(result.screen.as_ref(), Some(&result.text));
            assert_eq!(
                context.read_version(&path)?,
                Some(FileVersion::from_metadata(&fs::metadata(path)?))
            );
        }
    }
    Ok(())
}

#[test]
fn read_image_limits_reject_invalid_headers_and_preserve_text_fallback() -> TestResult {
    let fixture = Fixture::new()?;
    let tools = ToolSet::new(Mode::Restricted);
    let context = fixture.context(Mode::Restricted)?.with_image_input(true);
    let path = fixture.project.join("image");
    fs::write(&path, png_header(2000, 2000))?;
    let disabled = fixture.context(Mode::Restricted)?;
    for args in [
        json!({"path":"image","offset":1}),
        json!({"path":"image","limit":1}),
    ] {
        let output = tools.execute(&disabled, "read", args);
        assert!(output.is_error);
        assert!(output.text.contains("offset and limit"), "{}", output.text);
    }
    let output = tools.execute(&disabled, "read", json!({"path":"image"}));
    assert!(output.is_error);
    assert!(
        output
            .text
            .contains("current provider does not accept images")
    );
    fs::OpenOptions::new()
        .write(true)
        .open(&path)?
        .set_len(3_932_160)?;
    assert!(
        !tools
            .execute(&context, "read", json!({"path":"image"}))
            .is_error
    );
    fs::write(&path, b"GIF89a\xd0\x07\xd0\x07")?;
    fs::OpenOptions::new()
        .write(true)
        .open(&path)?
        .set_len(3_932_161)?;
    let output = tools.execute(&context, "read", json!({"path":"image"}));
    assert!(output.is_error);
    assert!(output.text.contains("3932161") && output.text.contains("3932160"));
    for (width, height, bytes) in [
        (2001, 123, b"GIF89a\xd1\x07\x7b\0".to_vec()),
        (123, 2001, b"GIF89a\x7b\0\xd1\x07".to_vec()),
        (
            2001,
            123,
            webp_header(b"VP8X", &[0, 0, 0, 0, 0xd0, 7, 0, 0x7a, 0, 0])?,
        ),
        (0, 123, png_header(0, 123)),
        (123, 0, png_header(123, 0)),
    ] {
        fs::write(&path, bytes)?;
        let output = tools.execute(&context, "read", json!({"path":"image"}));
        assert!(output.is_error);
        assert!(output.text.contains(&format!("{width}x{height}")) && output.text.contains("2000"));
        assert!(output.images.is_empty());
    }
    let mut animated = png_header(321, 123);
    animated.splice(
        33..33,
        b"\0\0\0\x08acTL\0\0\0\x01\0\0\0\0\0\0\0\0".iter().copied(),
    );
    fs::write(&path, animated)?;
    let output = tools.execute(&context, "read", json!({"path":"image"}));
    assert!(output.is_error && output.text.contains("Animated PNG"));
    for (mime, bytes) in image_headers()? {
        let signature_length = match mime {
            "image/png" => 8,
            "image/jpeg" => 3,
            "image/gif" => 6,
            _ => 12,
        };
        // 完全な見出しに至る前のどの長さでも panic せず拒む。
        for end in signature_length..bytes.len() {
            // JPEG は SOF の前に詰め物がある。GIF は 10 バイトで見出しが完結する。
            let part = bytes.get(..end).ok_or("bad test prefix")?;
            fs::write(&path, part)?;
            let output = tools.execute(&context, "read", json!({"path":"image"}));
            // PNG は IHDR のみでも寸法が読める。
            if mime == "image/png" && end == 33 {
                continue;
            }
            assert!(output.is_error, "{mime}/{end}: {}", output.text);
            assert!(output.images.is_empty());
        }
    }
    for (name, contents) in [
        ("picture.PNG", "ordinary text"),
        ("picture.svg", "<svg/>"),
        ("picture.bmp", "BM"),
    ] {
        fs::write(fixture.project.join(name), contents)?;
        let output = tools.execute(&context, "read", json!({"path":name}));
        assert!(output.is_error && output.text.contains("binary or image"));
    }
    for contents in [
        "PNG is plain text\n",
        "abcdefghijk界後半\n",
        "GIF89 is plain text\n",
    ] {
        fs::write(&path, contents)?;
        let output = tools.execute(&context, "read", json!({"path":"image"}));
        assert_eq!(output.text, format!("     1\t{contents}"));
        assert!(output.images.is_empty());
    }
    Ok(())
}

#[test]
fn read_image_stores_object_once_and_fails_when_object_cannot_be_saved() -> TestResult {
    use benitoiteh::session::{Session, SessionId};
    let fixture = Fixture::new()?;
    let session_paths = DataPaths::from_values(
        Some(&fixture.directory),
        Some(&fixture.directory.join("session-config")),
        Some(&fixture.directory.join("session-state")),
        Some(&fixture.temporary),
    )?;
    let session = Session::create(
        &session_paths,
        SessionId::generate(std::time::SystemTime::now())?,
    )?;
    let context = fixture
        .context(Mode::Restricted)?
        .with_image_input(true)
        .with_session(session.clone());
    let tools = ToolSet::new(Mode::Restricted);
    let bytes = png_header(321, 123);
    fs::write(fixture.project.join("image"), &bytes)?;
    let result = tools.execute(&context, "read", json!({"path":"image"}));
    assert!(!result.is_error, "{}", result.text);
    let image = result.images.first().ok_or("missing image")?;
    assert_eq!(
        fs::read(session.directory().join("objects").join(&image.sha256))?,
        bytes
    );
    assert_eq!(
        session.store_object(&bytes)?,
        Some((image.sha256.clone(), false))
    );
    assert_eq!(
        tools
            .execute(&context, "read", json!({"path":"image"}))
            .images,
        result.images
    );
    assert!(
        context
            .read_version(&fixture.project.join("image"))?
            .is_some()
    );
    // 容量がなくても、既に保存した同じ画像は再利用できる。別の画像は返さない。
    session.configure_budget(&benitoiteh::session::StorageUsage {
        total: benitoiteh::session::TOTAL_LIMIT,
        current: 0,
        current_outputs: 0,
    });
    assert!(
        !tools
            .execute(&context, "read", json!({"path":"image"}))
            .is_error
    );
    fs::write(fixture.project.join("another"), png_header(322, 123))?;
    let output = tools.execute(&context, "read", json!({"path":"another"}));
    assert!(output.is_error && output.images.is_empty());
    assert!(output.text.contains("storage limit"), "{}", output.text);
    assert_eq!(
        context.read_version(&fixture.project.join("another"))?,
        None
    );
    assert_eq!(
        fs::read_dir(session.directory().join("objects"))?.count(),
        1
    );
    // 名前が正しくても同名のディレクトリでは保存に失敗する。

    fs::remove_file(session.directory().join("objects").join(&image.sha256))?;
    fs::create_dir(session.directory().join("objects").join(&image.sha256))?;
    let output = tools.execute(&context, "read", json!({"path":"image"}));
    assert!(output.is_error);
    assert!(output.images.is_empty());
    Ok(())
}
