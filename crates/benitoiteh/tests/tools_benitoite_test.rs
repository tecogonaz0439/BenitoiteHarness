//! テストの対象の解決、報告、確認の単位を実行器の境界で確かめる（設計書 01-03、03-03）。
#[allow(dead_code)]
#[path = "support/benitoite_run.rs"]
mod fixture;
#[allow(dead_code)]
mod support;
use benitoiteh::{
    approval::{self, ApprovalKind},
    tools::{ApprovalStatus, Invocation, ToolOutput, ToolSet},
};
use fixture::*;
use serde_json::{Value, json};
use std::{collections::BTreeSet, fs, sync::atomic::AtomicBool};
const TEST: &str = "import Benitoite.Assert\n@test(\"example\")\nfunction example() -> Unit uses Assert.Check\n Assert.equal(actual: 1, expected: 1)\nend function\n";

// 関門: Skill のディレクトリを test の対象にしたときも、絶対パスと確認の事実を保つ。
#[test]
fn loaded_skill_test_directory_is_checked_before_requesting_confirmation() -> TestResult {
    let f = Fixture::with_user_skill(TEST)?;
    let directory = f.paths.home.join(".agents/skills/sample");
    let output = call(&f, json!({"paths":[directory]}), false);
    let request = output.approval.as_ref().ok_or(format!("{output:?}"))?;
    assert_eq!(request.facts.targets, std::slice::from_ref(&directory));
    assert_eq!(request.facts.unread_imports, 0);
    assert_eq!(
        request.summary,
        format!("Run tests in {} with effects: none", directory.display())
    );
    Ok(())
}
fn call(f: &Fixture, args: Value, approved: bool) -> ToolOutput {
    let stop = AtomicBool::new(false);
    let call = match f.tools.validate("benitoite_test", args) {
        Ok(c) => c,
        Err(e) => return ToolOutput::error(e.to_string()),
    };
    f.tools.execute_validated(
        &f.context,
        &call,
        if approved {
            ApprovalStatus::Approved
        } else {
            ApprovalStatus::Unapproved
        },
        Invocation {
            call_id: "test",
            cancelled: &stop,
            confirmed_effects: &f.confirmed,
            session_domains: &BTreeSet::new(),
        },
    )
}
// 関門: 指定順の事実と並べ替えた確認の単位を混同せず、検査の誤りと追加した権限を実行前に断る。
#[test]
fn targets_are_canonical_deduplicated_and_confirmed_as_an_unordered_set() -> TestResult {
    let mut f = Fixture::new()?;
    fs::create_dir(f.project.join("tests"))?;
    fs::write(f.project.join("a.bnt"), TEST)?;
    fs::write(f.project.join("tests/b.bnt"), TEST)?;
    std::os::unix::fs::symlink("a.bnt", f.project.join("alias.bnt"))?;
    let first = call(
        &f,
        json!({"paths":["tests","a.bnt","alias.bnt","tests"]}),
        false,
    );
    let request = first
        .approval
        .as_ref()
        .ok_or(format!("request: {first:?}"))?;
    assert_eq!(request.kind, ApprovalKind::TestEffects);
    assert_eq!(
        request.facts.targets,
        vec![f.project.join("tests"), f.project.join("a.bnt")]
    );
    assert!(request.facts.effects.is_empty() && !request.facts.sources.is_empty());
    assert!(request.facts.restricted.is_none());
    assert_eq!(
        request.summary,
        "Run tests in tests, a.bnt with effects: none"
    );
    assert_eq!(
        approval::user_denial_message(request.kind, &request.facts, &f.project, Some("wait")),
        "The user declined to run tests in tests, a.bnt with effects: none. Ask the user how to proceed, or change the tests.\nMessage from the user: wait"
    );
    f.confirm(&first)?;
    let reordered = call(&f, json!({"paths":["a.bnt","tests"]}), false);
    assert!(reordered.approval.is_none(), "{reordered:?}");
    // 順序だけが同じでも別の組には確認を引き継がない。
    assert!(
        call(&f, json!({"paths":["a.bnt"]}), false)
            .approval
            .is_some()
    );
    let changed = TEST
        .replace("uses Assert.Check", "uses Assert.Check, File.Write")
        .replace(
            "import Benitoite.Assert",
            "import Benitoite.Assert\nimport Benitoite.Unofficial.IO.File",
        );
    fs::write(f.project.join("a.bnt"), changed)?;
    let denied = call(&f, json!({"paths":["a.bnt","tests"]}), true);
    assert!(
        denied.is_error
            && denied.text.starts_with("The script changed")
            && denied.processor_launches == 1,
        "{denied:?}"
    );
    let ask = call(&f, json!({"paths":["a.bnt","tests"]}), false);
    assert!(
        ask.approval
            .as_ref()
            .is_some_and(|a| a.facts.added_effects == ["File.Write"]),
        "{ask:?}"
    );
    fs::write(
        f.project.join("bad.bnt"),
        "@test\nfunction broken() -> Unit\n UNKNOWN\nend function",
    )?;
    let bad = call(&f, json!({"paths":["a.bnt","bad.bnt"]}), false);
    assert!(
        bad.is_error
            && bad.approval.is_none()
            && bad.processor_launches == 1
            && bad.text.contains("error["),
        "{bad:?}"
    );
    Ok(())
}

