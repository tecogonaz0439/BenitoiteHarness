//! 検査と修正の連続回数と起動の集計をループで確かめる（設計書 01-01「上限」、07-02）。
use super::*;
use benitoiteh::benitoite::{BenitoiteError, CheckQuery, QueryOutput};
use std::collections::VecDeque;

#[derive(Clone, Copy)]
pub(super) enum Response {
    Failed,
    Warning,
    Passed,
    Usage,
    Processor,
}
pub(super) fn query(responses: Vec<Response>) -> Arc<CheckQuery> {
    let responses = Mutex::new(VecDeque::from(responses));
    Arc::new(move |args, _, _| {
        let response = responses
            .lock()
            .map_err(|_| BenitoiteError::Invalid("poisoned"))?
            .pop_front()
            .ok_or(BenitoiteError::Invalid("unexpected check"))?;
        let file = args
            .last()
            .and_then(|path| path.to_str())
            .unwrap_or_default();
        let diagnostic = json!({"kind":"check", "severity":if matches!(response, Response::Warning) {"warning"} else {"error"}, "code":"E0301", "message":"last diagnostic", "primary":{"file":file,"start":{"line":1,"column":1,"offset":0},"end":{"line":1,"column":2,"offset":1},"label":"here"}, "secondary":[],"notes":[],"helps":[]}).to_string().into_bytes();
        match response {
            Response::Failed => Err(BenitoiteError::Exit {
                stdout: Vec::new(),
                code: Some(2),
                stderr: diagnostic,
            }),
            Response::Warning => Ok(QueryOutput {
                stdout: vec![],
                stderr: diagnostic,
            }),
            Response::Passed => Ok(QueryOutput {
                stdout: vec![],
                stderr: vec![],
            }),
            Response::Usage => Err(BenitoiteError::Exit {
                stdout: Vec::new(),
                code: Some(2),
                stderr: b"error: usage".to_vec(),
            }),
            Response::Processor => Err(BenitoiteError::TimedOut),
        }
    })
}
fn check(path: &str) -> (&'static str, Value) {
    ("benitoite_check", json!({"path":path}))
}
fn check_script(path: &str) -> ResponseScript {
    ResponseScript::sse(calls(&[check(path)], "tool_calls"))
}

