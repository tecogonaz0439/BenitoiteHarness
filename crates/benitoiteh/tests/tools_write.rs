//! 書く道具の安全規則と置換の契約を実行器で確かめる（設計書 07-01「持ち主の境界」）。

use std::error::Error;
use std::fs;
use std::os::unix::fs::symlink;
use std::path::PathBuf;
use std::time::{Duration, SystemTime};

use benitoiteh::config::{Mode, ReadConfig, paths::DataPaths};
use benitoiteh::safe_fs::ScopeRoot;
use benitoiteh::tools::exclude::ExcludeRules;
use benitoiteh::tools::{ApprovalStatus, FileVersion, Invocation, ToolContext, ToolSet};
use serde_json::{Value, json};

type TestResult = Result<(), Box<dyn Error>>;

struct Fixture {
    directory: PathBuf,
    project: PathBuf,
    temporary: PathBuf,
    context: ToolContext,
    tools: ToolSet,
}

impl Fixture {
    fn new(mode: Mode) -> Result<Self, Box<dyn Error>> {
        let mut random = [0u8; 16];
        getrandom::fill(&mut random)?;
        let directory = std::env::temp_dir().join(format!(
            "benitoiteh-write-{:032x}",
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
        );
        Ok(Self {
            directory,
            project,
            temporary,
            context,
            tools: ToolSet::new(mode),
        })
    }

    fn read(&self, path: &str) {
        let output = self
            .tools
            .execute(&self.context, "read", json!({"path":path}));
        assert!(!output.is_error, "{}", output.text);
    }

    fn success(&self, tool: &str, args: Value) -> benitoiteh::tools::ToolOutput {
        let output = self.tools.execute(&self.context, tool, args);
        assert!(!output.is_error, "{}", output.text);
        assert!(output.approval.is_none(), "{}", output.text);
        output
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _removed = fs::remove_dir_all(&self.directory);
    }
}

fn args(tool: &str, path: Value) -> Value {
    if tool == "write" {
        json!({"path":path, "content":"after\n"})
    } else {
        json!({"path":path, "old_string":"before", "new_string":"after"})
    }
}

#[test]
fn writes_create_parents_report_only_summary_and_keep_versions_for_following_edits() -> TestResult {
    for mode in [Mode::Restricted, Mode::Normal] {
        let f = Fixture::new(mode)?;
        for path in [
            f.project.join("new/deep/main.bnt"),
            f.temporary.join("new/scratch.bnt"),
        ] {
            let output = f.success("write", json!({"path":path, "content":"one\ntwo\n"}));
            assert_eq!(output.text, format!("Wrote {} (2 lines).", path.display()));
            assert!(
                output
                    .display
                    .as_deref()
                    .is_some_and(|diff| diff.contains("+one\n+two\n"))
            );
            assert_eq!(fs::read(&path)?, b"one\ntwo\n");
            assert_eq!(
                f.context.read_version(&path)?,
                Some(FileVersion::from_metadata(&fs::metadata(&path)?))
            );
            f.success(
                "edit",
                json!({"path":path, "old_string":"two", "new_string":"three"}),
            );
            let output = f.success("write", json!({"path":path, "content":"four\n"}));
            assert!(
                output
                    .display
                    .as_deref()
                    .is_some_and(|diff| diff.contains("-one\n-three\n+four\n"))
            );
            f.success(
                "edit",
                json!({"path":path, "old_string":"four", "new_string":"five"}),
            );
            assert_eq!(fs::read(&path)?, b"five\n");
        }
        for (name, content, count) in [("empty.bnt", "", 0), ("last.bnt", "last", 1)] {
            let output = f.success("write", json!({"path":name, "content":content}));
            assert_eq!(output.text, format!("Wrote {name} ({count} lines)."));
        }
    }
    Ok(())
}

#[test]
fn replacing_requires_a_read_and_detects_external_content_inode_and_timestamp_changes() -> TestResult
{
    for tool in ["write", "edit"] {
        let f = Fixture::new(Mode::Restricted)?;
        let path = f.project.join("source.bnt");
        fs::write(&path, "before\nsecond\n")?;
        let output = f
            .tools
            .execute(&f.context, tool, args(tool, json!("source.bnt")));
        assert!(output.is_error);
        assert_eq!(
            output.text,
            "File has not been read in this session. Read it with read, then retry."
        );
        for change in ["size", "inode", "mtime"] {
            f.read("source.bnt");
            match change {
                "size" => fs::write(&path, "before\nsecond\nadded\n")?,
                "inode" => {
                    let replacement = f.project.join("replacement.bnt");
                    fs::write(&replacement, fs::read(&path)?)?;
                    fs::rename(replacement, &path)?;
                }
                _ => fs::File::options()
                    .write(true)
                    .open(&path)?
                    .set_modified(SystemTime::now() + Duration::from_secs(60))?,
            }
            let before = fs::read(&path)?;
            let output = f
                .tools
                .execute(&f.context, tool, args(tool, json!("source.bnt")));
            assert!(output.is_error);
            assert_eq!(
                output.text,
                "File changed since it was last read. Read it again, then retry."
            );
            assert_eq!(fs::read(&path)?, before);
        }
        let output = f
            .tools
            .execute(&f.context, "read", json!({"path":"source.bnt", "limit":1}));
        assert!(!output.is_error);
        f.success(tool, args(tool, json!("source.bnt")));
        f.success(
            "edit",
            json!({"path":"source.bnt", "old_string":"after", "new_string":"done"}),
        );
        assert!(fs::read_to_string(path)?.starts_with("done\n"));
    }
    Ok(())
}

#[test]
fn restricted_writes_reject_extensions_escape_and_real_protected_paths() -> TestResult {
    let f = Fixture::new(Mode::Restricted)?;
    let outside = f.directory.join("outside");
    fs::create_dir(&outside)?;
    for name in [".git", ".benitoiteh", "sub/.git", ".GIT", "sub/.BeNiToItEh"] {
        fs::create_dir_all(f.project.join(name))?;
    }
    symlink(&outside, f.project.join("escape"))?;
    symlink(".git", f.project.join("hidden"))?;
    let paths = [
        (f.project.join("bad.txt"), ".bnt"),
        (f.project.join("bad.BNT"), ".bnt"),
        (outside.join("source.bnt"), "scope"),
        (f.project.join("../outside/source.bnt"), "safely"),
        (f.project.join("escape/source.bnt"), "scope"),
        (f.project.join(".git/source.bnt"), "cannot write inside"),
        (
            f.project.join(".benitoiteh/source.bnt"),
            "cannot write inside",
        ),
        (f.project.join("sub/.git/source.bnt"), "cannot write inside"),
        (f.project.join(".GIT/source.bnt"), "cannot write inside"),
        (
            f.project.join("sub/.BeNiToItEh/source.bnt"),
            "cannot write inside",
        ),
        (f.project.join("hidden/source.bnt"), "cannot write inside"),
    ];
    for (path, reason) in paths {
        fs::write(&path, "before\n")?;
        for tool in ["write", "edit"] {
            let output = f.tools.execute(&f.context, tool, args(tool, json!(path)));
            assert!(
                output.is_error && output.approval.is_none(),
                "{tool} {path:?}: {}",
                output.text
            );
            assert!(
                output.text.contains(reason),
                "{tool} {path:?}: {}",
                output.text
            );
            assert_eq!(fs::read(&path)?, b"before\n");
        }
    }
    let output = f.tools.execute(
        &f.context,
        "write",
        args(
            "write",
            json!(f.context.spill.real_path().join("source.bnt")),
        ),
    );
    assert!(output.is_error);
    let output = f.tools.execute(
        &f.context,
        "write",
        args("write", json!(".git/missing/deep/source.bnt")),
    );
    assert!(output.is_error && output.text.contains("cannot write inside"));
    assert!(!f.project.join(".git/missing").exists());
    Ok(())
}

#[test]
fn final_links_including_dangling_hooks_and_multiple_hardlinks_are_never_written() -> TestResult {
    for mode in [Mode::Restricted, Mode::Normal] {
        let f = Fixture::new(mode)?;
        fs::create_dir_all(f.project.join(".git/hooks"))?;
        fs::write(f.project.join("actual.bnt"), "before\n")?;
        symlink("actual.bnt", f.project.join("link.bnt"))?;
        symlink(".git/hooks/pre-commit", f.project.join("x.bnt"))?;
        fs::hard_link(f.project.join("actual.bnt"), f.project.join("hard.bnt"))?;
        f.read("actual.bnt");
        f.read("link.bnt");
        f.read("hard.bnt");
        for tool in ["write", "edit"] {
            for (path, reason) in [
                ("link.bnt", "symbolic link"),
                ("x.bnt", "symbolic link"),
                ("hard.bnt", "hard link"),
            ] {
                let output = f.tools.execute(&f.context, tool, args(tool, json!(path)));
                assert!(
                    output.is_error && output.approval.is_none(),
                    "{mode:?} {tool} {path}: {}",
                    output.text
                );
                assert!(output.text.contains(reason), "{}", output.text);
            }
        }
        assert!(!f.project.join(".git/hooks/pre-commit").exists());
        assert!(
            fs::symlink_metadata(f.project.join("x.bnt"))?
                .file_type()
                .is_symlink()
        );
        assert_eq!(fs::read(f.project.join("actual.bnt"))?, b"before\n");
    }
    Ok(())
}

#[test]
fn normal_mode_requests_approval_before_any_mutation_and_accepts_trusted_approval() -> TestResult {
    let f = Fixture::new(Mode::Normal)?;
    fs::create_dir(f.project.join(".git"))?;
    symlink(".git", f.project.join("alias"))?;
    for (path, protected) in [
        (f.directory.join("outside/new.bnt"), false),
        (f.project.join(".git/hooks/new.bnt"), true),
        (f.project.join(".benitoiteh/new.bnt"), true),
        (f.project.join("sub/.git/config"), true),
        (f.project.join(".GIT/config"), true),
        (f.project.join("alias/through-link"), true),
        (f.temporary.join(".benitoiteh/config"), true),
    ] {
        let parent_existed = path.parent().is_some_and(|parent| parent.exists());
        for tool in ["write", "edit"] {
            let output = f.tools.execute(&f.context, tool, args(tool, json!(path)));
            assert!(
                !output.is_error && output.approval.is_some(),
                "{}",
                output.text
            );
            let reason = if protected {
                "protected location"
            } else {
                "outside the writable roots"
            };
            assert!(output.text.contains(reason) && output.text.contains("Ask the user"));
            assert_eq!(
                path.parent().is_some_and(|parent| parent.exists()),
                parent_existed
            );
        }
        let call = f.tools.validate("write", args("write", json!(path)))?;
        let output = f.tools.execute_validated(
            &f.context,
            &call,
            ApprovalStatus::Approved,
            Invocation {
                session_domains: &Default::default(),
                call_id: "write-test",
                cancelled: &std::sync::atomic::AtomicBool::new(false),
            },
        );
        assert!(
            !output.is_error && output.approval.is_none(),
            "{}",
            output.text
        );
        assert_eq!(fs::read(&path)?, b"after\n");
        let call = f.tools.validate(
            "edit",
            json!({"path":path, "old_string":"after", "new_string":"approved"}),
        )?;
        let output = f.tools.execute_validated(
            &f.context,
            &call,
            ApprovalStatus::Approved,
            Invocation {
                session_domains: &Default::default(),
                call_id: "write-test",
                cancelled: &std::sync::atomic::AtomicBool::new(false),
            },
        );
        assert!(!output.is_error, "{}", output.text);
        assert_eq!(fs::read(&path)?, b"approved\n");
    }
    f.success("write", json!({"path":"source.txt", "content":"before\n"}));
    f.success("edit", args("edit", json!("source.txt")));
    let output = f.tools.execute(
        &f.context,
        "write",
        json!({"path":".git/forged", "content":"bad", "approved":true}),
    );
    assert!(output.is_error && output.text.contains("Unknown argument"));
    assert!(!f.project.join(".git/forged").exists());
    Ok(())
}

#[test]
fn edit_errors_are_actionable_and_exact_matching_precedes_folding() -> TestResult {
    let f = Fixture::new(Mode::Restricted)?;
    f.success(
        "write",
        json!({"path":"source.bnt", "content":"one one\n'smart'\n‘smart’\n"}),
    );
    for (old, new, error) in [
        (
            "missing",
            "new",
            "Could not find old_string in source.bnt. It must match the file exactly, including whitespace and line breaks. Read the file again if it may have changed.",
        ),
        (
            "one",
            "new",
            "Found 2 occurrences of old_string in source.bnt. Add surrounding lines to make it unique, or set replace_all to true.",
        ),
        (
            "one",
            "one",
            "old_string and new_string are identical. No change was made to source.bnt.",
        ),
        (
            "",
            "new",
            "old_string must not be empty. Use write to create a file.",
        ),
    ] {
        let before = fs::read(f.project.join("source.bnt"))?;
        let output = f.tools.execute(
            &f.context,
            "edit",
            json!({"path":"source.bnt", "old_string":old, "new_string":new}),
        );
        assert!(output.is_error);
        assert_eq!(output.text, error);
        assert_eq!(fs::read(f.project.join("source.bnt"))?, before);
    }
    f.success(
        "edit",
        json!({"path":"source.bnt", "old_string":"'smart'", "new_string":"exact"}),
    );
    f.success(
        "edit",
        json!({"path":"source.bnt", "old_string":"one", "new_string":"two", "replace_all":true}),
    );
    assert_eq!(
        fs::read(f.project.join("source.bnt"))?,
        "two two\nexact\n‘smart’\n".as_bytes()
    );
    let output = f.tools.execute(
        &f.context,
        "edit",
        json!({"path":"source.bnt", "old_string":"'smart'", "new_string":"‘smart’"}),
    );
    assert!(output.is_error);
    assert_eq!(
        output.text,
        "old_string and new_string are identical. No change was made to source.bnt."
    );
    Ok(())
}

#[test]
fn folded_edits_preserve_untouched_bytes_bom_and_crlf_and_replace_all() -> TestResult {
    let f = Fixture::new(Mode::Restricted)?;
    for (before, old, new, expected) in [
        (
            "head ‘keep’  \nα\t \nbeta \n尾—  \n",
            "α\nbeta",
            "changed",
            "head ‘keep’  \nchanged \n尾—  \n",
        ),
        (
            "\u{feff}head  \r\n‘alpha’  \r\nbeta\t\r\n尾—  \r\n",
            "'alpha'\nbeta",
            "new\nline",
            "\u{feff}head  \r\nnew\r\nline\t\r\n尾—  \r\n",
        ),
        (
            "\u{feff}old\r\nkeep\t \r\n",
            "old",
            "new\nnext",
            "\u{feff}new\r\nnext\r\nkeep\t \r\n",
        ),
        ("a\r\nb\r\nc\r\n", "a\r\nb", "new", "new\r\nc\r\n"),
        (
            "同じ\t \n同じ  \nlast\n",
            "同じ\nlast",
            "changed",
            "同じ\t \nchanged\n",
        ),
    ] {
        f.success("write", json!({"path":"source.bnt", "content":before}));
        let output = f.success(
            "edit",
            json!({"path":"source.bnt", "old_string":old, "new_string":new}),
        );
        assert_eq!(fs::read(f.project.join("source.bnt"))?, expected.as_bytes());
        assert_eq!(
            output.text,
            format!("Wrote source.bnt ({} lines).", expected.lines().count())
        );
        assert!(output.display.is_some());
    }
    f.success(
        "write",
        json!({"path":"source.bnt", "content":"‘x’\r\n'x'  \r\n"}),
    );
    let output = f.tools.execute(
        &f.context,
        "edit",
        json!({"path":"source.bnt", "old_string":"‘x’\n", "new_string":"y\n"}),
    );
    assert!(output.is_error && output.text.starts_with("Found 2 occurrences"));
    f.success(
        "edit",
        json!({"path":"source.bnt", "old_string":"‘x’\n", "new_string":"y\n", "replace_all":true}),
    );
    assert_eq!(fs::read(f.project.join("source.bnt"))?, b"y\r\ny\r\n");
    Ok(())
}

#[test]
fn all_selected_quote_and_dash_variants_match_without_changing_neighboring_text() -> TestResult {
    let f = Fixture::new(Mode::Restricted)?;
    for (variants, ascii) in [("‘’‚‛", '\''), ("“”„‟", '"'), ("‐‑‒–—―−", '-')]
    {
        for variant in variants.chars() {
            let before = format!("{variant}keep  \nfind{variant}here\n{variant}keep\t\n");
            f.success("write", json!({"path":"source.bnt", "content":before}));
            f.success("edit", json!({"path":"source.bnt", "old_string":format!("find{ascii}here"), "new_string":"changed"}));
            assert_eq!(
                fs::read_to_string(f.project.join("source.bnt"))?,
                format!("{variant}keep  \nchanged\n{variant}keep\t\n")
            );
        }
    }
    Ok(())
}

#[test]
fn existing_file_rules_use_real_spelling_on_case_insensitive_filesystems() -> TestResult {
    let f = Fixture::new(Mode::Restricted)?;
    fs::write(f.project.join("upper.BNT"), "before\n")?;
    if !f.project.join("upper.bnt").exists() {
        return Ok(());
    }
    for tool in ["write", "edit"] {
        let output = f
            .tools
            .execute(&f.context, tool, args(tool, json!("upper.bnt")));
        assert!(
            output.is_error && output.text.contains("only .bnt"),
            "{}",
            output.text
        );
    }
    fs::write(f.project.join("lower.bnt"), "before\n")?;
    f.read("LOWER.BNT");
    f.success("edit", args("edit", json!("LOWER.BNT")));
    assert_eq!(fs::read(f.project.join("lower.bnt"))?, b"after\n");
    Ok(())
}
