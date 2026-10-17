//! 端末を差し替え、描画と入力の契約を検査する（設計書 07-01、実装プラン P4-06）。

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

use super::*;

fn status() -> Status {
    Status {
        mode: Mode::Normal,
        provider: "mock".into(),
        model: "model".into(),
        host: "host".into(),
        sandbox: "workspace-write".into(),
        sandbox_disabled: false,
        context_window: Some(1000),
        estimate: 100,
    }
}

fn key(code: KeyCode, modifiers: KeyModifiers) -> Event {
    Event::Key(KeyEvent::new(code, modifiers))
}

// 作成時の関門: 幅の計算と端末の折り返し任せへの退行を、描画の純関数の境界で捕まえる。
#[test]
fn wrapping_and_clipping_use_visible_cell_width() {
    let safe = SafeText::new("日\t本a", true);
    let (rows, cursor) = format::wrapped(&safe, 6, safe.as_str().len());
    assert_eq!(rows, ["日    ", "本a"]);
    assert_eq!(cursor, (1, 3));
    assert_eq!(format::clipped("a日本", 4).as_str(), "a日");
    let (rows, cursor) = format::wrapped(&SafeText::new("ab", true), 2, 2);
    assert_eq!(rows, ["ab", ""]);
    assert_eq!(cursor, (1, 0));
}

// 作成時の関門: 字面の検査ではなく、Markdown の表示の文字と装飾の契約を表で確かめる。
#[test]
fn markdown_preserves_unpaired_marks_lists_and_fenced_content() {
    let mut markdown = format::Markdown::default();
    for (source, visible, styled) in [
        ("# Heading", "# Heading", Some(Decoration::Bold)),
        (
            "- **bold** and *em*",
            "- bold and em",
            Some(Decoration::Bold),
        ),
        ("* item", "* item", None),
        ("  * item", "  * item", None),
        ("**a*", "**a*", None),
        (
            "**bold `*literal*`**",
            "bold *literal*",
            Some(Decoration::Bold),
        ),
        ("# *title*", "# title", Some(Decoration::Bold)),
        ("*a **b** c*", "a b c", Some(Decoration::Bold)),
        ("+ item", "+ item", None),
        ("1. item", "1. item", None),
        ("`**literal**`", "**literal**", Some(Decoration::Code)),
        (
            "unpaired **star and `code",
            "unpaired **star and `code",
            None,
        ),
        ("```rust", "```rust", None),
        ("*literal*", "*literal*", None),
        ("~~~", "~~~", None),
        ("```", "```", None),
        ("*em*", "em", Some(Decoration::Bold)),
        ("~~~", "~~~", None),
        ("**literal**", "**literal**", None),
        ("~~~", "~~~", None),
    ] {
        let line = markdown.line(&SafeText::new(source, false));
        assert_eq!(
            line.iter()
                .map(|(text, _)| text.as_str())
                .collect::<String>(),
            visible,
            "{source}"
        );
        assert_eq!(
            line.iter()
                .find_map(|(_, style)| (*style != Decoration::Plain).then_some(*style)),
            styled,
            "{source}"
        );
    }
}

