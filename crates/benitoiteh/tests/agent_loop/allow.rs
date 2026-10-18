//! 非対話の許可と連続拒否を、本物の報告・道具・記録で確かめる（設計書 03-03）。
use super::*;
const WRITE: &str = "import Benitoite.Unofficial.IO.File\nfunction main() -> Unit uses File.Write\n bind _ <- File.writeText(path: \"MUST_NOT_EXIST\", text: \"ran\")\nend function\n";
const TEST_WRITE: &str = "import Benitoite.Unofficial.IO.File\n@test(\"write\")\nfunction testWrite() -> Unit uses File.Write\n bind _ <- File.writeText(path: \"MUST_NOT_EXIST\", text: \"ran\")\nend function\n";
fn run() -> (&'static str, Value) {
    ("benitoite_run", json!({"path":"a.bnt"}))
}
fn test() -> (&'static str, Value) {
    ("benitoite_test", json!({"paths":["c.bnt"]}))
}
fn setup(f: &Fixture) -> TestResult {
    fs::write(f.project.join("a.bnt"), WRITE)?;
    fs::write(f.project.join("c.bnt"), TEST_WRITE)?;
    Ok(())
}
// 関門: 別の道具や不完全な検査で拒否の回数を戻す、最後の番の結果を落とす、次の要求を送る退行。
#[test]
fn third_denial_stops_after_turn_end_and_records_all_calls_without_approval_records() -> TestResult
{
    let f = Fixture::new()?;
    setup(&f)?;
    let mut server = MockProvider::start(vec![ResponseScript::sse(calls(
        &[
            run(),
            ("read", json!({"path":"b.bnt"})),
            ("benitoite_run", json!({"path":"b.bnt"})),
            run(),
            test(),
            ("read", json!({"path":"a.bnt"})),
        ],
        "tool_calls",
    ))])?;
    let mut config = f.config(&server, AdapterKind::OpenAiChat);
    config.mode = Mode::Restricted;
    config.allow_effects = Some(Default::default());
    let (mut agent, directory) =
        f.agent_with_check(config, None, Some(support::checks(f.project.clone())?))?;
    let (report, events) = complete(&mut agent)?;
    assert_eq!(report.outcome, Outcome::Failed);
    assert_eq!(report.summary.approvals.policy_denied, 3);
    assert_eq!(report.summary.checks, 4);
    assert_eq!(
        report.summary.reason,
        "The run stopped because benitoite_run or benitoite_test was refused 3 times in a row: the effects were outside --allow."
    );
    assert_eq!(server.requests()?.len(), 1);
    assert_eq!(results(&agent).len(), 6);
    let outputs = results(&agent);
    assert!(
        outputs
            .get(5)
            .is_some_and(|o| !o.is_error && o.text.contains("File.Write"))
    );
    assert!(outputs.get(4).is_some_and(|o|o.text=="This non-interactive run allows only these effects: none. c.bnt use: File.Write. Narrow the uses of the tests, or stop and report."));
    assert!(!f.project.join("MUST_NOT_EXIST").exists());
    assert!(
        !read_records(&directory)?
            .iter()
            .any(|r| matches!(r, Record::Approval { .. }))
    );
    assert!(!events.iter().any(|e| matches!(
        e,
        AgentEvent::ApprovalRequest { .. } | AgentEvent::ApprovalResult { .. }
    )));
    let turn = events
        .iter()
        .position(|e| matches!(e, AgentEvent::TurnEnd { .. }))
        .ok_or("TurnEnd")?;
    let error = events
        .iter()
        .position(|e| matches!(e,AgentEvent::Error {kind,..} if kind=="limit"))
        .ok_or("Error")?;
    assert!(turn < error);
    server.shutdown()?;
    Ok(())
}
// 関門: 範囲に収まった判断と依頼の開始で回数を戻す。空の許可にも確認の単位を作って呼び直す。
#[test]
fn allowed_effects_reset_denials_and_each_request_starts_a_new_sequence() -> TestResult {
    let f = Fixture::new()?;
    setup(&f)?;
    fs::write(
        f.project.join("pure.bnt"),
        "function main() -> Unit\n return ()\nend function\n",
    )?;
    let pure = ("benitoite_run", json!({"path":"pure.bnt"}));
    let mut server = MockProvider::start(vec![
        ResponseScript::sse(calls(
            &[run(), run(), pure.clone(), run(), run()],
            "tool_calls",
        )),
        final_text(),
        ResponseScript::sse(calls(&[test(), run(), pure], "tool_calls")),
        final_text(),
    ])?;
    let mut config = f.config(&server, AdapterKind::OpenAiChat);
    config.mode = Mode::Restricted;
    config.allow_effects = Some(Default::default());
    let (mut agent, directory) =
        f.agent_with_check(config, None, Some(support::checks(f.project.clone())?))?;
    for denied in [4, 2] {
        let (report, _) = complete(&mut agent)?;
        assert_eq!(
            report.outcome,
            Outcome::Completed,
            "{}",
            report.summary.reason
        );
        assert_eq!(report.summary.approvals.policy_denied, denied);
    }
    // この fixture は実行用の文脈を持たない。確認済みの空集合で二度目の報告まで進み、
    // EFFECTS_CHANGED で誤って止まらないことを観測する。実行自体は sandbox のテストが守る。
    let outputs = results(&agent);
    assert_eq!(
        outputs
            .iter()
            .filter(|o| o.text.contains("execution context is unavailable"))
            .count(),
        2
    );
    assert!(!outputs.iter().any(|o| o.text.contains("script changed")));
    assert!(
        !read_records(&directory)?
            .iter()
            .any(|r| matches!(r, Record::Approval { .. }))
    );
    server.shutdown()?;
    Ok(())
}
// 関門: --allow の集合を事実の部分集合と逆に比べる、Assert.Check を別の許可として数える退行。
#[test]
fn named_allow_set_is_honored_without_asking_the_approver() -> TestResult {
    for allowed in [
        vec![benitoiteh::benitoite::Effect::FileWrite],
        vec![benitoiteh::benitoite::Effect::FileRead],
    ] {
        let f = Fixture::new()?;
        setup(&f)?;
        let mut server = MockProvider::start(vec![
            ResponseScript::sse(calls(&[run()], "tool_calls")),
            final_text(),
        ])?;
        let mut config = f.config(&server, AdapterKind::OpenAiChat);
        config.mode = Mode::Restricted;
        config.allow_effects = Some(allowed.iter().copied().collect());
        let (mut agent, _) =
            f.agent_with_check(config, None, Some(support::checks(f.project.clone())?))?;
        let (report, events) = complete(&mut agent)?;
        let permitted = allowed.contains(&benitoiteh::benitoite::Effect::FileWrite);
        assert_eq!(
            report.summary.approvals.policy_denied,
            u64::from(!permitted)
        );
        assert_eq!(report.summary.checks, if permitted { 2 } else { 1 });
        assert!(
            !events
                .iter()
                .any(|e| matches!(e, AgentEvent::ApprovalRequest { .. }))
        );
        server.shutdown()?;
    }
    let f = Fixture::new()?;
    let mut server = MockProvider::start(vec![])?;
    let mut config = f.config(&server, AdapterKind::OpenAiChat);
    config.allow_effects = Some(Default::default());
    assert!(matches!(f.agent(config),Err(e) if e.to_string().contains("configuration")));
    server.shutdown()?;
    Ok(())
}

