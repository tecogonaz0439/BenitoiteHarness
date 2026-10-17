//! 報告を不完全な集合として使わない契約を守る（設計書 04-01）。

use benitoiteh::benitoite::{
    Effect,
    report::{self, ReportError, ReportKind},
};
use serde_json::{Value, json};

fn valid() -> Value {
    json!({"schemaVersion":1,"kind":"checkReport","status":"ok","complete":true,"executed":false,
        "aggregateEffects":{"basis":"declaredUpperBound","expanded":["State","Console.Write","Assert.Check"]},
        "diagnostics":[{"not":"a diagnostic; ignored when ok"}],
        "targets":[{"main":{"processShellReference":"present"}}],
        "sources":[{"kind":"prelude"},{"kind":"user","displayPath":"main.bnt","contentHash":{"algorithm":"sha256","value":"hash"}}]})
}

// 関門: 純粋な入力境界で条件を一つずつ壊す。エフェクトを空へ読み替える退行は
// サンドボックスが全拒否しただけのテストでは検出できない。
#[test]
fn reports_require_every_authoritative_condition_and_preserve_evidence()
-> Result<(), Box<dyn std::error::Error>> {
    let bytes = serde_json::to_vec_pretty(&valid())?;
    let r = report::parse(&bytes, Some(0), ReportKind::Run).map_err(|e| format!("{e:?}"))?;
    assert_eq!(
        r.effects,
        [Effect::State, Effect::ConsoleWrite, Effect::AssertCheck]
            .into_iter()
            .collect()
    );
    assert!(r.shell_reference);
    assert_eq!(r.sources.len(), 1);
    assert_eq!(
        r.sources
            .first()
            .map(|s| (&*s.display_path, &*s.content_hash.value)),
        Some(("main.bnt", "hash"))
    );
    for (key, value) in [
        ("schemaVersion", json!(2)),
        ("kind", json!("testCheckReport")),
        ("status", json!("unknown")),
        ("complete", json!(false)),
        ("executed", json!(true)),
        ("aggregateEffects", Value::Null),
        (
            "aggregateEffects",
            json!({"basis":"observed","expanded":[]}),
        ),
        (
            "aggregateEffects",
            json!({"basis":"declaredUpperBound","expanded":["IO.All"]}),
        ),
        (
            "aggregateEffects",
            json!({"basis":"declaredUpperBound","expanded":["File.Write","Future.New"]}),
        ),
    ] {
        let mut object = valid();
        *object.get_mut(key).ok_or("field")? = value;
        assert!(
            report::parse(&serde_json::to_vec(&object)?, Some(0), ReportKind::Run).is_err(),
            "{key}: {object}"
        );
    }
    for status in [None, Some(1), Some(2), Some(130)] {
        assert!(report::parse(&bytes, status, ReportKind::Run).is_err());
    }
    for bytes in [
        b"".as_slice(),
        b"{} {}",
        b"[]",
        b"null",
        b"{\"schemaVersion\":",
    ] {
        assert!(report::parse(bytes, Some(0), ReportKind::Run).is_err());
    }
    let mut usage = valid();
    *usage.get_mut("status").ok_or("status")? = json!("usageError");
    *usage.get_mut("diagnostics").ok_or("diagnostics")? =
        json!([{"message":"first reason.\nsecond line"},{"message":"other"}]);
    assert!(
        matches!(report::parse(&serde_json::to_vec(&usage)?, Some(2), ReportKind::Run), Err(ReportError::Invalid(reason)) if reason == "first reason")
    );
    let mut failed = valid();
    *failed.get_mut("status").ok_or("status")? = json!("checkError");
    *failed.get_mut("diagnostics").ok_or("diagnostics")? = json!([{"kind":"check","severity":"error","code":"E0101","message":"missing","primary":null,"secondary":[],"notes":[],"helps":[]}]);
    assert!(
        matches!(report::parse(&serde_json::to_vec(&failed)?, Some(2), ReportKind::Run), Err(ReportError::Check(d)) if d.len() == 1)
    );
    let mut tests = valid();
    *tests.get_mut("kind").ok_or("kind")? = json!("testCheckReport");
    *tests.get_mut("targets").ok_or("targets")? =
        json!([{"processShellReference":"absent"},{"processShellReference":"present"}]);
    let r = report::parse(&serde_json::to_vec(&tests)?, Some(0), ReportKind::Test)
        .map_err(|e| format!("{e:?}"))?;
    assert!(r.shell_reference);
    // 状態ごとに使わない欄は、型を付けて読んだ後も受け入れ条件に加えない。
    let mut loose = valid();
    *loose.get_mut("diagnostics").ok_or("diagnostics")? = json!([0, false, {"unused":[1,2,3]}]);
    loose
        .get_mut("targets")
        .and_then(Value::as_array_mut)
        .ok_or("targets")?
        .extend([
            Value::Null,
            json!(42),
            json!({"main":[0]}),
            json!({"main":{"processShellReference":false}}),
        ]);
    loose
        .get_mut("sources")
        .and_then(Value::as_array_mut)
        .ok_or("sources")?
        .extend([
            Value::Null,
            json!(42),
            json!({"kind":"prelude","displayPath":false,"contentHash":{"algorithm":[]}}),
        ]);
    let r = report::parse(&serde_json::to_vec(&loose)?, Some(0), ReportKind::Run)
        .map_err(|e| format!("{e:?}"))?;
    assert!(r.shell_reference);
    assert_eq!(r.sources.len(), 1);
    for key in [
        "complete",
        "executed",
        "aggregateEffects",
        "targets",
        "sources",
    ] {
        *failed.get_mut(key).ok_or("field")? = json!([0, 1, 2]);
        *usage.get_mut(key).ok_or("field")? = json!([0, 1, 2]);
    }
    assert!(
        matches!(report::parse(&serde_json::to_vec(&failed)?,Some(2),ReportKind::Run),Err(ReportError::Check(d)) if d.len()==1)
    );
    assert!(
        matches!(report::parse(&serde_json::to_vec(&usage)?,Some(2),ReportKind::Run),Err(ReportError::Invalid(reason)) if reason=="first reason")
    );
    // 最上位の欄の並びと、最後の値を採る既存の扱いも変えない。
    let duplicate = valid().to_string().replacen(
        "{",
        "{\"schemaVersion\":0,\"kind\":null,\"status\":false,",
        1,
    );
    assert!(report::parse(duplicate.as_bytes(), Some(0), ReportKind::Run).is_ok());
    Ok(())
}
