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

// 作成時の関門: ACP の `allow_once`（一回限りのエフェクト許可）の 4 つの契約
// (1) 1 回目の再検査通過、(2) 2 回目の再確認、(3) 既存のセッション許可を消さず広げない、
// (4) 確認直後・再検査前のエフェクト増加を `EFFECTS_CHANGED` で拒む、を `benitoite_run` と
// `benitoite_test` の両方で本物の処理系報告を通して守る。既存のテストはセッション記憶しか通らない。
#[test]
fn one_shot_effect_approval_passes_reinspection_without_remembering_and_rejects_added_effects()
-> TestResult {
    type ApproveHook = Box<dyn FnMut(&ApprovalRecord) + Send>;
    struct SequenceApprover {
        requests: Arc<Mutex<Vec<ApprovalRecord>>>,
        answers: std::collections::VecDeque<ApprovalAnswer>,
        on_approve: Option<ApproveHook>,
    }
    impl Approver for SequenceApprover {
        fn approve(&mut self, request: &ApprovalRecord) -> ApprovalAnswer {
            self.requests
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push(request.clone());
            if let Some(hook) = &mut self.on_approve {
                hook(request);
            }
            self.answers
                .pop_front()
                .unwrap_or_else(|| ApprovalAnswer::new(ApprovalOutcome::Denied, None))
        }
    }

    let read_script = "import Benitoite.Unofficial.IO.File\nfunction main() -> Unit uses File.Read\n bind _ <- File.readText(\"input\")\n return ()\nend function\n";
    let read_write_script = "import Benitoite.Unofficial.IO.File\nfunction main() -> Unit uses File.Read, File.Write\n bind _ <- File.readText(\"input\")\n bind _ <- File.writeText(path: \"out\", text: \"x\")\nend function\n";
    let read_test = "import Benitoite.Unofficial.IO.File\n@test(\"read\")\nfunction testRead() -> Unit uses File.Read\n bind _ <- File.readText(\"input\")\n return ()\nend function\n";
    let read_write_test = "import Benitoite.Unofficial.IO.File\n@test(\"rw\")\nfunction testReadWrite() -> Unit uses File.Read, File.Write\n bind _ <- File.readText(\"input\")\n bind _ <- File.writeText(path: \"out\", text: \"x\")\nend function\n";

    // パターン 1・2: `benitoite_run` と `benitoite_test` で一回限り許可すると、
    // 1 回目は再検査を通過し、2 回目は記憶されず再び確認を求められる。
    for (tool, args, source, expected_ctx_err) in [
        (
            "benitoite_run",
            json!({"path": "a.bnt"}),
            read_script,
            "script execution context is unavailable",
        ),
        (
            "benitoite_test",
            json!({"paths": ["a.bnt"]}),
            read_test,
            "test execution context is unavailable",
        ),
    ] {
        let f = Fixture::new()?;
        fs::write(f.project.join("a.bnt"), source)?;
        let mut server = MockProvider::start(vec![
            ResponseScript::sse(calls(
                &[(tool, args.clone()), (tool, args.clone())],
                "tool_calls",
            )),
            final_text(),
        ])?;
        let mut config = f.config(&server, AdapterKind::OpenAiChat);
        config.mode = Mode::Restricted;
        let (agent, _) =
            f.agent_with_check(config, None, Some(support::checks(f.project.clone())?))?;
        let requests = Arc::new(Mutex::new(Vec::new()));
        let mut agent = agent.with_approvals(
            Box::new(SequenceApprover {
                requests: Arc::clone(&requests),
                answers: [
                    ApprovalAnswer::approved_once(),
                    ApprovalAnswer::approved_once(),
                ]
                .into_iter()
                .collect(),
                on_approve: None,
            }),
            ApprovalPolicy {
                approval: Approval::OnRequest,
                sandbox: Sandbox::ReadOnly,
                sandboxed: true,
            },
        );
        let (report, _) = complete(&mut agent)?;
        assert_eq!(report.outcome, Outcome::Completed);
        assert_eq!(report.summary.checks, 4, "{tool}");
        assert_eq!(
            requests.lock().map_err(|_| "requests")?.len(),
            2,
            "{tool}: 2nd call must ask again"
        );
        for out in results(&agent) {
            assert!(
                out.text.contains(expected_ctx_err) && !out.text.contains("script changed"),
                "{tool}: {out:?}"
            );
        }
        server.shutdown()?;
    }

    // パターン 3: 既存の `confirmed_effects`（File.Read）がある状態で File.Read, File.Write を
    // 一回限り許可しても、その 1 回は通過し、次の呼び出しで File.Read の記憶だけが保たれる
    // （消えず、File.Write へも広がらない）。
    {
        let f = Fixture::new()?;
        let script_path = f.project.join("a.bnt");
        fs::write(&script_path, read_script)?;
        let mut server = MockProvider::start(vec![
            // 1 回目: File.Read をセッション記憶で許可
            ResponseScript::sse(calls(
                &[("benitoite_run", json!({"path": "a.bnt"}))],
                "tool_calls",
            )),
            // 2 回目: スクリプトを File.Read, File.Write に変え、一回限りで許可
            ResponseScript::sse(calls(
                &[("benitoite_run", json!({"path": "a.bnt"}))],
                "tool_calls",
            )),
            // 3 回目: File.Read に戻す → 確認なしで通過すること（記憶が消えていない）
            ResponseScript::sse(calls(
                &[("benitoite_run", json!({"path": "a.bnt"}))],
                "tool_calls",
            )),
            // 4 回目: 再び File.Read, File.Write にする → 再確認になること（記憶が広がっていない）
            ResponseScript::sse(calls(
                &[("benitoite_run", json!({"path": "a.bnt"}))],
                "tool_calls",
            )),
            final_text(),
        ])?;
        let mut config = f.config(&server, AdapterKind::OpenAiChat);
        config.mode = Mode::Restricted;
        let step = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let query = support::checks(f.project.clone())?;
        let path_for_query = script_path.clone();
        let step_for_query = Arc::clone(&step);
        let query: Arc<benitoiteh::benitoite::CheckQuery> = Arc::new(move |args, limits, stop| {
            // 検査の呼び出し回数に応じてスクリプトの内容を切り替える:
            // 0,1: 1 回目(File.Read) / 2,3: 2 回目(File.Read+File.Write)
            // 4: 3 回目(File.Read; 確認なしなので検査 1 回) / 5: 4 回目(File.Read+File.Write; 拒否するので検査 1 回)
            let count = step_for_query.fetch_add(1, Ordering::AcqRel);
            if count == 2 {
                fs::write(&path_for_query, read_write_script)?;
            } else if count == 4 {
                fs::write(&path_for_query, read_script)?;
            } else if count == 5 {
                fs::write(&path_for_query, read_write_script)?;
            }
            query(args, limits, stop)
        });
        let (agent, _) = f.agent_with_check(config, None, Some(query))?;
        let requests = Arc::new(Mutex::new(Vec::new()));
        let mut agent = agent.with_approvals(
            Box::new(SequenceApprover {
                requests: Arc::clone(&requests),
                answers: [
                    ApprovalAnswer::new(ApprovalOutcome::Approved, None),
                    ApprovalAnswer::approved_once(),
                    ApprovalAnswer::new(ApprovalOutcome::Denied, None),
                ]
                .into_iter()
                .collect(),
                on_approve: None,
            }),
            ApprovalPolicy {
                approval: Approval::OnRequest,
                sandbox: Sandbox::ReadOnly,
                sandboxed: true,
            },
        );
        let (report, _) = complete(&mut agent)?;
        assert_eq!(report.outcome, Outcome::Completed);
        let asked = requests.lock().map_err(|_| "requests")?;
        assert_eq!(asked.len(), 3, "expected prompts on calls 1, 2, and 4 only");
        assert_eq!(
            asked.first().map(|r| r.facts.effects.clone()),
            Some(vec!["File.Read".into()])
        );
        assert_eq!(
            asked.get(1).map(|r| r.facts.added_effects.clone()),
            Some(vec!["File.Write".into()])
        );
        assert_eq!(
            asked.get(2).map(|r| r.facts.added_effects.clone()),
            Some(vec!["File.Write".into()])
        );
        drop(asked);
        let outs = results(&agent);
        assert_eq!(outs.len(), 4);
        for index in 0..3 {
            let out = outs.get(index).ok_or("missing result")?;
            assert!(
                out.text.contains("script execution context is unavailable")
                    && !out.text.contains("script changed"),
                "call {index}: {out:?}"
            );
        }
        assert!(
            outs.get(3)
                .is_some_and(|out| out.is_error && out.text.contains("declined"))
        );
        server.shutdown()?;
    }

    // パターン 4: 確認直後・再検査前にエフェクトが増えたときは、一回限りの許可でも `EFFECTS_CHANGED` で拒む。
    for (tool, args, initial, expanded) in [
        (
            "benitoite_run",
            json!({"path": "a.bnt"}),
            read_script,
            read_write_script,
        ),
        (
            "benitoite_test",
            json!({"paths": ["a.bnt"]}),
            read_test,
            read_write_test,
        ),
    ] {
        let f = Fixture::new()?;
        let script_path = f.project.join("a.bnt");
        fs::write(&script_path, initial)?;
        let mut server = MockProvider::start(vec![
            ResponseScript::sse(calls(&[(tool, args)], "tool_calls")),
            final_text(),
        ])?;
        let mut config = f.config(&server, AdapterKind::OpenAiChat);
        config.mode = Mode::Restricted;
        let (agent, _) =
            f.agent_with_check(config, None, Some(support::checks(f.project.clone())?))?;
        let requests = Arc::new(Mutex::new(Vec::new()));
        let path_for_hook = script_path.clone();
        let expanded_owned = expanded.to_owned();
        let mut agent = agent.with_approvals(
            Box::new(SequenceApprover {
                requests: Arc::clone(&requests),
                answers: [ApprovalAnswer::approved_once()].into_iter().collect(),
                on_approve: Some(Box::new(move |_| {
                    let _written = fs::write(&path_for_hook, &expanded_owned);
                })),
            }),
            ApprovalPolicy {
                approval: Approval::OnRequest,
                sandbox: Sandbox::ReadOnly,
                sandboxed: true,
            },
        );
        let (report, _) = complete(&mut agent)?;
        assert_eq!(report.outcome, Outcome::Completed);
        assert_eq!(report.summary.checks, 2, "{tool}");
        let outs = results(&agent);
        let out = outs.first().ok_or("missing result")?;
        assert!(
            out.is_error
                && out
                    .text
                    .contains("The script changed after the effects were confirmed"),
            "{tool}: {out:?}"
        );
        server.shutdown()?;
    }
    Ok(())
}

