//! exec の引数の契約を解釈の入口で確かめる（設計書 05-01「オプション」）。

use std::error::Error;
use std::ffi::OsString;
use std::io::Cursor;
use std::os::unix::ffi::OsStringExt;
use std::path::PathBuf;

use benitoiteh::cli::args::{ArgsError, ExecArgs, Request};
use benitoiteh::config::{Approval, Mode, Sandbox};

// 作成時の関門: 引数の落とし込み、stdin とフラグ、非 UTF-8 のパスを失う退行を捕まえる。
#[test]
fn exec_arguments_preserve_explicit_values_and_the_request_source() -> Result<(), Box<dyn Error>> {
    let args = ExecArgs::parse([
        "--mode=normal",
        "--project",
        "dir",
        "--provider",
        "p",
        "--model",
        "m",
        "--effort",
        "high",
        "--max-turns",
        "12",
        "--max-check-fix",
        "3",
        "--max-run-tokens",
        "400",
        "--sandbox",
        "read-only",
        "--approval",
        "never",
        "--no-sandbox",
        "--json",
        "--output-last-message",
        "answer.txt",
        "-",
    ])?;
    assert_eq!(args.mode, Some(Mode::Normal));
    assert_eq!(args.project, Some(PathBuf::from("dir")));
    assert_eq!(args.provider.as_deref(), Some("p"));
    assert_eq!(args.model.as_deref(), Some("m"));
    assert_eq!(args.effort.as_deref(), Some("high"));
    assert_eq!(args.max_turns, Some(12));
    assert_eq!(args.max_check_fix, Some(3));
    assert_eq!(args.max_run_tokens, Some(400));
    assert_eq!(args.sandbox, Some(Sandbox::ReadOnly));
    assert_eq!(args.approval, Some(Approval::Never));
    assert!(args.no_sandbox);
    assert!(args.json);
    assert_eq!(args.output_last_message, Some(PathBuf::from("answer.txt")));
    assert_eq!(args.request, Request::Stdin);
    assert_eq!(
        args.request.read(&mut Cursor::new("stdin request"))?,
        "stdin request"
    );
    let args = ExecArgs::parse(["--", "--request"])?;
    assert_eq!(args.request, Request::Text("--request".to_owned()));
    assert_eq!(args.request.read(&mut Cursor::new("unused"))?, "--request");
    assert_eq!(
        ExecArgs::parse(["--resume", "id", "request"])?
            .resume
            .as_deref(),
        Some("id")
    );
    assert!(ExecArgs::parse(["--continue", "request"])?.continue_session);
    assert!(ExecArgs::parse(["--ephemeral", "request"])?.ephemeral);
    let path = OsString::from_vec(vec![b'p', 0xff]);
    let args = ExecArgs::parse([
        OsString::from("--project"),
        path.clone(),
        OsString::from("request"),
    ])?;
    assert_eq!(args.project, Some(PathBuf::from(path)));
    Ok(())
}

