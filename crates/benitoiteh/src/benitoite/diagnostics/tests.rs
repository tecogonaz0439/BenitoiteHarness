//! 診断の JSON と表示の契約を純粋な入力で確かめる（設計書 04-01）。

use super::*;
use serde_json::{Value, json};

struct Sources;
impl DiagnosticSources for Sources {
    fn resolve(&self, file: &str) -> DiagnosticFile {
        DiagnosticFile {
            path: file.into(),
            display: Some(file.into()),
            exists: true,
            relative: Some(file.into()),
        }
    }
    fn line(&self, _: &Path, _: u64) -> Option<SourceLine> {
        Some(SourceLine {
            text: "\t変数 + xs".into(),
            truncated: false,
        })
    }
}
fn span(file: &str) -> Value {
    json!({"file": file, "start":{"line":1,"column":2,"offset":1},"end":{"line":1,"column":4,"offset":7},"label":"here"})
}
fn diagnostic() -> Value {
    json!({"kind":"diagnostic", "severity":"warning", "code":"W0501", "message":"unused value", "primary":span("a.bnt"), "secondary":[span("b.bnt")], "notes":["a note"], "helps":[{"message":"rename it", "edits":[{"file":"a.bnt", "start":{"line":1,"column":2,"offset":1},"end":{"line":1,"column":4,"offset":7},"replacement":"x\ny"},{"file":"b.bnt", "start":{"line":2,"column":3,"offset":0},"end":{"line":2,"column":5,"offset":2},"replacement":"z"}]}]})
}

// 関門: パスの可読性が同じでも、異なる内容・不正なハッシュ・不明な方式では確認を省かない。
#[test]
fn unread_imports_require_matching_well_formed_sha256_and_count_distinct_hashes() {
    const ABC: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
    struct ContentSources;
    impl DiagnosticSources for ContentSources {
        fn resolve(&self, file: &str) -> DiagnosticFile {
            Sources.resolve(file)
        }
        fn line(&self, file: &Path, line: u64) -> Option<SourceLine> {
            Sources.line(file, line)
        }
        fn content_hash(&self, _: &Path, _: &AtomicBool) -> Option<String> {
            Some(ABC.into())
        }
    }
    let source = |algorithm: &str, value: &str| super::super::report::ApprovalSource {
        display_path: "same.bnt".into(),
        content_hash: super::super::report::ContentHash {
            algorithm: algorithm.into(),
            value: value.into(),
        },
    };
    let stop = AtomicBool::new(false);
    for (algorithm, value, expected) in [
        ("sha256", ABC.to_owned(), 0),
        ("sha256", "0".repeat(64), 1),
        ("sha512", ABC.to_owned(), 1),
        ("SHA256", ABC.to_owned(), 1),
        ("sha256", ABC.to_uppercase(), 1),
        ("sha256", "0".repeat(63), 1),
        ("sha256", "0".repeat(65), 1),
        ("sha256", "g".repeat(64), 1),
    ] {
        assert_eq!(
            unread_imports(&[source(algorithm, &value)], &ContentSources, &stop),
            expected,
            "{algorithm}: {value}"
        );
    }
    assert_eq!(unread_imports(&[source("sha256", ABC)], &Sources, &stop), 1);
    // 同じパスの先頭の正しいハッシュで、後ろの違うハッシュを隠せない。
    // 全く同じ組の重複は数え直さず、方式の違いも握り潰さない。
    assert_eq!(
        unread_imports(
            &[
                source("sha256", ABC),
                source("sha256", &"0".repeat(64)),
                source("sha256", &"0".repeat(64)),
                source("sha512", ABC),
            ],
            &ContentSources,
            &stop
        ),
        2
    );
}
// 関門: severity を無視する表示、Unicode のバイト数の桁、改行をそのまま出す
// 修正案、別のファイルのパスの欠落を、診断の変換の境界で守る。
#[test]
fn renders_warning_positions_labels_notes_and_edits() {
    let parsed = parse(diagnostic().to_string().as_bytes());
    let prepared = PreparedDiagnostics::new(parsed.diagnostics, &Sources, &AtomicBool::new(false));
    assert_eq!(
        prepared
            .render(&Sources, None, &AtomicBool::new(false))
            .text,
        "warning[W0501] a.bnt:1:2: unused value\n  1 | \t変数 + xs\n    | \t^^ here\n  note: here (b.bnt:1:2)\n  note: a note\n  help: rename it\n    replace 1:2-1:4 with \"x\\ny\"\n    replace b.bnt:2:3-2:5 with \"z\""
    );
}