#[test]
fn tool_summaries_diffs_and_reasoning_are_bounded() -> io::Result<()> {
    use serde_json::json;
    for (name, args, expected) in [
        (
            "read",
            json!({"path":"a.bnt","offset":1,"limit":120}),
            "read a.bnt (1-121)",
        ),
        ("glob", json!({"pattern":"*.bnt"}), "glob *.bnt"),
        ("grep", json!({"pattern":"fn","path":"src"}), "grep fn src"),
        ("write", json!({"path":"a.bnt"}), "write a.bnt"),
        ("edit", json!({"path":"a.bnt"}), "edit a.bnt"),
        (
            "bash",
            json!({"command":"cargo test\nsecret"}),
            "bash: cargo test",
        ),
        (
            "web_fetch",
            json!({"url":"https://example.org"}),
            "web_fetch https://example.org",
        ),
        ("other", json!({}), "other {}"),
    ] {
        let line = format::tool_call(name, &args, 80);
        assert_eq!(
            line.iter()
                .map(|(text, _)| text.as_str())
                .collect::<String>(),
            expected
        );
    }
    let diff = format::diff(&format!("-old\n+new\n{}", " context\n".repeat(201)));
    assert_eq!(diff.len(), 201);
    assert_eq!(
        diff.first()
            .and_then(|line| line.first())
            .map(|(_, style)| *style),
        Some(Decoration::Removed)
    );
    assert_eq!(
        diff.get(1)
            .and_then(|line| line.first())
            .map(|(_, style)| *style),
        Some(Decoration::Added)
    );
    assert_eq!(
        diff.last()
            .and_then(|line| line.first())
            .map(|(text, _)| text.as_str()),
        Some("3 more lines")
    );
    let mut screen = Screen::new((100, 20), status());
    screen.event(&AgentEvent::TurnStart { turn: 1 });
    screen.event(&AgentEvent::ReasoningDelta {
        turn: 1,
        text: "one\ntwo\nthree\nfour\nfive".into(),
    });
    screen.event(&AgentEvent::TextDelta {
        turn: 1,
        text: "answer\n".into(),
    });
    let mut bytes = Vec::new();
    screen.draw(&mut bytes, None)?;
    let output = String::from_utf8_lossy(&bytes);
    assert!(output.contains("one") && output.contains("three") && output.contains("..."));
    assert!(!output.contains("four") && !output.contains("five"));
    assert!(output.contains("answer"));
    Ok(())
}

// 作成時の関門: 入力の公開のイベント列から状態を作り、キー解釈の独立した契約をまとめる。
#[test]
fn editing_history_and_interrupts_follow_input_events() {
    let mut screen = Screen::new((80, 24), status());
    let at = Duration::from_millis(10);
    for event in [
        key(KeyCode::Char('日'), KeyModifiers::NONE),
        key(KeyCode::Char('a'), KeyModifiers::NONE),
        key(KeyCode::Left, KeyModifiers::NONE),
        key(KeyCode::Delete, KeyModifiers::NONE),
        key(KeyCode::Home, KeyModifiers::NONE),
        key(KeyCode::Char('b'), KeyModifiers::NONE),
        key(KeyCode::End, KeyModifiers::NONE),
        key(KeyCode::Backspace, KeyModifiers::NONE),
    ] {
        assert_eq!(screen.input(event, at, false), Action::None);
    }
    assert_eq!(screen.input.text, "b");
    assert_eq!(
        screen.input(key(KeyCode::Enter, KeyModifiers::NONE), at, false),
        Action::Submit("b".into())
    );
    screen.request("b");
    screen.input(Event::Paste("draft".into()), at, false);
    screen.input(key(KeyCode::Up, KeyModifiers::NONE), at, false);
    assert_eq!(screen.input.text, "b");
    screen.input(key(KeyCode::Down, KeyModifiers::NONE), at, false);
    assert_eq!(screen.input.text, "draft");
    screen.input(key(KeyCode::Char('c'), KeyModifiers::CONTROL), at, false);
    screen.input(Event::Paste("a\r\nb\rc".into()), at, false);
    screen.input(key(KeyCode::Char('j'), KeyModifiers::CONTROL), at, false);
    screen.input(key(KeyCode::Enter, KeyModifiers::SHIFT), at, false);
    assert_eq!(screen.input.text, "a\nb\nc\n\n");
    assert_eq!(
        screen.input(key(KeyCode::Enter, KeyModifiers::ALT), at, false),
        Action::Submit("a\nb\nc\n\n".into())
    );
    screen.input(Event::Paste("steer".into()), at, true);
    assert_eq!(
        screen.input(key(KeyCode::Enter, KeyModifiers::NONE), at, true),
        Action::Steer("steer".into())
    );
    let mut released = KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE);
    released.kind = KeyEventKind::Release;
    screen.input(Event::Key(released), at, false);
    assert!(screen.input.text.is_empty());
    assert_eq!(
        screen.input(key(KeyCode::Char('c'), KeyModifiers::CONTROL), at, true),
        Action::Cancel
    );
    assert_eq!(
        screen.input(key(KeyCode::Char('c'), KeyModifiers::CONTROL), at, true),
        Action::Quit
    );
    screen.complete(101);
    screen.input(Event::Paste("draft".into()), at, false);
    screen.input(key(KeyCode::Char('c'), KeyModifiers::CONTROL), at, false);
    assert!(screen.input.text.is_empty());
    screen.input(key(KeyCode::Char('c'), KeyModifiers::CONTROL), at, false);
    assert!(screen.input.quit_hint);
    screen.input(key(KeyCode::Left, KeyModifiers::NONE), at, false);
    assert_eq!(
        screen.input(key(KeyCode::Char('c'), KeyModifiers::CONTROL), at, false),
        Action::None
    );
    assert_eq!(
        screen.input(
            key(KeyCode::Char('c'), KeyModifiers::CONTROL),
            Duration::from_secs(2),
            false
        ),
        Action::None
    );
    assert_eq!(
        screen.input(
            key(KeyCode::Char('c'), KeyModifiers::CONTROL),
            Duration::from_millis(2200),
            false
        ),
        Action::Quit
    );
    assert_eq!(
        screen.input(key(KeyCode::Char('d'), KeyModifiers::CONTROL), at, true),
        Action::Quit
    );
}

