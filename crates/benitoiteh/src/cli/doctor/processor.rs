//! 処理系の探索・形・版・Skill の結果を診断の項目にする（設計書 05-01「doctor」、04-01）。

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use serde_json::Value;

use super::{Item, Status, probe::Probe, text};
use crate::benitoite::skill::{self, SkillDocument, SkillDocuments};
use crate::benitoite::{self, BenitoiteError, Processor, QueryContext, QueryLauncher, display};
use crate::config::paths::DataPaths;
use crate::config::writable::validate_writable_with_executable;
use crate::config::{UserConfig, child_environment};

/// 形を検査した処理系と、規則の検査・問い合わせで共有する方針の入力。
pub(super) struct Located {
    pub(super) path: PathBuf,
    sha256: String,
    pub(super) context: QueryContext,
}

/// 問い合わせをせずに探索と形を検査し、結果の二項目を加える。
pub(super) fn prepare(
    items: &mut Vec<Item>,
    paths: Option<&DataPaths>,
    project: &Path,
    config: Option<&UserConfig>,
    probe: Result<&Probe, &str>,
) -> Result<Located, String> {
    let preparation = (|| {
        let (Some(paths), Some(config)) = (paths, config) else {
            return Err(text::CONFIG_REQUIRED.to_owned());
        };
        let probe = probe.map_err(str::to_owned)?;
        let locations =
            validate_writable_with_executable(paths, project, &probe.write, &[], &probe.helper)
                .map_err(|error| error.to_string())?;
        Ok((
            config,
            QueryContext {
                helper: probe.helper.clone(),
                scratch: probe.scratch(),
                placeholders: probe.placeholders(),
                env: child_environment(
                    std::env::vars_os(),
                    &config.referenced_env_vars(),
                    &probe.write,
                    Some(("restricted", false)),
                ),
                locations,
                denied_candidates: super::super::denied_candidates(paths),
                launcher: QueryLauncher::Sandboxed,
            },
        ))
    })();
    let (config, context) = match preparation {
        Ok(prepared) => prepared,
        Err(reason) => {
            for name in ["benitoite.discovery", "benitoite.executable"] {
                items.push(Item::new(name, Status::Unchecked, &reason));
            }
            return Err(reason);
        }
    };
    let configured = config.benitoite.path.as_deref();
    let found = benitoite::discover(
        configured,
        std::env::var_os("PATH").as_deref(),
        &context.locations,
    );
    items.push(discovery_item(configured, found.as_ref()));
    // 探索の失敗を形の検査の失敗として重ねて示さない。
    let Ok(path) = found else {
        items.push(Item::new(
            "benitoite.executable",
            Status::Unchecked,
            text::PROCESSOR_REQUIRED,
        ));
        return Err(text::PROCESSOR_REQUIRED.into());
    };
    let executable = benitoite::validate_executable(&path, &context.locations, false);
    items.push(executable_item(executable.as_ref()));
    let (path, sha256) = executable.map_err(|_| text::PROCESSOR_REQUIRED.to_owned())?;
    Ok(Located {
        path,
        sha256,
        context,
    })
}

fn discovery_item(configured: Option<&Path>, result: Result<&PathBuf, &BenitoiteError>) -> Item {
    let (status, detail) = match result {
        Ok(path) => (Status::Ok, path.display().to_string()),
        Err(error) => (Status::Unavailable, error.to_string()),
    };
    Item::new(
        "benitoite.discovery",
        status,
        text::discovery(configured, &display::one_line(&detail)),
    )
}

fn executable_item(result: Result<&(PathBuf, String), &BenitoiteError>) -> Item {
    match result {
        Ok((path, hash)) => Item::new(
            "benitoite.executable",
            Status::Ok,
            text::executable(path, hash),
        ),
        Err(error) => Item::new(
            "benitoite.executable",
            Status::Unavailable,
            display::one_line(&error.to_string()),
        ),
    }
}

