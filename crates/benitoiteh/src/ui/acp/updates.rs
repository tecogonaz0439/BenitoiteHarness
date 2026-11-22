//! `AgentEvent`・会話履歴・設定項目を ACP 版 1 の `session/update` と `configOptions` に変換する
//! （設計書 05-03「ACP 版 1 のメソッドと通信の契約」「イベントから `session/update` への変換」）。

use std::collections::BTreeMap;

use serde_json::{Value, json};

use crate::agent::AgentEvent;
use crate::approval::ApprovalOutcome;
use crate::provider::{AssistantPart, Conversation, Item, UserPartKind};

use super::text;

/// 道具の呼び出しの状態遷移（`pending` → `in_progress` → `completed`/`failed`）を追う。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum ToolState {
    Pending,
    InProgress,
    Declined,
}

/// `AgentEvent` を `session/update` の `update` 配列と標準エラー出力向けの診断へ変換する。
pub(super) struct EventTranslation {
    pub updates: Vec<Value>,
    pub diagnostics: Vec<String>,
}

/// 実行中のイベント列を ACP 版 1 の `session/update` と診断へ変換する。
pub(super) fn translate_event(
    event: &AgentEvent,
    tool_states: &mut BTreeMap<String, ToolState>,
    context_window: Option<u64>,
    current_estimate: &mut u64,
) -> EventTranslation {
    let mut updates = Vec::new();
    let mut diagnostics = Vec::new();
    match event {
        AgentEvent::TextDelta { text, .. } => {
            updates.push(json!({
                "sessionUpdate": "agent_message_chunk",
                "content": { "type": "text", "text": text },
            }));
        }
        AgentEvent::ReasoningDelta { text, .. } => {
            updates.push(json!({
                "sessionUpdate": "agent_thought_chunk",
                "content": { "type": "text", "text": text },
            }));
        }
        AgentEvent::ToolCall {
            call_id,
            name,
            arguments,
            ..
        } => {
            updates.push(json!({
                "sessionUpdate": "tool_call",
                "toolCallId": call_id,
                "title": tool_call_title(name, arguments),
                "kind": tool_call_kind(name),
                "status": "pending",
                "rawInput": arguments,
            }));
            if tool_never_needs_approval(name) {
                updates.push(json!({
                    "sessionUpdate": "tool_call_update",
                    "toolCallId": call_id,
                    "status": "in_progress",
                }));
                tool_states.insert(call_id.clone(), ToolState::InProgress);
            } else {
                tool_states.insert(call_id.clone(), ToolState::Pending);
            }
        }
        AgentEvent::ApprovalRequest { call_id, .. } => {
            tool_states.insert(call_id.clone(), ToolState::Pending);
        }
        AgentEvent::ApprovalResult {
            call_id, decision, ..
        } => match decision {
            ApprovalOutcome::Approved | ApprovalOutcome::ApprovedForSession => {
                if tool_states.get(call_id) != Some(&ToolState::InProgress) {
                    updates.push(json!({
                        "sessionUpdate": "tool_call_update",
                        "toolCallId": call_id,
                        "status": "in_progress",
                    }));
                }
                tool_states.insert(call_id.clone(), ToolState::InProgress);
            }
            ApprovalOutcome::Denied
            | ApprovalOutcome::DeniedWithMessage
            | ApprovalOutcome::Cancelled => {
                tool_states.insert(call_id.clone(), ToolState::Declined);
            }
        },
        AgentEvent::ToolResult {
            call_id,
            is_error,
            summary,
            screen,
            display,
            ..
        } => {
            // 確認が不要だった呼び出しも、完了の前に in_progress を経て遷移させる（設計書 05-03）。
            if tool_states.remove(call_id) == Some(ToolState::Pending) {
                updates.push(json!({
                    "sessionUpdate": "tool_call_update",
                    "toolCallId": call_id,
                    "status": "in_progress",
                }));
            }
            let status = if *is_error { "failed" } else { "completed" };
            let content = tool_result_content(summary, screen.as_deref(), display.as_deref());
            updates.push(json!({
                "sessionUpdate": "tool_call_update",
                "toolCallId": call_id,
                "status": status,
                "content": content,
            }));
        }
        AgentEvent::TurnEnd { usage, .. } => {
            if let Some(input) = usage.input_tokens {
                *current_estimate = input.saturating_add(usage.output_tokens.unwrap_or(0));
            }
            if let Some(size) = context_window {
                updates.push(usage_update(*current_estimate, size));
            }
        }
        AgentEvent::Compaction {
            reason,
            summarized_items,
            kept_items,
            estimate_after,
            ..
        } => {
            *current_estimate = *estimate_after;
            diagnostics.push(crate::ui::text::compaction(
                *reason,
                *summarized_items,
                *kept_items,
            ));
            if let Some(size) = context_window {
                updates.push(usage_update(*estimate_after, size));
            }
        }
        AgentEvent::ToolResultsPruned {
            results,
            estimate_after,
            ..
        } => {
            *current_estimate = *estimate_after;
            diagnostics.push(crate::ui::text::tool_results_pruned(results.len()));
        }
        AgentEvent::SessionStart {
            notice,
            instruction_files,
            skipped_instructions,
            plugins,
            skipped_plugins,
            plugin_notices,
            skills,
            skipped_skills,
            ..
        } => {
            if let Some(notice) = notice {
                diagnostics.push(notice.clone());
            }
            for plugin in plugins {
                diagnostics.push(crate::ui::text::plugin_loaded(plugin));
            }
            for plugin in skipped_plugins {
                diagnostics.push(crate::ui::text::plugin_skipped(plugin));
            }
            for plugin in plugin_notices {
                diagnostics.push(crate::ui::text::plugin_notice(plugin));
            }
            for skill in skills {
                diagnostics.push(crate::ui::text::skill_loaded(skill));
            }
            for skipped in skipped_skills {
                diagnostics.push(crate::ui::text::skill_skipped(
                    &skipped.path,
                    &skipped.reason,
                ));
            }
            for path in instruction_files {
                diagnostics.push(crate::ui::text::instruction_loaded(path));
            }
            for skipped in skipped_instructions {
                diagnostics.push(crate::ui::text::instruction_skipped(
                    &skipped.path,
                    &skipped.reason,
                ));
            }
        }
        AgentEvent::Error { message, .. } => {
            // AgentEvent::Error は診断として標準エラー出力に書くだけで、session/prompt の応答は返さない
            // （設計書 05-03「イベントから `session/update` への変換」）。
            diagnostics.push(crate::ui::text::error(message));
        }
        AgentEvent::ProjectDirChanged { project_dir } => {
            diagnostics.push(crate::ui::text::project_changed(project_dir));
        }
        AgentEvent::Checkpoint { checkpoint } => {
            diagnostics.push(crate::ui::text::checkpoint(checkpoint));
        }
        AgentEvent::HardLinks { paths } => {
            diagnostics.push(crate::ui::text::hard_links(paths));
        }
        AgentEvent::ImagesRemoved { .. } => {
            diagnostics.push(crate::ui::text::IMAGES_REMOVED.into());
        }
        AgentEvent::NestedInstructions { paths, skipped, .. } => {
            diagnostics.push(crate::ui::text::nested_instructions(paths, skipped));
        }
        AgentEvent::Steered { .. }
        | AgentEvent::TurnStart { .. }
        | AgentEvent::Retry { .. }
        | AgentEvent::SessionEnd { .. } => {}
    }
    EventTranslation {
        updates,
        diagnostics,
    }
}

