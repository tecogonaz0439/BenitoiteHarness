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

// 関門: 実の取り込み先を Skill の根で読めないと、読める兄弟まで毎回の確認になる。
#[test]
fn loaded_skill_scripts_and_sibling_imports_are_readable_for_confirmation() -> TestResult {
    let f = Fixture::with_user_skill(
        "import Helper\nfunction main() -> Unit\n Helper.value()\nend function\n",
    )?;
    let directory = f.paths.home.join(".agents/skills/sample");
    fs::write(
        directory.join("Helper.bnt"),
        "public function value() -> Unit\n return ()\nend function\n",
    )?;
    let output = f.call(
        json!({"path":directory.join("main.bnt")}),
        false,
        "skill",
        &AtomicBool::new(false),
    );
    let request = output.approval.as_ref().ok_or(format!("{output:?}"))?;
    assert_eq!(request.facts.unread_imports, 0, "{request:?}");
    assert_eq!(request.facts.targets, [directory.join("main.bnt")]);
    assert_eq!(
        request.summary,
        format!(
            "Run {} with effects: none",
            directory.join("main.bnt").display()
        )
    );
    assert!(request.facts.sources.len() >= 2);
    Ok(())
}

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
    assert!(request.facts.restricted.is_none());
    assert!(
        serde_json::to_value(&request.facts)?
            .get("restricted")
            .is_none()
    );
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

// 関門: 実行前の確認が、再検査で使いうる和の権限より狭い範囲を示す退行を守る。
#[test]
fn restricted_facts_cover_current_and_previously_confirmed_effects() -> TestResult {
    let mut f = Fixture::new()?;
    f.sandbox()?;
    let run = f.context.run.as_mut().ok_or("run context")?;
    run.allowed_programs = vec!["/usr/bin/sort".into()];
    let processor = run.processor.clone();
    let stop = AtomicBool::new(false);
    for (uses, write, network, programs) in [
        ("", false, false, false),
        ("File.Write", true, false, false),
        ("Http.Connect", false, true, false),
        ("Process.Run", false, false, true),
        ("Console.Write", true, true, true),
    ] {
        let declaration = if uses.is_empty() {
            String::new()
        } else {
            format!(" uses {uses}")
        };
        fs::write(
            f.project.join("scope.bnt"),
            format!(
                "import Benitoite.Unofficial.IO.File\nimport Benitoite.Unofficial.Network.Http\nimport Benitoite.Unofficial.IO.Process\nimport Benitoite.Unofficial.IO.Console\nfunction main() -> Unit{declaration}\n return ()\nend function\n"
            ),
        )?;
        let key = ConfirmKey::Run(f.project.join("scope.bnt"));
        f.confirmed.clear();
        if uses == "Console.Write" {
            f.confirmed.insert(
                key,
                [Effect::FileWrite, Effect::HttpConnect, Effect::ProcessRun]
                    .into_iter()
                    .collect(),
            );
        }
        let output = f.call(json!({"path":"scope.bnt"}), false, "scope", &stop);
        let facts = &output.approval.as_ref().ok_or(format!("{output:?}"))?.facts;
        let restricted = facts.restricted.as_ref().ok_or("restricted facts")?;
        let mut writable = Vec::new();
        if write {
            writable.push(f.project.clone());
        }
        writable.push(f.session.temporary_dir().into());
        assert_eq!(restricted.writable, writable, "{uses}");
        assert_eq!(restricted.network, network, "{uses}");
        assert_eq!(restricted.process_run, programs, "{uses}");
        let mut expected_programs = vec![processor.clone()];
        if programs {
            expected_programs.push("/usr/bin/sort".into());
        }
        assert_eq!(restricted.programs, expected_programs, "{uses}");
        // 確認の記録で使う直列化でも実の範囲を失わない。
        let json = serde_json::to_value(facts)?;
        assert_eq!(
            json.get("restricted")
                .and_then(|value| value.get("network")),
            Some(&serde_json::Value::Bool(network))
        );
        assert_eq!(
            serde_json::from_value::<approval::ApprovalFacts>(json)?,
            *facts
        );
        if uses.is_empty() {
            // 欄の追加より前の確認の記録も読める。
            let mut legacy = serde_json::to_value(facts)?;
            legacy
                .get_mut("restricted")
                .and_then(serde_json::Value::as_object_mut)
                .ok_or("restricted record")?
                .remove("process_run");
            let legacy: approval::ApprovalFacts = serde_json::from_value(legacy)?;
            assert!(!legacy.restricted.ok_or("legacy restricted")?.process_run);
        }
    }
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
