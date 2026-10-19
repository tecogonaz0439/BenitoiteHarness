//! 本物の処理系と OS のサンドボックスでテストの実行と秘密の除外を確かめる（設計書 07-01）。
#[allow(dead_code)]
#[path = "support/benitoite_run.rs"]
mod fixture;
#[allow(dead_code)]
mod support;
use benitoiteh::tools::{ApprovalStatus, Invocation, ToolOutput};
use fixture::*;
use serde_json::{Value, json};
use std::{
    collections::BTreeSet,
    fs,
    sync::{Arc, atomic::AtomicBool},
    time::Duration,
};
const PASS: &str = "import Benitoite.Assert\n@test(\"passes\")\nfunction passes() -> Unit uses Assert.Check\n Assert.equal(actual: 1, expected: 1)\nend function\n";

// 関門: Skill のテストが、確認の後に本物のサンドボックスの中で動くことを守る。
#[test]
fn loaded_skill_tests_run_after_confirmation() -> TestResult {
    let mut f = Fixture::with_user_skill(PASS)?;
    f.sandbox()?;
    let directory = f.paths.home.join(".agents/skills/sample");
    let output = execute(&mut f, json!([directory]), "skill")?;
    assert!(
        !output.is_error && output.text.contains("Tests: 1 passed, 0 failed."),
        "{output:?}"
    );
    Ok(())
}
fn call(f: &Fixture, paths: Value, approved: bool, id: &str, stop: &AtomicBool) -> ToolOutput {
    let call = match f.tools.validate("benitoite_test", json!({"paths":paths})) {
        Ok(c) => c,
        Err(e) => return ToolOutput::error(e.to_string()),
    };
    f.tools.execute_validated(
        &f.context,
        &call,
        if approved {
            ApprovalStatus::Approved
        } else {
            ApprovalStatus::Unapproved
        },
        Invocation {
            call_id: id,
            cancelled: stop,
            confirmed_effects: &f.confirmed,
            session_domains: &BTreeSet::new(),
        },
    )
}
fn execute(
    f: &mut Fixture,
    paths: Value,
    id: &str,
) -> Result<ToolOutput, Box<dyn std::error::Error>> {
    let stop = AtomicBool::new(false);
    let first = call(f, paths.clone(), false, id, &stop);
    if first.approval.is_none() {
        return Ok(first);
    }
    f.confirm(&first)?;
    Ok(call(f, paths, true, id, &stop))
}
// 関門: 本物の対象の重なり、Assert.Check、全体の検査、集計の条件を実行器で守る。
#[test]
fn files_directories_overlap_and_empty_targets_run_with_the_reported_effects() -> TestResult {
    let mut f = Fixture::new()?;
    f.sandbox()?;
    fs::create_dir(f.project.join("tests"))?;
    fs::write(f.project.join("tests/a.bnt"), PASS)?;
    let out = execute(
        &mut f,
        json!(["tests", "tests/a.bnt", "tests/a.bnt"]),
        "overlap",
    )?;
    assert!(
        !out.is_error
            && out
                .text
                .contains("Tests: 2 passed, 0 failed.\nAll tests passed.")
            && out.processor_launches == 2,
        "{out:?}"
    );
    let cached = call(
        &f,
        json!(["tests/a.bnt", "tests"]),
        false,
        "cached",
        &AtomicBool::new(false),
    );
    assert!(
        !cached.is_error && cached.approval.is_none() && cached.processor_launches == 2,
        "{cached:?}"
    );
    fs::create_dir(f.project.join("empty"))?;
    let empty = execute(&mut f, json!(["empty"]), "empty")?;
    assert!(
        !empty.is_error
            && empty
                .text
                .contains("Tests: 0 passed, 0 failed.\nNo tests ran."),
        "{empty:?}"
    );
    fs::write(
        f.project.join("bad.bnt"),
        "@test\nfunction broken() -> Unit\n UNKNOWN\nend function",
    )?;
    let bad = execute(&mut f, json!(["tests", "bad.bnt"]), "bad")?;
    assert!(
        bad.is_error && bad.processor_launches == 1 && !bad.text.contains("Tests:"),
        "{bad:?}"
    );
    Ok(())
}
// 関門: .env.bnt の結果を本文だけでなく spill へ残す漏れと、全対象の20件の上限で秘密を数える退行。
#[test]
fn excluded_test_names_values_output_and_diagnostics_never_reach_results_or_spill() -> TestResult {
    let mut f = Fixture::new()?;
    f.sandbox()?;
    fs::create_dir(f.project.join("tests"))?;
    let hidden = "import Benitoite.Assert\nimport Benitoite.Unofficial.IO.Console\n@test(\"SECRET_NAME\")\nfunction secret() -> Unit uses Assert.Check, Console.Write\n Console.writeLine(\"SECRET_OUTPUT\")\n Assert.equal(actual: \"SECRET_LEFT\", expected: \"SECRET_RIGHT\")\nend function\n";
    fs::write(f.project.join("tests/.env.bnt"), hidden)?;
    let mut visible =
        String::from("import Benitoite.Assert\nimport Benitoite.Unofficial.IO.Console\n");
    for n in 0..22 {
        visible.push_str(&format!("@test(\"visible {n}\")\nfunction visible{n}() -> Unit uses Assert.Check, Console.Write\n Console.write(\"{}TAIL\")\n Assert.equal(actual: 1, expected: 2)\nend function\n","x".repeat(5000)));
    }
    fs::write(f.project.join("tests/visible.bnt"), visible)?;
    let out = execute(&mut f, json!(["tests"]), "secrets")?;
    assert!(
        out.is_error
            && out
                .text
                .contains("1 failed test in a file that benitoiteh does not read is not shown.")
            && out.text.contains("2 more failed tests not shown")
            && out.text.contains("Tests: 0 passed, 23 failed."),
        "{out:?}"
    );
    assert_eq!(out.text.matches("FAILED ").count(), 20);
    assert!(
        out.text.len() <= 64 * 1024 && out.text.contains("combined 32 KiB"),
        "{out:?}"
    );
    assert!(!out.text.contains("SECRET_") && out.spill.is_none() && out.stderr_spill.is_none());
    assert!(
        fs::read_dir(f.session.directory().join("spill"))?
            .next()
            .is_none()
    );
    Ok(())
}
// 関門: 四つの理由と die の診断の受け渡しを、固定の JSON だけでなく本物の test で守る。
#[test]
fn actual_failures_show_assert_exit_die_error_and_runtime() -> TestResult {
    let mut f = Fixture::new()?;
    f.sandbox()?;
    let source = "import Benitoite.Assert\nimport Benitoite.Unofficial.IO.Process\nimport Benitoite.Unofficial.IO.Console\n@test(\"assert\")\nfunction a() -> Unit uses Assert.Check\n Assert.equal(actual: 1, expected: 2)\nend function\n@test(\"exit\")\nfunction b() -> Unit uses Process.Exit\n Process.exit(5)\nend function\n@test(\"die\")\nfunction c() -> Unit uses Process.Exit\n bind value <- try Option.None or die(\"die detail\")\nend function\n@test(\"error\")\nfunction d() -> Result[Unit, String]\n return Result.Error(\"application error\")\nend function\n@test(\"runtime\")\nfunction e() -> Unit uses Console.Write\n Console.write(\"${1 div 0}\")\nend function\n";
    fs::write(f.project.join("failures.bnt"), source)?;
    let out = execute(&mut f, json!(["failures.bnt"]), "failures")?;
    assert!(
        out.is_error && out.text.contains("Tests: 0 passed, 5 failed."),
        "{out:?}"
    );
    for needle in [
        "assert: ",
        "  left: 1",
        "  right: 2",
        "exit: \"called `Process.exit` with status 5\"",
        "die detail",
        "error: \"application error\"",
        "runtime: ",
        "[exit code: 1]",
    ] {
        assert!(out.text.contains(needle), "{needle}: {out:?}");
    }
    assert!(out.spill.is_none() && out.stderr_spill.is_none());
    Ok(())
}
// 関門: 報告を得た後にディレクトリへ追加された入口にも処理系のエフェクト上限が掛かる。
#[test]
fn entry_added_after_report_is_not_executed_and_gets_the_effect_change_notice() -> TestResult {
    let mut f = Fixture::new()?;
    f.sandbox()?;
    fs::create_dir(f.project.join("tests"))?;
    fs::write(f.project.join("tests/a.bnt"), PASS)?;
    let first = call(&f, json!(["tests"]), false, "ask", &AtomicBool::new(false));
    f.confirm(&first)?;
    let query = Arc::clone(f.context.check_query.as_ref().ok_or("query")?);
    let added = f.project.join("tests/new.bnt");
    f.query(Arc::new(move |args,limits,stop| {let output=query(args,limits,stop)?;fs::write(&added,"import Benitoite.Unofficial.IO.File\n@test\nfunction write() -> Unit uses File.Write\n bind _ <- File.writeText(path: \"MUST_NOT_EXIST\", text: \"ran\")\nend function\n")?;Ok(output)}));
    let out = call(&f, json!(["tests"]), true, "race", &AtomicBool::new(false));
    assert!(
        out.is_error
            && out.processor_launches == 2
            && out.text.contains("error[E0512]")
            && out
                .text
                .contains("The script changed after the effects were confirmed")
            && !f.project.join("MUST_NOT_EXIST").exists(),
        "{out:?}"
    );
    Ok(())
}
// 関門: test の上限で SIGINT が届き、処理系の中断の集計を失わない。
#[test]
fn timeout_interrupts_tests_and_reports_interrupted_summary() -> TestResult {
    let mut f = Fixture::new()?;
    f.sandbox()?;
    f.context.run.as_mut().ok_or("run")?.test_timeout = Duration::from_secs(1);
    fs::write(
        f.project.join("slow.bnt"),
        "import Benitoite.Unofficial.IO.Clock\n@test\nfunction slow() -> Unit uses Clock.Time\n Clock.sleep(60000)\nend function\n",
    )?;
    let out = execute(&mut f, json!(["slow.bnt"]), "timeout")?;
    assert!(
        out.is_error
            && out.text.contains("The test run was interrupted.")
            && out.text.contains(
                "[exit code: 130]\n[The tests timed out after 1 seconds and were stopped.]"
            ),
        "{out:?}"
    );
    Ok(())
}