// 関門: 切り替えが write・edit・一時ファイルの注記に届き、起動も集計も止まる契約。
// 本物の道具と模擬の提供者を通し、query の呼び出しを集計とは独立に数える。
#[test]
fn after_write_switch_controls_checks_without_changing_edits_or_normal_mode() -> TestResult {
    use std::sync::atomic::AtomicU64;
    for (mode, enabled, expected) in [
        (Mode::Restricted, true, 2),
        (Mode::Restricted, false, 0),
        (Mode::Normal, true, 0),
        (Mode::Normal, false, 0),
    ] {
        let f = Fixture::new()?;
        let session = Session::create(&f.paths, SessionId::generate(SystemTime::now())?)?;
        let spill = session.directory().join("spill");
        let temporary_file = session.temporary_dir().join("scratch.bnt");
        let mut server = MockProvider::start(vec![
            ResponseScript::sse(calls(
                &[
                    ("write", json!({"path":"new.bnt","content":"before\n"})),
                    (
                        "edit",
                        json!({"path":"new.bnt","old_string":"before","new_string":"after"}),
                    ),
                    (
                        "write",
                        json!({"path":temporary_file,"content":"temporary\n"}),
                    ),
                ],
                "tool_calls",
            )),
            final_text(),
        ])?;
        let mut config = f.config(&server, AdapterKind::OpenAiChat);
        config.mode = mode;
        let launches = Arc::new(AtomicU64::new(0));
        let count = Arc::clone(&launches);
        let base = query(vec![Response::Passed; 2]);
        let query: Arc<CheckQuery> = Arc::new(move |args, limits, cancelled| {
            count.fetch_add(1, Ordering::Relaxed);
            base(args, limits, cancelled)
        });
        let context = ToolContext::new(
            mode,
            ScopeRoot::open(&f.project)?,
            ScopeRoot::open(session.temporary_dir())?,
            ScopeRoot::open(&spill)?,
            ExcludeRules::new(
                &f.project,
                &f.paths,
                &spill,
                &ReadConfig::default(),
                &ReadConfig::default(),
            )?,
        )
        .with_eval_switches(enabled, true)
        .with_skill(support::skills()?)
        .with_check_query(query)
        .with_session(session.clone());
        let environment = Environment::capture(
            &f.project,
            mode,
            &[f.project.clone(), session.temporary_dir().into()],
            false,
            SystemTime::now(),
        )?;
        let mut agent = Agent::new(
            config,
            "synthetic-key".into(),
            context,
            environment,
            session,
        )?;
        let (report, _) = complete(&mut agent)?;
        assert_eq!(report.outcome, Outcome::Completed);
        assert_eq!(launches.load(Ordering::Relaxed), expected);
        assert_eq!(
            (report.summary.checks, report.summary.after_write_checks),
            (expected, expected)
        );
        let outputs = results(&agent);
        assert_eq!(outputs.len(), 3);
        let write = outputs.first().ok_or("write result")?;
        assert!(!write.is_error);
        if expected > 0 {
            assert_eq!(
                write.text,
                "Wrote new.bnt (1 lines).\nCheck of new.bnt after this change: ok"
            );
        } else {
            assert_eq!(write.text, "Wrote new.bnt (1 lines).");
        }
        let edit = outputs.get(1).ok_or("edit result")?;
        assert!(!edit.is_error);
        assert_eq!(
            edit.text.contains("Check of new.bnt after this change: ok"),
            expected > 0
        );
        let temporary = outputs.last().ok_or("temporary result")?;
        assert!(!temporary.is_error, "{}", temporary.text);
        assert_eq!(
            temporary.text.contains("Not checked automatically:"),
            mode == Mode::Restricted && enabled
        );
        assert_eq!(fs::read_to_string(f.project.join("new.bnt"))?, "after\n");
        assert_eq!(fs::read_to_string(temporary_file)?, "temporary\n");
        server.shutdown()?;
    }
    Ok(())
}

// 関門: 10 回を番数で数える、最後の番の結果を落とす、止めたキーを戻さない退行は
// 診断の部品では検出できない。模擬の提供者から本物の道具を通して守る。
#[test]
fn ten_failures_stop_after_recording_whole_turn_and_reset_for_next_request() -> TestResult {
    let f = Fixture::new()?;
    let mut scripts = (0..9).map(|_| check_script("a.bnt")).collect::<Vec<_>>();
    scripts.push(ResponseScript::sse(calls(
        &[check("a.bnt"), ("read", json!({"path":"b.bnt"}))],
        "tool_calls",
    )));
    scripts.extend([check_script("a.bnt"), final_text()]);
    let mut server = MockProvider::start(scripts)?;
    let mut config = f.config(&server, AdapterKind::OpenAiChat);
    config.mode = Mode::Restricted;
    let (mut agent, directory) =
        f.agent_with_check(config, None, Some(query(vec![Response::Failed; 11])))?;
    let (report, events) = complete(&mut agent)?;
    assert_eq!(report.outcome, Outcome::Failed);
    assert_eq!(report.summary.checks, 10);
    assert_eq!(report.summary.fixes, 10);
    assert_eq!(report.summary.after_write_checks, 0);
    assert_eq!(
        report.summary.reason,
        "The run stopped because checks of a.bnt failed 10 times without passing (limit: 10; change it with --max-check-fix or limits.max_check_fix)."
    );
    assert!(events.iter().any(|event| matches!(event, AgentEvent::Error { kind, message } if kind == "limit" && message.starts_with(&report.summary.reason) && message.contains("Remaining diagnostics:\n") && message.contains("last diagnostic"))));
    let records = read_records(&directory)?;
    assert_eq!(
        records
            .iter()
            .filter(|record| matches!(record, Record::ToolResult { .. }))
            .count(),
        11
    );
    let results = results(&agent);
    assert!(
        results
            .get(9)
            .is_some_and(|r| r.text.contains("last diagnostic"))
    );
    assert!(
        results
            .get(10)
            .is_some_and(|r| !r.is_error && r.text.contains("second"))
    );
    let (report, _) = complete(&mut agent)?;
    assert_eq!(report.outcome, Outcome::Completed);
    assert_eq!(report.summary.checks, 1);
    assert_eq!(report.summary.fixes, 1);
    assert_eq!(server.requests()?.len(), 12);
    server.shutdown()?;
    Ok(())
}

