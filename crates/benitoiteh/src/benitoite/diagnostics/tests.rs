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
// 関門: severity を無視する表示、Unicode のバイト数の桁、改行をそのまま出す
// 修正案、別のファイルのパスの欠落を、診断の変換の境界で守る。
#[test]
fn renders_warning_positions_labels_notes_and_edits() {
    let parsed = parse(diagnostic().to_string().as_bytes());
    let prepared = PreparedDiagnostics::new(parsed.diagnostics, &Sources);
    assert_eq!(
        prepared.render(&Sources, None).text,
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
    let no_position =
        PreparedDiagnostics::new(parse(usage.to_string().as_bytes()).diagnostics, &Sources)
            .render(&Sources, None);
    assert!(no_position.text.starts_with("error unused value\n"));
    let mut no_code = error;
    *no_code.get_mut("code").ok_or("field")? = Value::Null;
    *no_code.pointer_mut("/primary/label").ok_or("label")? = json!("");
    let text =
        PreparedDiagnostics::new(parse(no_code.to_string().as_bytes()).diagnostics, &Sources)
            .render(&Sources, None)
            .text;
    assert!(text.starts_with("error a.bnt:1:2: unused value\n"));
    assert!(text.contains("\n    | \t^^\n"));
    for (end_line, end_column, marker) in [(1, 2, "\t^ here"), (2, 1, "\t^^^^^^^ here")] {
        let mut value = diagnostic();
        *value.pointer_mut("/primary/end/line").ok_or("end line")? = json!(end_line);
        *value
            .pointer_mut("/primary/end/column")
            .ok_or("end column")? = json!(end_column);
        let text =
            PreparedDiagnostics::new(parse(value.to_string().as_bytes()).diagnostics, &Sources)
                .render(&Sources, None)
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
        let prepared =
            PreparedDiagnostics::new(parse(lines.join("\n").as_bytes()).diagnostics, &Sources);
        let rendered = prepared.render(&Sources, None);
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
        (0..22)
            .map(|_| json!({"message":"rename", "edits":[edit.clone()]}))
            .collect::<Vec<_>>()
    );
    let output =
        PreparedDiagnostics::new(parse(value.to_string().as_bytes()).diagnostics, &sources)
            .render(&sources, None)
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