// 関門: 許したプログラムがあっても、確認済みの Assert.Check を Process.Run へ広げない。
#[test]
fn process_run_added_after_confirmation_cannot_launch_an_allowed_program() -> TestResult {
    let mut f = Fixture::new()?;
    f.sandbox()?;
    fs::write(f.project.join("test.bnt"), PASS)?;
    let first = call(
        &f,
        json!(["test.bnt"]),
        false,
        "ask",
        &AtomicBool::new(false),
    );
    f.confirm(&first)?;
    let probe = std::env::current_exe()?
        .parent()
        .and_then(std::path::Path::parent)
        .ok_or("debug directory")?
        .join("sandbox-probe")
        .canonicalize()?;
    f.context
        .run
        .as_mut()
        .ok_or("run")?
        .allowed_programs
        .push(probe.clone());
    let source = format!(
        "import Benitoite.Unofficial.IO.Process\n@test\nfunction start() -> Unit uses Process.Run\n bind _ <- Process.run(Process.command(program: {}, arguments: [\"write\", \"{}\"]))\nend function\n",
        json!(probe),
        f.project.join("MUST_NOT_EXIST").display()
    );
    fs::write(f.project.join("test.bnt"), source)?;
    let out = call(
        &f,
        json!(["test.bnt"]),
        true,
        "changed",
        &AtomicBool::new(false),
    );
    assert!(
        out.is_error && out.processor_launches == 1 && out.text.starts_with("The script changed"),
        "{out:?}"
    );
    assert!(!f.project.join("MUST_NOT_EXIST").exists());
    Ok(())
}