// 作成時の関門: 同じ指定の後勝ち、欠けた値、誤った値、未対応の指定を受理する退行を表で捕まえる。
#[test]
fn exec_arguments_reject_duplicates_invalid_values_and_unavailable_options() {
    for option in [
        "--mode",
        "--project",
        "--project-parent",
        "--provider",
        "--model",
        "--effort",
        "--max-turns",
        "--max-check-fix",
        "--max-run-tokens",
        "--output-last-message",
        "--sandbox",
        "--approval",
    ] {
        let value = match option {
            "--mode" => "restricted",
            "--sandbox" => "workspace-write",
            "--approval" => "never",
            "--max-turns" | "--max-check-fix" | "--max-run-tokens" => "1",
            _ => "value",
        };
        assert!(
            matches!(
                ExecArgs::parse([option, value, option, value, "request"]),
                Err(ArgsError::Duplicate(_))
            ),
            "{option}"
        );
        assert!(ExecArgs::parse([option]).is_err(), "{option}");
    }
    assert!(matches!(
        ExecArgs::parse(["--json", "--json", "request"]),
        Err(ArgsError::Duplicate(_))
    ));
    for args in [
        vec![],
        vec!["--unknown", "request"],
        vec!["--mode", "invalid", "request"],
        vec!["--sandbox", "invalid", "request"],
        vec!["--approval", "invalid", "request"],
        vec!["--no-sandbox=true", "request"],
        vec!["--max-turns", "-1", "request"],
        vec!["--max-turns", "18446744073709551616", "request"],
        vec!["--max-check-fix", "abc", "request"],
        vec!["--json=true", "request"],
        vec!["one", "two"],
        vec!["--resume", "id", "--continue", "request"],
        // 記録を残さないセッションは、既存の記録を続けない。
        vec!["--ephemeral", "--resume", "id", "request"],
        vec!["--ephemeral", "--continue", "request"],
        vec!["--ephemeral=true", "request"],
    ] {
        assert!(ExecArgs::parse(args.clone()).is_err(), "{args:?}");
    }
    assert!(
        ExecArgs::parse(["--project-parent", "/project", "request"])
            .is_ok_and(|args| args.project_parent == Some(PathBuf::from("/project")))
    );
    assert!(matches!(
        ExecArgs::parse_interactive(["--project-parent", "/project"]),
        Err(ArgsError::Invalid)
    ));
    // 対話では、.agents への書き込みは一回ごとの確認で決めるので、許す指定を受け付けない。
    assert!(
        ExecArgs::parse(["--allow-agents-write", "request"])
            .is_ok_and(|args| args.allow_agents_write)
    );
    for args in [
        vec!["--allow-agents-write=yes", "request"],
        vec!["--allow-agents-write", "--allow-agents-write", "request"],
    ] {
        assert!(ExecArgs::parse(args.clone()).is_err(), "{args:?}");
    }
    assert!(matches!(
        ExecArgs::parse_interactive(["--allow-agents-write"]),
        Err(ArgsError::Invalid)
    ));
    // 信頼は対話では確認で決めるので、--trust-project も exec の非対話の形だけで受け付ける。
    assert!(ExecArgs::parse(["--trust-project", "request"]).is_ok_and(|args| args.trust_project));
    assert!(ExecArgs::parse(["--trust-project=yes", "request"]).is_err());
    assert!(ExecArgs::parse(["--trust-project", "--trust-project", "request"]).is_err());
    assert!(matches!(
        ExecArgs::parse_interactive(["--trust-project"]),
        Err(ArgsError::Invalid)
    ));
}

// 関門: --allow の名前の検査を CLI の境界で守る。集合の比較だけでは不正な指定の受理を捕まえない。
#[test]
fn allow_accepts_atomic_effects_and_rejects_aliases_duplicates_and_interactive_use()
-> Result<(), Box<dyn Error>> {
    use benitoiteh::benitoite::Effect;
    assert_eq!(
        ExecArgs::parse(["--allow", "Assert.Check,File.Write,Log", "request"])?.allow,
        Some(
            [Effect::AssertCheck, Effect::FileWrite, Effect::Log]
                .into_iter()
                .collect()
        )
    );
    for value in [
        "IO.All",
        "Fail",
        "File.Unknown",
        "File.Read,File.Read",
        "File.Read,",
        ",File.Read",
        "",
        " File.Read",
    ] {
        assert!(
            matches!(
                ExecArgs::parse(["--allow", value, "request"]),
                Err(ArgsError::Invalid)
            ),
            "{value}"
        );
    }
    assert!(matches!(
        ExecArgs::parse_interactive(["--allow", "Assert.Check"]),
        Err(ArgsError::Invalid)
    ));
    Ok(())
}

// 作成時の関門: `debug prompt-input` は exec と同じ指定を受け、出力と記録と既存のセッションの指定を
// 拒み、記録を残さない形にする（設計書 05-01「debug prompt-input」）。
#[test]
fn prompt_input_arguments_reject_output_and_session_options() -> Result<(), ArgsError> {
    let args = ExecArgs::parse_preview(["--mode", "normal", "request"])?;
    assert!(args.preview && args.ephemeral);
    assert_eq!(args.mode, Some(Mode::Normal));
    for args in [
        vec!["--json", "request"],
        vec!["--output-last-message", "file", "request"],
        vec!["--ephemeral", "request"],
        vec!["--resume", "id", "request"],
        vec!["--continue", "request"],
        vec![],
    ] {
        assert!(ExecArgs::parse_preview(args.clone()).is_err(), "{args:?}");
    }
    Ok(())
}

