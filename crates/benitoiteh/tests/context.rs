//! LLM に見せる全文、環境の追記、見積もりの契約（設計書 01-04、07-01「LLM に見せる文の固定」）。

#[path = "support/processor.rs"]
mod processor;
#[path = "support/skill.rs"]
mod skill;

use std::error::Error;
use std::path::{Path, PathBuf};
use std::time::{Duration, UNIX_EPOCH};

use benitoiteh::config::Mode;
use benitoiteh::context::{
    ContextError, Environment, SystemContext, estimate_tokens, system_prompt_hash,
    tool_definitions_hash, tool_definitions_json, transmission_notice,
};
use benitoiteh::provider::Usage;
use benitoiteh::tools::ToolSet;

#[test]
fn llm_context_matches_reviewed_files_in_both_modes() -> Result<(), Box<dyn Error>> {
    // 更新は examples/update_context_expected.rs を明示的に実行し、全文を確認する。
    // このテストでは期待ファイルを一切書き換えない（設計書 07-01「LLM に見せる文の固定」）。
    let documents = skill::skills()?;
    let skill = documents.document("SKILL.md", &std::sync::atomic::AtomicBool::new(false))?;
    let cases = [
        (
            Mode::Restricted,
            true,
            true,
            include_str!("expected/system-restricted.txt"),
            include_str!("expected/tools-restricted.json"),
        ),
        (
            Mode::Normal,
            true,
            true,
            include_str!("expected/system-normal.txt"),
            include_str!("expected/tools-normal.json"),
        ),
    ];
    for (mode, enabled, line_numbers, expected_system, expected_tools) in cases.into_iter().chain([
        (
            Mode::Normal,
            false,
            true,
            include_str!("expected/system-normal-without-benitoite.txt"),
            include_str!("expected/tools-normal-without-benitoite.json"),
        ),
        (
            Mode::Restricted,
            true,
            false,
            include_str!("expected/system-restricted-read-plain.txt"),
            include_str!("expected/tools-restricted-read-plain.json"),
        ),
    ]) {
        let tools = if enabled {
            ToolSet::new(mode)
        } else {
            ToolSet::without_benitoite(mode)
        };
        let tools = if line_numbers {
            tools
        } else {
            tools.plain_read()
        };
        let context = SystemContext::new(&tools, enabled.then_some(skill.as_ref()))?;
        if mode == Mode::Restricted {
            assert!(!context.system_prompt.contains("<!--"));
            assert!(
                !context
                    .system_prompt
                    .contains("How the code examples in this file are checked")
            );
        }
        assert_eq!(context.system_prompt, expected_system, "{mode:?}");
        assert_eq!(
            format!(
                "{}\n",
                serde_json::to_string_pretty(&tool_definitions_json(&context.tools))?
            ),
            expected_tools,
            "{mode:?}",
        );
        assert_eq!(
            context.system_prompt.contains("# Benitoite\n"),
            mode == Mode::Restricted
        );
        assert!(!context.system_prompt.contains("{{benitoite-version}}"));
        assert!(!context.system_prompt.contains("<environment>"));

        let environment = Environment::capture(
            Path::new("/workspace/context-project"),
            mode,
            &[PathBuf::from("/workspace/context-project")],
            false,
            UNIX_EPOCH + Duration::from_secs(951782400),
        )?;
        let environment = environment.render();
        for value in ["/workspace/context-project", "2000-02-29"] {
            assert!(environment.contains(value));
            assert!(!context.system_prompt.contains(value));
        }
    }
    Ok(())
}