// 関門: 二度目の実行で確認を省く経路でも拒否の回数を戻し、実行前の時点と戻せない操作を記録する。
#[test]
fn noninteractive_execution_resets_denials_counts_launches_and_records_rewind_points() -> TestResult
{
    use benitoiteh::{
        agent::{Agent, AgentConfig, Outcome},
        config::{Limits, Mode},
        context::Environment,
        http::HttpTimeouts,
        provider::{AdapterKind, Capabilities, Destination},
        rewind::{CheckpointReason, RewindScope},
        safe_fs::ScopeRoot,
        session::{Record, read_records},
        tools::{ToolContext, exclude::ExcludeRules},
    };
    use support::mock_provider::{MockProvider, ResponseScript};
    fn response(calls: &[(&str, Value)]) -> ResponseScript {
        let tools=calls.iter().enumerate().map(|(n,(name,args))|json!({"index":n,"id":format!("call-{n}"),"type":"function","function":{"name":name,"arguments":args.to_string()}})).collect::<Vec<_>>();
        ResponseScript::sse(format!(
            "data: {}\n\ndata: [DONE]\n\n",
            json!({"choices":[{"delta":if calls.is_empty(){json!({"content":"done"})}else{json!({"tool_calls":tools})},"finish_reason":if calls.is_empty(){"stop"}else{"tool_calls"}}]})
        ))
    }
    let mut f = Fixture::new()?;
    f.sandbox()?;
    fs::write(f.project.join("pass.bnt"), PASS)?;
    fs::write(
        f.project.join("write.bnt"),
        "import Benitoite.Unofficial.IO.File\nfunction main() -> Unit uses File.Write\n bind _ <- File.writeText(path: \"MUST_NOT_EXIST\", text: \"ran\")\nend function\n",
    )?;
    let denied = ("benitoite_run", json!({"path":"write.bnt"}));
    let passed = ("benitoite_test", json!({"paths":["pass.bnt"]}));
    let mut server = MockProvider::start(vec![
        response(&[
            denied.clone(),
            denied.clone(),
            passed.clone(),
            denied.clone(),
            denied.clone(),
            passed,
            denied.clone(),
            denied,
        ]),
        response(&[]),
    ])?;
    let spill = f.session.directory().join("spill");
    let context = ToolContext::new(
        Mode::Restricted,
        ScopeRoot::open(&f.project)?,
        ScopeRoot::open(f.session.temporary_dir())?,
        ScopeRoot::open(&spill)?,
        ExcludeRules::new(
            &f.project,
            &f.paths,
            &spill,
            &Default::default(),
            &Default::default(),
        )?,
    )
    .with_session(f.session.clone())
    .with_check_query(support::checks(f.project.clone())?)
    .with_skill(support::skills()?)
    .with_run_context(f.context.run.clone().ok_or("run")?);
    let config = AgentConfig {
        project_parent: None,
        allow_effects: Some(Default::default()),
        mode: Mode::Restricted,
        destination: Destination {
            provider: "mock".into(),
            base_url: server.base_url(),
            model: "model".into(),
        },
        kind: AdapterKind::OpenAiChat,
        capabilities: Capabilities::default(),
        effort: "high".into(),
        limits: Limits::default(),
        http_timeouts: HttpTimeouts::default(),
        retry_initial: Duration::from_millis(1),
    };
    let environment = Environment::capture(
        &f.project,
        Mode::Restricted,
        &[],
        false,
        std::time::SystemTime::now(),
    )?;
    let mut agent = Agent::new(
        config,
        "synthetic".into(),
        context,
        environment,
        f.session.clone(),
    )?;
    let (send, recv) = std::sync::mpsc::sync_channel(0);
    let report = std::thread::scope(|scope| -> Result<_, Box<dyn std::error::Error>> {
        let task = scope.spawn(|| agent.run("test".into(), Arc::new(AtomicBool::new(false)), send));
        let _events = recv.into_iter().collect::<Vec<_>>();
        Ok(task.join().map_err(|_| "agent thread")??)
    })?;
    assert_eq!(
        report.outcome,
        Outcome::Completed,
        "{}",
        report.summary.reason
    );
    assert_eq!(report.summary.checks, 11);
    assert_eq!(report.summary.approvals.policy_denied, 6);
    let records = read_records(f.session.directory())?;
    assert!(!records.iter().any(|r| matches!(r, Record::Approval { .. })));
    assert_eq!(
        records
            .iter()
            .filter(
                |r| matches!(r,Record::Checkpoint(c) if c.reason==CheckpointReason::BenitoiteTest)
            )
            .count(),
        2
    );
    let request = records
        .iter()
        .find_map(|r| {
            if let Record::Checkpoint(c) = r
                && c.reason == CheckpointReason::Request
            {
                Some(&c.id)
            } else {
                None
            }
        })
        .ok_or("request checkpoint")?;
    let preview = agent.rewind_preview(request, RewindScope::Files)?;
    assert!(
        preview
            .irreversible
            .iter()
            .any(|op| op.summary.contains("benitoite_test"))
    );
    assert!(!f.project.join("MUST_NOT_EXIST").exists());
    server.shutdown()?;
    Ok(())
}

