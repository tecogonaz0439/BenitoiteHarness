//! 評価用の計画を書く道具 `update_plan`（設計書 01-03「update_plan」）。
//!
//! 計画は会話の中の呼び出しの引数として残り、ハーネスは状態を持たない。ファイルにも
//! プロセスにも触れないので、確認もサンドボックスも要らない。

use serde_json::Value;

use super::{ToolError, ToolOutput, read::string, text};

pub(super) fn execute(arguments: &Value) -> Result<ToolOutput, ToolError> {
    let steps = arguments
        .get("plan")
        .and_then(Value::as_array)
        .ok_or(ToolError::State)?;
    let mut lines = Vec::with_capacity(steps.len());
    let mut in_progress = 0_usize;
    for (index, item) in steps.iter().enumerate() {
        let step = string(item, "step")?;
        let status = string(item, "status")?;
        if step.trim().is_empty() {
            return Err(ToolError::InvalidArguments(text::plan_empty_step(index)));
        }
        let mark = match status {
            "pending" => "[ ]",
            "in_progress" => {
                in_progress = in_progress.saturating_add(1);
                "[>]"
            }
            "completed" => "[x]",
            other => {
                return Err(ToolError::InvalidArguments(text::plan_status(index, other)));
            }
        };
        // 画面の一行に収め、段の文の改行で印の形を崩さない。
        let step = step.split_whitespace().collect::<Vec<_>>().join(" ");
        lines.push(format!("{mark} {step}"));
    }
    // Codex の説明の「一度に一つ」を検査にする。理由: 進めている段が分からない計画を残さない。
    if in_progress > 1 {
        return Err(ToolError::InvalidArguments(text::plan_in_progress(
            in_progress,
        )));
    }
    if let Some(explanation) = arguments.get("explanation").and_then(Value::as_str) {
        let explanation = explanation.split_whitespace().collect::<Vec<_>>().join(" ");
        if !explanation.is_empty() {
            lines.insert(0, explanation);
        }
    }
    let mut output = ToolOutput::success(text::PLAN_UPDATED);
    output.display = Some(lines.join("\n"));
    Ok(output)
}