// 関門: 終了状態 2 の使い方の誤りを修正の回数に入れる退行を表の全分類で守る。
#[test]
fn classifies_status_and_requires_every_typed_field() -> Result<(), Box<dyn std::error::Error>> {
    let warning = diagnostic();
    let mut error = warning.clone();
    *error.get_mut("severity").ok_or("field")? = json!("error");
    let mut discovery = error.clone();
    *discovery.get_mut("primary").ok_or("field")? = Value::Null;
    let mut usage = discovery.clone();
    *usage.get_mut("code").ok_or("field")? = Value::Null;
    for (status, bytes, class) in [
        (0, "".into(), CheckClass::Ok),
        (0, warning.to_string(), CheckClass::Warnings),
        (2, error.to_string(), CheckClass::Failed),
        (2, discovery.to_string(), CheckClass::Failed),
        (
            2,
            usage.to_string(),
            CheckClass::Usage("unused value".into()),
        ),
        (
            2,
            "\nerror: missing main.bnt\nignored".into(),
            CheckClass::Usage("error: missing main.bnt".into()),
        ),
        (
            0,
            error.to_string(),
            CheckClass::Processor("benitoite exited with status 0".into()),
        ),
        (
            2,
            warning.to_string(),
            CheckClass::Processor("benitoite exited with status 2".into()),
        ),
        (
            3,
            "".into(),
            CheckClass::Processor("benitoite exited with status 3".into()),
        ),
    ] {
        assert_eq!(classify_check(status, &parse(bytes.as_bytes())), class);
    }
    for key in [
        "kind",
        "severity",
        "code",
        "message",
        "primary",
        "secondary",
        "notes",
        "helps",
    ] {
        let mut missing = warning.clone();
        missing.as_object_mut().ok_or("object")?.remove(key);
        let mut wrong = warning.clone();
        *wrong.get_mut(key).ok_or("field")? = json!(123);
        for invalid in [missing, wrong] {
            let text = invalid.to_string();
            let parsed = parse(text.as_bytes());
            assert!(parsed.diagnostics.is_empty(), "{text}");
            assert_eq!(parsed.other_lines, [text]);
        }
    }
    let no_position = PreparedDiagnostics::new(
        parse(usage.to_string().as_bytes()).diagnostics,
        &Sources,
        &AtomicBool::new(false),
    )
    .render(&Sources, None, &AtomicBool::new(false));
    assert!(no_position.text.starts_with("error unused value\n"));
    let mut no_code = error;
    *no_code.get_mut("code").ok_or("field")? = Value::Null;
    *no_code.pointer_mut("/primary/label").ok_or("label")? = json!("");
    let text = PreparedDiagnostics::new(
        parse(no_code.to_string().as_bytes()).diagnostics,
        &Sources,
        &AtomicBool::new(false),
    )
    .render(&Sources, None, &AtomicBool::new(false))
    .text;
    assert!(text.starts_with("error a.bnt:1:2: unused value\n"));
    assert!(text.contains("\n    | \t^^\n"));
    for (end_line, end_column, marker) in [(1, 2, "\t^ here"), (2, 1, "\t^^^^^^^ here")] {
        let mut value = diagnostic();
        *value.pointer_mut("/primary/end/line").ok_or("end line")? = json!(end_line);
        *value
            .pointer_mut("/primary/end/column")
            .ok_or("end column")? = json!(end_column);
        let text = PreparedDiagnostics::new(
            parse(value.to_string().as_bytes()).diagnostics,
            &Sources,
            &AtomicBool::new(false),
        )
        .render(&Sources, None, &AtomicBool::new(false))
        .text;
        assert!(text.contains(marker), "{text}");
    }
    Ok(())
}