/// `session/load` で現在の枝の会話を順に再生する `session/update` の列を作る（設計書 05-03）。
pub(super) fn replay_conversation(conversation: &Conversation) -> Vec<Value> {
    let mut updates = Vec::new();
    for item in &conversation.items {
        match item {
            Item::UserMessage(message) => {
                for part in &message.parts {
                    if matches!(part.kind, UserPartKind::Request | UserPartKind::Steering) {
                        updates.push(json!({
                            "sessionUpdate": "user_message_chunk",
                            "content": { "type": "text", "text": part.text },
                        }));
                    }
                }
            }
            Item::AssistantTurn(turn) => {
                for part in &turn.parts {
                    match part {
                        AssistantPart::Reasoning(reasoning) if !reasoning.text.is_empty() => {
                            updates.push(json!({
                                "sessionUpdate": "agent_thought_chunk",
                                "content": { "type": "text", "text": reasoning.text },
                            }));
                        }
                        AssistantPart::Text(text) if !text.is_empty() => {
                            updates.push(json!({
                                "sessionUpdate": "agent_message_chunk",
                                "content": { "type": "text", "text": text },
                            }));
                        }
                        AssistantPart::ToolCall(call) => {
                            updates.push(json!({
                                "sessionUpdate": "tool_call",
                                "toolCallId": call.id,
                                "title": tool_call_title(&call.name, &call.arguments),
                                "kind": tool_call_kind(&call.name),
                                "status": "pending",
                                "rawInput": call.arguments,
                            }));
                        }
                        AssistantPart::Reasoning(_) | AssistantPart::Text(_) => {}
                    }
                }
            }
            Item::ToolResult(result) => {
                let status = if result.is_error {
                    "failed"
                } else {
                    "completed"
                };
                updates.push(json!({
                    "sessionUpdate": "tool_call_update",
                    "toolCallId": result.id,
                    "status": status,
                    "content": [
                        {
                            "type": "content",
                            "content": { "type": "text", "text": result.text },
                        }
                    ],
                }));
            }
        }
    }
    updates
}

