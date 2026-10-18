//! コマンドは画面の入力からだけ働き、確認・記録・セッションに接続する（実装プラン P4-08）。

use super::*;
#[path = "../support/rewind.rs"]
#[allow(dead_code)]
mod rewind_fixture;
use benitoiteh::ui::tui::{Action, Command, NewSession};
use std::cell::RefCell;
use std::collections::VecDeque;
use std::io::Write;
use std::rc::Rc;

fn key(code: KeyCode, modifiers: KeyModifiers) -> Event {
    Event::Key(KeyEvent::new(code, modifiers))
}
fn send(text: &str) -> Vec<Event> {
    vec![
        Event::Paste(text.into()),
        key(KeyCode::Enter, KeyModifiers::NONE),
    ]
}
fn submit(screen: &mut Screen, text: &str, running: bool, alt: bool) -> Action {
    screen.input(Event::Paste(text.into()), Duration::ZERO, running);
    screen.input(
        key(
            KeyCode::Enter,
            if alt {
                KeyModifiers::ALT
            } else {
                KeyModifiers::NONE
            },
        ),
        Duration::ZERO,
        running,
    )
}

// 関門: OS のパスとコマンドの混同、空白の除去、操縦・後続の異なる解釈を、入力の口で守る。
#[test]
fn command_grammar_busy_rules_and_literal_slashes_apply_only_at_submission() -> TestResult {
    for (raw, expected) in [
        ("/help", Action::Command(Command::Help)),
        ("/status \n\t", Action::Command(Command::Status)),
        ("/rewind", Action::Command(Command::Rewind)),
        ("/new", Action::Command(Command::New)),
        ("/quit", Action::Quit),
        (
            "/compact keep names\nand paths",
            Action::Command(Command::Compact(Some("keep names\nand paths".into()))),
        ),
        ("/Help", Action::Submit("/Help".into())),
        ("/usr/bin/env", Action::Submit("/usr/bin/env".into())),
        (" /quit", Action::Submit(" /quit".into())),
        ("//new", Action::Submit("/new".into())),
        ("/", Action::None),
        ("/unknown argument", Action::None),
    ] {
        let mut screen = Screen::new((120, 24), status());
        assert_eq!(submit(&mut screen, raw, false, false), expected, "{raw:?}");
    }
    for command in ["/help", "/status", "/rewind", "/new", "/quit"] {
        let mut screen = Screen::new((120, 24), status());
        assert_eq!(
            submit(&mut screen, &format!("{command} unwanted"), false, false),
            Action::None
        );
        let mut bytes = Vec::new();
        screen.draw(&mut bytes, None)?;
        assert!(safe_output(&bytes)?.contains("does not accept arguments"));
    }
    for raw in [
        "/help",
        "/status",
        "/compact",
        "/rewind",
        "/new",
        "/quit",
        "/usr/bin/env",
    ] {
        let mut screen = Screen::new((120, 24), status());
        assert_eq!(submit(&mut screen, raw, true, true), Action::None, "{raw}");
    }
    for raw in ["/help", "/compact", "/new", "/rewind", "/unknown", "/"] {
        let mut screen = Screen::new((120, 24), status());
        assert_eq!(submit(&mut screen, raw, true, false), Action::None);
    }
    let mut screen = Screen::new((120, 24), status());
    assert_eq!(submit(&mut screen, "/quit", true, false), Action::Quit);
    for (running, alt, expected) in [
        (false, false, Action::Submit("/new".into())),
        (true, false, Action::Steer("/new".into())),
        (true, true, Action::FollowUp("/new".into())),
    ] {
        let mut screen = Screen::new((120, 24), status());
        assert_eq!(submit(&mut screen, "//new", running, alt), expected);
        screen.input(key(KeyCode::Up, KeyModifiers::NONE), Duration::ZERO, false);
        assert_eq!(
            screen.input(
                key(KeyCode::Enter, KeyModifiers::NONE),
                Duration::ZERO,
                false
            ),
            Action::Submit("/new".into())
        );
    }
    Ok(())
}