#[test]
fn hashes_identify_prompt_bytes_and_ordered_plain_tool_definitions() -> Result<(), Box<dyn Error>> {
    // SHA-256 の既知の値を基準にして、同じ実装同士の比較だけにしない。
    assert_eq!(
        system_prompt_hash("abc"),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
    assert_eq!(
        tool_definitions_hash(&[])?,
        "4f53cda18c2baa0c0354bb5f9a3ecbe5ed12ab4d8e11ba873c2f11161202b945"
    );
    assert_ne!(system_prompt_hash("abc"), system_prompt_hash("abc\n"));

    let tools = ToolSet::new(Mode::Restricted).specs();
    let expected: serde_json::Value =
        serde_json::from_str(include_str!("expected/tools-restricted.json"))?;
    assert_eq!(
        tool_definitions_hash(&tools)?,
        system_prompt_hash(&serde_json::to_string(&expected)?)
    );
    let mut reordered = tools.clone();
    reordered.reverse();
    assert_ne!(
        tool_definitions_hash(&tools)?,
        tool_definitions_hash(&reordered)?
    );
    let mut changed = tools.clone();
    if let Some(tool) = changed.first_mut() {
        tool.description.push('.');
    }
    assert_ne!(
        tool_definitions_hash(&tools)?,
        tool_definitions_hash(&changed)?
    );
    Ok(())
}

#[test]
fn environment_renders_full_state_and_only_changed_fields() -> Result<(), Box<dyn Error>> {
    let project = Path::new("/workspace/<project&>");
    let roots = [project.to_path_buf(), PathBuf::from("/session/tmp")];
    let before_midnight = UNIX_EPOCH + Duration::from_secs(86399);
    let initial = Environment::capture(project, Mode::Restricted, &roots, false, before_midnight)?;
    assert_eq!(
        initial.render(),
        format!(
            concat!(
                "<environment>\n",
                "<project_directory>/workspace/&lt;project&amp;&gt;</project_directory>\n",
                "<os>{}</os>\n",
                "<date_utc>1970-01-01</date_utc>\n",
                "<mode>restricted</mode>\n",
                "<writable_roots>[\"/workspace/&lt;project&amp;&gt;\",\"/session/tmp\"]</writable_roots>\n",
                "<network>denied</network>\n",
                "<benitoite_version>not configured</benitoite_version>\n",
                "</environment>\n",
            ),
            std::env::consts::OS
        )
    );
    assert_eq!(initial.changes_since(&initial), None);

    let next_day = Environment::capture(
        project,
        Mode::Restricted,
        &roots,
        false,
        UNIX_EPOCH + Duration::from_secs(86400),
    )?;
    assert_eq!(
        next_day.changes_since(&initial).as_deref(),
        Some("<environment>\n<date_utc>1970-01-02</date_utc>\n</environment>\n")
    );
    let relocated = Environment::capture(
        Path::new("/new-project"),
        Mode::Restricted,
        &roots,
        false,
        before_midnight,
    )?;
    assert_eq!(
        relocated.changes_since(&initial).as_deref(),
        Some(
            "<environment>\n<project_directory>/new-project</project_directory>\n</environment>\n"
        )
    );
    let read_only = Environment::capture(project, Mode::Normal, &[], true, before_midnight)?;
    assert_eq!(
        read_only.changes_since(&initial).as_deref(),
        Some(concat!(
            "<environment>\n<mode>normal</mode>\n",
            "<writable_roots>[]</writable_roots>\n<network>allowed</network>\n</environment>\n"
        ))
    );
    Ok(())
}

#[test]
fn environment_uses_utc_calendar_dates_and_rejects_unrepresentable_input()
-> Result<(), Box<dyn Error>> {
    for (seconds, date) in [
        (951782399, "2000-02-28"),
        (951782400, "2000-02-29"),
        (951868800, "2000-03-01"),
        (4107456000, "2100-02-28"),
        (4107542400, "2100-03-01"),
        (253402300799, "9999-12-31"),
    ] {
        let environment = Environment::capture(
            Path::new("/project"),
            Mode::Normal,
            &[],
            false,
            UNIX_EPOCH + Duration::from_secs(seconds),
        )?;
        assert!(
            environment
                .render()
                .contains(&format!("<date_utc>{date}</date_utc>")),
            "{seconds}"
        );
    }
    for now in [
        UNIX_EPOCH - Duration::from_secs(1),
        UNIX_EPOCH + Duration::from_secs(253402300800),
    ] {
        assert!(matches!(
            Environment::capture(Path::new("/project"), Mode::Normal, &[], false, now),
            Err(ContextError::InvalidDate)
        ));
    }
    assert!(matches!(
        Environment::capture(Path::new("relative"), Mode::Normal, &[], false, UNIX_EPOCH),
        Err(ContextError::InvalidPath)
    ));
    assert!(matches!(
        Environment::capture(
            Path::new("/project"),
            Mode::Normal,
            &[PathBuf::from("relative")],
            false,
            UNIX_EPOCH
        ),
        Err(ContextError::InvalidPath)
    ));
    Ok(())
}

#[test]
fn estimate_uses_reported_total_or_all_utf8_bytes() -> Result<(), Box<dyn Error>> {
    let cases = [
        (None, 0, 0, 0),
        (None, 10, 1, 4),
        (Some(0), 1000, 1, 1),
        (Some(300), 9000, 0, 300),
        (Some(300), 9000, 9, 303),
        (Some(300), 9000, 10, 304),
        (Some(u64::MAX), 9000, 1, u64::MAX),
        (None, u64::MAX, 0, 6148914691236517205),
    ];
    for (input_tokens, total, added, expected) in cases {
        let usage = Usage {
            input_tokens,
            cache_read_tokens: Some(200),
            cache_write_tokens: Some(50),
            ..Usage::default()
        };
        assert_eq!(estimate_tokens(Some(&usage), total, added), expected);
    }
    let japanese_bytes = u64::try_from("日本語".len())?;
    assert_eq!(estimate_tokens(None, japanese_bytes, 0), 3);
    assert_eq!(estimate_tokens(None, 10, 1000), 4);
    Ok(())
}

#[test]
fn transmission_notice_names_destination_and_sent_content_on_one_line() {
    assert_eq!(
        transmission_notice("example", "model", "api.example.invalid"),
        concat!(
            "Provider: \"example\"; model: \"model\"; host: \"api.example.invalid\". ",
            "Read file contents, script and command output, and diagnostics are sent to this provider."
        )
    );
    assert_eq!(
        transmission_notice("provider\nname", "model\rname", "host")
            .lines()
            .count(),
        1
    );
}

// 収集の安全条件は本物のファイルで、起動から送信・記録への接続は exec のテストで守る
// （設計書 01-04「プロジェクトの指示のファイル」、07-01「持ち主の境界」）。
struct InstructionsFixture {
    root: PathBuf,
    paths: benitoiteh::config::paths::DataPaths,
}

impl InstructionsFixture {
    fn new() -> Result<Self, Box<dyn Error>> {
        let id = benitoiteh::session::SessionId::generate(std::time::SystemTime::now())?;
        let root = std::env::temp_dir().join(format!("benitoiteh-instructions-{id}"));
        std::fs::create_dir(&root)?;
        let root = std::fs::canonicalize(root)?;
        let paths = benitoiteh::config::paths::DataPaths::from_values(
            Some(&root.join("home")),
            Some(&root.join("config")),
            Some(&root.join("state")),
            Some(&root),
        )?;
        std::fs::create_dir_all(&paths.config_dir)?;
        Ok(Self { root, paths })
    }

    fn write(&self, path: &str, contents: impl AsRef<[u8]>) -> std::io::Result<()> {
        let path = self.root.join(path);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, contents)
    }

    fn collect(
        &self,
        project: &str,
        home: &str,
        mode: Mode,
    ) -> Result<benitoiteh::context::instructions::ProjectInstructions, Box<dyn Error>> {
        let project = self.root.join(project);
        let rules = benitoiteh::tools::exclude::ExcludeRules::new(
            &project,
            &self.paths,
            &self.root.join("spill"),
            &benitoiteh::config::ReadConfig::default(),
            &benitoiteh::config::ReadConfig::default(),
        )?;
        Ok(benitoiteh::context::instructions::collect(
            mode,
            &project,
            &self.paths.instructions_file,
            &self.root.join(home),
            &rules,
        ))
    }
}

