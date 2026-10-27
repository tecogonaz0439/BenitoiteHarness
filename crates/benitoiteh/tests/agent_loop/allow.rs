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
            test(),
            ("benitoite_scratch", json!({"code":WRITE})),
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
        "The run stopped because benitoite_run, benitoite_test or benitoite_scratch was refused 3 times in a row: the effects were outside --allow."
    );
    assert_eq!(server.requests()?.len(), 1);
    assert_eq!(results(&agent).len(), 6);
    let outputs = results(&agent);
    assert!(
        outputs
            .get(5)
            .is_some_and(|o| !o.is_error && o.text.contains("File.Write"))
    );
    let scratch = outputs.get(4).ok_or("scratch result")?;
    let path = scratch.text.lines().next().ok_or("scratch path")?;
    assert!(path.ends_with("/scratch-1.bnt"), "{scratch:?}");
    assert_eq!(
        scratch.text,
        format!(
            "{path}\nThis non-interactive run allows only these effects: Console.Write, Log, Process.Exit, State. {path} uses: File.Write. Narrow the uses of main, or stop and report."
        )
    );
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
// scratch の自動の Log と明示した File.Write の和を、再検査にも渡す（設計書 03-03「非対話での扱い」）。
#[test]
fn named_allow_set_is_honored_without_asking_the_approver() -> TestResult {
    use benitoiteh::benitoite::Effect;
    let log_write = "import Benitoite.Unofficial.IO.File\nfunction main() -> Unit uses Log, File.Write\n Log.info(message: \"write\", attributes: [], location: SourceLocation.here())\n bind _ <- File.writeText(path: \"MUST_NOT_EXIST\", text: \"ran\")\nend function\n";
    for (source, tool, args, needed) in [
        (
            WRITE,
            "benitoite_run",
            json!({"path":"a.bnt"}),
            Effect::FileWrite,
        ),
        (
            "function main() -> Unit uses Log\n return ()\nend function\n",
            "benitoite_run",
            json!({"path":"a.bnt"}),
            Effect::Log,
        ),
        (
            "uses Log\n@test\nfunction example() -> Unit uses Assert.Check\n Assert.equal(actual: 1, expected: 1)\nend function\n",
            "benitoite_test",
            json!({"paths":["a.bnt"]}),
            Effect::Log,
        ),
        (
            log_write,
            "benitoite_scratch",
            json!({"code":log_write}),
            Effect::FileWrite,
        ),
    ] {
        for permitted in [false, true] {
            let f = Fixture::new()?;
            fs::write(f.project.join("a.bnt"), source)?;
            let mut server = MockProvider::start(vec![
                ResponseScript::sse(calls(&[(tool, args.clone())], "tool_calls")),
                final_text(),
            ])?;
            let mut config = f.config(&server, AdapterKind::OpenAiChat);
            config.mode = Mode::Restricted;
            config.allow_effects = Some(if permitted {
                [needed].into_iter().collect()
            } else {
                Default::default()
            });
            let (mut agent, _) =
                f.agent_with_check(config, None, Some(support::checks(f.project.clone())?))?;
            let (report, events) = complete(&mut agent)?;
            assert_eq!(
                report.summary.approvals.policy_denied,
                u64::from(!permitted),
                "{tool}"
            );
            assert_eq!(
                report.summary.checks,
                if permitted { 2 } else { 1 },
                "{tool}: {:?}",
                results(&agent)
            );
            let outputs = results(&agent);
            let output = outputs.first().ok_or("result")?;
            assert!(
                if permitted {
                    output.text.contains("execution context is unavailable")
                } else {
                    output.text.contains("allows only these effects:")
                },
                "{tool}: {output:?}"
            );
            if tool == "benitoite_scratch" && !permitted {
                assert!(output.text.contains("uses: File.Write, Log."), "{output:?}");
            }
            assert!(
                !events
                    .iter()
                    .any(|e| matches!(e, AgentEvent::ApprovalRequest { .. }))
            );
            server.shutdown()?;
        }
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

// 関門: --allow と自動の集合の和を判定と再検査の両方に渡す。片方だけだと EFFECTS_CHANGED になる。
#[test]
fn scratch_noninteractive_allow_unions_automatic_effects_and_reports_the_union_on_denial()
-> TestResult {
    let source = "import Benitoite.Unofficial.IO.File\nimport Benitoite.Unofficial.IO.Console\nfunction main() -> Unit uses File.Read, Console.Write\n bind _ <- File.readText(\"input\")\n Console.write(\"ran\")\nend function\n";
    for permitted in [true, false] {
        let f = Fixture::new()?;
        let mut server = MockProvider::start(vec![
            ResponseScript::sse(calls(
                &[("benitoite_scratch", json!({"code":source}))],
                "tool_calls",
            )),
            final_text(),
        ])?;
        let mut config = f.config(&server, AdapterKind::OpenAiChat);
        config.mode = Mode::Restricted;
        config.allow_effects = Some(if permitted {
            [benitoiteh::benitoite::Effect::FileRead]
                .into_iter()
                .collect()
        } else {
            Default::default()
        });
        let (mut agent, directory) =
            f.agent_with_check(config, None, Some(support::checks(f.project.clone())?))?;
        let (report, events) = complete(&mut agent)?;
        assert_eq!(report.summary.checks, if permitted { 2 } else { 1 });
        assert_eq!(
            report.summary.approvals.policy_denied,
            u64::from(!permitted)
        );
        let outputs = results(&agent);
        let out = outputs.first().ok_or("scratch result")?;
        assert!(
            out.text
                .lines()
                .next()
                .is_some_and(|path| path.ends_with("/scratch-1.bnt")),
            "{out:?}"
        );
        if permitted {
            // 実行は sandbox の境界が守る。この準備では二度目の報告まで通し、文脈の不足で止まる。
            assert!(
                out.text.contains("execution context is unavailable")
                    && !out.text.contains("script changed"),
                "{out:?}"
            );
        } else {
            assert!(
                out.text.contains(
                    "allows only these effects: Console.Write, Log, Process.Exit, State."
                ) && out.text.contains("uses: Console.Write, File.Read."),
                "{out:?}"
            );
        }
        assert!(
            !events
                .iter()
                .any(|event| matches!(event, AgentEvent::ApprovalRequest { .. }))
        );
        assert!(
            !read_records(&directory)?
                .iter()
                .any(|record| matches!(record, Record::Approval { .. }))
        );
        server.shutdown()?;
    }
    Ok(())
}

// 関門: 自動の集合に収まる取り込みも非対話では拒み、連続したエフェクトの拒否には数えない。
#[test]
fn scratch_unread_dependencies_are_denied_without_starting_a_refusal_sequence() -> TestResult {
    for case in ["outside", "excluded", "decoy-package", "decoy-root"] {
        let f = Fixture::new()?;
        let (dependency, module, excluded) = if case.starts_with("decoy") {
            support::import_decoy::prepare(&f.project, case == "decoy-package")?;
            fs::write(
                f.project.join("t5/Lib/T.bnt"),
                "import Secret.Keys\npublic function token() -> String\n return Keys.token()\nend function\n",
            )?;
            (f.project.join("t5"), "Lib.T", "t5/Secret/**")
        } else {
            let path = if case == "outside" {
                f.root.join("outside")
            } else {
                f.project.join("Secrets")
            };
            fs::create_dir(&path)?;
            fs::write(
                path.join("Keys.bnt"),
                "public function token() -> String\n return \"SECRET_SCRATCH_VALUE\"\nend function\n",
            )?;
            (path, "Keys", "Secrets/**")
        };
        fs::write(
            dependency.join("package.bnt"),
            "language \"0.0.12\"\npackage Ext\nend package\n",
        )?;
        let alias = module.rsplit('.').next().ok_or("module")?;
        let code = format!(
            "dependency Ext\n path {}\nend dependency\nimport Ext.{module}\nimport Benitoite.Unofficial.IO.Console\nfunction main() -> Unit uses Console.Write\n Console.write({alias}.token())\nend function\n",
            json!(dependency)
        );
        let call = ("benitoite_scratch", json!({"code":code}));
        let mut server = MockProvider::start(vec![
            ResponseScript::sse(calls(&[call.clone(), call.clone(), call], "tool_calls")),
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
                exclude: vec![excluded.into()],
            },
        )?;
        let (report, events) = complete(&mut agent)?;
        assert_eq!(
            report.outcome,
            Outcome::Completed,
            "{case}: {}",
            report.summary.reason
        );
        assert_eq!(report.summary.checks, 3);
        assert_eq!(report.summary.approvals.policy_denied, 3);
        for (number, out) in results(&agent).iter().enumerate() {
            let path = out.text.lines().next().ok_or("path")?;
            assert!(
                path.ends_with(&format!("/scratch-{}.bnt", number + 1)),
                "{case}: {out:?}"
            );
            assert_eq!(out.text.lines().nth(1), Some(format!("{path} import files that benitoiteh does not read; run them interactively to confirm.").as_str()));
            assert!(
                out.is_error
                    && !out.text.contains("SECRET_HASH_COLLISION")
                    && !out.text.contains("SECRET_SCRATCH_VALUE")
            );
        }
        assert!(
            !events
                .iter()
                .any(|event| matches!(event, AgentEvent::ApprovalRequest { .. }))
        );
        assert!(!read_records(&directory)?.iter().any(|record| matches!(
            record,
            Record::Approval { .. }
                | Record::Checkpoint(benitoiteh::rewind::Checkpoint {
                    reason: benitoiteh::rewind::CheckpointReason::BenitoiteScratch,
                    ..
                })
        )));
        server.shutdown()?;
    }
    Ok(())
}

// 関門: 自動の許可をパスに残すと、書き換えた一時ファイルを run で動かせてしまう。
// OS を要しないループの境界では、自動の Console.Write が run に引き継がれないことを守る。
#[test]
fn scratch_automatic_effects_do_not_authorize_later_runs_of_the_same_path() -> TestResult {
    let f = Fixture::new()?;
    let session = Session::create(&f.paths, SessionId::generate(SystemTime::now())?)?;
    let path = session.temporary_dir().join("scratch-1.bnt");
    let initial = "import Benitoite.Unofficial.IO.Console\nimport Benitoite.Unofficial.IO.File\nfunction main() -> Unit uses Console.Write, File.Read\n bind _ <- File.readText(\"input\")\n Console.write(\"initial\")\nend function\n";
    let changed = "import Benitoite.Unofficial.IO.Console\nfunction main() -> Unit uses Console.Write\n Console.write(\"changed\")\nend function\n";
    fs::write(f.project.join("same.bnt"), changed)?;
    let mut server = MockProvider::start(vec![
        ResponseScript::sse(calls(
            &[
                ("benitoite_scratch", json!({"code":initial})),
                ("read", json!({"path":path})),
                ("write", json!({"path":path,"content":changed})),
                ("benitoite_run", json!({"path":path})),
                ("benitoite_run", json!({"path":"same.bnt"})),
            ],
            "tool_calls",
        )),
        final_text(),
    ])?;
    let mut config = f.config(&server, AdapterKind::OpenAiChat);
    config.mode = Mode::Restricted;
    config.allow_effects = Some(
        [benitoiteh::benitoite::Effect::FileRead]
            .into_iter()
            .collect(),
    );
    let mut agent = scratch_agent_in_session(
        &f,
        session.clone(),
        config,
        support::checks(f.project.clone())?,
    )?;
    let (report, events) = complete(&mut agent)?;
    assert_eq!(report.outcome, Outcome::Completed);
    assert_eq!(report.summary.approvals.policy_denied, 2);
    let outputs = results(&agent);
    let first = outputs.first().ok_or("scratch")?;
    assert!(
        first.text.contains("execution context is unavailable")
            && !first.text.contains("script changed"),
        "{first:?}"
    );
    assert!(!outputs.get(2).ok_or("write")?.is_error, "{outputs:?}");
    for out in outputs.get(3..).ok_or("run results")? {
        assert!(
            out.is_error
                && out.text.contains("allows only these effects: File.Read.")
                && out.text.contains("uses: Console.Write."),
            "{out:?}"
        );
    }
    assert_eq!(fs::read_to_string(path)?, changed);
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, AgentEvent::ApprovalRequest { .. }))
    );
    server.shutdown()?;
    Ok(())
}

