//! Skill の参照を道具の公開の境界で確かめる（設計書 01-03「benitoite_explain と benitoite_reference」）。

#[path = "support/processor.rs"]
mod processor;
#[path = "support/skill.rs"]
mod skill;

use benitoiteh::benitoite::{BenitoiteError, QueryOutput, skill::SkillDocuments};
use benitoiteh::config::{Mode, ReadConfig, paths::DataPaths};
use benitoiteh::safe_fs::ScopeRoot;
use benitoiteh::session::SessionId;
use benitoiteh::tools::{ToolContext, ToolSet, exclude::ExcludeRules};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::error::Error;
use std::fs;
use std::path::PathBuf;
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};

type TestResult = Result<(), Box<dyn Error>>;
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Result<Self, Box<dyn Error>> {
        let root = std::env::temp_dir().join(format!(
            "benitoiteh-skill-tools-{}",
            SessionId::generate(std::time::SystemTime::now())?
        ));
        fs::create_dir(&root)?;
        Ok(Self(fs::canonicalize(root)?))
    }
    fn context(&self, mode: Mode) -> Result<ToolContext, Box<dyn Error>> {
        let paths = DataPaths::from_values(Some(&self.0), None, None, Some(&self.0))?;
        let project = self.0.join("project");
        let temporary = self.0.join("temporary");
        let spill = paths.data_dir.join("sessions/current/spill");
        for path in [&project, &temporary, &spill, &paths.config_dir] {
            fs::create_dir_all(path)?;
        }
        Ok(ToolContext::new(
            mode,
            ScopeRoot::open(&project)?,
            ScopeRoot::open(&temporary)?,
            ScopeRoot::open(&spill)?,
            ExcludeRules::new(
                &project,
                &paths,
                &spill,
                &ReadConfig::default(),
                &ReadConfig::default(),
            )?,
        ))
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _removed = fs::remove_dir_all(&self.0);
    }
}

// 関門: 本物の処理系と道具を結び、名前の公開範囲と SKILL のモード差、長い行を独立に守る。
#[test]
fn references_use_processor_names_and_preserve_long_lines() -> TestResult {
    let f = Fixture::new()?;
    let documents = skill::skills()?;
    for mode in [Mode::Restricted, Mode::Normal] {
        let context = f.context(mode)?.with_skill(Arc::clone(&documents));
        let tools = ToolSet::new(mode);
        let listing = tools.execute(&context, "benitoite_reference", json!({}));
        assert!(!listing.is_error, "{}", listing.text);
        assert!(listing.text.starts_with(if mode == Mode::Normal {
            "Available Benitoite Skill documents (pass one as `name`):\nSKILL\n"
        } else {
            "Available Benitoite Skill documents (pass one as `name`):\n"
        }));
        assert_eq!(
            listing.text.lines().any(|line| line == "SKILL"),
            mode == Mode::Normal
        );
        assert!(listing.text.lines().any(|line| line == "stdlib/index"));
        assert!(!listing.text.contains("LICENSE"));
        for name in [
            "LICENSE-MIT",
            "LICENSE-APACHE",
            "../x",
            "references/../LICENSE-MIT",
            "/references/grammar.md",
            "absent",
        ] {
            let output = tools.execute(&context, "benitoite_reference", json!({"name": name}));
            assert!(output.is_error);
            assert_eq!(
                output.text,
                format!("Unknown Skill document \"{name}\".\n{}", listing.text)
            );
        }
        let index = tools.execute(
            &context,
            "benitoite_reference",
            json!({"name":"stdlib/index"}),
        );
        assert!(!index.is_error);
        assert!(index.text.starts_with("# "));
        let skill = tools.execute(&context, "benitoite_reference", json!({"name":"SKILL"}));
        assert_eq!(skill.is_error, mode == Mode::Restricted);
        if mode == Mode::Normal {
            let expected = include_str!("expected/system-restricted.txt")
                .split_once("# Benitoite\n")
                .ok_or("missing reviewed Skill")?
                .1
                .split_once("\n</benitoite>")
                .ok_or("missing section end")?
                .0;
            assert_eq!(skill.text, format!("# Benitoite\n{expected}\n"));
        }
        let grammar = tools.execute(
            &context,
            "benitoite_reference",
            json!({"name":"grammar", "offset":17,"limit":1}),
        );
        let document = documents.document("references/grammar.md", &AtomicBool::new(false))?;
        let line = document
            .text()
            .lines()
            .nth(16)
            .ok_or("missing grammar line")?;
        assert_eq!(line.chars().count(), 2089);
        assert_eq!(
            grammar.text,
            format!(
                "{line}\n[Showing lines 17-17 of {}. Use offset=18 to continue.]",
                document.text().lines().count()
            )
        );
        let outside = tools.execute(
            &context,
            "benitoite_reference",
            json!({"name":"grammar","offset":1000000}),
        );
        assert!(
            outside.is_error
                && outside
                    .text
                    .starts_with("Offset 1000000 is beyond end of file")
        );
    }
    Ok(())
}

