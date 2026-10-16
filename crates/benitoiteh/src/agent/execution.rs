//! 道具を最大八つで並べ、書き込みを境に順序を守る（設計書 01-01「道具の実行」）。

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::SyncSender;
use std::thread;

use crate::approval::{self, ApprovalKind, ApprovalOutcome, ApprovalRecord, Approver};
use crate::provider::{FinishReason, Item, ToolCall, ToolResult};
use crate::session::{Record, Summary};
use crate::tools::{ApprovalStatus, ToolOutput, ValidatedCall};

use super::{Agent, AgentError, AgentEvent, text};

struct Prepared {
    call: ToolCall,
    validated: Result<ValidatedCall, String>,
    parallel: bool,
}

impl Agent {
    pub(super) fn execute_calls(
        &mut self,
        calls: &[ToolCall],
        finish: &FinishReason,
        cancelled: &AtomicBool,
        events: &SyncSender<AgentEvent>,
        summary: &mut Summary,
    ) -> Result<(), AgentError> {
        let mut prepared = Vec::new();
        for call in calls {
            let arguments = if call.arguments.is_null() {
                serde_json::from_str(&call.raw_arguments)
                    .map_err(|_| text::INVALID_ARGUMENTS.to_owned())
            } else {
                Ok(call.arguments.clone())
            };
            let validated = arguments.and_then(|arguments| {
                self.tools
                    .validate(&call.name, arguments)
                    .map_err(|error| error.to_string())
            });
            let parallel = validated.as_ref().is_ok_and(|validated| {
                self.tools
                    .definitions()
                    .iter()
                    .any(|definition| definition.id == validated.id() && definition.parallel)
            });
            self.writer.append(&Record::ToolCall {
                turn: self.turn,
                call: call.clone(),
                validation_error: validated.as_ref().err().cloned(),
            })?;
            self.emit(
                events,
                AgentEvent::ToolCall {
                    turn: self.turn,
                    call_id: call.id.clone(),
                    name: call.name.clone(),
                    arguments: call.arguments.clone(),
                },
            )?;
            if validated.is_ok() {
                summary.add_tool_call(&call.name);
            }
            prepared.push(Prepared {
                call: call.clone(),
                validated,
                parallel,
            });
        }
        let mut remaining = prepared.as_slice();
        while let Some(first) = remaining.first() {
            let suppressed = if cancelled.load(Ordering::Acquire) {
                Some(text::CANCELLED)
            } else if *finish == FinishReason::OutputLimit {
                Some(text::OUTPUT_LIMIT)
            } else if self.token_limit_reached(summary) {
                Some(text::TOKEN_LIMIT)
            } else if matches!(finish, FinishReason::Refusal(_)) {
                Some(text::CANCELLED)
            } else {
                None
            };
            if let Some(message) = suppressed {
                self.result(&first.call, ToolOutput::error(message), events)?;
                remaining = remaining.get(1..).ok_or(AgentError::Worker)?;
                continue;
            }
            if first.parallel {
                let count = remaining
                    .iter()
                    .take_while(|call| call.parallel)
                    .take(8)
                    .count();
                let (batch, rest) = remaining.split_at(count);
                let tools = &self.tools;
                let context = &self.tool_context;
                let outputs = thread::scope(|scope| {
                    let mut workers = Vec::new();
                    let mut failed = false;
                    for prepared in batch {
                        let Ok(validated) = prepared.validated.as_ref() else {
                            failed = true;
                            break;
                        };
                        match thread::Builder::new().spawn_scoped(scope, move || {
                            if cancelled.load(Ordering::Acquire) {
                                ToolOutput::error(text::CANCELLED)
                            } else {
                                tools.execute_validated(
                                    context,
                                    validated,
                                    ApprovalStatus::Unapproved,
                                )
                            }
                        }) {
                            Ok(worker) => workers.push(worker),
                            Err(_) => {
                                failed = true;
                                break;
                            }
                        }
                    }
                    // 一つが失敗しても全員を join する。scope の暗黙の join に
                    // パニックを伝えない（設計書 06-01「lint と実装の規約」）。
                    let joined = workers
                        .into_iter()
                        .map(|worker| worker.join().map_err(|_| AgentError::Worker))
                        .collect::<Vec<_>>();
                    if failed {
                        return Err(AgentError::Worker);
                    }
                    joined.into_iter().collect::<Result<Vec<_>, AgentError>>()
                })?;
                // join の順序と会話の順序を応答に揃える。完了時刻は順序に使わない。
                for (prepared, output) in batch.iter().zip(outputs) {
                    self.result(&prepared.call, output, events)?;
                }
                remaining = rest;
            } else {
                let output = match &first.validated {
                    Ok(validated) => {
                        // P1 の並行不可は write/edit。呼び出しの実行前に flush する。
                        // （設計書 01-05「セッションの記録」）。
                        self.writer.append(&Record::ToolStart {
                            call: first.call.clone(),
                        })?;
                        let output = self.tools.execute_validated(
                            &self.tool_context,
                            validated,
                            ApprovalStatus::Unapproved,
                        );
                        self.approve(&first.call, validated, output, cancelled, events, summary)?
                    }
                    Err(message) => ToolOutput::error(message.clone()),
                };
                self.result(&first.call, output, events)?;
                remaining = remaining.get(1..).ok_or(AgentError::Worker)?;
            }
        }
        Ok(())
    }

