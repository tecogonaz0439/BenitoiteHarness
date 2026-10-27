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