impl Drop for InstructionsFixture {
    fn drop(&mut self) {
        let _removed = std::fs::remove_dir_all(&self.root);
    }
}

#[test]
fn instructions_follow_repository_order_fallback_and_home_boundary() -> Result<(), Box<dyn Error>> {
    for git_file in [false, true] {
        let f = InstructionsFixture::new()?;
        f.write("AGENTS.md", "outside")?;
        if git_file {
            f.write("repo/.git", "gitdir: elsewhere")?;
        } else {
            std::fs::create_dir_all(f.root.join("repo/.git"))?;
        }
        f.write("repo/AGENTS.md", "root")?;
        f.write("repo/CLAUDE.md", "ignored")?;
        f.write("repo/sub/CLAUDE.md", "middle")?;
        f.write("repo/sub/project/AGENTS.md", "project")?;
        std::os::unix::fs::symlink("AGENTS.md", f.root.join("repo/sub/project/CLAUDE.md"))?;
        f.write("config/benitoiteh/AGENTS.md", "user")?;
        f.write("config/benitoiteh/CLAUDE.md", "ignored-user")?;
        let result = f.collect("repo/sub/project", "home", Mode::Normal)?;
        assert_eq!(
            result
                .files
                .iter()
                .map(|file| file.contents.as_str())
                .collect::<Vec<_>>(),
            ["user", "root", "middle", "project"]
        );
        assert!(result.skipped.is_empty());
        assert_eq!(
            result
                .files
                .iter()
                .map(|file| file.path.clone())
                .collect::<Vec<_>>(),
            [
                "config/benitoiteh/AGENTS.md",
                "repo/AGENTS.md",
                "repo/sub/CLAUDE.md",
                "repo/sub/project/AGENTS.md"
            ]
            .map(|path| f.root.join(path))
        );
        // ホームと同じ根、ホームより上の根は、どちらも祖先の指示を入れない。
        for home in ["repo", "repo/sub"] {
            let result = f.collect("repo/sub/project", home, Mode::Normal)?;
            assert_eq!(
                result
                    .files
                    .iter()
                    .map(|file| file.contents.as_str())
                    .collect::<Vec<_>>(),
                ["user", "project"]
            );
        }
        let result = f.collect("repo/sub/project", "home", Mode::Restricted)?;
        assert!(result.files.is_empty() && result.skipped.is_empty());
        if git_file {
            std::fs::remove_file(f.root.join("repo/.git"))?;
        } else {
            std::fs::remove_dir(f.root.join("repo/.git"))?;
        }
        let result = f.collect("repo/sub/project", "home", Mode::Normal)?;
        assert_eq!(
            result
                .files
                .iter()
                .map(|file| file.contents.as_str())
                .collect::<Vec<_>>(),
            ["user", "project"]
        );
    }
    Ok(())
}