// 関門: キーの混同、警告でリセットしないこと、使い方や処理系の誤りで増減させる
// こと、通常モードも停止させることを、小さい上限の同じ表で守る。
#[test]
fn counts_are_per_path_reset_on_success_and_ignore_incomplete_checks_and_normal_mode() -> TestResult
{
    for (mode, response, sequence, expected) in [
        (
            Mode::Restricted,
            vec![
                Response::Failed,
                Response::Warning,
                Response::Failed,
                Response::Passed,
                Response::Failed,
            ],
            vec!["a.bnt"; 5],
            Outcome::Completed,
        ),
        (
            Mode::Restricted,
            vec![Response::Failed, Response::Failed],
            vec!["a.bnt", "b.bnt"],
            Outcome::Completed,
        ),
        (
            Mode::Restricted,
            vec![
                Response::Failed,
                Response::Usage,
                Response::Processor,
                Response::Failed,
            ],
            vec!["a.bnt"; 4],
            Outcome::Failed,
        ),
        (
            Mode::Normal,
            vec![Response::Failed; 3],
            vec!["a.bnt"; 3],
            Outcome::Completed,
        ),
    ] {
        let f = Fixture::new()?;
        let fixes = response
            .iter()
            .filter(|r| matches!(r, Response::Failed))
            .count();
        let launches = response.len();
        let mut scripts = sequence
            .iter()
            .map(|path| check_script(path))
            .collect::<Vec<_>>();
        if expected == Outcome::Completed {
            scripts.push(final_text());
        }
        let mut server = MockProvider::start(scripts)?;
        let mut config = f.config(&server, AdapterKind::OpenAiChat);
        config.mode = mode;
        config.limits.max_check_fix = 2;
        let (mut agent, _) = f.agent_with_check(config, None, Some(query(response)))?;
        let (report, _) = complete(&mut agent)?;
        assert_eq!(report.outcome, expected, "{}", report.summary.reason);
        assert_eq!(report.summary.checks, u64::try_from(launches)?);
        assert_eq!(report.summary.fixes, u64::try_from(fixes)?);
        if expected == Outcome::Failed {
            assert!(report.summary.reason.contains("limit: 2;"));
        }
        server.shutdown()?;
    }
    Ok(())
}

// 関門: 書いた後だけ集計を忘れることと、新欄のない既存の集計の拒否を守る。
#[test]
fn after_write_launches_and_failed_checks_are_counted_and_legacy_summaries_still_read() -> TestResult
{
    let f = Fixture::new()?;
    let mut server = MockProvider::start(vec![
        ResponseScript::sse(calls(
            &[(
                "write",
                json!({"path":"new.bnt","content":"function main() -> Unit\n return UNKNOWN\nend function\n"}),
            )],
            "tool_calls",
        )),
        final_text(),
    ])?;
    let mut config = f.config(&server, AdapterKind::OpenAiChat);
    config.mode = Mode::Restricted;
    let (mut agent, _) =
        f.agent_with_check(config, None, Some(support::checks(f.project.clone())?))?;
    let (report, _) = complete(&mut agent)?;
    assert_eq!(report.outcome, Outcome::Completed);
    assert_eq!(
        (
            report.summary.checks,
            report.summary.after_write_checks,
            report.summary.fixes
        ),
        (1, 1, 1)
    );
    let legacy = r#"{"turns":1,"tool_calls":{},"checks":3,"fixes":2,"approvals":{"approved":0,"denied":0,"cancelled":0},"tokens":{"input":1,"output":1,"cache_read":0,"cache_write":0},"elapsed_ms":0,"reason":"done"}"#;
    let summary: benitoiteh::session::Summary = serde_json::from_str(legacy)?;
    assert_eq!(
        (summary.checks, summary.fixes, summary.after_write_checks),
        (3, 2, 0)
    );
    server.shutdown()?;
    Ok(())
}

