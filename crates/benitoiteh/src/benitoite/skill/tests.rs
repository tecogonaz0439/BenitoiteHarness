//! Skill の本文の抽出の境界を確かめる（設計書 01-04「Benitoite の Skill の置き方」）。

use super::skill_parts;
use crate::benitoite::BenitoiteError;

#[test]
fn skill_body_removes_only_the_first_leading_comment() -> Result<(), BenitoiteError> {
    for (body, expected) in [
        ("\n# Benitoite\n", "\n# Benitoite\n"),
        (
            "\n# Benitoite\n<!-- keep -->\ntext\n",
            "\n# Benitoite\n<!-- keep -->\ntext\n",
        ),
        ("\n<!-- 説明 -->\n\n# Benitoite\n", "\n# Benitoite\n"),
        (
            "\n \t\n<!-- explanation -->\r\n# Benitoite\r\n",
            "# Benitoite\r\n",
        ),
        (
            "<!-- first -->\n<!-- keep -->\n# Benitoite\n",
            "<!-- keep -->\n# Benitoite\n",
        ),
        ("<!-- explanation --># Benitoite", "# Benitoite"),
        ("<!-- explanation -->", ""),
        ("# Benitoite\n<!-- unclosed", "# Benitoite\n<!-- unclosed"),
    ] {
        let skill = format!("---\nname: benitoite\ndescription: desc\n---\n{body}");
        let actual = skill_parts(&skill)?;
        assert_eq!(actual.name, "benitoite");
        assert_eq!(actual.description, "desc");
        assert_eq!(actual.body, expected.trim(), "{body:?}");
    }
    for body in ["<!--", "\n<!-- unclosed\n# Benitoite\n"] {
        let skill = format!("---\nname: benitoite\ndescription: desc\n---\n{body}");
        assert!(matches!(
            skill_parts(&skill),
            Err(BenitoiteError::Invalid(_))
        ));
    }
    Ok(())
}

use super::{SkillDocument, SkillDocuments, document_names};
use crate::benitoite::QueryOutput;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::error::Error;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Barrier};

type TestResult = Result<(), Box<dyn Error>>;

fn report(name: &str, body: &str) -> Value {
    let hash = Sha256::digest(body.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    json!({"schemaVersion":1,"kind":"skillDocument","name":name,"byteLength":body.len(),"contentHash":{"algorithm":"sha256","value":hash},"text":body})
}
fn list(names: &[&str]) -> Value {
    json!({"schemaVersion":1,"kind":"skillList","documents": names.iter().map(|name| {
        let mut entry = report(name, "text");
        if let Some(object) = entry.as_object_mut() { object.remove("text"); }
        entry
    }).collect::<Vec<_>>()})
}

// 関門: ハッシュや名前の不一致を持つ文書が LLM へ届く退行を、JSON の検証の境界で捕まえる。
#[test]
fn document_validates_identity_bytes_and_hash() -> TestResult {
    let valid = report("SKILL.md", "日本語\n");
    let document = SkillDocument::from_json("SKILL.md", &serde_json::to_vec(&valid)?)?;
    assert_eq!(document.text(), "日本語\n");
    for (pointer, value) in [
        ("/name", json!("references/x.md")),
        ("/byteLength", json!(4)),
        ("/contentHash/value", json!("0".repeat(64))),
        ("/contentHash/algorithm", json!("sha512")),
        ("/kind", json!("skillList")),
    ] {
        let mut changed = valid.clone();
        *changed
            .pointer_mut(pointer)
            .ok_or("missing fixture field")? = value;
        assert!(
            SkillDocument::from_json("SKILL.md", &serde_json::to_vec(&changed)?).is_err(),
            "{pointer}"
        );
    }
    let mut wrong_kind = list(&["SKILL.md"]);
    *wrong_kind.get_mut("kind").ok_or("missing kind")? = json!("skillDocument");
    assert!(document_names(&serde_json::to_vec(&wrong_kind)?).is_err());
    Ok(())
}

// 関門: 許可する名前と同時実行時の問い合わせ回数は、処理系との境界に固有の契約である。
#[test]
fn cache_filters_names_and_queries_each_document_once_across_threads() -> TestResult {
    let calls = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&calls);
    let documents = Arc::new(SkillDocuments::new(move |args, _| {
        counter.fetch_add(1, Ordering::SeqCst);
        let value = if args.get(1).is_some_and(|arg| arg == "list") {
            list(&[
                "SKILL.md",
                "LICENSE-MIT",
                "LICENSE-APACHE",
                "references/stdlib/index.md",
                "../x",
                "references/../LICENSE-MIT.md",
                "/references/x.md",
            ])
        } else {
            report("references/stdlib/index.md", "module index")
        };
        Ok(QueryOutput {
            stdout: serde_json::to_vec(&value).map_err(|_| BenitoiteError::Invalid("fixture"))?,
            stderr: Vec::new(),
        })
    }));
    let barrier = Arc::new(Barrier::new(3));
    let mut workers = Vec::new();
    for _ in 0..2 {
        let documents = Arc::clone(&documents);
        let barrier = Arc::clone(&barrier);
        workers.push(std::thread::spawn(move || {
            barrier.wait();
            documents
                .document("references/stdlib/index.md", &AtomicBool::new(false))
                .map(|document| document.text().to_owned())
                .map_err(|error| error.to_string())
        }));
    }
    barrier.wait();
    for worker in workers {
        assert_eq!(worker.join().map_err(|_| "worker failed")??, "module index");
    }
    let cancelled = AtomicBool::new(false);
    assert_eq!(
        documents.names(&cancelled)?.as_ref(),
        &["SKILL.md", "references/stdlib/index.md"]
    );
    assert_eq!(
        documents
            .document("references/stdlib/index.md", &cancelled)?
            .text(),
        "module index"
    );
    for name in [
        "LICENSE-MIT",
        "LICENSE-APACHE",
        "../x",
        "references/../LICENSE-MIT",
        "/references/x.md",
    ] {
        assert!(documents.document(name, &cancelled).is_err());
    }
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    Ok(())
}