// 作成時の関門: `ExecArgs::parse_acp` と 5 分離の方針が、ACP 専用の許容・拒否オプションと
// 制限モードの `--approval on-request|never`（`untrusted` 拒否）を設計書 05-01・05-03 のとおり
// 判定する契約を守る。`ExecArgs::parse` / `parse_interactive` のテストでは ACP の組み合わせを捕まえない。
#[test]
fn acp_arguments_enforce_policy_and_restricted_approval_rules() -> Result<(), Box<dyn Error>> {
    use benitoiteh::benitoite::Effect;
    use benitoiteh::cli::args::{Entrypoint, RuntimeApprovalPolicy};
    use benitoiteh::config::{ProjectConfig, UserConfig};
    use std::path::Path;

    let args = ExecArgs::parse_acp([
        "--mode",
        "restricted",
        "--approval",
        "on-request",
        "--allow",
        "File.Read,Console.Write",
        "--allow-agents-write",
        "--trust-project",
        "--project-parent",
        "/tmp/projects",
        "--ephemeral",
        "--max-turns",
        "20",
        "--max-check-fix",
        "4",
        "--max-run-tokens",
        "8000",
    ])?;
    assert_eq!(args.entrypoint, Entrypoint::Acp);
    let policy = args.policy();
    assert!(!policy.startup_confirmations);
    assert_eq!(policy.runtime_approval, RuntimeApprovalPolicy::Acp);
    assert!(policy.preapproval_options);
    assert!(policy.network_proxy_prompts);
    assert!(!policy.askpass);
    assert_eq!(args.mode, Some(Mode::Restricted));
    assert_eq!(args.approval, Some(Approval::OnRequest));
    assert_eq!(
        args.allow,
        Some(
            [Effect::FileRead, Effect::ConsoleWrite]
                .into_iter()
                .collect()
        )
    );
    assert!(args.allow_agents_write);
    assert!(args.trust_project);
    assert_eq!(args.project_parent, Some(PathBuf::from("/tmp/projects")));
    assert!(args.ephemeral);
    assert_eq!(args.max_turns, Some(20));
    assert_eq!(args.max_check_fix, Some(4));
    assert_eq!(args.max_run_tokens, Some(8000));

    let never_args = ExecArgs::parse_acp(["--mode", "restricted", "--approval", "never"])?;
    assert_eq!(never_args.approval, Some(Approval::Never));

    let normal_untrusted = ExecArgs::parse_acp([
        "--mode",
        "normal",
        "--approval",
        "untrusted",
        "--sandbox",
        "read-only",
        "--no-sandbox",
    ])?;
    assert_eq!(normal_untrusted.mode, Some(Mode::Normal));
    assert_eq!(normal_untrusted.approval, Some(Approval::Untrusted));
    assert_eq!(normal_untrusted.sandbox, Some(Sandbox::ReadOnly));
    assert!(normal_untrusted.no_sandbox);

    // ACP で禁止された指定と、制限モードでの `--approval untrusted` を拒む。
    for invalid in [
        vec!["--mode", "restricted", "--approval", "untrusted"],
        vec!["--json"],
        vec!["--output-last-message", "out.txt"],
        vec!["--output-schema", "schema.json"],
        vec!["--resume", "session-id"],
        vec!["--continue"],
        vec!["request"],
        vec!["-"],
        vec!["--", "request"],
    ] {
        assert!(
            matches!(
                ExecArgs::parse_acp(invalid.clone()),
                Err(ArgsError::Invalid)
            ),
            "{invalid:?}"
        );
    }

    // `--mode` 省略時は `UserConfig.default_mode` で解決し、制限モードなら `untrusted` を拒み、
    // 通常モードなら受理する。制限モードの `on-request` と `never` は受理する。
    let restricted_user: UserConfig = toml::from_str(
        "default_mode = 'restricted'\ndefault_provider = 'p'\n[providers.p]\nkind = 'openai-chat'\nmodel = 'm'\n",
    )?;
    let normal_user: UserConfig = toml::from_str(
        "default_mode = 'normal'\ndefault_provider = 'p'\n[providers.p]\nkind = 'openai-chat'\nmodel = 'm'\n",
    )?;
    let untrusted_default_mode = ExecArgs::parse_acp(["--approval", "untrusted"])?;
    assert!(
        restricted_user
            .resolve_exec(
                &untrusted_default_mode,
                &ProjectConfig::default(),
                Path::new("/project")
            )
            .is_err()
    );
    assert!(
        normal_user
            .resolve_exec(
                &untrusted_default_mode,
                &ProjectConfig::default(),
                Path::new("/project")
            )
            .is_ok()
    );
    for approval in ["on-request", "never"] {
        let parsed = ExecArgs::parse_acp(["--approval", approval])?;
        assert!(
            restricted_user
                .resolve_exec(&parsed, &ProjectConfig::default(), Path::new("/project"))
                .is_ok(),
            "{approval}"
        );
    }
    Ok(())
}