#[derive(Clone, Default)]
struct Output(Rc<RefCell<Vec<u8>>>);
impl Write for Output {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0.borrow_mut().extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
struct Driver {
    output: Output,
    steps: VecDeque<(String, Vec<Event>)>,
    consumed: usize,
    started: Instant,
}
impl Driver {
    fn new(output: Output, steps: Vec<(&str, Vec<Event>)>) -> Self {
        Self {
            output,
            steps: steps
                .into_iter()
                .map(|(marker, events)| (marker.into(), events))
                .collect(),
            consumed: 0,
            started: Instant::now(),
        }
    }
}
impl InputSource for Driver {
    fn poll(&mut self, _: Duration) -> io::Result<Vec<Event>> {
        if self.started.elapsed() > Duration::from_secs(8) {
            return Err(io::Error::other(format!(
                "UI deadline: {:?}\n{}",
                self.steps.front(),
                String::from_utf8_lossy(&self.output.0.borrow())
            )));
        }
        thread::sleep(Duration::from_millis(1));
        let visible = safe_output(&self.output.0.borrow())
            .map_err(|error| io::Error::other(error.to_string()))?;
        if let Some((_, events)) = self.steps.front()
            && events.iter().any(|event| matches!(event, Event::Paste(value) if value.starts_with('/') && value != "/quit")) {
            let bytes=self.output.0.borrow();
            let raw=String::from_utf8_lossy(&bytes);
            let frame=raw.rfind("\x1b[?2026h").and_then(|index|raw.get(index..)).unwrap_or_default();
            if Regex::new(r" \| [0-9]+s \|").map_err(io::Error::other)?.is_match(frame) { return Ok(Vec::new()); }
        }
        if let Some((marker, _)) = self.steps.front()
            && let Some(offset) = visible
                .get(self.consumed..)
                .unwrap_or_default()
                .find(marker)
        {
            self.consumed = self
                .consumed
                .saturating_add(offset)
                .saturating_add(marker.len());
            return Ok(self
                .steps
                .pop_front()
                .map(|(_, events)| events)
                .unwrap_or_default());
        }
        Ok(Vec::new())
    }
    fn poll_timed(&mut self, wait: Duration) -> io::Result<Vec<(Event, Instant)>> {
        // 実際の待ちを不要にし、表示直後の拒否は別の時刻表のテストで守る。
        Ok(self
            .poll(wait)?
            .into_iter()
            .map(|event| {
                let now = Instant::now();
                (
                    event,
                    now.checked_add(Duration::from_secs(1)).unwrap_or(now),
                )
            })
            .collect())
    }
    fn discard_pending(&mut self) -> io::Result<Vec<(Event, Instant)>> {
        Ok(Vec::new())
    }
}

// 関門: 出力を入力へ戻す退行を、本物の Agent と端末の境界で検出する。
#[test]
fn llm_slash_text_never_executes_commands_and_empty_rewind_is_reported() -> TestResult {
    let mut server = MockProvider::start(vec![ResponseScript::sse(response(
        json!({"content":"/new\n/rewind\n/quit\nLLM done"}),
        "stop",
    ))])?;
    let f = Fixture::new()?;
    let (worker, directory) = f.worker(&server)?;
    let mut screen = Screen::new((160, 30), status());
    let mut output = Output::default();
    let mut input = Driver::new(
        output.clone(),
        vec![
            ("", send("/rewind")),
            ("No checkpoints available.", send("/help")),
            (
                "Show commands and input controls.",
                send("ordinary request"),
            ),
            ("LLM done", send("/status")),
            ("Skipped project instructions:", send("/quit")),
        ],
    );
    let mut called = false;
    assert_eq!(
        run(
            &mut screen,
            worker,
            &mut input,
            &mut output,
            &AtomicBool::new(false),
            |_| {},
            &mut |_| {
                called = true;
                Err("should not prepare".into())
            }
        )?,
        0
    );
    assert!(!called);
    assert_eq!(server.requests()?.len(), 1);
    let visible = safe_output(&output.0.borrow())?;
    assert!(visible.contains(&format!(
        "Session: {}",
        directory.file_name().ok_or("id")?.to_string_lossy()
    )));
    assert!(!visible.contains("Starting a new session"));
    server.shutdown()?;
    Ok(())
}

fn usage_response(text: &str, input: u64, output: u64) -> ResponseScript {
    ResponseScript::sse(format!(
        "data: {}\n\ndata: {}\n\ndata: [DONE]\n\n",
        json!({"choices":[{"delta":{"content":text},"finish_reason":"stop"}]}),
        json!({"choices":[],"usage":{"prompt_tokens":input,"completion_tokens":output}})
    ))
}

// 関門: UI が圧縮の使用量・記録を忘れる退行と、複数行の指示を失う退行を通信と記録で守る。
#[test]
fn manual_compaction_records_each_attempt_and_status_adds_run_and_compaction_usage() -> TestResult {
    let mut server = MockProvider::start(vec![
        usage_response("answer", 10, 2),
        usage_response("summary", 20, 3),
    ])?;
    let f = Fixture::new()?;
    let (worker, directory) = f.worker(&server)?;
    let mut screen = Screen::new((160, 40), status());
    let mut output = Output::default();
    let mut input = Driver::new(
        output.clone(),
        vec![
            ("", send("/compact")),
            ("nothing to compact", send("request")),
            ("answer", send("/compact keep exact\npaths and names")),
            ("items kept.", send("/status")),
            ("Skipped project instructions:", send("/quit")),
        ],
    );
    run(
        &mut screen,
        worker,
        &mut input,
        &mut output,
        &AtomicBool::new(false),
        |_| {},
        &mut |_| Err("unexpected new".into()),
    )?;
    let records = read_records(&directory)?;
    let summaries: Vec<_> = records
        .iter()
        .filter_map(|record| {
            if let Record::Summary { summary } = record {
                Some(summary)
            } else {
                None
            }
        })
        .collect();
    assert_eq!(summaries.len(), 3);
    assert_eq!(
        summaries
            .iter()
            .map(|summary| summary.compactions)
            .sum::<u64>(),
        1
    );
    assert!(
        safe_output(&output.0.borrow())?
            .contains("input 30, cache read 0, cache write 0, output 5")
    );
    let requests = server.requests()?;
    assert_eq!(requests.len(), 2);
    assert!(
        requests
            .last()
            .ok_or("summary request")?
            .body
            .to_string()
            .contains("keep exact\\npaths and names")
    );
    server.shutdown()?;
    Ok(())
}

// 関門: 準備失敗で古い会話を捨てる退行と、終了記録・破棄より先に新しい Agent を動かす退行を守る。
#[test]
fn new_session_prepares_first_ends_and_drops_old_agent_and_preserves_session_on_failure()
-> TestResult {
    let mut server = MockProvider::start(Vec::new())?;
    let f = Fixture::new()?;
    let old = f.new_session(&server)?;
    let old_directory = old.session.directory().to_owned();
    let old_temp = old.session.temporary_dir().to_owned();
    let old_id = old.session.id().to_string();
    let worker = Worker::start(old.agent, old.session)?;
    let mut screen = Screen::new((160, 40), status());
    let mut output = Output::default();
    let mut input = Driver::new(
        output.clone(),
        vec![
            ("", send("/new")),
            ("preparation failed", send("/status")),
            ("Skipped project instructions:", send("/new")),
            ("New session started.", send("/status")),
            ("Skipped project instructions:", send("/quit")),
        ],
    );
    let mut attempts = 0;
    let mut new_id = String::new();
    let mut factory = |current: &benitoiteh::agent::AgentStatus| -> Result<NewSession, String> {
        attempts += 1;
        assert_eq!(current.session_id, old_id);
        assert!(old_temp.exists());
        if attempts == 1 {
            return Err("preparation failed".into());
        }
        let mut new = f.new_session(&server).map_err(|error| error.to_string())?;
        new_id = new.session.id().to_string();
        let directory = old_directory.clone();
        let temporary = old_temp.clone();
        new.diagnostics.push("New session started.".into());
        Ok(new.on_start(move || {
            assert!(!temporary.exists());
            assert!(matches!(read_records(&directory).ok().and_then(|records|records.last().cloned()),Some(Record::SessionEnd{outcome,reason}) if outcome=="completed" && reason=="New session started."));
        }))
    };
    assert_eq!(
        run(
            &mut screen,
            worker,
            &mut input,
            &mut output,
            &AtomicBool::new(false),
            |_| {},
            &mut factory
        )?,
        0
    );
    assert_eq!(attempts, 2);
    assert_ne!(old_id, new_id);
    let visible = safe_output(&output.0.borrow())?;
    assert!(visible.contains(&format!("Session: {old_id}")));
    assert!(visible.contains(&format!("Session: {new_id}")));
    assert!(server.requests()?.is_empty());
    server.shutdown()?;
    Ok(())
}

// 状態の行が出ただけでは HTTP の送信完了は保証されない。途中の接続切断を
// 模擬サーバーの壊れた要求と混同しないよう、要求の受信を待ってから取り消す。
struct CancelAfterRequest<'a> {
    driver: Driver,
    server: &'a MockProvider,
    pending: Vec<Event>,
}
impl InputSource for CancelAfterRequest<'_> {
    fn poll(&mut self, wait: Duration) -> io::Result<Vec<Event>> {
        if !self.pending.is_empty() {
            if self.server.requests()?.len() >= 2 {
                return Ok(std::mem::take(&mut self.pending));
            }
            if self.driver.started.elapsed() > Duration::from_secs(8) {
                return Err(io::ErrorKind::TimedOut.into());
            }
            thread::sleep(Duration::from_millis(1));
            return Ok(Vec::new());
        }
        let events = self.driver.poll(wait)?;
        if events.iter().any(|event| matches!(event, Event::Key(key) if key.code == KeyCode::Esc || key.modifiers.contains(KeyModifiers::CONTROL))) {
            self.pending = events;
            return Ok(Vec::new());
        }
        Ok(events)
    }
    fn discard_pending(&mut self) -> io::Result<Vec<(Event, Instant)>> {
        Ok(Vec::new())
    }
}