// 関門: 重複の数を上限に含めることと、警告で誤りが押し出されることを防ぐ。
#[test]
fn deduplicates_orders_errors_first_and_counts_omitted_diagnostics()
-> Result<(), Box<dyn std::error::Error>> {
    for extra in [1usize, 2] {
        let mut lines = vec![diagnostic().to_string(); 3];
        for n in 0..20usize.saturating_add(extra).saturating_sub(1) {
            let mut d = diagnostic();
            *d.get_mut("severity").ok_or("field")? = json!("error");
            *d.get_mut("message").ok_or("field")? = json!(format!("error {n}"));
            lines.push(d.to_string());
        }
        let prepared = PreparedDiagnostics::new(
            parse(lines.join("\n").as_bytes()).diagnostics,
            &Sources,
            &AtomicBool::new(false),
        );
        let rendered = prepared.render(&Sources, None, &AtomicBool::new(false));
        assert_eq!(rendered.selected, 20usize.saturating_add(extra));
        assert!(
            rendered
                .text
                .starts_with("error[W0501] a.bnt:1:2: error 0\n")
        );
        assert!(!rendered.text.contains("warning[W0501]"));
        let expected = if extra == 1 {
            "[1 more diagnostic not shown. Fix the ones above, then check again.]"
        } else {
            "[2 more diagnostics not shown. Fix the ones above, then check again.]"
        };
        assert!(rendered.text.ends_with(expected));
    }
    Ok(())
}

// 関門: 偽の診断に大量の補助位置・置き換えを付けると、同じパスを何度も開き、
// 結果の上限まで処理し続ける。診断の変換の境界で表示と解決の回数を守る。
#[test]
fn secondary_and_edits_are_bounded_and_share_file_resolution()
-> Result<(), Box<dyn std::error::Error>> {
    struct CountingSources(std::cell::Cell<usize>);
    impl DiagnosticSources for CountingSources {
        fn resolve(&self, file: &str) -> DiagnosticFile {
            self.0.set(self.0.get().saturating_add(1));
            Sources.resolve(file)
        }
        fn line(&self, file: &Path, line: u64) -> Option<SourceLine> {
            Sources.line(file, line)
        }
    }
    let sources = CountingSources(std::cell::Cell::new(0));
    let mut value = diagnostic();
    *value.get_mut("secondary").ok_or("secondary")? = json!(vec![span("a.bnt"); 22]);
    let edit = value.pointer("/helps/0/edits/0").ok_or("edit")?.clone();
    // 置き換えの上限は一件の診断全体に掛ける。help を分けても増やせない。
    *value.get_mut("helps").ok_or("helps")? = json!(
        (0..2)
            .map(|_| json!({"message":"rename", "edits":vec![edit.clone();11]}))
            .collect::<Vec<_>>()
    );
    let output = PreparedDiagnostics::new(
        parse(value.to_string().as_bytes()).diagnostics,
        &sources,
        &AtomicBool::new(false),
    )
    .render(&sources, None, &AtomicBool::new(false))
    .text;
    assert_eq!(output.matches("  note: here (a.bnt:1:2)").count(), 20);
    assert_eq!(output.matches("    replace ").count(), 20);
    assert!(
        output.contains("[2 secondary locations not shown.]"),
        "{output}"
    );
    assert!(
        output.contains("[2 suggested edits not shown.]"),
        "{output}"
    );
    assert_eq!(sources.0.get(), 1);
    Ok(())
}

// 関門: 読まないパスの存在だけで診断の件数が変わる情報漏れを共有の変換境界で守る。
#[test]
fn hidden_file_existence_does_not_change_diagnostic_counts()
-> Result<(), Box<dyn std::error::Error>> {
    struct Hidden(bool);
    impl DiagnosticSources for Hidden {
        fn resolve(&self, file: &str) -> DiagnosticFile {
            if file == "a.bnt" {
                Sources.resolve(file)
            } else {
                DiagnosticFile {
                    path: file.into(),
                    display: None,
                    relative: None,
                    exists: self.0,
                }
            }
        }
        fn line(&self, file: &Path, line: u64) -> Option<SourceLine> {
            Sources.line(file, line)
        }
    }
    let mut visible = diagnostic();
    *visible.get_mut("secondary").ok_or("secondary")? = json!([]);
    *visible.get_mut("helps").ok_or("helps")? = json!([]);
    let mut hidden = visible.clone();
    *hidden.get_mut("primary").ok_or("primary")? = span("/home/user/.ssh/id_ed25519");
    let input = format!("{visible}\n{hidden}");
    for exists in [false, true] {
        let sources = Hidden(exists);
        let rendered = PreparedDiagnostics::new(
            parse(input.as_bytes()).diagnostics,
            &sources,
            &AtomicBool::new(false),
        )
        .render(&sources, None, &AtomicBool::new(false));
        assert_eq!(rendered.unread, 1, "exists={exists}");
        assert!(!rendered.text.contains("id_ed25519"));
    }
    Ok(())
}

