//! 確認なしの一時スクリプトの読み取り範囲と実行を OS で守る（設計書 03-03、07-01）。
#[allow(dead_code)]
#[path = "support/benitoite_run.rs"]
mod fixture;
#[allow(dead_code)]
mod support;
use benitoiteh::benitoite::{Effect, effects};
use benitoiteh::sandbox::{self, ProgramAccess, ReadAccess};
use fixture::*;
use serde_json::json;
use std::fs;
use std::sync::atomic::AtomicBool;

const CONSOLE: &str = "import Benitoite.Unofficial.IO.Console\nfunction main() -> Unit uses Console.Write\n Console.write(\"scratch output\")\nend function\n";

// 関門: 確認なしで実際に実行でき、File.Read は設定で追加した場合だけ自動になる。
// 再実行時にファイルを作り直すと、確認の単位と実行の対象がずれる退行を捕まえる。
#[test]
fn automatic_runs_and_confirmed_retries_execute_the_original_script() -> TestResult {
    let mut f = Fixture::new()?;
    f.sandbox()?;
    let stop = AtomicBool::new(false);
    let first = f.call_tool(
        "benitoite_scratch",
        json!({"code":CONSOLE}),
        false,
        "console",
        &stop,
    );
    assert!(
        !first.is_error && first.approval.is_none() && first.text.contains("scratch output"),
        "{first:?}"
    );
    assert_eq!(first.processor_launches, 2);
    assert_eq!(
        first.text.lines().next(),
        f.session.temporary_dir().join("scratch-1.bnt").to_str()
    );
    fs::write(f.project.join("input"), "project input")?;
    let read = "import Benitoite.Unofficial.IO.File\nimport Benitoite.Unofficial.IO.Console\nimport Benitoite.Unofficial.IO.Process\nfunction main() -> Unit uses File.Read, Console.Write, Process.Exit\n Console.write(try File.readText(\"input\") or die)\nend function\n";
    let ask = f.call_tool(
        "benitoite_scratch",
        json!({"code":read}),
        false,
        "read",
        &stop,
    );
    assert_eq!(
        ask.approval
            .as_ref()
            .ok_or("read approval")?
            .facts
            .unread_imports,
        0
    );
    f.confirm(&ask)?;
    let out = f.call_tool(
        "benitoite_scratch",
        json!({"code":read}),
        true,
        "read",
        &stop,
    );
    assert!(
        !out.is_error && out.text.contains("project input"),
        "{out:?}"
    );
    assert_eq!(out.processor_launches, 2);
    assert!(!f.session.temporary_dir().join("scratch-3.bnt").exists());
    f.context
        .run
        .as_mut()
        .ok_or("run")?
        .scratch_auto_effects
        .insert(Effect::FileRead);
    let out = f.call_tool(
        "benitoite_scratch",
        json!({"code":read}),
        false,
        "auto-read",
        &stop,
    );
    assert!(
        !out.is_error && out.approval.is_none() && out.text.contains("project input"),
        "{out:?}"
    );
    Ok(())
}