/// `usage_update` の通知本文を作る（設計書 05-03）。
pub(super) fn usage_update(used: u64, size: u64) -> Value {
    json!({
        "sessionUpdate": "usage_update",
        "used": used,
        "size": size,
    })
}

/// ACP 版 1 の `configOptions` を組み立てる（モデルと、対応時のみ推論の度合い。モードは出さない）。
pub(super) fn config_options(model: &str, effort: &str, supports_effort: bool) -> Value {
    let mut options = vec![json!({
        "id": "model",
        "name": text::CONFIG_MODEL_NAME,
        "category": "model",
        "type": "select",
        "currentValue": model,
        "options": [
            { "value": model, "name": model }
        ],
    })];
    if supports_effort {
        let mut choices = vec![("low", "Low"), ("medium", "Medium"), ("high", "High")];
        if !choices.iter().any(|(value, _)| *value == effort) {
            choices.push((effort, effort));
        }
        let effort_options = choices
            .into_iter()
            .map(|(value, name)| json!({ "value": value, "name": name }))
            .collect::<Vec<_>>();
        options.push(json!({
            "id": "effort",
            "name": text::CONFIG_EFFORT_NAME,
            "category": "thought_level",
            "type": "select",
            "currentValue": effort,
            "options": effort_options,
        }));
    }
    Value::Array(options)
}

/// 設計書 05-03「イベントから `session/update` への変換」の `tool_call` の分類。
pub(super) fn tool_call_kind(name: &str) -> &'static str {
    match name {
        "read" | "benitoite_reference" | "benitoite_explain" => "read",
        "glob" | "grep" => "search",
        "write" | "edit" | "create_project" => "edit",
        "benitoite_check" | "benitoite_run" | "benitoite_test" | "benitoite_scratch" | "bash" => {
            "execute"
        }
        "web_fetch" => "fetch",
        "update_plan" => "think",
        _ => "other",
    }
}

fn tool_never_needs_approval(name: &str) -> bool {
    matches!(
        name,
        "read"
            | "glob"
            | "grep"
            | "benitoite_check"
            | "benitoite_reference"
            | "benitoite_explain"
            | "update_plan"
    )
}

fn tool_call_title(name: &str, arguments: &Value) -> String {
    let arg = |key| {
        arguments
            .get(key)
            .and_then(Value::as_str)
            .unwrap_or_default()
    };
    let raw = match name {
        "read" => {
            let mut value = format!("read {}", arg("path"));
            if let (Some(offset), Some(limit)) = (
                arguments.get("offset").and_then(Value::as_u64),
                arguments.get("limit").and_then(Value::as_u64),
            ) {
                value.push_str(&format!(" ({offset}-{})", offset.saturating_add(limit)));
            }
            value
        }
        "glob" => format!("glob {}", arg("pattern")),
        "grep" => format!("grep {} {}", arg("pattern"), arg("path")),
        "write" | "edit" => format!("{name} {}", arg("path")),
        "bash" => format!(
            "bash: {}",
            arg("command").lines().next().unwrap_or_default()
        ),
        "web_fetch" => format!("web_fetch {}", arg("url")),
        "benitoite_run" | "benitoite_check" => format!("{name} {}", arg("path")),
        "benitoite_test" => {
            let targets = arguments
                .get("paths")
                .and_then(Value::as_array)
                .map(|list| {
                    list.iter()
                        .filter_map(Value::as_str)
                        .collect::<Vec<_>>()
                        .join(", ")
                })
                .unwrap_or_default();
            if targets.is_empty() {
                name.to_owned()
            } else {
                format!("{name} {targets}")
            }
        }
        "benitoite_scratch" => "benitoite_scratch".to_owned(),
        "benitoite_reference" => format!("benitoite_reference {}", arg("name")),
        "benitoite_explain" => format!("benitoite_explain {}", arg("code")),
        "create_project" => format!("create_project {}", arg("path")),
        "update_plan" => "update_plan".to_owned(),
        _ => format!("{name} {arguments}"),
    };
    crate::ui::terminal_text(&raw, false)
}

fn tool_result_content(summary: &str, screen: Option<&str>, display: Option<&str>) -> Value {
    let mut blocks = Vec::new();
    let main_text = screen.unwrap_or(summary);
    if !main_text.is_empty() {
        blocks.push(json!({
            "type": "content",
            "content": { "type": "text", "text": main_text },
        }));
    }
    if let Some(diff) = display
        && !diff.is_empty()
    {
        blocks.push(json!({
            "type": "content",
            "content": { "type": "text", "text": diff },
        }));
    }
    Value::Array(blocks)
}
