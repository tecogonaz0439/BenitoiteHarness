//! ループの診断と LLM に返す固定の結果（設計書 01-01「番の流れ」「取り消し」「同じ呼び出しの繰り返し」）。

pub const OUTPUT_LIMIT: &str = "Tool call was not executed: the response hit the output token limit, so its arguments may be truncated. Re-issue the tool call with complete arguments.";
pub const CANCELLED: &str = "Tool call was cancelled by the user.";
pub const COMPLETED: &str = "The run completed.";
pub const RUN_CANCELLED: &str =
    "The run was cancelled. Effects of tools that already ran have not been undone.";
pub const TURN_LIMIT: &str = "The run reached the maximum number of turns.";
pub const TOKEN_LIMIT: &str = "The run reached the maximum number of tokens.";
pub const CONTEXT_OVERFLOW: &str =
    "The provider reported a context overflow. Context compaction is not implemented in P1.";
pub const INVALID_CONFIGURATION: &str =
    "The agent configuration does not match its tool context or provider URL.";
pub const CHANNEL_CLOSED: &str = "The agent event receiver was closed.";
pub const WORKER_FAILED: &str = "An agent worker failed.";
pub const INVALID_ARGUMENTS: &str = "Tool arguments are not valid JSON.";
pub const RANDOM_FAILED: &str = "Could not obtain randomness for retry timing.";

pub fn refusal(category: &str) -> String {
    format!("The provider refused the response (category: {category}).")
}

pub fn repeated(count: u64) -> String {
    let ordinal = match count {
        3 => "3rd",
        5 => "5th",
        _ => "8th",
    };
    format!(
        "[note: this is the {ordinal} identical call. Its result will not change unless something else changes. Try a different approach.]"
    )
}