// 関門: 読まないモジュールの値を出す実行には、確認済みの集合があっても毎回対話の確認を要する。
#[test]
fn interactively_confirmed_unread_imports_run_but_are_never_remembered_as_permission() -> TestResult
{
    let mut f = Fixture::new()?;
    f.sandbox()?;
    fs::create_dir(f.project.join("Secrets"))?;
    fs::write(
        f.project.join("Secrets/Keys.bnt"),
        "public function token() -> String\n return \"SECRET_APPROVED_VALUE\"\nend function\n",
    )?;
    fs::write(
        f.project.join("test.bnt"),
        "import Secrets.Keys\nimport Benitoite.Assert\n@test\nfunction example() -> Unit uses Assert.Check\n Assert.equal(actual: Keys.token(), expected: \"\")\nend function\n",
    )?;
    fs::write(
        f.project.join("main.bnt"),
        "import Secrets.Keys\nfunction main() -> Result[Unit, String]\n return Result.Error(Keys.token())\nend function\n",
    )?;
    f.exclude(&["Secrets/**"])?;
    let stop = AtomicBool::new(false);
    for tests in [false, true] {
        for _ in 0..2 {
            let ask = if tests {
                call(&f, json!(["test.bnt"]), false, "ask", &stop)
            } else {
                f.call(json!({"path":"main.bnt"}), false, "ask", &stop)
            };
            assert_eq!(
                ask.approval
                    .as_ref()
                    .ok_or(format!("{ask:?}"))?
                    .facts
                    .unread_imports,
                1
            );
            f.confirm(&ask)?;
            let out = if tests {
                call(&f, json!(["test.bnt"]), true, "approved", &stop)
            } else {
                f.call(json!({"path":"main.bnt"}), true, "approved", &stop)
            };
            assert!(
                out.is_error
                    && out.processor_launches == 2
                    && out.text.contains("SECRET_APPROVED_VALUE"),
                "{out:?}"
            );
        }
    }
    Ok(())
}
