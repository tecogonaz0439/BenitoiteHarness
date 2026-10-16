//! LLM に見せる全文、環境の追記、見積もりの契約（設計書 01-04、07-01「LLM に見せる文の固定」）。

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
    let cases = [
        (
            Mode::Restricted,
            include_str!("expected/system-restricted.txt"),
            include_str!("expected/tools-restricted.json"),
        ),
        (
            Mode::Normal,
            include_str!("expected/system-normal.txt"),
            include_str!("expected/tools-normal.json"),
        ),
    ];
    for (mode, expected_system, expected_tools) in cases {
        let context = SystemContext::new(&ToolSet::new(mode))?;
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