#[test]
fn instructions_deduplicate_relative_links_and_allow_user_links() -> Result<(), Box<dyn Error>> {
    let f = InstructionsFixture::new()?;
    f.write("repo/.git", "worktree")?;
    f.write("repo/AGENTS.md", "root")?;
    std::fs::create_dir_all(f.root.join("repo/project"))?;
    std::os::unix::fs::symlink("../AGENTS.md", f.root.join("repo/project/AGENTS.md"))?;
    f.write("user-source", "user")?;
    std::os::unix::fs::symlink("../../user-source", &f.paths.instructions_file)?;
    let result = f.collect("repo/project", "home", Mode::Normal)?;
    assert_eq!(
        result
            .files
            .iter()
            .map(|file| file.contents.as_str())
            .collect::<Vec<_>>(),
        ["user", "root"]
    );
    let skipped = result
        .skipped
        .first()
        .ok_or("missing duplicate diagnostic")?;
    assert_eq!(skipped.path, f.root.join("repo/project/AGENTS.md"));
    assert!(skipped.reason.contains("already been included"));
    Ok(())
}

#[test]
fn instructions_reject_unsafe_candidates_without_claude_fallback() -> Result<(), Box<dyn Error>> {
    use std::os::unix::fs::symlink;
    for (kind, reason) in [
        ("outside", "scope"),
        ("secret", "secret"),
        ("hardlink", "hard link"),
        ("directory", "regular file"),
        ("utf8", "utf-8"),
        ("broken", "does not exist"),
    ] {
        let f = InstructionsFixture::new()?;
        f.write("outside", "never send")?;
        f.write("repo/.git", "worktree")?;
        f.write("repo/.env", "secret")?;
        f.write("repo/CLAUDE.md", "must not fall back")?;
        let candidate = f.root.join("repo/AGENTS.md");
        match kind {
            "outside" => symlink("../outside", &candidate)?,
            "secret" => symlink(".env", &candidate)?,
            "hardlink" => std::fs::hard_link(f.root.join("outside"), &candidate)?,
            "directory" => std::fs::create_dir(&candidate)?,
            "utf8" => std::fs::write(&candidate, [0xff, 0xfe])?,
            "broken" => symlink("missing", &candidate)?,
            _ => return Err("unknown test case".into()),
        }
        let result = f.collect("repo", "home", Mode::Normal)?;
        assert!(result.files.is_empty(), "{kind}");
        assert_eq!(result.skipped.len(), 1, "{kind}");
        let skipped = result.skipped.first().ok_or("missing exclusion")?;
        assert_eq!(skipped.path, candidate);
        assert!(
            skipped.reason.to_lowercase().contains(reason),
            "{kind}: {}",
            skipped.reason
        );
    }
    Ok(())
}

