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
        ApprovalOutcome::Denied => "denied",
        ApprovalOutcome::Cancelled => "cancelled",
    };
    format!("Approval {decision}: {summary}")
}

/// 診断に実行ファイルの名前を添える。
pub fn error(message: &str) -> String {
    format!("benitoiteh: {message}")
}