fn scratch_agent_in_session(
    f: &Fixture,
    session: Session,
    config: AgentConfig,
    query: Arc<benitoiteh::benitoite::CheckQuery>,
) -> Result<Agent, Box<dyn Error>> {
    let spill = session.directory().join("spill");
    let context = ToolContext::new(
        Mode::Restricted,
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
    .with_session(session.clone())
    .with_check_query(query)
    .with_skill(support::skills()?);
    let environment = Environment::capture(
        &f.project,
        Mode::Restricted,
        &[f.project.clone(), session.temporary_dir().into()],
        false,
        SystemTime::now(),
    )?;
    Ok(Agent::new(
        config,
        "synthetic".into(),
        context,
        environment,
        session,
    )?)
}

// 関門: 呼び直しの範囲は初回の報告だけで決める。設定や --allow の未報告の分は引き継がない。
#[test]
fn scratch_retry_rejects_effects_added_after_the_initial_report() -> TestResult {
    for extra in ["Process.Exit", "Clock.Time"] {
        let f = Fixture::new()?;
        let session = Session::create(&f.paths, SessionId::generate(SystemTime::now())?)?;
        let path = session.temporary_dir().join("scratch-1.bnt");
        let initial = "import Benitoite.Unofficial.IO.Console\nimport Benitoite.Unofficial.IO.File\nfunction main() -> Unit uses Console.Write, File.Read\n bind _ <- File.readText(\"input\")\n Console.write(\"initial\")\nend function\n";
        let expanded = format!(
            "import Benitoite.Unofficial.IO.Console\nimport Benitoite.Unofficial.IO.File\nimport Benitoite.Unofficial.IO.Process\nimport Benitoite.Unofficial.IO.Clock\nfunction main() -> Unit uses Console.Write, File.Read, {extra}\n bind _ <- File.readText(\"input\")\n Console.write(\"changed\")\nend function\n"
        );
        let mut server = MockProvider::start(vec![
            ResponseScript::sse(calls(
                &[("benitoite_scratch", json!({"code":initial}))],
                "tool_calls",
            )),
            final_text(),
        ])?;
        let mut config = f.config(&server, AdapterKind::OpenAiChat);
        config.mode = Mode::Restricted;
        config.allow_effects = Some(
            [
                benitoiteh::benitoite::Effect::FileRead,
                benitoiteh::benitoite::Effect::ClockTime,
            ]
            .into_iter()
            .collect(),
        );
        let query = support::checks(f.project.clone())?;
        let first = AtomicBool::new(true);
        let query: Arc<benitoiteh::benitoite::CheckQuery> = Arc::new(move |args, limits, stop| {
            if !first.swap(false, Ordering::AcqRel) {
                fs::write(&path, &expanded)?;
            }
            query(args, limits, stop)
        });
        let mut agent = scratch_agent_in_session(&f, session, config, query)?;
        let (report, _) = complete(&mut agent)?;
        assert_eq!(report.summary.checks, 2);
        let outputs = results(&agent);
        let out = outputs.first().ok_or("scratch result")?;
        assert!(
            out.is_error
                && out
                    .text
                    .contains("The script changed after the effects were confirmed"),
            "{extra}: {out:?}"
        );
        server.shutdown()?;
    }
    Ok(())
}
