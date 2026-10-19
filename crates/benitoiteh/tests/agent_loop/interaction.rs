//! 操縦と二段階の取り消しを模擬の提供者と本物の道具で確かめる（実装プラン P4-07）。

use super::*;
use benitoiteh::agent::RunControl;
use benitoiteh::provider::{FinishReason, UserPartKind};

fn controlled(
    agent: &mut Agent,
    control: RunControl,
    mut observe: impl FnMut(&AgentEvent),
) -> Result<(RunReport, Vec<AgentEvent>), Box<dyn Error>> {
    let (sender, receiver) = mpsc::sync_channel(0);
    let step = Arc::clone(&control.cancel_step);
    let run = Arc::clone(&control.cancel_run);
    thread::scope(|scope| {
        let worker = scope.spawn(move || agent.run_controlled("Request".into(), control, sender));
        let mut events = Vec::new();
        loop {
            match receiver.recv_timeout(Duration::from_secs(8)) {
                Ok(event) => {
                    observe(&event);
                    events.push(event);
                }
                Err(RecvTimeoutError::Disconnected) => break,
                Err(RecvTimeoutError::Timeout) => {
                    step.store(true, Ordering::Release);
                    run.store(true, Ordering::Release);
                    drop(receiver);
                    let _joined = worker.join();
                    return Err("agent timed out".into());
                }
            }
        }
        Ok((worker.join().map_err(|_| "agent panicked")??, events))
    })
}