#[derive(Default)]
struct Writes {
    bytes: Vec<u8>,
    writes: usize,
    flushes: usize,
    unflushed: Vec<Vec<u8>>,
}
impl Write for Writes {
    fn write(&mut self, value: &[u8]) -> io::Result<usize> {
        self.writes = self.writes.saturating_add(1);
        self.bytes.extend_from_slice(value);
        self.unflushed.push(value.to_vec());
        Ok(value.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        assert_eq!(self.unflushed.len(), 1);
        let frame = self.unflushed.pop().ok_or(io::ErrorKind::InvalidData)?;
        assert!(frame.starts_with(b"\x1b[?2026h\x1b[?25l"));
        assert!(frame.ends_with(b"\x1b[?7h\x1b[?25h\x1b[?2026l"));
        self.flushes = self.flushes.saturating_add(1);
        Ok(())
    }
}

#[test]
fn drawing_uses_one_frame_bounds_the_bottom_and_never_rewrites_history() -> io::Result<()> {
    for (width, height) in [(80, 24), (8, 6), (5, 2), (1, 1), (0, 0)] {
        let mut screen = Screen::new((width, height), status());
        screen.event(&AgentEvent::TextDelta {
            turn: 1,
            text: "committed\n".into(),
        });
        screen.input(
            Event::Paste("日\t本\nline\nline\nline\nlast".into()),
            Duration::ZERO,
            false,
        );
        screen.event(&AgentEvent::TextDelta {
            turn: 1,
            text: "provisional".repeat(5),
        });
        let mut writer = Writes::default();
        screen.draw(&mut writer, None)?;
        assert_eq!((writer.writes, writer.flushes), (1, 1));
        assert!(screen.rows_above.saturating_add(screen.rows_below) < height.max(1));
        let first = String::from_utf8_lossy(&writer.bytes);
        assert!(first.contains("context") == (height >= 3 && width > 30));
        let mut next = Vec::new();
        screen.draw(&mut next, None)?;
        assert!(!String::from_utf8_lossy(&next).contains("committed"));
        assert!(next.starts_with(b"\x1b[?2026h\x1b[?25l"));
        assert!(next.ends_with(b"\x1b[?7h\x1b[?25h\x1b[?2026l"));
    }
    Ok(())
}

// 作成時の関門: 一つの論理行の残りが柵や見出しになる退行を、描画まで通して検出する。
#[test]
fn overflow_fragments_do_not_start_markdown_blocks() -> io::Result<()> {
    for suffix in ["```", "~~~", "# **literal**"] {
        let mut screen = Screen::new((40, 4), status());
        screen.event(&AgentEvent::TextDelta {
            turn: 1,
            text: format!("{}{suffix}", "p".repeat(40)),
        });
        let mut bytes = Vec::new();
        screen.draw(&mut bytes, None)?;
        screen.event(&AgentEvent::TextDelta {
            turn: 1,
            text: "\n**visible**\n".into(),
        });
        screen.draw(&mut bytes, None)?;
        let output = String::from_utf8_lossy(&bytes);
        assert!(!output.contains("**visible**"), "{suffix}: {output}");
        assert!(output.contains(suffix), "{suffix}: {output}");
        assert!(output.contains("\x1b[1mvisible"));
    }
    Ok(())
}

#[test]
fn reasoning_limits_restart_after_text_and_tool_calls_in_the_same_turn() -> io::Result<()> {
    let mut screen = Screen::new((100, 24), status());
    screen.event(&AgentEvent::TurnStart { turn: 1 });
    for (index, label) in ["first", "second", "third"].into_iter().enumerate() {
        screen.event(&AgentEvent::ReasoningDelta {
            turn: 1,
            text: format!("{label} one\n{label} two\n{label} three\n{label} omitted\n"),
        });
        if index == 0 {
            screen.event(&AgentEvent::TextDelta {
                turn: 1,
                text: "answer\n".into(),
            });
        } else {
            screen.event(&AgentEvent::ToolCall {
                turn: 1,
                call_id: "call".into(),
                name: "read".into(),
                arguments: serde_json::json!({"path":"a.bnt"}),
            });
        }
    }
    let mut bytes = Vec::new();
    screen.draw(&mut bytes, None)?;
    let output = String::from_utf8_lossy(&bytes);
    for label in ["first", "second", "third"] {
        assert!(output.contains(&format!("{label} three")), "{output}");
        assert!(!output.contains(&format!("{label} omitted")));
    }
    assert_eq!(output.matches("...").count(), 3);
    Ok(())
}

#[test]
fn a_partial_frame_is_not_completed_with_a_second_write() {
    struct Partial {
        calls: usize,
    }
    impl Write for Partial {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.calls = self.calls.saturating_add(1);
            Ok(bytes.len().saturating_sub(1))
        }
        fn flush(&mut self) -> io::Result<()> {
            Err(io::ErrorKind::InvalidData.into())
        }
    }
    let mut screen = Screen::new((80, 24), status());
    let mut output = Partial { calls: 0 };
    assert_eq!(
        screen
            .draw(&mut output, None)
            .err()
            .map(|error| error.kind()),
        Some(io::ErrorKind::WriteZero)
    );
    assert_eq!(output.calls, 1);
}