    fn approve(
        &mut self,
        call: &ToolCall,
        validated: &ValidatedCall,
        output: ToolOutput,
        cancelled: &AtomicBool,
        events: &SyncSender<AgentEvent>,
        summary: &mut Summary,
    ) -> Result<ToolOutput, AgentError> {
        let Some(request) = output.approval else {
            return Ok(output);
        };
        let mut record = ApprovalRecord {
            call_id: call.id.clone(),
            kind: ApprovalKind::WriteScope,
            summary: request.summary,
            decision: None,
        };
        self.emit(
            events,
            AgentEvent::ApprovalRequest {
                call_id: record.call_id.clone(),
                kind: record.kind,
                summary: record.summary.clone(),
                decision: None,
            },
        )?;
        let decision = if cancelled.load(Ordering::Acquire) {
            ApprovalOutcome::Cancelled
        } else {
            self.approver.approve(&record)
        };
        record.decision = Some(decision);
        summary.add_approval(decision);
        self.emit(
            events,
            AgentEvent::ApprovalResult {
                call_id: record.call_id,
                kind: record.kind,
                summary: record.summary,
                decision,
            },
        )?;
        Ok(match decision {
            ApprovalOutcome::Approved => {
                if cancelled.load(Ordering::Acquire) {
                    ToolOutput::error(text::CANCELLED)
                } else {
                    self.tools.execute_validated(
                        &self.tool_context,
                        validated,
                        ApprovalStatus::Approved,
                    )
                }
            }
            ApprovalOutcome::Denied => ToolOutput::error(approval::denial_message()),
            ApprovalOutcome::Cancelled => ToolOutput::error(text::CANCELLED),
        })
    }

    fn result(
        &mut self,
        call: &ToolCall,
        mut output: ToolOutput,
        events: &SyncSender<AgentEvent>,
    ) -> Result<(), AgentError> {
        let arguments = self
            .tools
            .validate(&call.name, call.arguments.clone())
            .map(|validated| validated.arguments().clone())
            .unwrap_or_else(|_| call.arguments.clone());
        let count = match &self.repeated {
            Some((name, previous, count)) if *name == call.name && *previous == arguments => {
                count.saturating_add(1)
            }
            Some(_) | None => 1,
        };
        self.repeated = Some((call.name.clone(), arguments, count));
        if [3, 5, 8].contains(&count) {
            output.text.push('\n');
            output.text.push_str(&text::repeated(count));
        }
        output.text = crate::tools::truncate::cap_middle(&output.text);
        let result = ToolResult {
            id: call.id.clone(),
            provider_id: call.provider_id.clone(),
            text: output.text,
            is_error: output.is_error,
        };
        self.writer.append(&Record::ToolResult {
            turn: self.turn,
            name: call.name.clone(),
            result: result.clone(),
            spill: None,
        })?;
        self.emit(
            events,
            AgentEvent::ToolResult {
                turn: self.turn,
                call_id: call.id.clone(),
                name: call.name.clone(),
                is_error: result.is_error,
                summary: result.text.lines().next().unwrap_or_default().to_owned(),
            },
        )?;
        self.conversation.items.push(Item::ToolResult(result));
        Ok(())
    }
}
