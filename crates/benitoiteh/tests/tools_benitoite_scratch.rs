//! 一時スクリプトの作成と報告の判定を実行器で守る（設計書 01-03、03-03、04-01）。
#[allow(dead_code)]
#[path = "support/benitoite_run.rs"]
mod fixture;
#[allow(dead_code)]
mod support;
use benitoiteh::approval::ApprovalKind;
use benitoiteh::benitoite::Effect;
use benitoiteh::config::Mode;
use benitoiteh::tools::{ConfirmKey, ToolSet};
use fixture::*;
use serde_json::json;
use std::fs;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::sync::atomic::AtomicBool;

const CONSOLE: &str = "import Benitoite.Unofficial.IO.Console\nfunction main() -> Unit uses Console.Write\n Console.write(\"scratch output\")\nend function\n";
const READ: &str = "import Benitoite.Unofficial.IO.File\nfunction main() -> Unit uses File.Read\n bind _ <- File.readText(\"input\")\nend function\n";

// 関門: 既存のリンク・消された名前の再利用・呼び直しで別のファイルを作る退行を守る。
#[test]
fn new_names_never_overwrite_or_follow_links_and_approved_calls_reuse_the_path() -> TestResult {
    let mut f = Fixture::new()?;
    let stop = AtomicBool::new(false);
    let temp = f.session.temporary_dir().to_owned();
    let target = f.root.join("outside");
    fs::write(&target, "unchanged")?;
    symlink(&target, temp.join("scratch-1.bnt"))?;
    let first = f.call_tool(
        "benitoite_scratch",
        json!({"code":READ}),
        false,
        "one",
        &stop,
    );
    let request = first
        .approval
        .as_ref()
        .ok_or_else(|| format!("{first:?}"))?;
    assert_eq!(request.kind, ApprovalKind::Effects);
    assert_eq!(request.facts.targets, [temp.join("scratch-2.bnt")]);
    assert_eq!(request.facts.effects, ["File.Read"]);
    assert_eq!(request.facts.unread_imports, 0);
    assert_eq!(
        first.text.lines().next(),
        temp.join("scratch-2.bnt").to_str()
    );
    assert_eq!(fs::read_to_string(&target)?, "unchanged");
    assert_eq!(
        fs::metadata(temp.join("scratch-2.bnt"))?
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
    f.confirm(&first)?;
    let approved = f.call_tool(
        "benitoite_scratch",
        json!({"code":READ}),
        true,
        "one",
        &stop,
    );
    assert!(
        approved.approval.is_none() && approved.text.contains("execution context is unavailable"),
        "{approved:?}"
    );
    assert!(!temp.join("scratch-3.bnt").exists());
    fs::write(
        temp.join("scratch-2.bnt"),
        "import Benitoite.Unofficial.IO.File\nfunction main() -> Unit uses File.Write\n bind _ <- File.writeText(path: \"changed\", text: \"bad\")\nend function\n",
    )?;
    let changed = f.call_tool(
        "benitoite_scratch",
        json!({"code":READ}),
        true,
        "one",
        &stop,
    );
    assert!(
        changed.is_error
            && changed.text.contains("The script changed")
            && changed.processor_launches == 1,
        "{changed:?}"
    );
    fs::remove_file(temp.join("scratch-2.bnt"))?;
    let second = f.call_tool(
        "benitoite_scratch",
        json!({"code":READ}),
        false,
        "two",
        &stop,
    );
    assert_eq!(
        second.approval.as_ref().ok_or("second")?.facts.targets,
        [temp.join("scratch-3.bnt")]
    );
    // 同じ実パスに前の確認があっても、初回は自動の集合だけで判定する。
    f.confirmed.insert(
        ConfirmKey::Run(temp.join("scratch-4.bnt")),
        [Effect::FileRead].into_iter().collect(),
    );
    let third = f.call_tool(
        "benitoite_scratch",
        json!({"code":READ}),
        false,
        "three",
        &stop,
    );
    assert!(third.approval.is_some(), "{third:?}");
    fs::remove_file(temp.join("scratch-4.bnt"))?;
    let missing = f.call_tool(
        "benitoite_scratch",
        json!({"code":READ}),
        true,
        "three",
        &stop,
    );
    assert!(missing.is_error && !temp.join("scratch-4.bnt").exists());
    let unknown = f.call_tool(
        "benitoite_scratch",
        json!({"code":READ}),
        true,
        "unknown",
        &stop,
    );
    assert!(unknown.is_error && unknown.processor_launches == 0);
    Ok(())
}

// 関門: 読み返せないコードを作らず、名前の衝突の探索を有限にし、モードの境界を守る。
#[test]
fn invalid_input_and_exhausted_names_do_not_write_scripts() -> TestResult {
    let f = Fixture::new()?;
    let stop = AtomicBool::new(false);
    for code in ["\0".into(), " ".repeat(64 * 1024 * 1024 + 1)] {
        let out = f.call_tool(
            "benitoite_scratch",
            json!({"code":code}),
            false,
            "invalid",
            &stop,
        );
        assert!(
            out.is_error && out.processor_launches == 0 && out.text.contains("64 MiB"),
            "{out:?}"
        );
        assert_eq!(fs::read_dir(f.session.temporary_dir())?.count(), 0);
    }
    for mode in [Mode::Normal, Mode::Restricted] {
        let tools = if mode == Mode::Normal {
            ToolSet::new(mode)
        } else {
            ToolSet::without_benitoite(mode)
        };
        assert!(
            tools
                .validate("benitoite_scratch", json!({"code":PURE}))
                .is_err()
        );
    }
    assert!(
        f.tools
            .validate("benitoite_scratch", json!({"code":PURE,"timeout":1}))
            .is_err()
    );
    for number in 1..=128 {
        fs::write(
            f.session
                .temporary_dir()
                .join(format!("scratch-{number}.bnt")),
            "keep",
        )?;
    }
    let out = f.call_tool(
        "benitoite_scratch",
        json!({"code":PURE}),
        false,
        "collisions",
        &stop,
    );
    assert!(
        out.is_error && out.text.contains("128 attempts") && out.processor_launches == 0,
        "{out:?}"
    );
    assert_eq!(
        fs::read_to_string(f.session.temporary_dir().join("scratch-1.bnt"))?,
        "keep"
    );
    let out = f.call_tool(
        "benitoite_scratch",
        json!({"code":READ}),
        false,
        "next",
        &stop,
    );
    assert_eq!(
        out.approval.as_ref().ok_or("next")?.facts.targets,
        [f.session.temporary_dir().join("scratch-129.bnt")]
    );
    Ok(())
}

// 関門: 一時スクリプト自身は読める。プロジェクトの暗黙の import は検査で拒み、結果にパスを保つ。
#[test]
fn automatic_effects_and_import_roots_use_the_real_inspection_report() -> TestResult {
    let f = Fixture::new()?;
    let stop = AtomicBool::new(false);
    for code in [PURE, CONSOLE] {
        let out = f.call_tool(
            "benitoite_scratch",
            json!({"code":code}),
            false,
            "automatic",
            &stop,
        );
        assert!(
            out.approval.is_none() && out.text.contains("execution context is unavailable"),
            "{out:?}"
        );
        assert_eq!(out.processor_launches, 1);
    }
    let log = f.call_tool(
        "benitoite_scratch",
        json!({"code":"function main() -> Unit uses Log\n return ()\nend function\n"}),
        false,
        "log",
        &stop,
    );
    assert_eq!(
        log.approval
            .as_ref()
            .ok_or(format!("{log:?}"))?
            .facts
            .effects,
        ["Log"]
    );
    fs::write(
        f.project.join("Local.bnt"),
        "public function value() -> Unit\n return ()\nend function\n",
    )?;
    let out = f.call_tool(
        "benitoite_scratch",
        json!({"code":"import Local\nfunction main() -> Unit\n Local.value()\nend function\n"}),
        false,
        "import",
        &stop,
    );
    assert!(
        out.is_error && out.approval.is_none() && out.text.contains("E0318"),
        "{out:?}"
    );
    assert_eq!(
        out.text.lines().next(),
        f.session.temporary_dir().join("scratch-4.bnt").to_str()
    );
    Ok(())
}

// 関門: コンパイル時に取り込む秘密は実行時の狭い方針では防げない。外・除外・BH-029 の囮を守る。
#[test]
fn unread_dependencies_always_require_confirmation_even_with_automatic_effects() -> TestResult {
    for case in ["outside", "excluded", "decoy-package", "decoy-root"] {
        let mut f = Fixture::new()?;
        let stop = AtomicBool::new(false);
        let (dependency, module) = if case.starts_with("decoy") {
            support::import_decoy::prepare(&f.project, case == "decoy-package")?;
            fs::write(
                f.project.join("t5/Lib/T.bnt"),
                "import Secret.Keys\npublic function token() -> String\n return Keys.token()\nend function\n",
            )?;
            f.exclude(&["t5/Secret/**"])?;
            (f.project.join("t5"), "Lib.T")
        } else {
            let path = if case == "outside" {
                f.root.join("outside")
            } else {
                f.project.join("Secrets")
            };
            fs::create_dir(&path)?;
            fs::write(
                path.join("Keys.bnt"),
                "public function token() -> String\n return \"SECRET_VALUE\"\nend function\n",
            )?;
            f.exclude(&["Secrets/**"])?;
            (path, "Keys")
        };
        fs::write(
            dependency.join("package.bnt"),
            "language \"0.0.12\"\npackage Ext\nend package\n",
        )?;
        let alias = module.rsplit('.').next().ok_or("module")?;
        let source = format!(
            "dependency Ext\n path {}\nend dependency\nimport Ext.{module}\nimport Benitoite.Unofficial.IO.Console\nfunction main() -> Unit uses Console.Write\n Console.write({alias}.token())\nend function\n",
            json!(dependency)
        );
        let out = f.call_tool(
            "benitoite_scratch",
            json!({"code":source}),
            false,
            case,
            &stop,
        );
        let request = out
            .approval
            .as_ref()
            .ok_or_else(|| format!("{case}: {out:?}"))?;
        assert!(request.facts.unread_imports > 0, "{case}: {request:?}");
        assert_eq!(request.facts.effects, ["Console.Write"]);
        assert!(!out.text.contains("SECRET_VALUE") && !out.text.contains("SECRET_HASH_COLLISION"));
        f.confirm(&out)?;
        let approved = f.call_tool(
            "benitoite_scratch",
            json!({"code":source}),
            true,
            case,
            &stop,
        );
        assert!(
            approved.approval.is_none()
                && approved.text.contains("execution context is unavailable"),
            "{case}: {approved:?}"
        );
    }
    Ok(())
}