/// 規則の検査に通ったときだけ、制限モードのサンドボックス内で問い合わせる。
pub(super) fn finish(items: &mut Vec<Item>, located: Result<Located, String>, restricted: bool) {
    let result = located.and_then(|located| {
        if !restricted {
            return Err(text::PROCESSOR_SANDBOX_REQUIRED.into());
        }
        Ok((
            located.sha256,
            Processor::inspect(Some(&located.path), None, located.context),
        ))
    });
    let (expected_sha256, result) = match result {
        Ok(result) => result,
        Err(reason) => {
            for name in ["benitoite.version", "benitoite.skill"] {
                items.push(Item::new(name, Status::Unchecked, &reason));
            }
            return;
        }
    };
    if !inspection_items(
        items,
        &expected_sha256,
        result
            .as_ref()
            .map(|p| (p.version.as_str(), &p.build, p.sha256.as_str())),
    ) {
        return;
    }
    let Ok(processor) = result else {
        return;
    };
    let documents = SkillDocuments::from_processor(Arc::new(processor));
    let skill = documents
        .document("SKILL.md", &AtomicBool::new(false))
        .and_then(|document| {
            skill::skill_parts(document.text())?;
            Ok(document)
        });
    items.push(skill_item(skill.as_deref()));
}

fn inspection_items(
    items: &mut Vec<Item>,
    expected_sha256: &str,
    result: Result<(&str, &Value, &str), &BenitoiteError>,
) -> bool {
    match result {
        Ok((version, build, sha256)) if sha256 == expected_sha256 => {
            items.push(version_item(Ok((version, build))));
            true
        }
        Ok(_) => {
            for name in ["benitoite.version", "benitoite.skill"] {
                items.push(Item::new(
                    name,
                    Status::Unavailable,
                    text::PROCESSOR_CHANGED,
                ));
            }
            false
        }
        Err(error) => {
            items.push(version_item(Err(error)));
            items.push(Item::new(
                "benitoite.skill",
                Status::Unchecked,
                text::PROCESSOR_VERSION_REQUIRED,
            ));
            false
        }
    }
}

fn version_item(result: Result<(&str, &Value), &BenitoiteError>) -> Item {
    let (version, build) = match result {
        Ok(report) => report,
        Err(error) => {
            return Item::new(
                "benitoite.version",
                Status::Unavailable,
                display::one_line(&error.to_string()),
            );
        }
    };
    let field = |name| {
        display::one_line(
            build
                .get(name)
                .and_then(Value::as_str)
                .unwrap_or(text::UNKNOWN),
        )
    };
    let dirty = build
        .get("dirty")
        .and_then(Value::as_bool)
        .map(|dirty| dirty.to_string())
        .unwrap_or_else(|| text::UNKNOWN.into());
    let mut detail = text::version(
        &display::one_line(version),
        &field("commit"),
        &dirty,
        &field("target"),
    );
    let status = if version == benitoite::PROMPT_VERIFIED_VERSION {
        Status::Ok
    } else {
        let warning = benitoite::text::version_warning(&display::one_line(version));
        detail.push_str("; ");
        detail.push_str(warning.strip_prefix("Warning: ").unwrap_or(&warning));
        Status::Warning
    };
    Item::new("benitoite.version", status, detail)
}

fn skill_item(result: Result<&SkillDocument, &BenitoiteError>) -> Item {
    match result {
        Ok(document) => Item::new(
            "benitoite.skill",
            Status::Ok,
            text::skill(document.content_hash()),
        ),
        Err(error) => Item::new(
            "benitoite.skill",
            Status::Unavailable,
            display::one_line(&skill::query_reason(error)),
        ),
    }
}

#[cfg(test)]
mod tests {
    //! 処理系の出力の契約を繰り返さず、doctor の状態と表示への変換を確かめる。

    use super::*;
    use serde_json::json;