// 関門: 小さな入力要素で Vec の容量が膨らむ退行を、JSON の読み取り結果で検出する。
#[test]
fn diagnostic_arrays_are_bounded_while_deserializing() -> Result<(), Box<dyn std::error::Error>> {
    for field in ["secondary", "notes", "helps", "edits"] {
        let mut value = diagnostic();
        let pointer = if field == "edits" {
            "/helps/0/edits".to_owned()
        } else {
            format!("/{field}")
        };
        let first = value
            .pointer(&pointer)
            .and_then(Value::as_array)
            .and_then(|a| a.first())
            .ok_or("item")?
            .clone();
        *value.pointer_mut(&pointer).ok_or("field")? = json!(vec![first; 1000]);
        let parsed = parse(value.to_string().as_bytes());
        let d = parsed.diagnostics.first().ok_or("diagnostic")?;
        assert!(
            d.secondary.len() <= 20
                && d.notes.len() <= 20
                && d.helps.len() <= 20
                && d.helps.iter().all(|help| help.edits.len() <= 20),
            "unbounded {field}"
        );
        let omitted = match field {
            "secondary" => d.secondary.omitted,
            "notes" => d.notes.omitted,
            "helps" => d.helps.omitted,
            "edits" => d.helps.first().ok_or("help")?.edits.omitted,
            _ => return Err("unknown field".into()),
        };
        assert_eq!(omitted, 980, "{field}");
        let stop = AtomicBool::new(false);
        let rendered = PreparedDiagnostics::new(parsed.diagnostics, &Sources, &stop)
            .render(&Sources, None, &stop);
        assert!(rendered.text.contains("[980 "), "{}", rendered.text);
    }
    Ok(())
}

// 関門: 主位置だけの上限では補助位置・置き換えを使う DoS が残る。解決に入らないことを確かめる。
#[test]
fn path_limits_cover_primary_secondary_and_edits_before_resolution()
-> Result<(), Box<dyn std::error::Error>> {
    struct Count(std::cell::Cell<usize>);
    impl DiagnosticSources for Count {
        fn resolve(&self, file: &str) -> DiagnosticFile {
            self.0.set(self.0.get().saturating_add(1));
            Sources.resolve(file)
        }
        fn line(&self, file: &Path, line: u64) -> Option<SourceLine> {
            Sources.line(file, line)
        }
    }
    for path in ["x".repeat(4097), "a/".repeat(256) + "a"] {
        for pointer in [
            "/primary/file",
            "/secondary/0/file",
            "/helps/0/edits/0/file",
        ] {
            let mut value = diagnostic();
            *value.pointer_mut(pointer).ok_or("file")? = json!(path);
            let sources = Count(std::cell::Cell::new(0));
            let stop = AtomicBool::new(false);
            let rendered = PreparedDiagnostics::new(
                parse(value.to_string().as_bytes()).diagnostics,
                &sources,
                &stop,
            )
            .render(&sources, None, &stop);
            assert_eq!(sources.0.get(), 0, "{pointer}");
            assert_eq!(rendered.unread, 1);
            assert!(rendered.text.is_empty());
        }
    }
    for path in ["x".repeat(4096), "a/".repeat(255) + "a"] {
        let mut value = diagnostic();
        *value.pointer_mut("/primary/file").ok_or("primary")? = json!(path);
        let sources = Count(std::cell::Cell::new(0));
        let stop = AtomicBool::new(false);
        let _prepared = PreparedDiagnostics::new(
            parse(value.to_string().as_bytes()).diagnostics,
            &sources,
            &stop,
        );
        assert_eq!(sources.0.get(), 1);
    }
    Ok(())
}