// 関門: テストの報告からも、前の確認を含む書き込み・通信・起動の範囲を欠かさず示す。
#[test]
fn test_restricted_facts_cover_current_and_previously_confirmed_effects() -> TestResult {
    use benitoiteh::{benitoite::Effect, tools::ConfirmKey};
    let mut f = Fixture::new()?;
    f.sandbox()?;
    let run = f.context.run.as_mut().ok_or("run context")?;
    run.allowed_programs = vec!["/usr/bin/sort".into()];
    let processor = run.processor.clone();
    for (uses, write, network, programs) in [
        ("", false, false, false),
        (", Log", false, false, false),
        (", File.Write", true, false, false),
        (", Http.Connect", false, true, false),
        (", Process.Run", false, false, true),
        (", Console.Write", true, true, true),
    ] {
        fs::write(
            f.project.join("scope.bnt"),
            format!(
                "import Benitoite.Unofficial.IO.File\nimport Benitoite.Unofficial.Network.Http\nimport Benitoite.Unofficial.IO.Process\nimport Benitoite.Unofficial.IO.Console\n{}",
                TEST.replace("uses Assert.Check", &format!("uses Assert.Check{uses}"))
            ),
        )?;
        f.confirmed.clear();
        if uses == ", Console.Write" {
            f.confirmed.insert(
                ConfirmKey::Test(vec![f.project.join("scope.bnt")]),
                [Effect::FileWrite, Effect::HttpConnect, Effect::ProcessRun]
                    .into_iter()
                    .collect(),
            );
        }
        let output = call(&f, json!({"paths":["scope.bnt"]}), false);
        let facts = &output.approval.as_ref().ok_or(format!("{output:?}"))?.facts;
        let restricted = facts.restricted.as_ref().ok_or("restricted facts")?;
        let mut writable = Vec::new();
        if write {
            writable.push(f.project.clone());
        }
        writable.push(f.session.temporary_dir().into());
        assert_eq!(restricted.writable, writable, "{uses}");
        assert_eq!(restricted.network, network, "{uses}");
        assert_eq!(restricted.process_run, programs, "{uses}");
        let mut expected_programs = vec![processor.clone()];
        if programs {
            expected_programs.push("/usr/bin/sort".into());
        }
        assert_eq!(restricted.programs, expected_programs, "{uses}");
        assert!(!facts.effects.iter().any(|name| name == "Assert.Check"));
    }
    Ok(())
}
// 関門: 多すぎる対象、拡張子、範囲外の拒否を報告の起動より前に守る。
#[test]
fn paths_are_bounded_readable_test_files_or_directories_and_tool_is_restricted() -> TestResult {
    let f = Fixture::new()?;
    fs::write(f.project.join("plain.txt"), TEST)?;
    fs::write(f.project.join("a.bnt"), TEST)?;
    fs::write(f.project.join(".env.bnt"), TEST)?;
    fs::write(f.root.join("outside.bnt"), TEST)?;
    for args in [
        json!({"paths":[]}),
        json!({"paths":vec!["a.bnt";101]}),
        json!({"paths":["plain.txt"]}),
        json!({"paths":["../outside.bnt"]}),
        json!({"paths":[".env.bnt"]}),
        json!({"paths":["."] ,"timeout":1}),
    ] {
        let out = call(&f, args, false);
        assert!(
            out.is_error && out.approval.is_none() && out.processor_launches == 0,
            "{out:?}"
        );
    }
    for set in [
        ToolSet::new(benitoiteh::config::Mode::Normal),
        ToolSet::without_benitoite(benitoiteh::config::Mode::Restricted),
    ] {
        assert!(
            set.validate("benitoite_test", json!({"paths":["."]}))
                .is_err()
        );
    }
    Ok(())
}

