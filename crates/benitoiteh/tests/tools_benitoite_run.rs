//! 実行前の道具の引数、報告、確認の境界を確かめる（設計書 01-03、04-01）。
#[allow(dead_code)]
#[path = "support/benitoite_run.rs"]
mod fixture;
#[allow(dead_code)]
mod support;
use benitoiteh::approval::{self, ApprovalKind};
use benitoiteh::benitoite::{Effect, diagnostics};
use benitoiteh::tools::{ConfirmKey, ToolSet};
use fixture::*;
use serde_json::json;
use std::fs;
use std::sync::atomic::AtomicBool;

// 関門: 実際の check --report の警告、パス解決、検査の誤りを実行器の境界で守る。
#[test]
fn inspection_precedes_confirmation_and_rejects_bad_paths_reports_and_changes() -> TestResult {
    let mut f = Fixture::new()?;
    let stop = AtomicBool::new(false);
    fs::write(f.project.join("-pure.bnt"), PURE)?;
    fs::write(f.project.join("text.txt"), PURE)?;
    fs::write(
        f.project.join("bad.bnt"),
        "function main() -> Unit\n return UNKNOWN\nend function",
    )?;
    for args in [
        json!({"path":"text.txt"}),
        json!({"path":"-pure.bnt","timeout":0}),
        json!({"path":"-pure.bnt","timeout":601}),
        json!({"path":"../outside.bnt"}),
        json!({"path":"-pure.bnt","approved":true}),
    ] {
        let output = f.call(args, false, "invalid", &stop);
        assert!(
            output.is_error && output.approval.is_none() && output.processor_launches == 0,
            "{output:?}"
        );
    }
    let bad = f.call(json!({"path":"bad.bnt"}), false, "bad", &stop);
    assert!(
        bad.is_error && bad.text.contains("E0301") && bad.processor_launches == 1,
        "{bad:?}"
    );
    let first = f.call(json!({"path":"-pure.bnt"}), false, "first", &stop);
    let request = first.approval.as_ref().ok_or("request")?;
    assert_eq!(request.kind, ApprovalKind::Effects);
    assert!(request.facts.effects.is_empty() && request.facts.added_effects.is_empty());
    assert!(!request.facts.sources.is_empty());
    assert_eq!(request.summary, "Run -pure.bnt with effects: none");
    assert_eq!(
        approval::user_denial_message(request.kind, &request.facts, &f.project, Some("wait")),
        "The user declined to run -pure.bnt with effects: none. Ask the user how to proceed, or change the script.\nMessage from the user: wait"
    );
    f.confirm(&first)?;
    let expanded = "import Benitoite.Unofficial.IO.Console\nfunction main() -> Unit uses Console.Write\n Console.write(\"hello\")\nend function\n";
    fs::write(f.project.join("-pure.bnt"), expanded)?;
    let changed = f.call(json!({"path":"-pure.bnt"}), true, "changed", &stop);
    assert!(
        changed.is_error
            && changed.text.starts_with("The script changed")
            && changed.processor_launches == 1,
        "{changed:?}"
    );
    let ask = f.call(json!({"path":"-pure.bnt"}), false, "ask", &stop);
    let facts = &ask.approval.as_ref().ok_or("new request")?.facts;
    assert_eq!(facts.effects, ["Console.Write"]);
    assert_eq!(facts.added_effects, ["Console.Write"]);
    f.confirm(&ask)?;
    let key = ConfirmKey::Run(f.project.join("-pure.bnt"));
    assert!(
        f.confirmed
            .get(&key)
            .is_some_and(|e| e.contains(&Effect::ConsoleWrite))
    );
    // 警告は run 側で一度だけ示すので、要求にもエラーにも混ぜない。
    fs::write(
        f.project.join("warning.bnt"),
        "function main() -> Unit\n bind x <- 1 div 0\n return ()\nend function",
    )?;
    let warning = f.call(json!({"path":"warning.bnt"}), false, "warning", &stop);
    assert!(
        !warning.text.contains("warning[") && warning.approval.is_some(),
        "{warning:?}"
    );
    assert!(
        ToolSet::new(benitoiteh::config::Mode::Normal)
            .validate("benitoite_run", json!({"path":"x.bnt"}))
            .is_err()
    );
    assert!(
        ToolSet::without_benitoite(benitoiteh::config::Mode::Restricted)
            .validate("benitoite_run", json!({"path":"x.bnt"}))
            .is_err()
    );
    let same = f.call(json!({"path":"-pure.bnt"}), false, "same", &stop);
    assert!(same.approval.is_none(), "{same:?}");
    fs::write(
        f.project.join("shell.bnt"),
        "import Benitoite.Unofficial.IO.Process\nfunction main() -> Unit uses Process.Run\n bind _ <- Process.shell(\"true\")\nend function\n",
    )?;
    let shell = f.call(json!({"path":"shell.bnt"}), false, "shell", &stop);
    assert!(
        shell
            .approval
            .as_ref()
            .is_some_and(|r| r.facts.shell_reference && r.facts.effects == ["Process.Run"]),
        "{shell:?}"
    );
    Ok(())
}

// 関門: die と runtime が診断に読めないと次の道具は実装できない。本物の行で確認する。
#[test]
fn real_die_and_runtime_reports_use_the_existing_diagnostic_contract() -> TestResult {
    let f = Fixture::new()?;
    for (name, source, kind, code) in [
        (
            "die",
            "import Benitoite.Unofficial.IO.Process\nfunction main() -> Unit uses Process.Exit\n bind value <- try Option.None or die(\"failure\")\nend function",
            "die",
            None,
        ),
        (
            "runtime",
            "import Benitoite.Unofficial.IO.Console\nfunction main() -> Unit uses Console.Write\n Console.write(\"${1 div 0}\")\nend function",
            "runtime",
            Some("R0101"),
        ),
    ] {
        let path = f.project.join(format!("{name}.bnt"));
        fs::write(&path, source)?;
        let out = std::process::Command::new(support::benitoite()?)
            .args([
                "run",
                "--diagnostics=json",
                "--allow-effects=Console.Write,Process.Exit",
            ])
            .arg(path)
            .output()?;
        assert_eq!(out.status.code(), Some(1));
        let parsed = diagnostics::parse(&out.stderr);
        assert!(
            parsed
                .diagnostics
                .iter()
                .any(|d| d.kind == kind && d.code.as_deref() == code),
            "{parsed:?}"
        );
    }
    Ok(())
}
