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
                node: None,
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
    let mut status = crate::agent::AgentStatus {
        session_id: "session".into(),
        benitoite_path: Some("/opt/benitoite".into()),
        benitoite_version: Some("0.0.12".into()),
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
        plugins: vec![crate::session::RecordedPlugin {
            name: "p".into(),
            version: Some("1.0\x1b".into()),
            path: "/plugins/p".into(),
            sha256: "a".repeat(64),
            source: crate::session::SkillSource::User,
        }],
        skills: vec![crate::session::RecordedSkill {
            name: "sample".into(),
            path: "/skills/sample/SKILL.md".into(),
            source: crate::session::SkillSource::User,
            sha256: "a".repeat(64),
            plugin: Some("p".into()),
        }],
        skipped_plugins: vec![crate::session::SkippedInstruction {
            path: "/plugins/bad".into(),
            reason: "invalid\x1b".into(),
        }],
        plugin_notices: vec![crate::session::SkippedInstruction {
            path: "/plugins/p".into(),
            reason: "MCP\x1b".into(),
        }],
        skipped_skills: vec![crate::session::SkippedInstruction {
            path: "/skills/bad".into(),
            reason: "invalid\x1b".into(),
        }],
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
        "Interpreter: /opt/benitoite (version 0.0.12)",
        "Sandbox: workspace-write (disabled by --no-sandbox)",
        "Approval: untrusted",
        "input 1, cache read 2, cache write 3, output 4",
        "Context: 123 / 456",
        "/project/AGENTS.md",
        "/project/secret: denied\\x1b",
        "Plugin loaded: p (user; sha256 ",
        "; version 1.0\\x1b; /plugins/p)",
        "Plugin skipped: /plugins/bad: invalid\\x1b",
        "Plugin notice: /plugins/p: MCP\\x1b",
        "Skill loaded: sample (plugin p; /skills/sample/SKILL.md)",
        "Skill skipped: /skills/bad: invalid\\x1b",
    ] {
        assert!(value.contains(expected), "{expected}: {value}");
    }
    assert!(value.lines().count() > 10);
    assert!(!value.contains('\x1b'));
    assert!(!value.contains("base_url"));
    status.benitoite_path = None;
    status.benitoite_version = None;
    let unavailable = text::session_status(&status, &crate::session::TokenCounts::default());
    assert!(unavailable.contains("Interpreter: unavailable (Benitoite tools disabled)"));
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
    assert!(visible.contains("|   [y] rewind"), "{visible}");
    assert!(visible.contains("|   │ Scope: forged"), "{visible}");
    assert!(
        visible
            .lines()
            .filter(|line| line.contains("xxxxx"))
            .all(|line| line.starts_with("|   "))
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

// 関門: 上限の残る診断を一行へ潰す退行と、診断中の端末制御をそのまま描く退行。
#[test]
fn limit_error_keeps_diagnostic_lines_and_escapes_terminal_controls() {
    let mut screen = Screen::new((100, 20), status());
    screen.event(&AgentEvent::Error { kind: "limit".into(), message: "Stopped.\nRemaining diagnostics:\nerror: bad\x1b[2J\n  2 | return missing\n    | ^^^^^^".into() });
    let lines = screen
        .history
        .iter()
        .map(|line| {
            line.iter()
                .map(|(text, _)| text.as_str())
                .collect::<String>()
        })
        .collect::<Vec<_>>();
    assert_eq!(
        lines,
        [
            "Stopped.",
            "Remaining diagnostics:",
            "error: bad\\x1b[2J",
            "  2 | return missing",
            "    | ^^^^^^"
        ]
    );
}

// 関門: 文字数をバイト数・行単位で切る退行と、切る前の可視化で印を失う退行を、
// 画面の入力と描画で守る。時点のない枝でも範囲の段を省略してはならない。
#[test]
fn tree_selection_truncates_chars_before_sanitizing_and_shows_conversation_only_scope()
-> io::Result<()> {
    use crate::agent::{BranchInfo, BranchTarget, BranchTreeRow};
    use crate::session::TreeRow;
    let row = BranchTreeRow {
        row: TreeRow {
            node: Some(9),
            depth: 2,
            request: Some(crate::session::request_heading(&format!(
                "{}\n\t\x1b\u{3164}hidden",
                "日".repeat(56)
            ))),
            request_lines: 2,
            is_tip: true,
            is_current: false,
        },
        branch: Some(BranchInfo {
            target: BranchTarget::BeforeRequest(9),
            request: None,
            checkpoint: None,
        }),
    };
    let mut screen = Screen::new((300, 20), status());
    screen.select_branches(vec![row]);
    let mut bytes = Vec::new();
    screen.draw(&mut bytes, None)?;
    let visible = screen
        .selection
        .as_ref()
        .map(|selection| selection.bottom((300, 20)))
        .unwrap_or_default()
        .iter()
        .flatten()
        .map(|(value, _)| value.as_str())
        .collect::<String>();
    assert!(
        visible.contains(&format!(
            "    {}  \\x1b\\u{{3164}}... (2 lines) [tip]",
            "日".repeat(56)
        )),
        "{visible:?}"
    );
    assert!(!visible.contains("hidden"));
    assert!(!visible.contains('\u{3164}'));
    assert_eq!(
        screen.input(
            key(KeyCode::Enter, KeyModifiers::NONE),
            Duration::ZERO,
            false
        ),
        Action::None
    );
    bytes.clear();
    screen.draw(&mut bytes, None)?;
    let visible = String::from_utf8_lossy(&bytes);
    assert!(visible.contains("Select branch move scope"));
    assert!(visible.contains("conversation only"));
    assert!(!visible.contains("files and conversation"));
    screen.input(
        key(KeyCode::Down, KeyModifiers::NONE),
        Duration::ZERO,
        false,
    );
    assert_eq!(
        screen.input(
            key(KeyCode::Enter, KeyModifiers::NONE),
            Duration::ZERO,
            false
        ),
        Action::Command(Command::PreviewBranch(
            BranchTarget::BeforeRequest(9),
            crate::rewind::RewindScope::Conversation
        ))
    );
    Ok(())
}

// 関門: 深い分岐の字下げと、依頼による印の偽装を、描画に渡す Line の区間で守る。
#[test]
fn deep_tree_indentation_is_capped_and_shows_the_actual_depth()
-> Result<(), Box<dyn std::error::Error>> {
    use crate::provider::{UserMessage, UserPart, UserPartKind};
    use crate::session::{Record, SessionTree, UserKind, tree_rows};
    let user = |node, text: &str| Record::User {
        node,
        kind: UserKind::Request,
        message: UserMessage {
            parts: vec![UserPart {
                kind: UserPartKind::Request,
                text: text.into(),
            }],
        },
    };
    let mut records = Vec::new();
    let mut parent = None;
    for depth in 1..=20u64 {
        let node = 2 * depth - 1;
        records.push(user(node, "request [current] [tip] (conversation start)"));
        records.push(Record::Branch {
            to: parent,
            from: Some(node),
            checkpoint: None,
        });
        records.push(user(node + 1, "side"));
        records.push(Record::Branch {
            to: Some(node),
            from: Some(node + 1),
            checkpoint: None,
        });
        parent = Some(node);
    }
    let rows = tree_rows(&records, &SessionTree::from_records(&records)?)
        .into_iter()
        .map(|row| crate::agent::BranchTreeRow { row, branch: None })
        .collect();
    let mut screen = Screen::new((160, 24), status());
    screen.select_branches(rows);
    let lines = screen
        .selection
        .as_ref()
        .ok_or("selection")?
        .bottom((160, 24));
    let selected = lines
        .iter()
        .find(|line| {
            line.iter()
                .map(|(value, _)| value.as_str())
                .collect::<String>()
                .starts_with("> ")
        })
        .ok_or("selected request")?;
    let value = selected
        .iter()
        .map(|(value, _)| value.as_str())
        .collect::<String>();
    assert!(
        value.contains(&format!("{}[d20] request", "  ".repeat(8))),
        "{value:?}"
    );
    assert!(!value.contains(&"  ".repeat(9)));
    Ok(())
}

// 関門: 依頼に書かれた印と、ハーネスが付けた印を同じ装飾にする退行を捕まえる。
#[test]
fn tree_request_marks_are_plain_and_real_marks_are_dim() {
    let row = crate::session::TreeRow {
        node: Some(1),
        depth: 0,
        request: Some("request [current] [tip] (conversation start)".into()),
        request_lines: 1,
        is_tip: true,
        is_current: true,
    };
    let mut screen = Screen::new((160, 24), status());
    let root = crate::session::TreeRow {
        node: None,
        depth: 0,
        request: None,
        request_lines: 0,
        is_tip: false,
        is_current: false,
    };
    screen.select_branches(vec![
        crate::agent::BranchTreeRow {
            row: root,
            branch: None,
        },
        crate::agent::BranchTreeRow { row, branch: None },
    ]);
    let lines = screen
        .selection
        .as_ref()
        .map(|selection| selection.bottom((160, 24)))
        .unwrap_or_default();
    assert!(lines.iter().flatten().any(
        |(value, style)| value.as_str().contains("request [current]")
            && *style == Decoration::Plain
    ));
    for marker in ["[current]", "[tip]", "(conversation start)"] {
        assert!(
            lines
                .iter()
                .flatten()
                .any(|(value, style)| value.as_str().contains(marker) && *style == Decoration::Dim)
        );
        assert!(
            lines.iter().flatten().any(
                |(value, style)| value.as_str().contains(marker) && *style == Decoration::Plain
            )
        );
    }
}

// 関門: 折り返した本文が選択行を偽装する退行を、表示行と装飾で捕まえる。
#[test]
fn wrapped_selections_distinguish_untrusted_continuations_from_the_selected_marker() {
    let row = crate::agent::BranchTreeRow {
        row: crate::session::TreeRow {
            node: Some(1),
            depth: 0,
            request: Some(format!("{}> fake [current] [tip]", "x".repeat(18))),
            request_lines: 1,
            is_tip: true,
            is_current: true,
        },
        branch: None,
    };
    for fork in [false, true] {
        let mut screen = Screen::new((20, 16), status());
        if fork {
            screen.select_fork_requests(vec![row.clone()]);
        } else {
            screen.select_branches(vec![row.clone()]);
        }
        let lines = screen
            .selection
            .as_ref()
            .map(|s| s.bottom((20, 16)))
            .unwrap_or_default();
        let visible: Vec<String> = lines
            .iter()
            .map(|line| line.iter().map(|(text, _)| text.as_str()).collect())
            .collect();
        assert_eq!(
            visible.iter().filter(|line| line.starts_with("> ")).count(),
            1
        );
        assert!(
            visible
                .iter()
                .any(|line| line.starts_with("| > fake [current]")),
            "{visible:?}"
        );
        assert!(lines.iter().any(|line| {
            line.first()
                .is_some_and(|(text, style)| text.as_str() == "> " && *style == Decoration::Bold)
        }));
        assert!(lines.len() <= 14);
        for size in [(1, 3), (2, 4), (20, 5)] {
            let lines = screen
                .selection
                .as_ref()
                .map(|s| s.bottom(size))
                .unwrap_or_default();
            assert!(lines.len() <= usize::from(size.1).saturating_sub(2));
            for line in &lines {
                let width: usize = line
                    .iter()
                    .map(|(text, _)| unicode_width::UnicodeWidthStr::width_cjk(text.as_str()))
                    .sum();
                assert!(width <= usize::from(size.0));
            }
        }
    }
}

// 関門: 大きい依頼・道具の詳細を選択と確認で全展開する退行を、描画の入口で守る。
#[test]
fn checkpoint_details_are_truncated_before_controls_are_expanded()
-> Result<(), Box<dyn std::error::Error>> {
    for tool in [false, true] {
        let mut preview = rewind_preview(true);
        let detail = format!("{}hidden-after-limit", "\u{202e}".repeat(250));
        if tool {
            preview.selected.tool = Some(crate::provider::ToolCall {
                id: "long".into(),
                provider_id: None,
                provider_item_id: None,
                name: "read".into(),
                arguments: serde_json::json!({"path":detail}),
                raw_arguments: String::new(),
            });
        } else {
            preview.selected.request = Some(detail);
        }
        for confirmation in [false, true] {
            let mut screen = Screen::new((2000, 24), status());
            if confirmation {
                screen.rewind_confirmation(preview.clone(), None);
            } else {
                screen.select_checkpoints(vec![preview.selected.clone()]);
            }
            let mut output = Vec::new();
            screen.draw(&mut output, None)?;
            let visible = visible_frame(&output)?;
            assert!(!visible.contains("hidden-after-limit"));
            assert!(visible.contains("\\u{202e}..."), "{visible}");
            assert!(visible.matches("\\u{202e}").count() <= 200);
        }
    }
    Ok(())
}

// 関門: 曖昧幅の印で続きの行の幅が端末ごとにずれる退行を、選択と確認の表示で守る。
#[test]
fn selection_and_confirmation_use_ascii_continuation_markers()
-> Result<(), Box<dyn std::error::Error>> {
    let mut screen = Screen::new((24, 20), status());
    let row = crate::agent::BranchTreeRow {
        row: crate::session::TreeRow {
            node: Some(1),
            depth: 0,
            request: Some("x".repeat(60)),
            request_lines: 1,
            is_tip: false,
            is_current: true,
        },
        branch: None,
    };
    screen.select_branches(vec![row]);
    let lines = screen
        .selection
        .as_ref()
        .map(|selection| selection.bottom((24, 20)))
        .unwrap_or_default();
    let continuations: Vec<_> = lines
        .iter()
        .filter_map(|line| line.first())
        .filter(|(text, _)| text.as_str() == "| ")
        .collect();
    assert!(!continuations.is_empty());
    for (prefix, _) in continuations {
        assert_eq!(unicode_width::UnicodeWidthStr::width(prefix.as_str()), 2);
        assert_eq!(
            unicode_width::UnicodeWidthStr::width_cjk(prefix.as_str()),
            2
        );
    }
    let mut preview = rewind_preview(true);
    preview.selected.request = Some("long request ".repeat(10));
    let mut screen = Screen::new((100, 30), status());
    screen.rewind_confirmation(preview, None);
    let mut output = Vec::new();
    screen.draw(&mut output, None)?;
    let visible = visible_frame(&output)?;
    assert!(visible.lines().any(|line| line.starts_with("|   ")));
    assert!(!visible.contains('┆'));
    Ok(())
}

// 関門: 実際の窓口の求めを描画し、早いキー、伏せ字、取消、画面の履歴への漏出を捕まえる。
#[test]
fn secret_confirmation_masks_guards_input_and_never_copies_the_prompt_or_value_to_history()
-> Result<(), Box<dyn std::error::Error>> {
    let f = crate::askpass::tests::Fixture::new()?;
    let receiver = f.broker.take_requests().ok_or("missing receiver")?;
    let mut stream = f.request(None, "password\n\x1b[2Jforged fact")?;
    let prompt = receiver.recv_timeout(Duration::from_secs(5))?;
    let mut screen = Screen::new((100, 50), status());
    screen.open_secret(prompt);
    let shown = Instant::now();
    screen.input_at(Event::Paste("queued".into()), shown, Duration::ZERO, true);
    assert!(screen.presented(shown));
    screen.input_at(
        Event::Paste("early".into()),
        shown + Duration::from_millis(299),
        Duration::ZERO,
        true,
    );
    let accepted = shown + Duration::from_millis(301);
    screen.input_at(
        Event::Paste("synthetic-value\n".into()),
        accepted,
        Duration::ZERO,
        true,
    );
    let mut output = Vec::new();
    screen.draw(&mut output, None)?;
    let frame = String::from_utf8(output)?;
    assert!(frame.contains("Helper report (unverified)"));
    assert!(frame.contains("Tool call") && frame.contains("call-17"));
    assert!(frame.contains("Untrusted prompt") && frame.contains("|   password"));
    assert!(frame.contains("\\x1b[2J"));
    assert!(frame.contains("***************"));
    assert!(
        !frame.contains("synthetic-value") && !frame.contains("early") && !frame.contains("queued")
    );
    screen.input_at(
        key(KeyCode::Tab, KeyModifiers::NONE),
        accepted,
        Duration::ZERO,
        true,
    );
    let mut output = Vec::new();
    screen.draw(&mut output, None)?;
    assert!(String::from_utf8(output)?.contains("synthetic-value"));
    screen.input_at(
        key(KeyCode::Enter, KeyModifiers::NONE),
        accepted,
        Duration::ZERO,
        true,
    );
    use std::io::Read as _;
    let mut reply = Vec::new();
    stream.read_to_end(&mut reply)?;
    assert_eq!(reply, b"Ssynthetic-value");
    let mut output = Vec::new();
    screen.draw(&mut output, None)?;
    let history = String::from_utf8(output)?;
    assert!(history.contains("Secret input for call-17: answered"));
    assert!(
        !history.contains("synthetic-value")
            && !history.contains("password")
            && !history.contains("********")
    );
    Ok(())
}

#[test]
fn secret_confirmation_cancels_only_prompt_refuses_hidden_facts_and_closes_when_invalidated()
-> Result<(), Box<dyn std::error::Error>> {
    for operation in ["escape", "small", "invalidate", "confirm"] {
        let f = crate::askpass::tests::Fixture::new()?;
        let receiver = f.broker.take_requests().ok_or("missing receiver")?;
        let mut stream = f.request((operation == "confirm").then_some("confirm"), "untrusted")?;
        let prompt = receiver.recv_timeout(Duration::from_secs(5))?;
        let mut screen = Screen::new(
            if operation == "small" {
                (30, 8)
            } else {
                (100, 50)
            },
            status(),
        );
        screen.open_secret(prompt);
        let shown = Instant::now();
        screen.presented(shown);
        let at = shown + Duration::from_millis(301);
        screen.input_at(Event::Paste("synthetic".into()), at, Duration::ZERO, true);
        if operation == "invalidate" {
            f.call.finish();
            assert!(screen.close_expired_secret());
            assert!(screen.confirming());
            screen.input_at(
                key(KeyCode::Esc, KeyModifiers::NONE),
                at,
                Duration::ZERO,
                true,
            );
        } else if operation == "confirm" {
            screen.input_at(
                key(KeyCode::Char('y'), KeyModifiers::CONTROL),
                at,
                Duration::ZERO,
                true,
            );
            assert!(screen.confirming());
            screen.input_at(
                key(KeyCode::Char('y'), KeyModifiers::NONE),
                at,
                Duration::ZERO,
                true,
            );
        } else {
            if operation == "small" {
                screen.input_at(
                    key(KeyCode::Enter, KeyModifiers::NONE),
                    at,
                    Duration::ZERO,
                    true,
                );
                assert!(screen.confirming());
            }
            assert_eq!(
                screen.input_at(
                    key(KeyCode::Esc, KeyModifiers::NONE),
                    at,
                    Duration::ZERO,
                    true
                ),
                Action::None
            );
        }
        assert!(!screen.confirming());
        use std::io::Read as _;
        let mut reply = Vec::new();
        stream.read_to_end(&mut reply)?;
        assert_eq!(reply, if operation == "confirm" { b"Y" } else { b"N" });
    }
    Ok(())
}

// 作成時の関門: コマンドによる閉鎖の直後の文字を、主の入力・操縦・永続履歴へ
// 渡さない契約。従来の正常回答のテストでは、外から閉じる競合を捕まえられない。
// 実際の窓口を使い、呼び出しの失効と注入した時計だけで順番を決める。
#[test]
fn externally_closed_secret_never_submits_trailing_keystrokes_or_records_history()
-> Result<(), Box<dyn std::error::Error>> {
    for (running, disconnected) in [(true, false), (false, false), (true, true), (false, true)] {
        let f = crate::askpass::tests::Fixture::new()?;
        let receiver = f.broker.take_requests().ok_or("missing receiver")?;
        let stream = f.request(None, "password")?;
        let prompt = receiver.recv()?;
        let mut screen = Screen::new((100, 50), status());
        let paths =
            crate::config::paths::DataPaths::from_values(Some(&f.root), None, None, Some(&f.root))?;
        std::fs::create_dir_all(&paths.data_dir)?;
        screen.load_history(&paths, &f.root);
        screen.open_secret(prompt);
        let shown = Instant::now();
        screen.presented(shown);
        let at = shown + Duration::from_secs(1);
        screen.input_at(Event::Paste("hunt".into()), at, Duration::ZERO, running);
        if disconnected {
            stream.shutdown(std::net::Shutdown::Both)?;
            let _next = f.request(None, "synchronize closure")?;
            drop(receiver.recv()?);
        } else {
            f.call.finish();
        }
        assert!(screen.close_expired_secret());
        assert!(screen.presented(at));
        for ch in "er2!".chars() {
            assert_eq!(
                screen.input_at(
                    key(KeyCode::Char(ch), KeyModifiers::NONE),
                    at,
                    Duration::ZERO,
                    running
                ),
                Action::None
            );
        }
        assert_eq!(
            screen.input_at(
                key(KeyCode::Enter, KeyModifiers::NONE),
                at,
                Duration::ZERO,
                running
            ),
            Action::None
        );
        assert!(screen.input.text.is_empty());
        let (_, entries) = history::History::open(&paths, &f.root)?;
        assert!(entries.is_empty());
        assert!(screen.confirming());
        let mut output = Vec::new();
        screen.draw(&mut output, None)?;
        assert!(String::from_utf8(output)?.contains("Secret input closed by the command"));
        screen.input_at(
            key(KeyCode::Esc, KeyModifiers::NONE),
            at,
            Duration::ZERO,
            running,
        );
        assert!(!screen.confirming());
        assert!(screen.presented(at));
        let early = at + Duration::from_millis(299);
        for event in [
            Event::Paste("late secret".into()),
            key(KeyCode::Enter, KeyModifiers::NONE),
        ] {
            assert_eq!(
                screen.input_at(event, early, Duration::ZERO, running),
                Action::None
            );
        }
        assert!(screen.input.text.is_empty());
        assert!(history::History::open(&paths, &f.root)?.1.is_empty());
    }
    Ok(())
}

// 関門: 上限までの入力と表示切り替えでも、秘密の入力の領域を再確保しない。
#[test]
fn secret_input_reserves_its_limit_and_reports_omitted_untrusted_prompt_lines()
-> Result<(), Box<dyn std::error::Error>> {
    let f = crate::askpass::tests::Fixture::new()?;
    let receiver = f.broker.take_requests().ok_or("missing receiver")?;
    let _stream = f.request(
        None,
        &(1..=13)
            .map(|n| format!("prompt-{n}"))
            .collect::<Vec<_>>()
            .join("\n"),
    )?;
    let prompt = receiver.recv()?;
    let mut screen = Screen::new((120, 60), status());
    screen.open_secret(prompt);
    let at = Instant::now();
    screen.presented(at);
    let accepted = at + Duration::from_secs(1);
    for value in ["x".repeat(2048), "x".repeat(2048)] {
        screen.input_at(Event::Paste(value), accepted, Duration::ZERO, true);
    }
    // 値を戻す・表示する公開の操作で、上限まで受け取ったことも確かめる。
    screen.input_at(
        key(KeyCode::Tab, KeyModifiers::NONE),
        accepted,
        Duration::ZERO,
        true,
    );
    let mut output = Vec::new();
    screen.draw(&mut output, None)?;
    let frame = String::from_utf8(output)?;
    assert!(frame.contains("|   prompt-10"));
    assert!(!frame.contains("|   prompt-11"));
    assert!(frame.contains(text::details_omitted(3).as_str()));
    screen.cancel_confirmation();
    Ok(())
}

// 関門: キーと外からの取消を同じ再開の守りに通す。実時間は待たない。
#[test]
fn cancelled_secret_discards_trailing_input_for_every_cancellation_source()
-> Result<(), Box<dyn std::error::Error>> {
    for source in ["ctrl-c", "ctrl-d", "signal", "escape"] {
        let f = crate::askpass::tests::Fixture::new()?;
        let receiver = f.broker.take_requests().ok_or("missing receiver")?;
        let stream = f.request(None, "password")?;
        let mut screen = Screen::new((100, 50), status());
        screen.open_secret(receiver.recv()?);
        let shown = Instant::now();
        screen.presented(shown);
        let at = shown + Duration::from_secs(1);
        screen.input_at(Event::Paste("hunt".into()), at, Duration::ZERO, true);
        let action = match source {
            "ctrl-c" => screen.input_at(
                key(KeyCode::Char('c'), KeyModifiers::CONTROL),
                at,
                Duration::ZERO,
                true,
            ),
            "ctrl-d" => screen.input_at(
                key(KeyCode::Char('d'), KeyModifiers::CONTROL),
                at,
                Duration::ZERO,
                true,
            ),
            "escape" => screen.input_at(
                key(KeyCode::Esc, KeyModifiers::NONE),
                at,
                Duration::ZERO,
                true,
            ),
            _ => {
                screen.cancel_confirmation_at(at);
                Action::None
            }
        };
        assert_eq!(
            action,
            if source == "ctrl-c" {
                Action::Cancel
            } else {
                Action::None
            }
        );
        assert!(!screen.confirming());
        assert!(screen.presented(at));
        for event in [
            Event::Paste("er2".into()),
            key(KeyCode::Enter, KeyModifiers::NONE),
        ] {
            assert_eq!(
                screen.input_at(event, at + Duration::from_millis(299), Duration::ZERO, true),
                Action::None
            );
        }
        assert!(screen.input.text.is_empty());
        assert_eq!(
            screen.input_at(
                Event::Paste("safe".into()),
                at + Duration::from_millis(300),
                Duration::ZERO,
                true
            ),
            Action::None
        );
        assert_eq!(screen.input.text, "safe");
        drop(stream);
    }
    Ok(())
}

#[test]
fn disconnected_touch_closes_automatically_and_guards_resumed_input()
-> Result<(), Box<dyn std::error::Error>> {
    let f = crate::askpass::tests::Fixture::new()?;
    let receiver = f.broker.take_requests().ok_or("missing receiver")?;
    let stream = f.request(Some("none"), "touch")?;
    let mut screen = Screen::new((100, 50), status());
    screen.open_secret(receiver.recv()?);
    let shown = Instant::now();
    screen.presented(shown);
    stream.shutdown(std::net::Shutdown::Both)?;
    let _next = f.request(None, "synchronize closure")?;
    drop(receiver.recv()?);
    let at = shown + Duration::from_secs(1);
    assert!(screen.close_expired_secret_at(at));
    assert!(!screen.confirming());
    assert!(screen.presented(at));
    assert_eq!(
        screen.input_at(
            Event::Paste("trailing".into()),
            at + Duration::from_millis(299),
            Duration::ZERO,
            true
        ),
        Action::None
    );
    assert!(screen.input.text.is_empty());
    Ok(())
}

// 転送の終了をソケットの EOF で同期し、入力の回答と道具の記録の食い違いを捕まえる。
#[test]
fn secret_history_uses_the_brokers_transfer_result_even_after_disconnect()
-> Result<(), Box<dyn std::error::Error>> {
    use std::io::Read;
    for delivered in [true, false] {
        let f = crate::askpass::tests::Fixture::new()?;
        let receiver = f.broker.take_requests().ok_or("missing receiver")?;
        let mut stream = f.request(None, "password")?;
        let mut screen = Screen::new((100, 50), status());
        screen.open_secret(receiver.recv()?);
        let shown = Instant::now();
        screen.presented(shown);
        let at = shown + Duration::from_secs(1);
        screen.input_at(Event::Paste("synthetic".into()), at, Duration::ZERO, true);
        if !delivered {
            stream.shutdown(std::net::Shutdown::Both)?;
        }
        screen.input_at(
            key(KeyCode::Enter, KeyModifiers::NONE),
            at,
            Duration::ZERO,
            true,
        );
        if delivered {
            let mut answer = Vec::new();
            stream.read_to_end(&mut answer)?;
            assert_eq!(answer, b"Ssynthetic");
        } else {
            let _next = f.request(None, "synchronize transfer")?;
            drop(receiver.recv()?);
            if screen.confirming() {
                screen.input_at(
                    key(KeyCode::Esc, KeyModifiers::NONE),
                    at,
                    Duration::ZERO,
                    true,
                );
            }
        }
        screen.finish_secret_results();
        let mut output = Vec::new();
        screen.draw(&mut output, None)?;
        let frame = String::from_utf8(output)?;
        let events = f.call.events();
        let first = events.first().ok_or("missing outcome")?;
        let expected = first
            .rsplit(": ")
            .next()
            .ok_or("missing outcome name")?
            .trim_end_matches('.');
        assert_eq!(expected == "answered", delivered);
        assert_eq!(frame.matches("Secret input for call-17:").count(), 1);
        assert!(
            frame.contains(&text::secret_history("call-17", expected)),
            "{frame}"
        );
        assert!(!frame.contains("synthetic"));
    }
    Ok(())
}

#[test]
fn revealed_secret_clips_at_terminal_width_and_remasks_without_changing_the_answer()
-> Result<(), Box<dyn std::error::Error>> {
    use std::io::Read;
    let f = crate::askpass::tests::Fixture::new()?;
    let receiver = f.broker.take_requests().ok_or("missing receiver")?;
    let mut stream = f.request(None, "password")?;
    let mut screen = Screen::new((100, 50), status());
    screen.open_secret(receiver.recv()?);
    let shown = Instant::now();
    screen.presented(shown);
    let at = shown + Duration::from_secs(1);
    let value = "sensitive".repeat(20);
    screen.input_at(Event::Paste(value.clone()), at, Duration::ZERO, true);
    screen.input_at(
        key(KeyCode::Tab, KeyModifiers::NONE),
        at,
        Duration::ZERO,
        true,
    );
    let mut shown = Vec::new();
    screen.draw(&mut shown, None)?;
    let frame = visible_frame(&shown)?;
    let row = frame
        .lines()
        .find(|line| line.starts_with("Secret: "))
        .ok_or("missing secret row")?;
    assert!(row.starts_with("Secret: sensitive"));
    assert!(row.chars().count() <= 100);
    assert!(!row.contains(&value));
    screen.input_at(
        key(KeyCode::Tab, KeyModifiers::NONE),
        at,
        Duration::ZERO,
        true,
    );
    let mut masked = Vec::new();
    screen.draw(&mut masked, None)?;
    assert!(!String::from_utf8(masked)?.contains("sensitive"));
    screen.input_at(
        key(KeyCode::Enter, KeyModifiers::NONE),
        at,
        Duration::ZERO,
        true,
    );
    let mut answer = Vec::new();
    stream.read_to_end(&mut answer)?;
    assert_eq!(answer, format!("S{value}").as_bytes());
    Ok(())
}

// 関門: パスの中の区切りの文字で、出所・SHA-256・版を装える退行を守る
// （設計書 01-04「Agent Plugins のプラグイン」の「記録と表示」）。
#[test]
fn plugin_line_puts_the_untrusted_path_last() {
    let spoof = format!("/x; sha256 {}) version 9.9 (user; /p", "0".repeat(64));
    let plugin = crate::session::RecordedPlugin {
        name: "p".into(),
        version: None,
        path: spoof.clone().into(),
        sha256: "a".repeat(64),
        source: crate::session::SkillSource::Project,
    };
    let line = crate::ui::text::plugin_loaded(&plugin);
    assert_eq!(
        line,
        format!(
            "Plugin loaded: p (project; sha256 {}; no version; {spoof})",
            "a".repeat(64)
        )
    );
    let fields = line.find(&spoof).is_some_and(|start| {
        line.get(..start).is_some_and(|prefix| {
            prefix.contains("project; sha256 a") && prefix.contains("no version")
        })
    });
    assert!(fields);
}