// 関門: 読める入口の値へ除外モジュールの秘密が移っても、毎回その取り込みを確認する。
#[test]
fn excluded_imports_always_require_confirmation_for_both_scripts_and_tests() -> TestResult {
    let mut f = Fixture::new()?;
    fs::create_dir(f.project.join("Secrets"))?;
    fs::write(
        f.project.join("Secrets/Keys.bnt"),
        "public function token() -> String\n return \"SECRET_IMPORT\"\nend function\n",
    )?;
    fs::write(
        f.project.join("main.bnt"),
        "import Secrets.Keys\nfunction main() -> Result[Unit, String]\n return Result.Error(Keys.token())\nend function\n",
    )?;
    fs::write(
        f.project.join("test.bnt"),
        "import Secrets.Keys\nimport Benitoite.Assert\n@test\nfunction example() -> Unit uses Assert.Check\n Assert.equal(actual: Keys.token(), expected: \"\")\nend function\n",
    )?;
    f.exclude(&["Secrets/**"])?;
    assert!(
        f.tools
            .execute(&f.context, "read", json!({"path":"Secrets/Keys.bnt"}))
            .is_error
    );
    let stop = AtomicBool::new(false);
    for tests in [false, true] {
        let first = if tests {
            call(&f, json!({"paths":["test.bnt"]}), false)
        } else {
            f.call(json!({"path":"main.bnt"}), false, "run", &stop)
        };
        let request = first.approval.as_ref().ok_or(format!("{first:?}"))?;
        assert_eq!(request.facts.unread_imports, 1, "{request:?}");
        assert!(!request.summary.contains("Secrets") && !first.text.contains("SECRET_IMPORT"));
        f.confirm(&first)?;
        let again = if tests {
            call(&f, json!({"paths":["test.bnt"]}), false)
        } else {
            f.call(json!({"path":"main.bnt"}), false, "again", &stop)
        };
        assert!(
            again
                .approval
                .as_ref()
                .is_some_and(|r| r.facts.unread_imports == 1),
            "{again:?}"
        );
    }
    Ok(())
}

// 関門: 同じ表示パスの読める囮が、除外ソースの確認を省く退行を実際の報告で捕まえる。
#[test]
fn readable_decoy_never_hides_unread_imports_in_real_reports() -> TestResult {
    for nested_package in [true, false] {
        let mut f = Fixture::new()?;
        support::import_decoy::prepare(&f.project, nested_package)?;
        f.exclude(&["t5/Secret/**"])?;
        let output = call(&f, json!({"paths":["t5"]}), false);
        let request = output.approval.as_ref().ok_or(format!("{output:?}"))?;
        let expected = if nested_package { 1 } else { 2 };
        assert_eq!(
            request.facts.unread_imports, expected,
            "{nested_package}: {request:?}"
        );
        // 下位パッケージがあると本物と囮は表示パスまで同じになる。
        let collisions: Vec<_> = request
            .facts
            .sources
            .iter()
            .filter(|s| std::path::Path::new(&s.display_path).ends_with("t5/Lib/Secret/Keys.bnt"))
            .collect();
        assert_eq!(collisions.len(), 2, "{request:?}");
        let mut hashes = BTreeSet::new();
        for source in collisions {
            hashes.insert(&source.content_hash.value);
        }
        assert_eq!(hashes.len(), 2);
        assert_eq!(output.processor_launches, 1);
        assert!(!output.text.contains("SECRET_HASH_COLLISION"));
        f.confirm(&output)?;
        let again = call(&f, json!({"paths":["t5"]}), false);
        assert!(
            again
                .approval
                .as_ref()
                .is_some_and(|r| r.facts.unread_imports == expected),
            "{again:?}"
        );
    }
    Ok(())
}