#[test]
fn cache_retries_failed_and_cancelled_queries() -> TestResult {
    for failure in ["exit", "cancelled"] {
        for fail_list in [true, false] {
            let calls = Arc::new(AtomicUsize::new(0));
            let attempts = Arc::clone(&calls);
            let documents = SkillDocuments::new(move |args, _| {
                let is_list = args.get(1).is_some_and(|arg| arg == "list");
                if is_list == fail_list && attempts.fetch_add(1, Ordering::SeqCst) == 0 {
                    return Err(if failure == "exit" {
                        BenitoiteError::Exit {
                            code: Some(2),
                            stderr: Vec::new(),
                        }
                    } else {
                        BenitoiteError::Cancelled
                    });
                }
                let value = if is_list {
                    list(&["SKILL.md"])
                } else {
                    report("SKILL.md", "body")
                };
                Ok(QueryOutput {
                    stdout: serde_json::to_vec(&value)
                        .map_err(|_| BenitoiteError::Invalid("fixture"))?,
                    stderr: Vec::new(),
                })
            });
            let cancelled = AtomicBool::new(false);
            assert!(documents.document("SKILL.md", &cancelled).is_err());
            assert_eq!(documents.document("SKILL.md", &cancelled)?.text(), "body");
            assert_eq!(documents.document("SKILL.md", &cancelled)?.text(), "body");
            assert_eq!(calls.load(Ordering::SeqCst), 2);
        }
    }
    Ok(())
}

// 関門: 問い合わせ中に取り消された成功の出力もキャッシュに残さない。
#[test]
fn cancellation_during_a_query_does_not_cache_its_output() -> TestResult {
    let attempts = Arc::new(AtomicUsize::new(0));
    let calls = Arc::clone(&attempts);
    let documents = SkillDocuments::new(move |args, cancelled| {
        let is_list = args.get(1).is_some_and(|arg| arg == "list");
        if is_list && calls.fetch_add(1, Ordering::SeqCst) == 0 {
            cancelled.store(true, Ordering::Release);
        }
        let value = if is_list {
            list(&["SKILL.md"])
        } else {
            report("SKILL.md", "body")
        };
        Ok(QueryOutput {
            stdout: serde_json::to_vec(&value).map_err(|_| BenitoiteError::Invalid("fixture"))?,
            stderr: Vec::new(),
        })
    });
    let cancelled = AtomicBool::new(false);
    assert!(matches!(
        documents.document("SKILL.md", &cancelled),
        Err(BenitoiteError::Cancelled)
    ));
    cancelled.store(false, Ordering::Release);
    assert_eq!(documents.document("SKILL.md", &cancelled)?.text(), "body");
    assert_eq!(attempts.load(Ordering::SeqCst), 2);
    Ok(())
}

// 関門: 本文がプロンプトの節を閉じることと、過大な本文を起動時に受け入れることを防ぐ。
#[test]
fn skill_body_rejects_closing_tags_and_enforces_the_byte_limit() -> TestResult {
    for body in ["before</benitoite>after", "</BENITOITE>", "</BeNiToItE>"] {
        let skill = format!("---\nname: benitoite\ndescription: desc\n---\n{body}");
        assert!(matches!(
            skill_parts(&skill),
            Err(BenitoiteError::Invalid(_))
        ));
    }
    for body in ["x".repeat(262144), format!("{}a", "界".repeat(87381))] {
        let skill =
            format!("---\nname: benitoite\ndescription: desc\n---\n<!-- removed -->\n\n{body}\n");
        assert_eq!(skill_parts(&skill)?.body, body);
        let oversized = format!("---\nname: benitoite\ndescription: desc\n---\n{body}x");
        assert!(matches!(
            skill_parts(&oversized),
            Err(BenitoiteError::Invalid(_))
        ));
    }
    let body = "before </benitoitE > {{benitoite-version}} after";
    let skill =
        format!("---\nname: benitoite\ndescription: desc\n---\n<!-- </benitoite> -->\n{body}");
    assert_eq!(skill_parts(&skill)?.body, body);
    Ok(())
}