// 関門: --allowでは秘密の取り込みを許せない。承認の呼び直しで初めて増えた取り込みも拒む。
#[test]
fn unread_imports_are_denied_even_with_allowed_effects_and_after_reinspection() -> TestResult {
    for tests in [false, true] {
        for late in [false, true] {
            let f = Fixture::new()?;
            fs::create_dir(f.project.join("Secrets"))?;
            fs::write(
                f.project.join("Secrets/Keys.bnt"),
                "public function token() -> String\n return \"SECRET_IMPORT_VALUE\"\nend function\n",
            )?;
            let expanded = if tests {
                "import Secrets.Keys\nimport Benitoite.Assert\n@test\nfunction example() -> Unit uses Assert.Check\n Assert.equal(actual: Keys.token(), expected: \"\")\nend function\n"
            } else {
                "import Secrets.Keys\nfunction main() -> Result[Unit, String]\n return Result.Error(Keys.token())\nend function\n"
            };
            let pure = if tests {
                "import Benitoite.Assert\n@test\nfunction example() -> Unit uses Assert.Check\n Assert.equal(actual: 1, expected: 1)\nend function\n"
            } else {
                "function main() -> Unit\n return ()\nend function\n"
            };
            let path = f.project.join("a.bnt");
            fs::write(&path, if late { pure } else { expanded })?;
            let mut server = MockProvider::start(vec![
                ResponseScript::sse(calls(
                    &[if tests {
                        ("benitoite_test", json!({"paths":["a.bnt"]}))
                    } else {
                        ("benitoite_run", json!({"path":"a.bnt"}))
                    }],
                    "tool_calls",
                )),
                final_text(),
            ])?;
            let mut config = f.config(&server, AdapterKind::OpenAiChat);
            config.mode = Mode::Restricted;
            config.allow_effects = Some(
                [benitoiteh::benitoite::Effect::AssertCheck]
                    .into_iter()
                    .collect(),
            );
            let query = support::checks(f.project.clone())?;
            let first = AtomicBool::new(true);
            let query: Arc<benitoiteh::benitoite::CheckQuery> =
                Arc::new(move |args, limits, stop| {
                    // 初回の内容照合を通した後、再検査の直前にソースを変える。
                    if late && !first.swap(false, Ordering::AcqRel) {
                        fs::write(&path, expanded)?;
                    }
                    query(args, limits, stop)
                });
            let (mut agent, directory) = f.agent_with_read_config(
                config,
                None,
                Some(query),
                ReadConfig {
                    exclude: vec!["Secrets/**".into()],
                },
            )?;
            let (report, events) = complete(&mut agent)?;
            assert_eq!(report.outcome, Outcome::Completed);
            assert_eq!(report.summary.checks, if late { 2 } else { 1 });
            let outputs = results(&agent);
            let output = outputs.first().ok_or("result")?;
            assert!(
                output.is_error
                    && output.text
                        == "a.bnt import files that benitoiteh does not read; run them interactively to confirm.",
                "{output:?}"
            );
            assert!(!output.text.contains("SECRET_IMPORT_VALUE"));
            assert!(
                !events
                    .iter()
                    .any(|e| matches!(e, AgentEvent::ApprovalRequest { .. }))
            );
            assert!(!read_records(&directory)?.iter().any(|r| matches!(
                r,
                Record::Approval { .. }
                    | Record::Checkpoint(benitoiteh::rewind::Checkpoint {
                        reason: benitoiteh::rewind::CheckpointReason::BenitoiteRun
                            | benitoiteh::rewind::CheckpointReason::BenitoiteTest,
                        ..
                    })
            )));
            server.shutdown()?;
        }
    }
    Ok(())
}