// 関門: File.Read を自動にしても、プロジェクトと一時領域の外の秘密は読めない。
// 方針の値は処理系の静的検査が使わせない書き込み・起動・IP の独立した契約を守る。
#[test]
fn automatic_file_reads_are_confined_to_project_and_temporary_roots() -> TestResult {
    let mut f = Fixture::new()?;
    f.sandbox()?;
    let stop = AtomicBool::new(false);
    f.context
        .run
        .as_mut()
        .ok_or("run")?
        .scratch_auto_effects
        .insert(Effect::FileRead);
    let inputs = [
        f.project.join("inside"),
        f.session.temporary_dir().join("inside"),
        f.root.join("outside"),
    ];
    for (number, path) in inputs.iter().enumerate() {
        fs::write(path, format!("secret-{number}"))?;
    }
    for (number, path) in inputs.iter().enumerate() {
        let source = format!(
            "import Benitoite.Unofficial.IO.File\nimport Benitoite.Unofficial.IO.Console\nfunction main() -> Unit uses File.Read, Console.Write\n match File.readText({}) with\n case Result.Ok(text) -> Console.write(text)\n case Result.Error(error) -> match IOError.kind(error) with\n case IOErrorKind.PermissionDenied -> Console.write(\"DENIED: ${{IOError.message(error)}}\")\n case other -> Console.write(\"OTHER_ERROR: ${{IOError.message(error)}}\")\n end match\n end match\nend function\n",
            json!(path)
        );
        let out = f.call_tool(
            "benitoite_scratch",
            json!({"code":source}),
            false,
            "scope",
            &stop,
        );
        assert!(!out.is_error && out.approval.is_none(), "{out:?}");
        if number == 2 {
            assert!(
                out.text.contains("DENIED:") && !out.text.contains("secret-2"),
                "{out:?}"
            );
            assert!(!out.text.contains("OTHER_ERROR"), "{out:?}");
        } else {
            assert!(out.text.contains(&format!("secret-{number}")), "{out:?}");
        }
    }
    let run = f.context.run.as_ref().ok_or("run")?;
    let policy = sandbox::restricted_policy(
        &effects::restricted_access(
            &[Effect::FileRead].into_iter().collect(),
            &run.processor,
            &run.allowed_programs,
        ),
        &run.locations,
        &[],
    )?;
    let policy = sandbox::scratch_policy(&policy, &run.locations, &run.processor)?;
    let config = policy.config();
    assert!(
        matches!(&config.readable, ReadAccess::Roots(roots) if roots.contains(&f.project) && roots.contains(&f.session.temporary_dir().into()) && !roots.contains(&f.root))
    );
    assert_eq!(config.writable, [f.session.temporary_dir()]);
    assert!(!config.allow_ip);
    assert_eq!(
        config.programs,
        ProgramAccess::Only(vec![run.processor.clone()])
    );
    Ok(())
}

// 関門: 静的リンクと OS の読み取りの最小の組で、利用者が追加できるすべての IO が動く。
// 時間帯を隠すと値が 0 に変わるため、同じ処理系の通常の値と比べる。
#[test]
fn optional_automatic_clock_random_and_environment_effects_work_in_the_narrow_policy() -> TestResult
{
    let mut f = Fixture::new()?;
    f.sandbox()?;
    let stop = AtomicBool::new(false);
    let run = f.context.run.as_mut().ok_or("run")?;
    run.scratch_auto_effects.extend([
        Effect::ClockTime,
        Effect::RandomGenerate,
        Effect::ProcessEnvironment,
    ]);
    run.env
        .insert("BH_SCRATCH_VALUE".into(), "environment-value".into());
    let clock = "import Benitoite.Unofficial.IO.Clock\nimport Benitoite.Unofficial.IO.Console\nfunction main() -> Unit uses Clock.Time, Console.Write\n Console.writeLine(\"offset=${Clock.localOffsetMinutes()}\")\n Console.writeLine(\"now=${Clock.monotonicMilliseconds()}\")\nend function\n";
    let clock_path = f.project.join("clock.bnt");
    fs::write(&clock_path, clock)?;
    let expected = std::process::Command::new(&run.processor)
        .args(["run", "--allow-effects=Clock.Time,Console.Write"])
        .arg(clock_path)
        .env_clear()
        .envs(&run.env)
        .output()?;
    assert!(
        expected.status.success(),
        "{}",
        String::from_utf8_lossy(&expected.stderr)
    );
    let expected = String::from_utf8(expected.stdout)?;
    for (id, code, needle) in [
        (
            "clock",
            clock,
            expected.lines().next().ok_or("clock output")?,
        ),
        (
            "random",
            "import Benitoite.Unofficial.IO.Random\nimport Benitoite.Unofficial.IO.Console\nfunction main() -> Unit uses Random.Generate, Console.Write\n Console.writeLine(\"random=${Random.integer(low: 1, high: 2)}\")\nend function\n",
            "random=1",
        ),
        (
            "environment",
            "import Benitoite.Unofficial.IO.Process\nimport Benitoite.Unofficial.IO.Console\nfunction main() -> Unit uses Process.Environment, Console.Write\n match Process.environmentVariable(\"BH_SCRATCH_VALUE\") with\n case Result.Ok(Option.Some(value)) -> Console.writeLine(value)\n case other -> Console.writeLine(\"MISSING_ENVIRONMENT\")\n end match\n List.forEach(xs: Process.arguments(), f: lambda(value) Console.writeLine(value))\nend function\n",
            "environment-value",
        ),
    ] {
        let out = f.call_tool(
            "benitoite_scratch",
            json!({"code":code,"args":["scratch-argument"],"stdin":"input"}),
            false,
            id,
            &stop,
        );
        assert!(
            !out.is_error && out.approval.is_none() && out.text.contains(needle),
            "{id}: {out:?}"
        );
        if id == "environment" {
            assert!(out.text.contains("scratch-argument"), "{out:?}");
        }
    }
    Ok(())
}