// 関門: 一度の ESC を圧縮へ渡し忘れる退行と、失敗時の Summary の欠落を公開の入力で守る。
#[test]
fn manual_compaction_cancels_with_escape_or_interrupt_and_failed_attempts_are_recorded()
-> TestResult {
    for event in [
        key(KeyCode::Esc, KeyModifiers::NONE),
        key(KeyCode::Char('c'), KeyModifiers::CONTROL),
    ] {
        let mut blocked = usage_response("never", 0, 0);
        blocked.delivery = Delivery::HeadersOnly;
        let mut server = MockProvider::start(vec![usage_response("answer", 1, 1), blocked])?;
        let f = Fixture::new()?;
        let (worker, directory) = f.worker(&server)?;
        let mut screen = Screen::new((120, 30), status());
        let mut output = Output::default();
        let input = Driver::new(
            output.clone(),
            vec![
                ("", send("request")),
                ("answer", send("/compact")),
                ("Compacting...", vec![event]),
                ("Compaction cancelled:", send("/quit")),
            ],
        );
        let mut input = CancelAfterRequest {
            driver: input,
            server: &server,
            pending: Vec::new(),
        };
        run(
            &mut screen,
            worker,
            &mut input,
            &mut output,
            &AtomicBool::new(false),
            |_| {},
            &mut |_| Err("unexpected new".into()),
        )?;
        let records = read_records(&directory)?;
        let summaries: Vec<_> = records
            .iter()
            .filter_map(|record| {
                if let Record::Summary { summary } = record {
                    Some(summary)
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(summaries.len(), 2);
        assert_eq!(
            summaries
                .iter()
                .map(|summary| summary.compactions)
                .sum::<u64>(),
            0
        );
        server.shutdown()?;
    }
    let mut server = MockProvider::start(vec![
        usage_response("answer", 1, 1),
        usage_response("", 2, 0),
        usage_response("", 3, 0),
    ])?;
    let f = Fixture::new()?;
    let (worker, directory) = f.worker(&server)?;
    let mut screen = Screen::new((120, 30), status());
    let mut output = Output::default();
    let mut input = Driver::new(
        output.clone(),
        vec![
            ("", send("request")),
            ("answer", send("/compact")),
            ("Compaction failed:", send("/quit")),
        ],
    );
    run(
        &mut screen,
        worker,
        &mut input,
        &mut output,
        &AtomicBool::new(false),
        |_| {},
        &mut |_| Err("unexpected new".into()),
    )?;
    let records = read_records(&directory)?;
    assert_eq!(
        records
            .iter()
            .filter(|record| matches!(record, Record::Summary { .. }))
            .count(),
        2
    );
    assert!(
        !records
            .iter()
            .any(|record| matches!(record,Record::Summary{summary} if summary.compactions>0))
    );
    server.shutdown()?;
    Ok(())
}

// 関門: 選択と確認を表示するだけで実際にはファイル・会話を戻さない接続の退行を守る。
#[test]
fn rewind_restores_files_and_conversation_after_explicit_selection_and_confirmation() -> TestResult
{
    use rewind_fixture as fixture;
    use std::sync::{Arc, mpsc};
    let mut server = MockProvider::start(vec![
        fixture::response(&[("write", json!({"path":"a.txt","content":"before"}))]),
        fixture::response(&[]),
        fixture::response(&[("read", json!({"path":"a.txt"}))]),
        fixture::response(&[(
            "edit",
            json!({"path":"a.txt","old_string":"before","new_string":"after"}),
        )]),
        fixture::response(&[]),
        usage_response("after rewind", 1, 1),
    ])?;
    let f = fixture::Fixture::new()?;
    let mut agent = f.agent(&server)?;
    for request in ["first request", "second \u{3164} request"] {
        let (tx, _rx) = mpsc::sync_channel(1024);
        agent.run(request.into(), Arc::new(AtomicBool::new(false)), tx)?;
    }
    assert_eq!(fs::read_to_string(f.project.join("a.txt"))?, "after");
    let points = agent.checkpoints()?;
    let request_point = points
        .iter()
        .rev()
        .find(|point| point.checkpoint.reason == benitoiteh::rewind::CheckpointReason::Request)
        .ok_or("request checkpoint")?;
    assert_eq!(
        request_point.request.as_deref(),
        Some("second \u{3164} request")
    );
    let worker = Worker::start(agent, f.session.clone())?;
    let mut screen = Screen::new((200, 40), status());
    let mut output = Output::default();
    let mut input = Driver::new(
        output.clone(),
        vec![
            ("", send("/rewind")),
            (
                "Select a checkpoint",
                vec![
                    key(KeyCode::Down, KeyModifiers::NONE),
                    key(KeyCode::Enter, KeyModifiers::NONE),
                ],
            ),
            (
                "Select rewind scope",
                vec![key(KeyCode::Enter, KeyModifiers::NONE)],
            ),
            (
                "[y] rewind",
                vec![key(KeyCode::Char('n'), KeyModifiers::NONE)],
            ),
            ("Rewind cancelled.", send("/rewind")),
            (
                "Select a checkpoint",
                vec![
                    key(KeyCode::Down, KeyModifiers::NONE),
                    key(KeyCode::Enter, KeyModifiers::NONE),
                ],
            ),
            (
                "Select rewind scope",
                vec![key(KeyCode::Enter, KeyModifiers::NONE)],
            ),
            (
                "[y] rewind",
                vec![key(KeyCode::Char('y'), KeyModifiers::NONE)],
            ),
            ("Rewind completed:", send("third request")),
            ("after rewind", send("/quit")),
        ],
    );
    run(
        &mut screen,
        worker,
        &mut input,
        &mut output,
        &AtomicBool::new(false),
        |_| {},
        &mut |_| Err("unexpected new".into()),
    )?;
    assert_eq!(fs::read_to_string(f.project.join("a.txt"))?, "before");
    let visible = safe_output(&output.0.borrow())?;
    assert!(visible.contains("second \\u{3164} request"));
    assert!(visible.contains("1 restored, 0 removed, 0 created"));
    let requests = server.requests()?;
    let last = requests
        .last()
        .ok_or("after rewind request")?
        .body
        .to_string();
    assert!(last.contains("first request"));
    assert!(last.contains("third request"));
    assert!(!last.contains("second \u{3164} request"));
    assert_eq!(
        read_records(f.session.directory())?
            .iter()
            .filter(|record| matches!(record, Record::Rewind { .. }))
            .count(),
        1
    );
    server.shutdown()?;
    Ok(())
}

// 関門: 不完全の理由を通知イベントに依存して失う退行と、y だけで不完全な backup を許す退行を守る。
#[test]
fn incomplete_before_rewind_requires_f_and_reports_the_error_reasons() -> TestResult {
    use rewind_fixture as fixture;
    let mut server = MockProvider::start(vec![
        fixture::response(&[("read", json!({"path":"a.txt"}))]),
        fixture::response(&[]),
    ])?;
    let f = fixture::Fixture::new()?;
    fs::write(f.project.join("a.txt"), "saved")?;
    let mut agent = f.agent(&server)?;
    fixture::run(&mut agent)?;
    fs::write(f.project.join("a.txt"), "changed")?;
    f.session
        .configure_budget(&benitoiteh::session::StorageUsage {
            current_outputs: 1024 * 1024 * 1024,
            ..Default::default()
        });
    let worker = Worker::start(agent, f.session.clone())?;
    let mut screen = Screen::new((200, 50), status());
    let mut output = Output::default();
    let mut input = Driver::new(
        output.clone(),
        vec![
            ("", send("/rewind")),
            (
                "Select a checkpoint",
                vec![key(KeyCode::Enter, KeyModifiers::NONE)],
            ),
            (
                "Select rewind scope",
                vec![key(KeyCode::Enter, KeyModifiers::NONE)],
            ),
            (
                "[y] rewind",
                vec![key(KeyCode::Char('y'), KeyModifiers::NONE)],
            ),
            (
                "[f] rewind anyway",
                vec![
                    key(KeyCode::Char('y'), KeyModifiers::NONE),
                    key(KeyCode::Char('f'), KeyModifiers::NONE),
                ],
            ),
            ("Rewind completed:", send("/quit")),
        ],
    );
    run(
        &mut screen,
        worker,
        &mut input,
        &mut output,
        &AtomicBool::new(false),
        |_| {},
        &mut |_| Err("unexpected new".into()),
    )?;
    assert_eq!(fs::read_to_string(f.project.join("a.txt"))?, "saved");
    let records = read_records(f.session.directory())?;
    let backups: Vec<_> = records
        .iter()
        .filter_map(|record| {
            if let Record::Checkpoint(point) = record {
                (point.reason == benitoiteh::rewind::CheckpointReason::BeforeRewind)
                    .then_some(point)
            } else {
                None
            }
        })
        .collect();
    assert_eq!(backups.len(), 2);
    let visible = safe_output(&output.0.borrow())?;
    assert!(visible.contains("Reasons the backup is incomplete:"));
    assert!(
        backups
            .first()
            .ok_or("backup")?
            .incomplete
            .iter()
            .all(|reason| visible.contains(reason))
    );
    server.shutdown()?;
    Ok(())
}

fn untrusted(mut new: NewSession) -> NewSession {
    new.agent = new.agent.with_approvals(
        Box::new(benitoiteh::approval::NonInteractiveApprover),
        benitoiteh::approval::ApprovalPolicy {
            approval: benitoiteh::config::Approval::Untrusted,
            sandbox: benitoiteh::config::Sandbox::DangerFullAccess,
            sandboxed: false,
        },
    );
    new
}

// 関門: /new が許可の集合か読んだ版を移す退行を、確認の要求と実際の編集拒否で検出する。
#[test]
fn new_does_not_carry_allowed_commands_domains_or_read_versions() -> TestResult {
    use rewind_fixture::response as tools;
    let command = json!({"command":"printf approved"});
    let domain = json!({"url":"https://mock.invalid/path"});
    let mut server = MockProvider::start(vec![
        tools(&[("web_fetch", domain.clone())]),
        tools(&[
            ("read", json!({"path":"a.bnt"})),
            ("bash", command.clone()),
            ("bash", command.clone()),
        ]),
        tools(&[]),
        tools(&[
            (
                "edit",
                json!({"path":"a.bnt","old_string":"before","new_string":"must not happen"}),
            ),
            ("bash", command),
            ("web_fetch", domain),
        ]),
        tools(&[]),
    ])?;
    let f = Fixture::new()?;
    let mut old = untrusted(f.new_session(&server)?);
    let directory = old.session.directory().to_owned();
    let (approver, approvals) = benitoiteh::approval::InteractiveApprover::channel();
    old.agent = old.agent.with_approver(Box::new(approver));
    let cancelled = std::sync::Arc::new(AtomicBool::new(false));
    // 許可の返事を届ける前に取り消す。順序を固定し、道具の名前解決へ進ませない。
    let report = thread::scope(|scope| -> Result<_, Box<dyn Error>> {
        let (events, _received) = std::sync::mpsc::sync_channel(1024);
        let running = scope.spawn(|| {
            old.agent.run(
                "Remember domain".into(),
                std::sync::Arc::clone(&cancelled),
                events,
            )
        });
        let prompt = approvals.recv_timeout(Duration::from_secs(5))?;
        assert_eq!(prompt.request.kind, ApprovalKind::WebFetch);
        cancelled.store(true, Ordering::Release);
        prompt.reply.send(benitoiteh::approval::ApprovalAnswer {
            outcome: benitoiteh::approval::ApprovalOutcome::ApprovedForSession,
            message: None,
        })?;
        Ok(running.join().map_err(|_| "agent thread")??)
    })?;
    assert_eq!(report.outcome, benitoiteh::agent::Outcome::Cancelled);
    assert!(read_records(&directory)?.iter().any(|record| matches!(record,
        Record::ToolResult { name, result, .. } if name == "web_fetch" && result.is_error && result.text.contains("cancelled")
    )));
    let worker = Worker::start(old.agent, old.session)?.with_approval_requests(approvals);
    let mut screen = Screen::new((240, 60), status());
    let mut output = Output::default();
    let mut input = Driver::new(
        output.clone(),
        vec![
            ("", send("first")),
            (
                "[a] allow",
                vec![key(KeyCode::Char('a'), KeyModifiers::NONE)],
            ),
            ("The run completed.", send("/new")),
            ("new ready", send("second")),
            (
                "[a] allow",
                vec![key(KeyCode::Char('n'), KeyModifiers::NONE)],
            ),
            (
                "[s] allow",
                vec![key(KeyCode::Char('n'), KeyModifiers::NONE)],
            ),
            ("The run completed.", send("/quit")),
        ],
    );
    let mut new_directory = PathBuf::new();
    run(
        &mut screen,
        worker,
        &mut input,
        &mut output,
        &AtomicBool::new(false),
        |_| {},
        &mut |_| {
            let mut new = untrusted(f.new_session(&server).map_err(|error| error.to_string())?);
            new_directory = new.session.directory().to_owned();
            new.diagnostics.push("new ready".into());
            Ok(new)
        },
    )?;
    assert_eq!(fs::read_to_string(f.project.join("a.bnt"))?, "before\n");
    for path in [&directory, &new_directory] {
        let records = read_records(path)?;
        assert_eq!(
            records
                .iter()
                .filter(|record| matches!(
                    record,
                    Record::Approval {
                        phase: benitoiteh::session::ApprovalPhase::Request,
                        kind: Some(ApprovalKind::Command),
                        ..
                    }
                ))
                .count(),
            1
        );
        assert_eq!(
            records
                .iter()
                .filter(|record| matches!(
                    record,
                    Record::Approval {
                        phase: benitoiteh::session::ApprovalPhase::Request,
                        kind: Some(ApprovalKind::WebFetch),
                        ..
                    }
                ))
                .count(),
            1
        );
    }
    assert!(read_records(&new_directory)?.iter().any(|record|matches!(record,Record::ToolResult{name,result,..} if name=="edit" && result.is_error && result.text.contains("read"))));
    server.shutdown()?;
    Ok(())
}

// 関門: y の直後のキーで許した巻き戻しを捨てる退行と、キーの二度目の Ctrl+C で途中終了する退行を守る。
#[test]
fn accepted_rewind_finishes_before_quit_and_repeated_keyboard_interrupts() -> TestResult {
    let mut server = MockProvider::start(vec![
        rewind_fixture::response(&[("read", json!({"path":"a.txt"}))]),
        rewind_fixture::response(&[]),
    ])?;
    let f = rewind_fixture::Fixture::new()?;
    fs::write(f.project.join("a.txt"), "saved")?;
    let mut agent = f.agent(&server)?;
    rewind_fixture::run(&mut agent)?;
    fs::write(f.project.join("a.txt"), "changed")?;
    let worker = Worker::start(agent, f.session.clone())?;
    let mut screen = Screen::new((200, 40), status());
    let mut output = Output::default();
    let mut finish = vec![
        key(KeyCode::Char('y'), KeyModifiers::NONE),
        key(KeyCode::Char('c'), KeyModifiers::CONTROL),
        key(KeyCode::Char('c'), KeyModifiers::CONTROL),
    ];
    finish.extend(send("/quit"));
    finish.push(key(KeyCode::Char('c'), KeyModifiers::CONTROL));
    let mut input = Driver::new(
        output.clone(),
        vec![
            ("", send("/rewind")),
            (
                "Select a checkpoint",
                vec![key(KeyCode::Enter, KeyModifiers::NONE)],
            ),
            (
                "Select rewind scope",
                vec![key(KeyCode::Enter, KeyModifiers::NONE)],
            ),
            ("[y] rewind", finish),
        ],
    );
    assert_eq!(
        run(
            &mut screen,
            worker,
            &mut input,
            &mut output,
            &AtomicBool::new(false),
            |_| {},
            &mut |_| Err("unexpected new".into())
        )?,
        0
    );
    assert_eq!(fs::read_to_string(f.project.join("a.txt"))?, "saved");
    assert!(safe_output(&output.0.borrow())?.contains("Waiting for rewind to finish"));
    assert!(screen.checkpoint_before_rewind().is_none());
    assert!(
        read_records(f.session.directory())?
            .iter()
            .any(|record| matches!(record, Record::Rewind { .. }))
    );
    server.shutdown()?;
    Ok(())
}

// 関門: 取り消しで未送信の原文を失う退行と、//new を /new に戻してしまう退行を守る。
#[test]
fn cancellation_restores_literal_slashes_for_compaction_followups_and_run_inputs() -> TestResult {
    for (compact, followup) in [(true, true), (false, true), (false, false)] {
        let mut blocked = usage_response("never", 0, 0);
        blocked.delivery = Delivery::HeadersOnly;
        let mut server = MockProvider::start(vec![usage_response("answer", 1, 1), blocked])?;
        let f = Fixture::new()?;
        let (worker, _) = f.worker(&server)?;
        let mut screen = Screen::new((160, 40), status());
        let mut output = Output::default();
        let stopped = AtomicBool::new(false);
        struct Inputs<'a> {
            server: &'a MockProvider,
            output: Output,
            stopped: &'a AtomicBool,
            stage: usize,
            compact: bool,
            followup: bool,
            started: Instant,
        }
        impl InputSource for Inputs<'_> {
            fn poll(&mut self, _: Duration) -> io::Result<Vec<Event>> {
                if self.started.elapsed() > Duration::from_secs(8) {
                    return Err(io::ErrorKind::TimedOut.into());
                }
                thread::sleep(Duration::from_millis(1));
                let visible = safe_output(&self.output.0.borrow())
                    .map_err(|e| io::Error::other(e.to_string()))?;
                match self.stage {
                    0 => {
                        self.stage = 1;
                        Ok(send("request"))
                    }
                    1 if visible.contains("answer") && {
                        let bytes = self.output.0.borrow();
                        let raw = String::from_utf8_lossy(&bytes);
                        let frame = raw
                            .rfind("\x1b[?2026h")
                            .and_then(|i| raw.get(i..))
                            .unwrap_or_default();
                        !Regex::new(r" \| [0-9]+s \|")
                            .map_err(io::Error::other)?
                            .is_match(frame)
                    } =>
                    {
                        self.stage = 2;
                        Ok(send(if self.compact {
                            "/compact"
                        } else {
                            "blocked request"
                        }))
                    }
                    2 if self.server.requests()?.len() >= 2 => {
                        self.stage = 3;
                        Ok(vec![
                            Event::Paste("//new".into()),
                            key(
                                KeyCode::Enter,
                                if self.followup {
                                    KeyModifiers::ALT
                                } else {
                                    KeyModifiers::NONE
                                },
                            ),
                            key(KeyCode::Char('c'), KeyModifiers::CONTROL),
                        ])
                    }
                    3 if visible.contains(if self.compact {
                        "Compaction cancelled:"
                    } else {
                        "The run was cancelled."
                    }) =>
                    {
                        self.stopped.store(true, Ordering::Release);
                        Ok(Vec::new())
                    }
                    _ => Ok(Vec::new()),
                }
            }
            fn discard_pending(&mut self) -> io::Result<Vec<(Event, Instant)>> {
                Ok(Vec::new())
            }
        }
        let mut input = Inputs {
            server: &server,
            output: output.clone(),
            stopped: &stopped,
            stage: 0,
            compact,
            followup,
            started: Instant::now(),
        };
        assert_eq!(
            run(
                &mut screen,
                worker,
                &mut input,
                &mut output,
                &stopped,
                |_| {},
                &mut |_| Err("unexpected new".into())
            )?,
            130
        );
        let mut final_frame = Vec::new();
        screen.draw(&mut final_frame, None)?;
        assert!(
            safe_output(&final_frame)?.contains("//new"),
            "compact={compact}, followup={followup}: {}",
            safe_output(&final_frame)?
        );
        assert_eq!(
            screen.input(
                key(KeyCode::Enter, KeyModifiers::NONE),
                Duration::ZERO,
                false
            ),
            Action::Submit("/new".into())
        );
        assert_eq!(server.requests()?.len(), 2);
        server.shutdown()?;
    }
    Ok(())
}

// 関門: 古い End を待つ間の終了が新しい Agent を起動する退行と、準備済みの記録を残す退行を守る。
#[test]
fn new_session_is_ended_without_activation_when_quit_or_signals_arrive_before_old_end() -> TestResult
{
    use std::cell::Cell;
    for signals in [0, 1, 2] {
        let mut server = MockProvider::start(Vec::new())?;
        let f = Fixture::new()?;
        let (worker, _) = f.worker(&server)?;
        let mut screen = Screen::new((160, 40), status());
        let mut output = Output::default();
        let stopped = AtomicBool::new(false);
        let count = Rc::new(Cell::new(0));
        let activated = Rc::new(Cell::new(false));
        struct Inputs {
            events: Vec<Event>,
            count: Rc<Cell<usize>>,
        }
        impl InputSource for Inputs {
            fn poll(&mut self, _: Duration) -> io::Result<Vec<Event>> {
                Ok(std::mem::take(&mut self.events))
            }
            fn termination_count(&mut self) -> io::Result<usize> {
                Ok(self.count.get())
            }
        }
        let mut events = send("/new");
        if signals == 0 {
            events.extend(send("/quit"));
        }
        let mut input = Inputs {
            events,
            count: Rc::clone(&count),
        };
        let mut directory = PathBuf::new();
        let mut temporary = PathBuf::new();
        let result = run(
            &mut screen,
            worker,
            &mut input,
            &mut output,
            &stopped,
            |_| {},
            &mut |_| {
                let new = f.new_session(&server).map_err(|e| e.to_string())?;
                directory = new.session.directory().to_owned();
                temporary = new.session.temporary_dir().to_owned();
                if signals > 0 {
                    count.set(signals);
                    stopped.store(true, Ordering::Release);
                }
                let activated = Rc::clone(&activated);
                Ok(new.on_start(move || activated.set(true)))
            },
        )?;
        assert_eq!(result, if signals == 0 { 0 } else { 130 });
        assert!(!activated.get(), "signals={signals}");
        assert!(
            matches!(
                read_records(&directory)?.last(),
                Some(Record::SessionEnd { .. })
            ),
            "signals={signals}"
        );
        assert!(!temporary.exists());
        assert!(server.requests()?.is_empty());
        server.shutdown()?;
    }
    Ok(())
}

// 関門: Agent の切り替えだけでは Worker が /new に古いプロジェクトを渡す退行を捕まえない。
#[test]
fn new_after_project_creation_uses_the_project_from_the_event() -> TestResult {
    use benitoiteh::approval::{ApprovalPolicy, InteractiveApprover};
    use benitoiteh::config::{Approval, Sandbox};
    let f = Fixture::new()?;
    let destination = f.root.join("created");
    let mut server = MockProvider::start(vec![
        ResponseScript::sse(response(
            json!({"tool_calls":[{"index":0,"id":"create","type":"function","function":{"name":"create_project","arguments":json!({"path":destination}).to_string()}}]}),
            "tool_calls",
        )),
        ResponseScript::sse(response(json!({"content":"Creation finished"}), "stop")),
    ])?;
    let mut prepared = f.new_session_in_mode(&server, Mode::Restricted)?;
    let (approver, requests) = InteractiveApprover::channel();
    prepared.agent = prepared.agent.with_approvals(
        Box::new(approver),
        ApprovalPolicy {
            approval: Approval::OnRequest,
            sandbox: Sandbox::WorkspaceWrite,
            sandboxed: true,
        },
    );
    let worker = Worker::start(prepared.agent, prepared.session)?.with_approval_requests(requests);
    let mut screen = Screen::new((160, 30), status());
    let mut output = Output::default();
    let mut input = Driver::new(
        output.clone(),
        vec![
            ("", send("Create")),
            (
                "[y] create and switch to this project",
                vec![key(KeyCode::Char('y'), KeyModifiers::NONE)],
            ),
            ("Creation finished", send("/new")),
            ("New session started.", send("/quit")),
        ],
    );
    let mut called = false;
    run(
        &mut screen,
        worker,
        &mut input,
        &mut output,
        &AtomicBool::new(false),
        |_| {},
        &mut |current| {
            assert_eq!(current.project, destination);
            called = true;
            let next = std::mem::ManuallyDrop::new(Fixture {
                root: f.root.clone(),
                project: destination.clone(),
                paths: f.paths.clone(),
            });
            let mut prepared = next
                .new_session_in_mode(&server, Mode::Restricted)
                .map_err(|error| error.to_string())?;
            prepared.diagnostics.push("New session started.".into());
            Ok(prepared)
        },
    )?;
    assert!(called);
    assert!(
        safe_output(&output.0.borrow())?
            .contains(&format!("Project changed to {}.", destination.display()))
    );
    server.shutdown()?;
    Ok(())
}