// 復元済みの書き先は、フレームと flush の両方を捨てる。進行中の一枚も wrap を戻して終える。
#[test]
fn restored_output_discards_later_frames_and_flushes() -> io::Result<()> {
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };
    struct Sink {
        restored: Arc<AtomicBool>,
        frames: Vec<Vec<u8>>,
        flushes: usize,
    }
    impl Write for Sink {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.frames.push(bytes.to_vec());
            self.restored.store(true, Ordering::Release);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            self.flushes = self.flushes.saturating_add(1);
            Ok(())
        }
    }
    for already_restored in [false, true] {
        let restored = Arc::new(AtomicBool::new(already_restored));
        let mut sink = Sink {
            restored: Arc::clone(&restored),
            frames: Vec::new(),
            flushes: 0,
        };
        let mut screen = Screen::new((80, 24), status());
        {
            let mut output = terminal::RestoredWriter::new(&mut sink, Arc::clone(&restored));
            screen.draw(&mut output, None)?;
            screen.notice("late event");
            screen.draw(&mut output, None)?;
        }
        assert_eq!(sink.frames.len(), usize::from(!already_restored));
        assert_eq!(sink.flushes, 0);
        if let Some(frame) = sink.frames.first() {
            assert!(frame.ends_with(b"\x1b[?7h\x1b[?25h\x1b[?2026l"));
        }
    }
    Ok(())
}

// panic は abort のため統合テストで起こさず、全経路で共用する復元のバイト列を確かめる。
#[test]
fn restoration_releases_every_enabled_terminal_mode() -> io::Result<()> {
    assert_eq!(
        restoration(2)?,
        b"\x1b[?2026l\x1b[0m\x1b[?25h\x1b[?7h\x1b[?2004l\x1b[<1u\r\n\r\n\r\n"
    );
    let mut setup = Vec::new();
    control::setup(&mut setup)?;
    assert_eq!(setup, b"\x1b[?2004h\x1b[>1u");
    Ok(())
}

