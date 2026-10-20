//! 完了の促しを一度だけ加え、上限と操縦・圧縮を通常どおり通す契約（設計書 01-01）。
use super::*;
use benitoiteh::agent::{CompactOptions, CompactOutcome, CompactionReason, RunControl};
use benitoiteh::session::UserKind;

const EXPECTED: &str = include_str!("../expected/completion-check.txt");

fn prompts(agent: &Agent) -> usize {
    agent
        .conversation()
        .items
        .iter()
        .filter_map(|item| match item {
            Item::UserMessage(message) => Some(&message.parts),
            Item::AssistantTurn(_) | Item::ToolResult(_) => None,
        })
        .flatten()
        .filter(|part| part.text.starts_with("<completion_check>\n"))
        .count()
}

// 関門: 設定の対照、固定の送信文、道具の実行、印の寿命はループの持ち物である。
// query は既存の検査テストと同じものを使い、促しの後も本物の道具を通す。
#[test]
fn completion_checks_and_edits_once_per_request_in_both_modes() -> TestResult {
    for mode in [Mode::Normal, Mode::Restricted] {
        for enabled in [false, true] {
            let f = Fixture::new()?;
            let mut scripts = vec![final_text()];
            if enabled {
                scripts.extend([
                    ResponseScript::sse(calls(&[("benitoite_check", json!({"path":"a.bnt"}))], "tool_calls")),
                    ResponseScript::sse(calls(&[
                        ("write", json!({"path":"new.bnt","content":"before\n"})),
                        ("edit", json!({"path":"new.bnt","old_string":"before","new_string":"after"})),
                    ], "tool_calls")),
                    final_text(),
                ]);
            }
            scripts.push(final_text());
            if enabled {
                scripts.push(final_text());
            }
            let mut server = MockProvider::start(scripts)?;
            let mut config = f.config(&server, AdapterKind::OpenAiChat);
            config.mode = mode;
            config.agent.completion_check = enabled;
            let (mut agent, directory) = f.agent_with_check(
                config,
                None,
                Some(checks::query(vec![checks::Response::Passed; 3])),
            )?;
            let (report, _) = complete(&mut agent)?;
            assert_eq!(report.outcome, Outcome::Completed);
            assert_eq!(report.summary.turns, if enabled { 4 } else { 1 });
            assert_eq!(prompts(&agent), usize::from(enabled));
            if enabled {
                assert_eq!(report.summary.tool_calls.get("benitoite_check"), Some(&1));
                assert_eq!(fs::read_to_string(f.project.join("new.bnt"))?, "after\n");
                let requests = server.requests()?;
                let text = requests
                    .get(1)
                    .ok_or("second request")?
                    .body
                    .get("messages")
                    .and_then(Value::as_array)
                    .and_then(|messages| messages.last())
                    .and_then(|message| message.pointer("/content/0/text"))
                    .and_then(Value::as_str);
                assert_eq!(text, Some(EXPECTED.trim_end()));
            }
            let (next, _) = complete(&mut agent)?;
            assert_eq!(next.outcome, Outcome::Completed);
            assert_eq!(next.summary.turns, if enabled { 2 } else { 1 });
            assert_eq!(prompts(&agent), if enabled { 2 } else { 0 });
            let records = read_records(&directory)?;
            assert_eq!(
                records
                    .iter()
                    .filter(|record| matches!(
                        record,
                        Record::User {
                            kind: UserKind::Environment,
                            ..
                        }
                    ))
                    .count(),
                if enabled { 2 } else { 0 }
            );
            assert_eq!(replay(&directory)?, *agent.conversation());
            server.shutdown()?;
        }
    }
    Ok(())
}

// 関門: calls が空なだけで促す退行、正常な最終番を失敗へ変える退行を表で捕まえる。
#[test]
fn completion_is_not_prompted_on_limits_refusal_or_cancellation() -> TestResult {
    for (reason, turns, tokens, cancel, expected, count) in [
        ("stop", 1, None, false, Outcome::Completed, 1),
        ("length", 5, None, false, Outcome::Failed, 2),
        ("tool_calls", 5, None, false, Outcome::Completed, 1),
        ("stop", 5, Some(12), false, Outcome::Failed, 1),
        ("stop", 0, None, false, Outcome::Failed, 0),
        ("stop", 5, Some(0), false, Outcome::Failed, 0),
        ("content_filter", 5, None, false, Outcome::Failed, 1),
        ("stop", 5, None, true, Outcome::Cancelled, 1),
    ] {
        let f = Fixture::new()?;
        let mut scripts = vec![ResponseScript::sse(chat(
            &json!({"content":"text"}),
            reason,
            true,
        ))];
        if reason == "length" {
            scripts.push(ResponseScript::sse(chat(
                &json!({}),
                "content_filter",
                true,
            )));
        }
        let mut server = MockProvider::start(scripts)?;
        let mut config = f.config(&server, AdapterKind::OpenAiChat);
        config.agent.completion_check = true;
        config.limits.max_turns = turns;
        config.limits.max_run_tokens = tokens;
        let (mut agent, _) = f.agent(config)?;
        let cancelled = Arc::new(AtomicBool::new(false));
        let set = Arc::clone(&cancelled);
        let (report, _) = run(&mut agent, cancelled, |event| {
            if cancel && matches!(event, AgentEvent::TextDelta { .. }) {
                set.store(true, Ordering::Release);
            }
        })?;
        assert_eq!(
            report.outcome, expected,
            "{reason} {turns} {tokens:?} {cancel}"
        );
        assert_eq!(prompts(&agent), 0);
        assert_eq!(server.requests()?.len(), count);
        server.shutdown()?;
    }
    Ok(())
}