// 関門: 自動実行でも拒否の連続を切り、実行直前の時点と戻せない操作を記録する。
// これらは道具の結果だけでは確認できないループ固有の契約である。
#[test]
fn automatic_runs_reset_denials_and_are_checkpointed_as_irreversible() -> TestResult {
    use benitoiteh::agent::Outcome;
    use benitoiteh::session::{Record, read_records};
    use std::sync::{Arc, mpsc};
    use support::mock_provider::MockProvider;
    let mut f = Fixture::new()?;
    f.sandbox()?;
    let write = "import Benitoite.Unofficial.IO.File\nfunction main() -> Unit uses File.Write\n bind _ <- File.writeText(path: \"MUST_NOT_EXIST\", text: \"bad\")\nend function\n";
    let scripts = [write, write, CONSOLE, write, write];
    let calls: Vec<_> = scripts
        .iter()
        .map(|source| ("benitoite_scratch", json!({"code":source})))
        .collect();
    let mut server = MockProvider::start(vec![response(&calls), response(&[])])?;
    let mut agent = scratch_agent(&f, &server, Default::default())?;
    let (send, recv) = mpsc::sync_channel(64);
    let report = agent.run("run".into(), Arc::new(AtomicBool::new(false)), send)?;
    let _events: Vec<_> = recv.into_iter().collect();
    assert_eq!(
        report.outcome,
        Outcome::Completed,
        "{}",
        report.summary.reason
    );
    assert_eq!(report.summary.approvals.policy_denied, 4);
    assert_eq!(report.summary.checks, 6);
    assert!(!f.project.join("MUST_NOT_EXIST").exists());
    let records = read_records(f.session.directory())?;
    let checkpoints: Vec<_> = records
        .iter()
        .filter_map(|record| {
            if let Record::Checkpoint(checkpoint) = record
                && checkpoint.reason == benitoiteh::rewind::CheckpointReason::BenitoiteScratch
            {
                Some(checkpoint)
            } else {
                None
            }
        })
        .collect();
    assert_eq!(checkpoints.len(), 1);
    let checkpoint = checkpoints.first().ok_or("checkpoint")?;
    assert_eq!(checkpoint.call_id.as_deref(), Some("turn-1-call-2"));
    let preview = agent.rewind_preview(&checkpoint.id, benitoiteh::rewind::RewindScope::Files)?;
    assert!(
        preview
            .irreversible
            .iter()
            .any(|operation| operation.summary.contains("benitoite_scratch"))
    );
    server.shutdown()?;
    Ok(())
}

fn scratch_agent(
    f: &Fixture,
    server: &support::mock_provider::MockProvider,
    allowed: std::collections::BTreeSet<Effect>,
) -> Result<benitoiteh::agent::Agent, Box<dyn std::error::Error>> {
    use benitoiteh::agent::{Agent, AgentConfig};
    use benitoiteh::config::{Limits, Mode, ReadConfig};
    use benitoiteh::context::Environment;
    use benitoiteh::provider::{AdapterKind, Capabilities, Destination};
    use benitoiteh::safe_fs::ScopeRoot;
    use benitoiteh::tools::{ToolContext, exclude::ExcludeRules};
    use std::time::{Duration, SystemTime};
    let config = AgentConfig {
        compaction: Default::default(),
        agent: Default::default(),
        project_parent: None,
        allow_effects: Some(allowed),
        mode: Mode::Restricted,
        destination: Destination {
            provider: "mock".into(),
            base_url: server.base_url(),
            model: "mock".into(),
        },
        kind: AdapterKind::OpenAiChat,
        capabilities: Capabilities::default(),
        effort: "high".into(),
        limits: Limits::default(),
        http_timeouts: benitoiteh::http::HttpTimeouts::default(),
        retry_initial: Duration::from_millis(1),
    };
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
            &ReadConfig::default(),
            &ReadConfig::default(),
        )?,
    )
    .with_session(f.session.clone())
    .with_run_context(f.context.run.clone().ok_or("run")?)
    .with_check_query(f.context.check_query.clone().ok_or("query")?)
    .with_skill(support::skills()?);
    let environment = Environment::capture(
        &f.project,
        Mode::Restricted,
        &[f.project.clone(), f.session.temporary_dir().into()],
        false,
        SystemTime::now(),
    )?;
    Ok(Agent::new(
        config,
        "synthetic".into(),
        context,
        environment,
        f.session.clone(),
    )?)
}