fn rewind_preview(conversation_available: bool) -> crate::agent::RewindPreview {
    crate::agent::RewindPreview {
        selected: crate::rewind::CheckpointInfo {
            checkpoint: crate::rewind::Checkpoint {
                id: "c000001".into(),
                reason: crate::rewind::CheckpointReason::Request,
                turn: 3,
                call_id: None,
                created_at: crate::rewind::Timestamp {
                    seconds: 0,
                    nanoseconds: 0,
                },
                items: Some(0),
                generation: 0,
                manifest: None,
                incomplete: Vec::new(),
            },
            conversation_available,
            request: Some("request \u{3164} \u{202e}".into()),
            tool: None,
        },
        scope: crate::rewind::RewindScope::FilesAndConversation,
        irreversible: vec![crate::rewind::IrreversibleOperation {
            call_id: "call".into(),
            summary: "bash: external operation".into(),
        }],
    }
}

// 関門: 選択画面の窓と会話不可の範囲を、入力と描画の境界で守る。
#[test]
fn checkpoint_selection_scrolls_esc_cancels_and_unavailable_conversation_has_only_files()
-> io::Result<()> {
    for available in [true, false] {
        let mut points = Vec::new();
        for index in 0..30 {
            let mut point = rewind_preview(available).selected;
            point.checkpoint.id = format!("c{index:06}");
            points.push(point);
        }
        let mut screen = Screen::new((150, 8), status());
        screen.select_checkpoints(points);
        for _ in 0..29 {
            screen.input(
                key(KeyCode::Down, KeyModifiers::NONE),
                Duration::ZERO,
                false,
            );
        }
        let mut bytes = Vec::new();
        screen.draw(&mut bytes, None)?;
        let output = String::from_utf8_lossy(&bytes);
        assert!(output.contains("c000000"));
        assert!(!output.contains("c000029"));
        assert!(output.contains("\\u{3164}"));
        assert!(!output.contains('\u{3164}'));
        screen.input(key(KeyCode::Up, KeyModifiers::NONE), Duration::ZERO, false);
        screen.input(
            key(KeyCode::Enter, KeyModifiers::NONE),
            Duration::ZERO,
            false,
        );
        for _ in 0..3 {
            screen.input(
                key(KeyCode::Down, KeyModifiers::NONE),
                Duration::ZERO,
                false,
            );
        }
        assert_eq!(
            screen.input(
                key(KeyCode::Enter, KeyModifiers::NONE),
                Duration::ZERO,
                false
            ),
            Action::Command(Command::PreviewRewind(
                "c000001".into(),
                if available {
                    crate::rewind::RewindScope::Conversation
                } else {
                    crate::rewind::RewindScope::Files
                }
            ))
        );
        screen.select_checkpoints(vec![rewind_preview(available).selected]);
        screen.input(key(KeyCode::Esc, KeyModifiers::NONE), Duration::ZERO, false);
        assert_eq!(
            screen.input(Event::Paste("request".into()), Duration::ZERO, false),
            Action::None
        );
        assert_eq!(
            screen.input(
                key(KeyCode::Enter, KeyModifiers::NONE),
                Duration::ZERO,
                false
            ),
            Action::Submit("request".into())
        );
    }
    Ok(())
}