// 関門: 見出しの取り違えと最終節の欠落は、JSON 検証や全文のプロンプトのテストでは捕まらない。
#[test]
fn explain_extracts_exact_diagnostic_sections() -> TestResult {
    let f = Fixture::new()?;
    let documents = skill::skills()?;
    let context = f
        .context(Mode::Restricted)?
        .with_skill(Arc::clone(&documents));
    let tools = ToolSet::new(Mode::Restricted);
    let diagnostics = documents.document("references/diagnostics.md", &AtomicBool::new(false))?;
    let output = tools.execute(&context, "benitoite_explain", json!({"code":"E0405"}));
    assert!(!output.is_error);
    assert!(output.text.starts_with("## E0405\n"));
    assert_eq!(
        output
            .text
            .lines()
            .filter(|line| line.starts_with("## "))
            .count(),
        1
    );
    assert_eq!(
        output.text,
        concat!(
            "## E0405\n\n",
            "- Kind: check (error)\n",
            "- Message: `{op}` cannot be applied to `{ty}`\n",
            "- `allowed`: `{op}` works on {allowed}\n",
            "- `div`: use `div` for integer division, or convert with `Integer.toFloat` before `/`\n",
            "- `type_param`: operators cannot be applied to values of a type parameter\n",
            "- `to_string`: convert the value with `{function}`, or use string interpolation such as `\"${x}\"`\n",
            "- `to_string_any`: convert the other value to a `String` before joining it with `+`, or use string interpolation\n\n",
            "Arithmetic and ordering operators work only on the listed basic types."
        )
    );
    let last = diagnostics.text().rfind("\n## ").ok_or("last heading")?;
    let section = diagnostics
        .text()
        .get(last.saturating_add(1)..)
        .ok_or("last section")?
        .trim_end();
    let code = section
        .lines()
        .next()
        .and_then(|line| line.strip_prefix("## "))
        .ok_or("last code")?;
    assert_eq!(
        tools
            .execute(&context, "benitoite_explain", json!({"code":code}))
            .text,
        section
    );
    for code in ["e0405", "E405", "E04050"] {
        let output = tools.execute(&context, "benitoite_explain", json!({"code":code}));
        assert!(output.is_error);
        assert_eq!(
            output.text,
            "Code must be one uppercase letter followed by four digits, such as E0405."
        );
    }
    let missing = tools.execute(&context, "benitoite_explain", json!({"code":"Z9999"}));
    assert!(missing.is_error);
    assert_eq!(
        missing.text,
        "No section for Z9999 in the diagnostics reference. Check the code, or read `diagnostics` with benitoite_reference."
    );
    Ok(())
}

