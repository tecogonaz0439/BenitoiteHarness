//! exec の進捗を示す英文（設計書 05-01「exec の出力」）。

use crate::approval::ApprovalOutcome;

/// 呼び出しの名前と引数を示す。
pub fn tool_call(name: &str, arguments: &serde_json::Value) -> String {
    format!("{name} {arguments}")
}

/// 結果の一行と成否を示す。
pub fn tool_result(name: &str, is_error: bool, summary: &str) -> String {
    let status = if is_error { "error" } else { "ok" };
    format!("{name}: {status}: {summary}")
}

/// 対象の操作と確認の結果を示す。
pub fn approval(summary: &str, decision: ApprovalOutcome) -> String {
    let decision = match decision {
        ApprovalOutcome::Approved => "approved",
        ApprovalOutcome::ApprovedForSession => "approved for this session",
        ApprovalOutcome::DeniedWithMessage => "denied with a message",
        ApprovalOutcome::Denied => "denied",
        ApprovalOutcome::Cancelled => "cancelled",
    };
    format!("Approval {decision}: {summary}")
}

/// 診断に実行ファイルの名前を添える。
pub fn error(message: &str) -> String {
    format!("benitoiteh: {message}")
}

/// 文脈に入れた指示の探索時のパスを示す。
pub fn instruction_loaded(path: &std::path::Path) -> String {
    format!("Instruction file loaded: {}", path.display())
}

/// 読まなかった指示と除外理由を示す。
pub fn instruction_skipped(path: &std::path::Path, reason: &str) -> String {
    format!("Instruction file skipped: {}: {reason}", path.display())
}

/// 圧縮で置き換えた項目と残した項目を示す。
pub fn compaction(
    reason: crate::session::CompactionReason,
    summarized: usize,
    kept: usize,
) -> String {
    format!(
        "Compacted the conversation ({}): {summarized} items summarized, {kept} kept.",
        reason.as_str()
    )
}

/// セッションの一覧の正常な一行を作る。
pub fn session_entry(
    id: &str,
    started: &str,
    mode: &str,
    project: &str,
    request: &str,
    active: bool,
) -> String {
    let active = if active { " [active]" } else { "" };
    format!("{id}  {started}  {mode}  {project}  {request}{active}")
}

/// 記録のモードを一覧に示す綴りにする。
pub fn session_mode(mode: Option<crate::session::RecordedMode>) -> &'static str {
    match mode {
        Some(crate::session::RecordedMode::Restricted) => "restricted",
        Some(crate::session::RecordedMode::Normal) => "normal",
        None => "",
    }
}

/// 読めない記録の ID と理由を示す。
pub fn session_error(id: &str, error: &str) -> String {
    format!("{id}  {error}")
}

pub fn checkpoint(checkpoint: &crate::rewind::Checkpoint) -> String {
    if checkpoint.incomplete.is_empty() {
        format!("Checkpoint {} recorded.", checkpoint.id)
    } else {
        format!(
            "Checkpoint {} is incomplete: {}",
            checkpoint.id,
            checkpoint.incomplete.join("; ")
        )
    }
}
pub fn hard_links(paths: &[std::path::PathBuf]) -> String {
    format!(
        "Files with multiple hard links: {}",
        paths
            .iter()
            .map(|path| path.display().to_string())
            .collect::<Vec<_>>()
            .join(", ")
    )
}

/// 切り替えた実のパスを一行で知らせる。
pub fn project_changed(path: &std::path::Path) -> String {
    format!(
        "Project changed to {}.",
        super::terminal_text(&path.to_string_lossy(), false)
    )
}