// 関門: 下位パッケージと読める囮を置いても、非対話の空の許可から秘密を取り出せない。
#[test]
fn noninteractive_tests_reject_real_unread_sources_behind_readable_decoys() -> TestResult {
    for nested_package in [true, false] {
        let f = Fixture::new()?;
        support::import_decoy::prepare(&f.project, nested_package)?;
        let mut server = MockProvider::start(vec![
            ResponseScript::sse(calls(
                &[("benitoite_test", json!({"paths":["t5"]}))],
                "tool_calls",
            )),
            final_text(),
        ])?;
        let mut config = f.config(&server, AdapterKind::OpenAiChat);
        config.mode = Mode::Restricted;
        config.allow_effects = Some(Default::default());
        let (mut agent, directory) = f.agent_with_read_config(
            config,
            None,
            Some(support::checks(f.project.clone())?),
            ReadConfig {
                exclude: vec!["t5/Secret/**".into()],
            },
        )?;
        let (report, events) = complete(&mut agent)?;
        assert_eq!(report.outcome, Outcome::Completed);
        assert_eq!(report.summary.checks, 1);
        let outputs = results(&agent);
        let output = outputs.first().ok_or("result")?;
        assert!(
            output.is_error
                && output.text
                    == "t5 import files that benitoiteh does not read; run them interactively to confirm.",
            "{nested_package}: {output:?}"
        );
        assert!(!output.text.contains("SECRET_HASH_COLLISION"));
        assert!(
            !events
                .iter()
                .any(|e| matches!(e, AgentEvent::ApprovalRequest { .. }))
        );
        assert!(!read_records(&directory)?.iter().any(|r| matches!(
            r,
            Record::Approval { .. }
                | Record::Checkpoint(benitoiteh::rewind::Checkpoint {
                    reason: benitoiteh::rewind::CheckpointReason::BenitoiteTest,
                    ..
                })
        )));
        server.shutdown()?;
    }
    Ok(())
}