// 関門: 巻き戻しの種類を足しても先行入力・貼り付け・縮小した端末で許可されないことを守る。
#[test]
fn rewind_confirmations_reject_queued_and_early_keys_paste_and_hidden_facts() -> io::Result<()> {
    for incomplete in [false, true] {
        let mut screen = Screen::new((200, 40), status());
        screen.rewind_confirmation(
            rewind_preview(true),
            incomplete.then(|| ("before-id".into(), vec!["entry limit reached".into()])),
        );
        let shown = Instant::now();
        let approve = if incomplete { 'f' } else { 'y' };
        screen.input_at(
            key(KeyCode::Char(approve), KeyModifiers::NONE),
            shown,
            Duration::ZERO,
            false,
        );
        assert_eq!(screen.take_confirmation_result(), None);
        let mut bytes = Vec::new();
        screen.draw(&mut bytes, None)?;
        screen.presented(shown);
        for at in [
            shown - Duration::from_millis(1),
            shown + Duration::from_millis(299),
        ] {
            screen.input_at(
                key(KeyCode::Char(approve), KeyModifiers::NONE),
                at,
                Duration::ZERO,
                false,
            );
            assert_eq!(screen.take_confirmation_result(), None);
        }
        screen.input_at(
            Event::Paste(approve.to_string()),
            shown + Duration::from_secs(1),
            Duration::ZERO,
            false,
        );
        assert_eq!(screen.take_confirmation_result(), None);
        screen.input_at(Event::Resize(40, 4), shown, Duration::ZERO, false);
        screen.draw(&mut Vec::new(), None)?;
        screen.presented(shown);
        screen.input_at(
            key(KeyCode::Char(approve), KeyModifiers::NONE),
            shown + Duration::from_secs(1),
            Duration::ZERO,
            false,
        );
        assert_eq!(screen.take_confirmation_result(), None);
        screen.input_at(Event::Resize(200, 40), shown, Duration::ZERO, false);
        screen.input_at(
            key(KeyCode::Char(approve), KeyModifiers::NONE),
            shown + Duration::from_secs(1),
            Duration::ZERO,
            false,
        );
        assert_eq!(screen.take_confirmation_result(), None);
        screen.draw(&mut Vec::new(), None)?;
        screen.presented(shown);
        if incomplete {
            screen.input_at(
                key(KeyCode::Char('y'), KeyModifiers::NONE),
                shown + Duration::from_secs(1),
                Duration::ZERO,
                false,
            );
            assert_eq!(screen.take_confirmation_result(), None);
        }
        screen.input_at(
            key(KeyCode::Char(approve), KeyModifiers::NONE),
            shown + Duration::from_secs(1),
            Duration::ZERO,
            false,
        );
        assert_eq!(screen.take_confirmation_result(), Some(true));
        let output = String::from_utf8_lossy(&bytes);
        assert!(!output.contains('\u{3164}'));
        assert!(output.contains("\\u{3164}"));
        assert!(output.find("bash: external operation") < output.find("│ Checkpoint:"));
        assert!(output.find("│ Scope:") < output.find("[n] cancel"));
    }
    Ok(())
}

// 関門: status に URL や未検査の処理系を載せる退行を、表示の純関数で守る。
#[test]
fn status_prints_all_fields_and_sanitizes_values_without_a_base_url() {
    let status = crate::agent::AgentStatus {
        session_id: "session".into(),
        mode: Mode::Normal,
        project: "/project".into(),
        provider: "mock\x1b[2J".into(),
        model: "model\nspoof".into(),
        host: "host".into(),
        sandbox: crate::config::Sandbox::WorkspaceWrite,
        sandbox_disabled: true,
        approval: crate::config::Approval::Untrusted,
        safety: crate::agent::SessionSafety::default(),
        estimate: 123,
        context_window: Some(456),
        instruction_files: vec!["/project/AGENTS.md".into()],
        skipped_instructions: vec![crate::session::SkippedInstruction {
            path: "/project/secret".into(),
            reason: "denied\x1b".into(),
        }],
    };
    let value = text::session_status(
        &status,
        &crate::session::TokenCounts {
            input: 1,
            cache_read: 2,
            cache_write: 3,
            output: 4,
        },
    );
    for expected in [
        "Session: session",
        "Mode: normal",
        "Provider: mock\\x1b[2J",
        "Model: model spoof",
        "Host: host",
        "Interpreter: not checked",
        "Sandbox: workspace-write (disabled by --no-sandbox)",
        "Approval: untrusted",
        "input 1, cache read 2, cache write 3, output 4",
        "Context: 123 / 456",
        "/project/AGENTS.md",
        "/project/secret: denied\\x1b",
    ] {
        assert!(value.contains(expected), "{expected}: {value}");
    }
    assert!(value.lines().count() > 10);
    assert!(!value.contains('\x1b'));
    assert!(!value.contains("base_url"));
}