// 関門: 操縦の前に促す、操縦で印を戻す退行は受け取った HTTP の順序で検出する。
#[test]
fn steering_precedes_completion_and_does_not_reset_the_prompt() -> TestResult {
    let f = Fixture::new()?;
    let mut server =
        MockProvider::start(vec![final_text(), final_text(), final_text(), final_text()])?;
    let mut config = f.config(&server, AdapterKind::OpenAiChat);
    config.agent.completion_check = true;
    let (mut agent, _) = f.agent(config)?;
    let (send, steering) = mpsc::channel();
    let control = RunControl {
        cancel_step: Arc::new(AtomicBool::new(false)),
        cancel_run: Arc::new(AtomicBool::new(false)),
        steering,
    };
    let (report, _) = interaction::controlled(&mut agent, control, |event| {
        if matches!(event, AgentEvent::TextDelta { turn: 1 | 3, .. }) {
            assert!(send.send("steer".into()).is_ok());
        }
    })?;
    assert_eq!(report.outcome, Outcome::Completed);
    assert_eq!(report.summary.turns, 4);
    assert_eq!(prompts(&agent), 1);
    let requests = server.requests()?;
    for (index, expected) in [(1, "steer"), (2, EXPECTED.trim_end()), (3, "steer")] {
        let last = requests
            .get(index)
            .ok_or("request")?
            .body
            .get("messages")
            .and_then(Value::as_array)
            .and_then(|messages| messages.last())
            .ok_or("last")?;
        assert_eq!(
            last.pointer("/content/0/text").and_then(Value::as_str),
            Some(expected)
        );
    }
    server.shutdown()?;
    Ok(())
}

// 関門: 閾値の圧縮で印が戻る退行と、残した促しが後の圧縮で環境として固定される退行。
// 別要求の要約では環境の部分を送らなくてよい（P5-29 補足）。
#[test]
fn compaction_keeps_the_once_flag_and_does_not_promote_old_prompts() -> TestResult {
    let kind = AdapterKind::OpenAiChat;
    let f = Fixture::new()?;
    let mut server = MockProvider::start(vec![
        compaction::response(kind, &"old ".repeat(24_000), None, 50_000),
        compaction::response(kind, "summary one", None, 10),
        compaction::response(kind, "verified", None, 10),
        compaction::response(kind, "summary two", None, 10),
        final_text(),
        final_text(),
    ])?;
    let mut config = f.config(&server, kind);
    config.agent.completion_check = true;
    config.capabilities.context_window = Some(60_000);
    let (mut agent, directory) = f.agent(config)?;
    let (report, _) = complete(&mut agent)?;
    assert_eq!(report.outcome, Outcome::Completed);
    assert_eq!((report.summary.turns, report.summary.compactions), (2, 1));
    assert_eq!(prompts(&agent), 1);
    let (send, receive) = mpsc::sync_channel(64);
    let compact = agent.compact_with_options(
        CompactOptions {
            keep_recent: false,
            separate_request: true,
            ..CompactOptions::for_reason(CompactionReason::Manual)
        },
        Arc::new(AtomicBool::new(false)),
        send,
    )?;
    drop(receive);
    assert_eq!(compact.outcome, CompactOutcome::Compacted);
    assert_eq!(prompts(&agent), 0);
    let (next, _) = complete(&mut agent)?;
    assert_eq!(next.outcome, Outcome::Completed);
    assert_eq!(prompts(&agent), 1);
    assert_eq!(replay(&directory)?, *agent.conversation());
    let requests = server.requests()?;
    let separate = requests.get(3).ok_or("separate summary")?.body.to_string();
    assert!(!separate.contains("Before finishing, verify"));
    server.shutdown()?;
    Ok(())
}