fn payload(name: &str, body: &str) -> Value {
    let hash = Sha256::digest(body.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    json!({"schemaVersion":1,"kind":"skillDocument","name":name,"byteLength":body.len(),"contentHash":{"algorithm":"sha256","value":hash},"text":body})
}
fn synthetic(body: String) -> Arc<SkillDocuments> {
    Arc::new(SkillDocuments::new(move |args, _| {
        let mut value = payload("references/pages.md", &body);
        if args.get(1).is_some_and(|arg| arg == "list") {
            value = json!({"schemaVersion":1,"kind":"skillList","documents":[value]});
        }
        Ok(QueryOutput {
            stdout: serde_json::to_vec(&value).map_err(|_| BenitoiteError::Invalid("fixture"))?,
            stderr: Vec::new(),
        })
    }))
}

#[test]
fn reference_pages_stop_at_line_and_byte_limits_and_continue() -> TestResult {
    let f = Fixture::new()?;
    let tools = ToolSet::new(Mode::Restricted);
    let context = f
        .context(Mode::Restricted)?
        .with_skill(synthetic("x\n".repeat(2001)));
    for limit in [2000, 9999] {
        let output = tools.execute(
            &context,
            "benitoite_reference",
            json!({"name":"pages", "limit":limit}),
        );
        assert!(!output.is_error);
        assert_eq!(
            output.text,
            format!(
                "{}[Showing lines 1-2000 of 2001. Use offset=2001 to continue.]",
                "x\n".repeat(2000)
            )
        );
    }
    let next = tools.execute(
        &context,
        "benitoite_reference",
        json!({"name":"pages","offset":2001}),
    );
    assert_eq!(next.text, "x\n");
    let body = format!("{}tail\n", format!("{}\n", "x".repeat(1000)).repeat(52));
    let context = f.context(Mode::Restricted)?.with_skill(synthetic(body));
    let first = tools.execute(&context, "benitoite_reference", json!({"name":"pages"}));
    assert_eq!(
        first.text,
        format!(
            "{}[Showing lines 1-51 of 53. Use offset=52 to continue.]",
            format!("{}\n", "x".repeat(1000)).repeat(51)
        )
    );
    assert!(first.text.len() <= 51200);
    let next = tools.execute(
        &context,
        "benitoite_reference",
        json!({"name":"pages", "offset":52}),
    );
    assert_eq!(next.text, format!("{}\ntail\n", "x".repeat(1000)));
    Ok(())
}

#[test]
fn query_errors_are_tool_errors_and_failed_documents_are_retried() -> TestResult {
    let f = Fixture::new()?;
    let tools = ToolSet::new(Mode::Restricted);
    let context = f.context(Mode::Restricted)?;
    for (name, args, document) in [
        ("benitoite_reference", json!({}), "the Skill document list"),
        (
            "benitoite_explain",
            json!({"code":"E0405"}),
            "references/diagnostics.md",
        ),
    ] {
        let output = tools.execute(&context, name, args);
        assert!(output.is_error);
        assert_eq!(
            output.text,
            format!(
                "Could not read {document} from the Benitoite processor: the processor is not available in this session."
            )
        );
    }
    let attempts = Arc::new(AtomicUsize::new(0));
    let calls = Arc::clone(&attempts);
    let docs = SkillDocuments::new(move |args, _| {
        if args.get(1).is_some_and(|arg| arg == "list") {
            return Ok(QueryOutput { stdout: serde_json::to_vec(&json!({"schemaVersion":1,"kind":"skillList","documents":[payload("references/pages.md", "ok")]})).map_err(|_| BenitoiteError::Invalid("fixture"))?, stderr: Vec::new() });
        }
        if calls.fetch_add(1, Ordering::SeqCst) == 0 {
            return Err(BenitoiteError::Exit {
                code: Some(2),
                stderr: b"untrusted".to_vec(),
            });
        }
        Ok(QueryOutput {
            stdout: serde_json::to_vec(&payload("references/pages.md", "ok"))
                .map_err(|_| BenitoiteError::Invalid("fixture"))?,
            stderr: Vec::new(),
        })
    });
    let context = context.with_skill(Arc::new(docs));
    let failed = tools.execute(&context, "benitoite_reference", json!({"name":"pages"}));
    assert!(failed.is_error);
    assert_eq!(
        failed.text,
        "Could not read references/pages.md from the Benitoite processor: benitoite exited with status 2."
    );
    for _ in 0..2 {
        assert_eq!(
            tools
                .execute(&context, "benitoite_reference", json!({"name":"pages"}))
                .text,
            "ok\n"
        );
    }
    assert_eq!(attempts.load(Ordering::SeqCst), 2);
    Ok(())
}

#[test]
fn list_failures_and_output_limits_identify_the_failed_query() -> TestResult {
    let f = Fixture::new()?;
    let tools = ToolSet::new(Mode::Restricted);
    for fail_list in [true, false] {
        let documents = SkillDocuments::new(move |args, _| {
            if !fail_list && args.get(1).is_some_and(|arg| arg == "list") {
                return Ok(QueryOutput { stdout: serde_json::to_vec(&json!({"schemaVersion":1,"kind":"skillList","documents":[payload("references/diagnostics.md", "ignored")]})).map_err(|_| BenitoiteError::Invalid("fixture"))?, stderr: Vec::new() });
            }
            Err(BenitoiteError::OutputLimit)
        });
        let context = f.context(Mode::Restricted)?.with_skill(Arc::new(documents));
        for (tool, args) in [
            ("benitoite_explain", json!({"code":"E0405"})),
            ("benitoite_reference", json!({"name":"diagnostics"})),
        ] {
            let output = tools.execute(&context, tool, args);
            assert!(output.is_error);
            let name = if fail_list {
                "the Skill document list"
            } else {
                "references/diagnostics.md"
            };
            assert_eq!(
                output.text,
                format!(
                    "Could not read {name} from the Benitoite processor: the Skill document from benitoite exceeded 16 MiB."
                )
            );
        }
    }
    Ok(())
}

// 関門: 最初の選択行が出力の上限を超えても、UTF-8 を壊さず次の行へ進める。
#[test]
fn oversized_reference_line_is_marked_and_advances_to_the_next_line() -> TestResult {
    let f = Fixture::new()?;
    let tools = ToolSet::new(Mode::Restricted);
    for (character, count, after) in [
        ('y', 51199, true),
        ('y', 51200, true),
        ('y', 61440, true),
        ('界', 20480, true),
        ('界', 20480, false),
    ] {
        let long = character.to_string().repeat(count);
        let body = format!("short\n{long}{}", if after { "\nafter\n" } else { "\n" });
        let context = f.context(Mode::Restricted)?.with_skill(synthetic(body));
        let output = tools.execute(
            &context,
            "benitoite_reference",
            json!({"name":"pages", "offset":2}),
        );
        assert!(!output.is_error);
        let marker = "\n[Line 2 truncated to fit the 50 KiB output limit.]\n";
        let footer = if after {
            "[Showing lines 2-2 of 3. Use offset=3 to continue.]"
        } else {
            ""
        };
        let kept = output
            .text
            .strip_suffix(&format!("{marker}{footer}"))
            .ok_or("missing truncation marker or advancing continuation")?;
        assert!(!kept.is_empty() && kept.chars().all(|ch| ch == character));
        assert!(kept.len() < long.len());
        assert!(output.text.len() <= 51200);
        assert!(output.text.len().saturating_add(character.len_utf8()) > 51200);
        if after {
            let next = tools.execute(
                &context,
                "benitoite_reference",
                json!({"name":"pages", "offset":3}),
            );
            assert!(!next.is_error);
            assert_eq!(next.text, "after\n");
        }
    }
    // 改行を含めてちょうど 50 KiB の最後の行は、印を付けずに全文を返す。
    for line in ["y".repeat(51199), format!("{}x", "界".repeat(17066))] {
        let expected = format!("{line}\n");
        let context = f
            .context(Mode::Restricted)?
            .with_skill(synthetic(format!("short\n{expected}")));
        let output = tools.execute(
            &context,
            "benitoite_reference",
            json!({"name":"pages", "offset":2}),
        );
        assert!(!output.is_error);
        assert_eq!(output.text, expected);
        assert_eq!(output.text.len(), 51200);
    }
    Ok(())
}