// 作成時の関門: `--allow File.Read` を持つ制限モードで、`PreapprovalOverflow::Ask`（ACP 既定の
// `on-request`）と `PreapprovalOverflow::Deny`（`exec` / `acp --approval never`）の分岐を守る。
// 既存の `allow.rs` のテストは `Deny` しか通らず、範囲超過時の確認フォールバックを捕まえない。
#[test]
fn preallowed_effects_with_on_request_ask_on_overflow_and_deny_with_never() -> TestResult {
    use benitoiteh::agent::PreapprovalOverflow;
    use benitoiteh::benitoite::Effect;

    let read_script = "import Benitoite.Unofficial.IO.File\nfunction main() -> Unit uses File.Read\n bind _ <- File.readText(\"input\")\n return ()\nend function\n";
    for ask_on_overflow in [true, false] {
        let f = Fixture::new()?;
        fs::write(f.project.join("read.bnt"), read_script)?;
        fs::write(f.project.join("write.bnt"), WRITE)?;
        let mut server = MockProvider::start(vec![
            ResponseScript::sse(calls(
                &[
                    ("benitoite_run", json!({"path": "read.bnt"})),
                    ("benitoite_run", json!({"path": "write.bnt"})),
                    (
                        "benitoite_run",
                        json!({"path": "write.bnt", "write_agents": true}),
                    ),
                ],
                "tool_calls",
            )),
            final_text(),
        ])?;
        let mut config = f.config(&server, AdapterKind::OpenAiChat);
        config.mode = Mode::Restricted;
        config.allow_effects = Some([Effect::FileRead].into_iter().collect());
        config.preapproval_overflow = if ask_on_overflow {
            PreapprovalOverflow::Ask
        } else {
            PreapprovalOverflow::Deny
        };
        let (agent, _) =
            f.agent_with_check(config, None, Some(support::checks(f.project.clone())?))?;
        let requests = Arc::new(Mutex::new(Vec::new()));
        let mut agent = agent.with_approvals(
            Box::new(ScriptedApprover {
                requests: Arc::clone(&requests),
                answer: ApprovalAnswer::approved_once(),
            }),
            ApprovalPolicy {
                approval: if ask_on_overflow {
                    Approval::OnRequest
                } else {
                    Approval::Never
                },
                sandbox: Sandbox::ReadOnly,
                sandboxed: true,
            },
        );
        let (report, _) = complete(&mut agent)?;
        assert_eq!(report.outcome, Outcome::Completed);
        let outs = results(&agent);
        assert_eq!(outs.len(), 3);
        // 1 つ目（File.Read）はどちらの方針でも確認なしで再検査まで通過する。
        assert!(
            outs.first()
                .is_some_and(|o| o.text.contains("script execution context is unavailable")),
            "{outs:?}"
        );
        if ask_on_overflow {
            // 2 つ目（File.Write）と 3 つ目（write_agents）は Approver へ尋ね、一回限り許可で通過する。
            let asked = requests.lock().map_err(|_| "requests")?;
            assert_eq!(asked.len(), 2, "expected 2 prompts on overflow");
            assert_eq!(
                asked.first().map(|r| r.facts.effects.clone()),
                Some(vec!["File.Write".into()])
            );
            assert!(asked.get(1).is_some_and(|r| r.facts.agents_write));
            assert_eq!(report.summary.approvals.policy_denied, 0);
            for index in [1, 2] {
                let out = outs.get(index).ok_or("missing output")?;
                assert!(
                    out.text.contains("script execution context is unavailable")
                        && !out.text.contains("script changed"),
                    "index {index}: {out:?}"
                );
            }
        } else {
            // Never のときは Approver を一切呼ばずに allow_denied / AGENTS_WRITE_DENIED で拒む。
            assert!(requests.lock().map_err(|_| "requests")?.is_empty());
            assert_eq!(report.summary.approvals.policy_denied, 2);
            assert!(outs.get(1).is_some_and(
                |o| o.is_error && o.text.contains("allows only these effects: File.Read.")
            ));
            assert!(
                outs.get(2)
                    .is_some_and(|o| o.is_error && o.text.contains("--allow-agents-write"))
            );
        }
        server.shutdown()?;
    }
    Ok(())
}