// 関門: 同じ番の後の write の成功で失敗回数が戻っても、古い停止の印が残る退行。
// 口を差し替えず、本物の処理系で誤りから成功までを通す。
#[test]
fn later_write_in_same_turn_clears_check_limit_before_next_request() -> TestResult {
    let f = Fixture::new()?;
    let mut server = MockProvider::start(vec![
        ResponseScript::sse(calls(
            &[
                check("a.bnt"),
                ("read", json!({"path":"a.bnt"})),
                (
                    "write",
                    json!({"path":"a.bnt","content":"function main() -> Unit\n  return ()\nend function\n"}),
                ),
            ],
            "tool_calls",
        )),
        final_text(),
    ])?;
    let mut config = f.config(&server, AdapterKind::OpenAiChat);
    config.mode = Mode::Restricted;
    config.limits.max_check_fix = 1;
    let (mut agent, _) =
        f.agent_with_check(config, None, Some(support::checks(f.project.clone())?))?;
    let (report, events) = complete(&mut agent)?;
    assert_eq!(
        report.outcome,
        Outcome::Completed,
        "{}",
        report.summary.reason
    );
    assert_eq!(
        (
            report.summary.checks,
            report.summary.after_write_checks,
            report.summary.fixes
        ),
        (2, 1, 1)
    );
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, AgentEvent::Error { kind, .. } if kind == "limit"))
    );
    assert_eq!(server.requests()?.len(), 2);
    server.shutdown()?;
    Ok(())
}

// 関門: 根の空の相対パス、単数形、停止時の TurnEnd と残る診断の欠落を守る。
#[test]
fn directory_check_limit_reports_dot_singular_diagnostics_and_turn_end() -> TestResult {
    let f = Fixture::new()?;
    let mut server = MockProvider::start(vec![ResponseScript::sse(calls(
        &[("benitoite_check", json!({"path":".","tests":true}))],
        "tool_calls",
    ))])?;
    let mut config = f.config(&server, AdapterKind::OpenAiChat);
    config.mode = Mode::Restricted;
    config.limits.max_check_fix = 1;
    let (mut agent, _) =
        f.agent_with_check(config, None, Some(support::checks(f.project.clone())?))?;
    let (report, events) = complete(&mut agent)?;
    assert_eq!(report.outcome, Outcome::Failed);
    assert!(
        report
            .summary
            .reason
            .starts_with("The run stopped because checks of . failed 1 time without passing"),
        "{}",
        report.summary.reason
    );
    assert!(!report.summary.reason.contains('\n'));
    let diagnostic = results(&agent).first().ok_or("result")?.text.clone();
    let ended = events
        .iter()
        .position(|event| matches!(event, AgentEvent::TurnEnd { .. }))
        .ok_or("turn end")?;
    let error = events.iter().position(|event| matches!(event, AgentEvent::Error { kind, message } if kind == "limit" && message.starts_with(&report.summary.reason) && message.ends_with(&diagnostic))).ok_or("full diagnostics")?;
    assert!(ended < error);
    server.shutdown()?;
    Ok(())
}