#[test]
fn instructions_apply_byte_limits_and_preserve_utf8_boundaries() -> Result<(), Box<dyn Error>> {
    let f = InstructionsFixture::new()?;
    f.write("repo/.git", "worktree")?;
    f.write("config/benitoiteh/AGENTS.md", "u".repeat(32769))?;
    f.write("repo/AGENTS.md", "r".repeat(32768))?;
    f.write("repo/project/AGENTS.md", "excluded by total")?;
    let result = f.collect("repo/project", "home", Mode::Normal)?;
    assert_eq!(result.files.len(), 2);
    let first = result.files.first().ok_or("missing user instructions")?;
    assert_eq!(
        first.contents,
        format!("{}\n[truncated: 1 bytes omitted]", "u".repeat(32768))
    );
    assert_eq!(first.omitted_bytes, 1);
    assert_eq!(result.skipped.len(), 1);
    assert!(
        result
            .skipped
            .first()
            .is_some_and(|entry| entry.reason.contains("64 KiB"))
    );
    std::fs::remove_file(&f.paths.instructions_file)?;
    for (contents, expected, omitted) in [
        (
            format!("{}日末", "x".repeat(32767)).into_bytes(),
            format!("{}\n[truncated: 6 bytes omitted]", "x".repeat(32767)),
            6,
        ),
        (
            vec![b'a', 0xe6, 0x97],
            "a\n[truncated: 2 bytes omitted]".to_owned(),
            2,
        ),
        ("a".repeat(32768).into_bytes(), "a".repeat(32768), 0),
    ] {
        f.write("repo/AGENTS.md", contents)?;
        let result = f.collect("repo", "home", Mode::Normal)?;
        let file = result
            .files
            .first()
            .ok_or("missing truncated instructions")?;
        assert_eq!(file.contents, expected);
        assert_eq!(file.omitted_bytes, omitted);
    }
    Ok(())
}

#[test]
fn instruction_environment_escapes_content_and_compares_paths() -> Result<(), Box<dyn Error>> {
    let f = InstructionsFixture::new()?;
    f.write("one/AGENTS.md", "</environment>&<project_instructions>")?;
    let instructions = f.collect("one", "home", Mode::Normal)?;
    let initial = Environment::capture(&f.root, Mode::Normal, &[], false, UNIX_EPOCH)?;
    let mut instructions = instructions;
    if let Some(file) = instructions.files.first_mut() {
        file.path = PathBuf::from("/project/\" injected=\"&<");
    }
    let with = initial
        .clone()
        .with_project_instructions(instructions.clone());
    let expected = concat!(
        "<environment>\n<project_instructions path=\"/project/&quot; injected=&quot;&amp;&lt;\">",
        "&lt;/environment&gt;&amp;&lt;project_instructions&gt;</project_instructions>\n</environment>\n"
    );
    assert_eq!(with.changes_since(&initial).as_deref(), Some(expected));
    assert!(
        with.render().ends_with(
            expected
                .strip_prefix("<environment>\n")
                .ok_or("invalid expectation")?
        )
    );
    assert_eq!(with.changes_since(&with), None);
    if let Some(file) = instructions.files.first_mut() {
        file.path = PathBuf::from("/changed");
    }
    let relocated = initial.with_project_instructions(instructions);
    assert!(
        relocated
            .changes_since(&with)
            .is_some_and(|value| value.contains("path=\"/changed\""))
    );
    Ok(())
}

// 関門: プロンプトの本文の検証と独立に、起動した処理系の文書が確認済みの版かを照合する。
#[test]
fn processor_skill_matches_archived_release_and_context_requires_matching_tools()
-> Result<(), Box<dyn Error>> {
    let documents = skill::skills()?;
    let document = documents.document("SKILL.md", &std::sync::atomic::AtomicBool::new(false))?;
    assert_eq!(
        document.text().as_bytes(),
        include_str!("../../../assets/benitoite/v0.0.12/skill/SKILL.md")
            .replace("{{benitoite-version}}", "0.0.12")
            .as_bytes()
    );
    for (tools, skill) in [
        (ToolSet::new(Mode::Restricted), None),
        (ToolSet::without_benitoite(Mode::Restricted), None),
        (ToolSet::new(Mode::Normal), None),
        (
            ToolSet::without_benitoite(Mode::Normal),
            Some(document.as_ref()),
        ),
    ] {
        assert!(matches!(
            SystemContext::new(&tools, skill),
            Err(ContextError::SkillMismatch)
        ));
    }
    Ok(())
}