    // 作成時の関門: 単体の版の検証では、探索時と問い合わせ時の実行ファイルの
    // 取り違えを捕まえられない。結果の棄却と Skill の問い合わせを止める判断を守る。
    #[test]
    fn changed_executable_rejects_both_version_and_skill_results() {
        let mut items = Vec::new();
        let usable = inspection_items(
            &mut items,
            &"a".repeat(64),
            Ok(("0.0.7", &Value::Null, &"b".repeat(64))),
        );
        assert!(!usable);
        assert_eq!(items.len(), 2);
        for (item, name) in items.iter().zip(["benitoite.version", "benitoite.skill"]) {
            assert_eq!(item.name, name);
            assert!(item.status == Status::Unavailable);
            assert_eq!(item.detail, text::PROCESSOR_CHANGED);
        }
        assert_eq!(
            super::super::exit_status(items.iter().map(|item| item.status)),
            1
        );
    }

    // 作成時の関門: formats の検証は benitoite が持つ。doctor 固有の失敗・警告・不明の
    // 表示と終了状態への変換は、正しい本物の処理系だけでは確かめられない。
    #[test]
    fn version_errors_warnings_and_unknown_build_fields_have_distinct_statuses() {
        let error = BenitoiteError::Invalid(benitoite::text::UNSUPPORTED);
        let item = version_item(Err(&error));
        assert!(item.status == Status::Unavailable);
        assert_eq!(super::super::exit_status([item.status]), 1);
        assert_eq!(item.detail, benitoite::text::UNSUPPORTED);

        for (build, expected) in [
            (
                Value::Null,
                "build.commit=unknown; build.dirty=unknown; build.target=unknown",
            ),
            (
                json!({"commit": null, "dirty": null, "target": 42}),
                "build.commit=unknown; build.dirty=unknown; build.target=unknown",
            ),
            (
                json!({"commit": "abc\nforged", "dirty": false, "target": "a\u{202e}b"}),
                "build.commit=abc\\nforged; build.dirty=false; build.target=a\\u{202e}b",
            ),
            (
                json!({"dirty": true}),
                "build.commit=unknown; build.dirty=true; build.target=unknown",
            ),
        ] {
            let item = version_item(Ok(("0.0.8", &build)));
            assert!(item.status == Status::Warning);
            assert_eq!(super::super::exit_status([item.status]), 0);
            assert!(item.detail.contains(expected), "{}", item.detail);
            assert!(
                item.detail
                    .contains("differs from the prompt-verified version 0.0.7")
            );
        }
        assert!(version_item(Ok(("0.0.7", &Value::Null))).status == Status::Ok);
    }

    // 作成時の関門: サンドボックスの失敗を無視して問い合わせる退行を、問い合わせの
    // 入り口で捕まえる。存在しないパスを置き、起動した結果による失敗と区別する。
    #[test]
    fn failed_rule_leaves_version_and_skill_unchecked_without_querying() {
        let located = Located {
            path: "/missing/benitoite".into(),
            sha256: String::new(),
            context: QueryContext {
                helper: PathBuf::new(),
                scratch: PathBuf::new(),
                placeholders: PathBuf::new(),
                env: Default::default(),
                locations: crate::config::writable::WritableLocations {
                    project: PathBuf::new(),
                    temporary: PathBuf::new(),
                    additional: Vec::new(),
                },
                denied_candidates: Vec::new(),
                launcher: QueryLauncher::Sandboxed,
            },
        };
        let mut items = Vec::new();
        finish(&mut items, Ok(located), false);
        assert_eq!(items.len(), 2);
        for (item, name) in items.iter().zip(["benitoite.version", "benitoite.skill"]) {
            assert_eq!(item.name, name);
            assert!(item.status == Status::Unchecked);
            assert_eq!(item.detail, text::PROCESSOR_SANDBOX_REQUIRED);
        }
    }
}
