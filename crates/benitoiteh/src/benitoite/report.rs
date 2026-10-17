//! 実行前の報告を全体として検証する（設計書 04-01「実行の前の検査の報告」）。

use super::{Effect, diagnostics::Diagnostic, text};
use serde::{Deserialize, Serialize};
mod decode;
use std::collections::BTreeSet;

/// 記録にそのまま残すソースのハッシュ。
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct ContentHash {
    pub algorithm: String,
    pub value: String,
}

/// 確認時に検査した利用者のソース。画面には出さない。
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct ApprovalSource {
    pub display_path: String,
    pub content_hash: ContentHash,
}

/// 報告ごとのシェル参照の位置を選ぶ。
#[derive(Clone, Copy, Debug)]
pub enum ReportKind {
    Run,
    Test,
}
impl ReportKind {
    fn name(self) -> &'static str {
        match self {
            Self::Run => "checkReport",
            Self::Test => "testCheckReport",
        }
    }
}

/// 実行の確認と方針に使ってよい、確定した報告。
#[derive(Debug)]
pub struct InspectionReport {
    pub effects: BTreeSet<Effect>,
    pub shell_reference: bool,
    pub sources: Vec<ApprovalSource>,
}

/// 検査の誤りと、報告自体を確定できない理由。
#[derive(Debug)]
pub enum ReportError {
    Check(Vec<Diagnostic>),
    Invalid(String),
}

/// 終了状態と一つの完全な JSON を照合し、不完全な集合で実行させない。
pub fn parse(
    bytes: &[u8],
    status: Option<i32>,
    kind: ReportKind,
) -> Result<InspectionReport, ReportError> {
    let invalid = |reason: &str| ReportError::Invalid(reason.into());
    let header = serde_json::from_slice::<decode::Object<decode::Header>>(bytes)
        .map_err(|_| invalid(text::REPORT_JSON))?
        .0
        .ok_or_else(|| invalid(text::REPORT_JSON))?;
    if header.schema.unsigned() != Some(1) {
        return Err(invalid(text::REPORT_SCHEMA));
    }
    if header.kind.string() != Some(kind.name()) {
        return Err(invalid(text::REPORT_KIND));
    }
    match header.status.string() {
        Some("checkError") => {
            let check = serde_json::from_slice::<decode::Object<decode::Check>>(bytes)
                .map_err(|_| invalid(text::REPORT_DIAGNOSTICS))?
                .0;
            return Err(ReportError::Check(
                check
                    .and_then(|c| c.diagnostics)
                    .ok_or_else(|| invalid(text::REPORT_DIAGNOSTICS))?,
            ));
        }
        Some("usageError") => {
            let usage = serde_json::from_slice::<decode::Object<decode::Usage>>(bytes)
                .map_err(|_| invalid(text::REPORT_DIAGNOSTICS))?
                .0;
            let message = usage
                .and_then(|u| u.diagnostics.0)
                .and_then(|d| d.first)
                .ok_or_else(|| invalid(text::REPORT_DIAGNOSTICS))?;
            return Err(invalid(&super::diagnostics::one_line(&message)));
        }
        Some("ok") => {}
        _ => return Err(invalid(text::REPORT_STATUS)),
    }
    if status != Some(0) {
        return Err(invalid(&text::skill_exit(status)));
    }
    let success = serde_json::from_slice::<decode::Object<decode::Success>>(bytes)
        .map_err(|_| invalid(text::REPORT_JSON))?
        .0
        .ok_or_else(|| invalid(text::REPORT_JSON))?;
    if success.complete.boolean() != Some(true) {
        return Err(invalid(text::REPORT_INCOMPLETE));
    }
    if success.executed.boolean() != Some(false) {
        return Err(invalid(text::REPORT_EXECUTED));
    }
    let aggregate = success
        .aggregate
        .0
        .ok_or_else(|| invalid(text::REPORT_AGGREGATE))?;
    if aggregate.basis.string() != Some("declaredUpperBound") {
        return Err(invalid(text::REPORT_BASIS));
    }
    let effects = aggregate
        .expanded
        .0
        .ok_or_else(|| invalid(text::REPORT_EFFECTS))?;
    if effects.invalid {
        return Err(invalid(text::REPORT_EFFECTS));
    }
    let targets = success
        .targets
        .0
        .ok_or_else(|| invalid(text::REPORT_TARGETS))?;
    let sources = success
        .sources
        .0
        .ok_or_else(|| invalid(text::REPORT_SOURCES))?;
    if sources.invalid {
        return Err(invalid(text::REPORT_SOURCES));
    }
    Ok(InspectionReport {
        effects: effects.values,
        shell_reference: match kind {
            ReportKind::Run => targets.run,
            ReportKind::Test => targets.test,
        },
        sources: sources.values,
    })
}