fn response(calls: &[(&str, serde_json::Value)]) -> support::mock_provider::ResponseScript {
    let delta = if calls.is_empty() {
        json!({"content":"done"})
    } else {
        json!({"tool_calls":calls.iter().enumerate().map(|(i,(name,args))| json!({"index":i,"id":format!("provider-call-{i}"),"type":"function","function":{"name":name,"arguments":args.to_string()}})).collect::<Vec<_>>()})
    };
    support::mock_provider::ResponseScript::sse(format!("data: {}\n\ndata: {}\n\ndata: [DONE]\n\n", json!({"choices":[{"delta":delta,"finish_reason":null}]}),json!({"choices":[{"delta":{},"finish_reason":if calls.is_empty() {"stop"} else {"tool_calls"}}]})).into_bytes())
}

// 関門: File.Read を自動の範囲だけで許した呼び直しは外を読めず、後の run にも許可を残さない。
// File.Read を --allow に加えた場合を対照にし、別のエフェクトの拒否で通ってしまうのを防ぐ。
#[test]
fn noninteractive_union_confines_automatic_file_reads_and_does_not_authorize_later_runs()
-> TestResult {
    use benitoiteh::agent::Outcome;
    use benitoiteh::provider::Item;
    use std::sync::{Arc, mpsc};
    use support::mock_provider::MockProvider;
    for explicit_read in [false, true] {
        let mut f = Fixture::new()?;
        f.sandbox()?;
        f.context
            .run
            .as_mut()
            .ok_or("run")?
            .scratch_auto_effects
            .insert(Effect::FileRead);
        let marker = f.root.join("outside-marker");
        fs::write(&marker, "OUTSIDE_SCRATCH_MARKER")?;
        let path = f.session.temporary_dir().join("scratch-1.bnt");
        let source = format!(
            "import Benitoite.Unofficial.IO.File\nimport Benitoite.Unofficial.IO.Console\nimport Benitoite.Unofficial.IO.Clock\nfunction main() -> Unit uses File.Read, Console.Write, Clock.Time\n bind _ <- Clock.monotonicMilliseconds()\n match File.readText({}) with\n case Result.Ok(value) -> Console.write(value)\n case Result.Error(error) -> match IOError.kind(error) with\n case IOErrorKind.PermissionDenied -> Console.write(\"OUTSIDE_DENIED\")\n case other -> Console.write(\"OTHER_ERROR\")\n end match\n end match\nend function\n",
            json!(marker)
        );
        let changed = format!(
            "import Benitoite.Unofficial.IO.File\nfunction main() -> Unit uses File.Read\n bind _ <- File.readText({})\nend function\n",
            json!(marker)
        );
        fs::write(f.project.join("same.bnt"), &changed)?;
        let mut server = MockProvider::start(vec![
            response(&[
                ("benitoite_scratch", json!({"code":source})),
                ("read", json!({"path":path})),
                ("write", json!({"path":path,"content":changed})),
                ("benitoite_run", json!({"path":path})),
                ("benitoite_run", json!({"path":"same.bnt"})),
            ]),
            response(&[]),
        ])?;
        let mut allowed = [Effect::ClockTime]
            .into_iter()
            .collect::<std::collections::BTreeSet<_>>();
        if explicit_read {
            allowed.insert(Effect::FileRead);
        }
        let mut agent = scratch_agent(&f, &server, allowed)?;
        let (send, recv) = mpsc::sync_channel(64);
        let report = agent.run("run".into(), Arc::new(AtomicBool::new(false)), send)?;
        let _events: Vec<_> = recv.into_iter().collect();
        assert_eq!(
            report.outcome,
            Outcome::Completed,
            "{}",
            report.summary.reason
        );
        let outputs: Vec<_> = agent
            .conversation()
            .items
            .iter()
            .filter_map(|item| {
                if let Item::ToolResult(result) = item {
                    Some(result)
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(outputs.len(), 5);
        let first = outputs.first().ok_or("scratch result")?;
        assert!(!first.is_error, "{first:?}");
        if explicit_read {
            assert!(first.text.contains("OUTSIDE_SCRATCH_MARKER"), "{first:?}");
        } else {
            assert!(
                first.text.contains("OUTSIDE_DENIED")
                    && !first.text.contains("OUTSIDE_SCRATCH_MARKER")
                    && !first.text.contains("OTHER_ERROR"),
                "{first:?}"
            );
        }
        assert!(
            !outputs.get(2).ok_or("write result")?.is_error,
            "{outputs:?}"
        );
        assert_eq!(fs::read_to_string(&path)?, changed);
        for output in outputs.get(3..).ok_or("run results")? {
            if explicit_read {
                assert!(
                    !output.is_error && output.text.contains("[exit code: 0]"),
                    "{output:?}"
                );
            } else {
                assert!(
                    output.is_error
                        && output
                            .text
                            .contains("allows only these effects: Clock.Time.")
                        && output.text.contains("uses: File.Read."),
                    "{output:?}"
                );
            }
        }
        assert_eq!(
            report.summary.approvals.policy_denied,
            if explicit_read { 0 } else { 2 }
        );
        server.shutdown()?;
    }
    Ok(())
}

// 関門: 設定で Log を自動にしても、その許可を後の run・test に残さない（設計書 03-03）。
#[test]
fn configured_log_does_not_authorize_later_scripts_or_tests() -> TestResult {
    use benitoiteh::agent::Outcome;
    use benitoiteh::provider::Item;
    use std::sync::{Arc, mpsc};
    use support::mock_provider::MockProvider;
    for configured in [false, true] {
        let mut f = Fixture::new()?;
        f.sandbox()?;
        if configured {
            f.context.run.as_mut().ok_or("run")?.scratch_auto_effects =
                effects::scratch_auto_effects(&["Log".into()]);
        }
        let source = "function main() -> Unit uses Log\n Log.info(message: \"scratch-log\", attributes: [], location: SourceLocation.here())\nend function\n";
        fs::write(
            f.project.join("test.bnt"),
            "uses Log\n@test\nfunction example() -> Unit uses Assert.Check\n Assert.equal(actual: 1, expected: 1)\nend function\n",
        )?;
        let path = f.session.temporary_dir().join("scratch-1.bnt");
        let mut server = MockProvider::start(vec![
            response(&[
                ("benitoite_scratch", json!({"code":source})),
                ("benitoite_run", json!({"path":path})),
                ("benitoite_test", json!({"paths":["test.bnt"]})),
            ]),
            response(&[]),
        ])?;
        let mut agent = scratch_agent(&f, &server, Default::default())?;
        let (send, recv) = mpsc::sync_channel(64);
        let report = agent.run("run".into(), Arc::new(AtomicBool::new(false)), send)?;
        let _events: Vec<_> = recv.into_iter().collect();
        assert_eq!(
            report.outcome,
            if configured {
                Outcome::Completed
            } else {
                Outcome::Failed
            }
        );
        assert_eq!(
            report.summary.approvals.policy_denied,
            if configured { 2 } else { 3 }
        );
        let outputs: Vec<_> = agent
            .conversation()
            .items
            .iter()
            .filter_map(|item| {
                if let Item::ToolResult(result) = item {
                    Some(result)
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(outputs.len(), 3);
        let first = outputs.first().ok_or("scratch")?;
        if configured {
            assert!(
                !first.is_error && first.text.contains("message=\"scratch-log\""),
                "{first:?}"
            );
        } else {
            assert!(
                first.is_error && first.text.contains("uses: Log."),
                "{first:?}"
            );
        }
        for output in outputs.get(1..).ok_or("later results")? {
            assert!(
                output.is_error
                    && output.text.contains("allows only these effects: none.")
                    && output.text.contains("Log"),
                "{output:?}"
            );
        }
        server.shutdown()?;
    }
    Ok(())
}