// 関門: LLM の結果を 64 KiB で切った後の文を、利用者向けの残る診断として
// 再利用する退行を守る。表示済みの診断の全文をイベントで観測する。
#[test]
fn check_limit_error_retains_diagnostics_before_tool_output_truncation() -> TestResult {
    let f = Fixture::new()?;
    let mut server = MockProvider::start(vec![check_script("a.bnt")])?;
    let mut config = f.config(&server, AdapterKind::OpenAiChat);
    config.mode = Mode::Restricted;
    config.limits.max_check_fix = 1;
    // 一つの欄の上限は守りつつ、診断の全体を道具の上限より大きくする。
    let notes = (0..20)
        .map(|n| format!("note {n}: {}", "🟦".repeat(3000)))
        .collect::<Vec<_>>();
    let expected = "note 19:";
    let query: Arc<CheckQuery> = Arc::new(move |_, _, _| {
        Err(BenitoiteError::Exit { stdout: Vec::new(), code: Some(2), stderr: json!({"kind":"check","severity":"error","code":"E0101","message":"discovery failed","primary":null,"secondary":[],"notes":notes,"helps":[]}).to_string().into_bytes() })
    });
    let (mut agent, _) = f.agent_with_check(config, None, Some(query))?;
    let (report, events) = complete(&mut agent)?;
    assert_eq!(report.outcome, Outcome::Failed);
    assert!(
        results(&agent)
            .first()
            .is_some_and(|result| result.text.contains("[Output truncated:"))
    );
    assert!(events.iter().any(|event| matches!(event, AgentEvent::Error { kind, message } if kind == "limit" && message.contains(expected) && !message.contains("[Output truncated:"))));
    assert!(!report.summary.reason.contains('\n'));
    server.shutdown()?;
    Ok(())
}

// 関門: 非対話の拒否でも一度目の問い合わせを数え、道具の本文で確認を偽れない契約。
#[test]
fn restricted_runs_need_trusted_approval_and_count_inspections_even_when_denied() -> TestResult {
    for (approval, answer, message) in [
        (Approval::Never, ApprovalOutcome::Approved, None),
        (Approval::OnRequest, ApprovalOutcome::Denied, None),
        (
            Approval::OnRequest,
            ApprovalOutcome::DeniedWithMessage,
            Some("wait"),
        ),
    ] {
        let f = Fixture::new()?;
        fs::write(
            f.project.join("a.bnt"),
            "function main() -> Unit\n return ()\nend function\n",
        )?;
        let mut server = MockProvider::start(vec![
            ResponseScript::sse(calls(
                &[("benitoite_run", json!({"path":"a.bnt"}))],
                "tool_calls",
            )),
            final_text(),
        ])?;
        let mut config = f.config(&server, AdapterKind::OpenAiChat);
        config.mode = Mode::Restricted;
        let (agent, directory) =
            f.agent_with_check(config, None, Some(support::checks(f.project.clone())?))?;
        let requests = Arc::new(Mutex::new(Vec::new()));
        let mut agent = agent.with_approvals(
            Box::new(ScriptedApprover {
                requests: Arc::clone(&requests),
                answer: ApprovalAnswer {
                    outcome: answer,
                    message: message.map(str::to_owned),
                },
            }),
            ApprovalPolicy {
                approval,
                sandbox: Sandbox::WorkspaceWrite,
                sandboxed: true,
            },
        );
        let (report, events) = complete(&mut agent)?;
        assert_eq!(report.summary.checks, 1);
        assert_eq!(report.summary.fixes, 0);
        assert_eq!(report.summary.after_write_checks, 0);
        let outputs = results(&agent);
        let result = outputs.first().ok_or("result")?;
        assert!(result.is_error);
        if approval == Approval::Never {
            assert_eq!(report.summary.approvals.policy_denied, 1);
            assert_eq!(
                result.text,
                "Tool call was denied by the never approval policy: running a Benitoite script requires confirming its effects. Stop and report."
            );
            assert!(requests.lock().map_err(|_| "requests")?.is_empty());
        } else {
            assert!(
                result
                    .text
                    .starts_with("The user declined to run a.bnt with effects: none.")
            );
            assert_eq!(
                result.text.contains("Message from the user: wait"),
                message.is_some()
            );
            let records = read_records(&directory)?;
            assert_eq!(
                records
                    .iter()
                    .filter(
                        |r| matches!(r,Record::Approval {facts,..} if !facts.sources.is_empty())
                    )
                    .count(),
                2
            );
            assert!(events.iter().any(|e|matches!(e,AgentEvent::ApprovalRequest {kind:ApprovalKind::Effects,facts,..} if !facts.sources.is_empty())));
        }
        assert!(!read_records(&directory)?.iter().any(|r|matches!(r,Record::Checkpoint(c) if c.reason==benitoiteh::rewind::CheckpointReason::BenitoiteRun)));
        server.shutdown()?;
    }
    Ok(())
}