// 関門: 操縦を道具の結果より前や同じ Anthropic メッセージへ混ぜる退行を HTTP 本文で検出する。
#[test]
fn steering_is_grouped_recorded_and_sent_after_results_in_all_three_adapters() -> TestResult {
    for kind in [
        AdapterKind::OpenAiChat,
        AdapterKind::AnthropicMessages,
        AdapterKind::OpenAiResponses,
    ] {
        let f = Fixture::new()?;
        let mut server = MockProvider::start(vec![
            compaction::response(kind, "Reading", Some(("read", json!({"path":"a.bnt"}))), 10),
            compaction::response(kind, "Done", None, 10),
        ])?;
        let (mut agent, directory) = f.agent(f.config(&server, kind))?;
        let (steering, receiver) = mpsc::channel();
        let control = RunControl {
            cancel_step: Arc::new(AtomicBool::new(false)),
            cancel_run: Arc::new(AtomicBool::new(false)),
            steering: receiver,
        };
        let mut sent = false;
        let (report, events) = controlled(&mut agent, control, |event| {
            if !sent && matches!(event, AgentEvent::ToolCall { .. }) {
                sent = true;
                assert!(steering.send("change direction".into()).is_ok());
                assert!(steering.send("keep the file".into()).is_ok());
            }
        })?;
        assert_eq!(report.outcome, Outcome::Completed);
        let result_index = events
            .iter()
            .position(|event| matches!(event, AgentEvent::ToolResult { .. }))
            .ok_or("missing result")?;
        let steer_index = events
            .iter()
            .position(|event| matches!(event, AgentEvent::Steered { .. }))
            .ok_or("missing steer")?;
        assert!(result_index < steer_index);
        let records = read_records(&directory)?;
        let steered: Vec<_> = records
            .iter()
            .filter_map(|record| {
                if let Record::User {
                    kind: benitoiteh::session::UserKind::Steer,
                    message,
                    ..
                } = record
                {
                    Some(message)
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(steered.len(), 1);
        assert_eq!(
            steered
                .first()
                .ok_or("missing steer record")?
                .parts
                .iter()
                .map(|part| (part.kind, part.text.as_str()))
                .collect::<Vec<_>>(),
            [
                (UserPartKind::Steering, "change direction"),
                (UserPartKind::Steering, "keep the file")
            ]
        );
        let requests = server.requests()?;
        let body = &requests.get(1).ok_or("missing second request")?.body;
        let messages = body
            .get(if kind == AdapterKind::OpenAiResponses {
                "input"
            } else {
                "messages"
            })
            .and_then(Value::as_array)
            .ok_or("missing messages")?;
        let last = messages.last().ok_or("missing steering")?;
        assert_eq!(last.get("role"), Some(&json!("user")));
        assert!(last.to_string().contains("change direction"));
        assert!(last.to_string().contains("keep the file"));
        let prior = messages.iter().rev().nth(1).ok_or("missing tool result")?;
        match kind {
            AdapterKind::OpenAiChat => assert_eq!(prior.get("role"), Some(&json!("tool"))),
            AdapterKind::AnthropicMessages => assert_eq!(
                prior.pointer("/content/0/type"),
                Some(&json!("tool_result"))
            ),
            AdapterKind::OpenAiResponses => {
                assert_eq!(prior.get("type"), Some(&json!("function_call_output")))
            }
        }
        assert_eq!(replay(&directory)?, *agent.conversation());
        assert_eq!(
            agent
                .branch_targets()?
                .iter()
                .filter(|info| matches!(
                    info.target,
                    benitoiteh::agent::BranchTarget::BeforeRequest(_)
                ))
                .count(),
            1
        );
        server.shutdown()?;
    }
    Ok(())
}

#[test]
fn steering_before_final_answer_continues_and_unapplied_steering_is_returned_on_cancel()
-> TestResult {
    for cancel in [false, true] {
        let f = Fixture::new()?;
        let mut server = MockProvider::start(vec![final_text(), final_text()])?;
        let (mut agent, _) = f.agent(f.config(&server, AdapterKind::OpenAiChat))?;
        let (sender, receiver) = mpsc::channel();
        let step = Arc::new(AtomicBool::new(false));
        let run = Arc::new(AtomicBool::new(false));
        let control = RunControl {
            cancel_step: Arc::clone(&step),
            cancel_run: Arc::clone(&run),
            steering: receiver,
        };
        let mut sent = false;
        let (report, _) = controlled(&mut agent, control, |event| {
            if !sent && matches!(event, AgentEvent::TextDelta { .. }) {
                sent = true;
                assert!(sender.send("one more thing".into()).is_ok());
                if cancel {
                    run.store(true, Ordering::Release);
                    step.store(true, Ordering::Release);
                }
            }
        })?;
        assert_eq!(
            report.outcome,
            if cancel {
                Outcome::Cancelled
            } else {
                Outcome::Completed
            }
        );
        assert_eq!(server.requests()?.len(), if cancel { 1 } else { 2 });
        assert_eq!(
            report.remaining_steering,
            if cancel {
                vec!["one more thing".to_string()]
            } else {
                vec![]
            }
        );
        assert_eq!(run.load(Ordering::Acquire), cancel);
        server.shutdown()?;
    }
    Ok(())
}

// 関門: 一度の ESC が次の番まで止める退行、全体の印を消す退行、子が止まらない退行を守る。
#[test]
fn step_cancel_stops_running_bash_cancels_remaining_calls_and_keeps_run_alive() -> TestResult {
    let f = Fixture::new()?;
    let marker = f.project.join("started");
    let command = format!("printf started > '{}'; exec sleep 30", marker.display());
    let mut server = MockProvider::start(vec![
        ResponseScript::sse(calls(
            &[
                ("bash", json!({"command":command})),
                ("write", json!({"path":"not-written", "content":"bad"})),
            ],
            "tool_calls",
        )),
        final_text(),
    ])?;
    let (mut agent, _) = f.agent_with_shell(
        f.config(&server, AdapterKind::OpenAiChat),
        Some((Sandbox::DangerFullAccess, false)),
    )?;
    let step = Arc::new(AtomicBool::new(false));
    let run = Arc::new(AtomicBool::new(false));
    let control = RunControl::without_steering(Arc::clone(&step), Arc::clone(&run));
    let (report, events) = thread::scope(|scope| {
        let stopper = scope.spawn(|| -> Result<(), String> {
            let start = Instant::now();
            while !marker.exists() {
                if start.elapsed() > Duration::from_secs(4) {
                    return Err("bash did not start".into());
                }
                thread::sleep(Duration::from_millis(2));
            }
            step.store(true, Ordering::Release);
            Ok(())
        });
        let result = controlled(&mut agent, control, |_| {});
        stopper
            .join()
            .map_err(|_| "stopper panicked")?
            .map_err(io::Error::other)?;
        result
    })?;
    assert_eq!(report.outcome, Outcome::Completed);
    assert!(!step.load(Ordering::Acquire));
    assert!(!run.load(Ordering::Acquire));
    assert!(!f.project.join("not-written").exists());
    assert_eq!(
        results(&agent)
            .last()
            .ok_or("missing cancelled result")?
            .text,
        "Tool call was cancelled by the user."
    );
    assert!(events.iter().any(|event| matches!(
        event,
        AgentEvent::TurnEnd {
            turn: 1,
            finish_reason: FinishReason::ToolCalls,
            ..
        }
    )));
    assert_eq!(server.requests()?.len(), 2);
    server.shutdown()?;
    Ok(())
}

#[test]
fn single_escape_during_response_stops_run_and_run_flag_is_never_reset() -> TestResult {
    let f = Fixture::new()?;
    let mut script = final_text();
    script.delivery = Delivery::Chunks {
        size: 95,
        delay: Duration::from_millis(10),
    };
    let mut server = MockProvider::start(vec![script])?;
    let (mut agent, _) = f.agent(f.config(&server, AdapterKind::OpenAiChat))?;
    let step = Arc::new(AtomicBool::new(false));
    let run = Arc::new(AtomicBool::new(false));
    let (report, _) = controlled(
        &mut agent,
        RunControl::without_steering(Arc::clone(&step), Arc::clone(&run)),
        |event| {
            if matches!(event, AgentEvent::TextDelta { .. }) {
                step.store(true, Ordering::Release);
            }
        },
    )?;
    assert_eq!(report.outcome, Outcome::Cancelled);
    assert!(!run.load(Ordering::Acquire));
    assert!(
        !agent
            .conversation()
            .items
            .iter()
            .any(|item| matches!(item, Item::AssistantTurn(_)))
    );
    server.shutdown()?;
    Ok(())
}

// 関門: 確認の ESC を段階の取り消しとして扱い、同じ応答の残りまで止める退行を守る。
#[test]
fn escape_declines_one_approval_and_remaining_calls_in_the_response_still_execute() -> TestResult {
    use benitoiteh::approval::InteractiveApprover;
    use benitoiteh::ui::tui::{Action, Screen, Status};
    use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
    let f = Fixture::new()?;
    let mut server = MockProvider::start(vec![
        ResponseScript::sse(calls(
            &[
                (
                    "write",
                    json!({"path":".benitoiteh/declined", "content":"bad"}),
                ),
                ("write", json!({"path":"allowed", "content":"good"})),
            ],
            "tool_calls",
        )),
        final_text(),
    ])?;
    let (agent, _) = f.agent_with_shell(
        f.config(&server, AdapterKind::OpenAiChat),
        Some((Sandbox::WorkspaceWrite, false)),
    )?;
    let (approver, prompts) = InteractiveApprover::channel();
    let mut agent = agent.with_approvals(
        Box::new(approver),
        ApprovalPolicy {
            approval: Approval::OnRequest,
            sandbox: Sandbox::WorkspaceWrite,
            sandboxed: false,
        },
    );
    let step = Arc::new(AtomicBool::new(false));
    let run = Arc::new(AtomicBool::new(false));
    let mut screen = Screen::new(
        (120, 24),
        Status {
            mode: Mode::Normal,
            provider: "mock".into(),
            model: "mock".into(),
            host: "localhost".into(),
            sandbox: "workspace-write".into(),
            sandbox_disabled: true,
            context_window: None,
            estimate: 0,
        },
    );
    let (report, _) = controlled(
        &mut agent,
        RunControl::without_steering(Arc::clone(&step), Arc::clone(&run)),
        |event| {
            screen.event(event);
            if matches!(event, AgentEvent::ApprovalRequest { .. }) {
                let prompt = prompts.recv_timeout(Duration::from_secs(2));
                assert!(prompt.is_ok());
                if let Ok(prompt) = prompt {
                    screen.open_approval(prompt);
                    assert!(screen.draw(&mut Vec::new(), None).is_ok());
                    let shown = Instant::now();
                    screen.presented(shown);
                    let action = screen.input_at(
                        Event::Key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)),
                        shown + Duration::from_millis(300),
                        Duration::from_millis(300),
                        true,
                    );
                    assert_eq!(action, Action::None);
                }
            }
        },
    )?;
    assert_eq!(report.outcome, Outcome::Completed);
    assert!(!step.load(Ordering::Acquire));
    assert!(!run.load(Ordering::Acquire));
    assert!(!f.project.join(".benitoiteh/declined").exists());
    assert_eq!(fs::read_to_string(f.project.join("allowed"))?, "good");
    assert_eq!(report.summary.approvals.denied, 1);
    server.shutdown()?;
    Ok(())
}