fn visible_frame(bytes: &[u8]) -> Result<String, Box<dyn std::error::Error>> {
    let controls = regex::Regex::new(r"\x1b\[[0-9;?<>]*[A-Za-z]")?;
    Ok(controls
        .replace_all(std::str::from_utf8(bytes)?, "")
        .replace("\r\n", "\n"))
}

// 関門: 詳細を省いた端末でも不可逆の件数を示し、空の一覧では見出しを出さない契約を描画と答えで守る。
#[test]
fn rewind_confirmation_keeps_irreversible_count_when_details_do_not_fit()
-> Result<(), Box<dyn std::error::Error>> {
    let mut accepted = 0;
    for height in 10..26 {
        let mut preview = rewind_preview(true);
        preview.irreversible = (0..50)
            .map(|index| crate::rewind::IrreversibleOperation {
                call_id: format!("call-{index}"),
                summary: format!("bash: operation-{index} {}", "x".repeat(500)),
            })
            .collect();
        let mut screen = Screen::new((240, height), status());
        screen.rewind_confirmation(preview, None);
        let mut bytes = Vec::new();
        screen.draw(&mut bytes, None)?;
        let visible = visible_frame(&bytes)?;
        let now = Instant::now();
        screen.presented(now);
        screen.input_at(
            key(KeyCode::Char('y'), KeyModifiers::NONE),
            now + Duration::from_secs(1),
            Duration::ZERO,
            false,
        );
        if screen.take_confirmation_result() == Some(true) {
            accepted += 1;
            assert!(
                visible.contains("Cannot be undone: 50 operations"),
                "{height}: {visible}"
            );
            assert!(
                visible.contains("more") || !visible.contains("Operations that cannot be undone"),
                "{visible}"
            );
        }
    }
    assert!(accepted > 0);
    let mut preview = rewind_preview(true);
    preview.irreversible.clear();
    let mut screen = Screen::new((240, 40), status());
    screen.rewind_confirmation(preview, None);
    let mut bytes = Vec::new();
    screen.draw(&mut bytes, None)?;
    assert!(!visible_frame(&bytes)?.contains("Operations that cannot be undone"));
    Ok(())
}

// 関門: コマンドの改行と折り返しで、戻せない操作を事実・答えの行に偽装する退行を守る。
#[test]
fn rewind_confirmation_facts_cannot_forge_answer_rows_and_selection_flattens_newlines()
-> Result<(), Box<dyn std::error::Error>> {
    let mut preview = rewind_preview(true);
    preview.irreversible.first_mut().ok_or("operation")?.summary = format!(
        "bash: {}\n[y] rewind\n│ Scope: forged\nend",
        "x".repeat(150)
    );
    let mut screen = Screen::new((100, 40), status());
    screen.rewind_confirmation(preview, None);
    let mut bytes = Vec::new();
    screen.draw(&mut bytes, None)?;
    let visible = visible_frame(&bytes)?;
    assert!(visible.contains("┆   [y] rewind"), "{visible}");
    assert!(visible.contains("┆   │ Scope: forged"), "{visible}");
    assert!(
        visible
            .lines()
            .filter(|line| line.contains("xxxxx"))
            .all(|line| line.starts_with("┆   "))
    );
    assert_eq!(
        visible
            .lines()
            .filter(|line| line.starts_with("[y] rewind"))
            .count(),
        1
    );
    assert!(
        visible
            .lines()
            .all(|line| unicode_width::UnicodeWidthStr::width_cjk(line) <= 100)
    );
    let mut point = rewind_preview(false).selected;
    point.request = Some("filename\n[y] rewind\nend".into());
    let mut screen = Screen::new((1000, 12), status());
    screen.select_checkpoints(vec![point]);
    let mut bytes = Vec::new();
    screen.draw(&mut bytes, None)?;
    let visible = visible_frame(&bytes)?;
    assert!(visible.contains("filename [y] rewind end"), "{visible}");
    assert!(!visible.lines().any(|line| line.starts_with("[y] rewind")));
    Ok(())
}