// 関門: 位置の解決は先頭千件で止め、途中の取り消しも次の解決より前に受け取る。
#[test]
fn resolution_limit_and_cancellation_stop_further_source_access()
-> Result<(), Box<dyn std::error::Error>> {
    struct Count<'a> {
        calls: std::cell::Cell<usize>,
        stop: Option<&'a AtomicBool>,
    }
    impl DiagnosticSources for Count<'_> {
        fn resolve(&self, file: &str) -> DiagnosticFile {
            self.calls.set(self.calls.get().saturating_add(1));
            if let Some(stop) = self.stop {
                stop.store(true, Ordering::Release);
            }
            Sources.resolve(file)
        }
        fn line(&self, file: &Path, line: u64) -> Option<SourceLine> {
            Sources.line(file, line)
        }
    }
    let input = (0..1005)
        .map(|i| {
            let mut d = diagnostic();
            if let Some(file) = d.pointer_mut("/primary/file") {
                *file = json!(format!("file-{i}.bnt"));
            }
            d.to_string()
        })
        .collect::<Vec<_>>()
        .join("\n");
    let stop = AtomicBool::new(false);
    let sources = Count {
        calls: std::cell::Cell::new(0),
        stop: None,
    };
    let prepared = PreparedDiagnostics::new(parse(input.as_bytes()).diagnostics, &sources, &stop);
    assert_eq!(sources.calls.get(), 1000);
    let rendered = prepared.render(&sources, None, &stop);
    assert!(rendered.text.contains("5 diagnostics were not inspected"));
    let stop = AtomicBool::new(false);
    let sources = Count {
        calls: std::cell::Cell::new(0),
        stop: Some(&stop),
    };
    let prepared = PreparedDiagnostics::new(parse(input.as_bytes()).diagnostics, &sources, &stop);
    assert_eq!(sources.calls.get(), 1);
    let rendered = prepared.render(&sources, None, &stop);
    assert_eq!(sources.calls.get(), 1);
    assert!(rendered.text.contains("cancelled"));
    struct CancelLine<'a> {
        stop: &'a AtomicBool,
        calls: std::cell::Cell<usize>,
    }
    impl DiagnosticSources for CancelLine<'_> {
        fn resolve(&self, file: &str) -> DiagnosticFile {
            self.calls.set(self.calls.get().saturating_add(1));
            Sources.resolve(file)
        }
        fn line(&self, file: &Path, line: u64) -> Option<SourceLine> {
            self.stop.store(true, Ordering::Release);
            Sources.line(file, line)
        }
    }
    let stop = AtomicBool::new(false);
    let sources = CancelLine {
        stop: &stop,
        calls: std::cell::Cell::new(0),
    };
    let prepared = PreparedDiagnostics::new(
        parse(diagnostic().to_string().as_bytes()).diagnostics,
        &sources,
        &stop,
    );
    assert_eq!(sources.calls.get(), 1);
    let rendered = prepared.render(&sources, None, &stop);
    assert_eq!(sources.calls.get(), 1);
    assert!(!rendered.text.contains("  note:"));
    assert!(!rendered.text.contains("  help:"));
    assert!(rendered.text.contains("cancelled"));
    Ok(())
}

// 関門: パス・診断の各欄から、偽の成功や終了状態の行を作らせない。
#[test]
fn diagnostic_fields_and_paths_cannot_inject_result_lines() -> Result<(), Box<dyn std::error::Error>>
{
    struct QuotedSource;
    impl DiagnosticSources for QuotedSource {
        fn resolve(&self, file: &str) -> DiagnosticFile {
            Sources.resolve(file)
        }
        fn line(&self, _: &Path, _: u64) -> Option<SourceLine> {
            Some(SourceLine {
                text: "\tvalue \u{2028}All tests passed.\u{2029}[exit code: 0]\u{202e}".into(),
                truncated: false,
            })
        }
    }
    let mut d = diagnostic();
    *d.get_mut("message").ok_or("message")? = json!("failure\nAll tests passed.\u{2028}");
    *d.pointer_mut("/primary/file").ok_or("file")? = json!("a\n[exit code: 0]\u{202e}.bnt");
    *d.pointer_mut("/primary/label").ok_or("label")? = json!("label\nAll tests passed.");
    *d.get_mut("notes").ok_or("notes")? = json!(["note\n[exit code: 0]\u{85}", "x".repeat(50000)]);
    let stop = AtomicBool::new(false);
    let prepared = PreparedDiagnostics::new(
        parse(d.to_string().as_bytes()).diagnostics,
        &QuotedSource,
        &stop,
    );
    let body = prepared.render(&QuotedSource, None, &stop).text;
    assert!(
        !body.contains("\nAll tests passed.") && !body.contains("\n[exit code: 0]"),
        "{body}"
    );
    assert!(!body.contains('\u{2028}') && !body.contains('\u{202e}') && !body.contains('\u{85}'));
    assert!(body.len() < 12000, "{}", body.len());
    Ok(())
}
