//! 要約の要求と、記録を起点とする会話の置き換えを管理する（設計書 01-04「圧縮」）。

mod conversation;

#[cfg(test)]
mod tests;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::SyncSender;

use crate::context::{self, SystemContext};
use crate::provider::{
    AssistantPart, Conversation, FinishReason, Item, ProviderError, Usage, UserMessage, UserPart,
    UserPartKind,
};
use crate::session::{Record, Summary};

use super::{Agent, AgentError, AgentEvent, CompactionReason, text};

const PROMPT: &str = include_str!("../../../prompts/compaction.md");
const SYSTEM: &str = include_str!("../../../prompts/compaction-system.md");

/// 一連の処理の外から求めた圧縮の結果の種類。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CompactOutcome {
    /// 要約を記録し、会話を置き換えた。
    Compacted,
    /// 前の要約のほかに、要約する依頼・操縦・会話の項目がなかった。
    NothingToCompact,
    /// 要約の要求を取り消し、会話を保った。
    Cancelled,
    /// 要約に失敗し、会話を保った。診断はイベントで返す。
    Failed,
}

/// 再開時にも、理由によらず残す範囲を無くせる圧縮の指定。
#[derive(Clone, Debug)]
pub struct CompactOptions {
    pub reason: CompactionReason,
    pub instructions: Option<String>,
    pub keep_recent: bool,
    pub separate_request: bool,
}

impl CompactOptions {
    /// 通常の残す範囲を持つ指定を作る。
    pub fn for_reason(reason: CompactionReason) -> Self {
        Self {
            reason,
            instructions: None,
            keep_recent: true,
            separate_request: false,
        }
    }
}

/// 要約の通信だけの使用量と、圧縮の前後の見積もり。
#[derive(Clone, Debug, PartialEq)]
pub struct CompactReport {
    pub outcome: CompactOutcome,
    pub usage: Usage,
    pub estimate_before: u64,
    pub estimate_after: u64,
    pub summarized_items: usize,
    pub kept_items: usize,
}

impl Agent {
    /// /compact の中身。要約は番として数えず、使用量を報告で返す。
    pub fn compact(
        &mut self,
        instructions: Option<String>,
        cancelled: Arc<AtomicBool>,
        events: SyncSender<AgentEvent>,
    ) -> Result<CompactReport, AgentError> {
        let mut options = CompactOptions::for_reason(CompactionReason::Manual);
        options.instructions = instructions;
        self.compact_with_options(options, cancelled, events)
    }

    /// 再開などの呼び出し側が、理由と残す範囲を明示して圧縮する。
    pub fn compact_with_options(
        &mut self,
        options: CompactOptions,
        cancelled: Arc<AtomicBool>,
        events: SyncSender<AgentEvent>,
    ) -> Result<CompactReport, AgentError> {
        let before = self.estimate();
        let mut summary = Summary::default();
        let mut usage = Usage::default();
        match self.perform_compaction(options, &cancelled, &events, &mut summary, &mut usage) {
            Ok(report) => Ok(report),
            Err(error)
                if matches!(
                    error,
                    AgentError::Compaction | AgentError::Provider(ProviderError::ContextOverflow)
                ) =>
            {
                self.emit(
                    &events,
                    AgentEvent::Error {
                        kind: error.kind().into(),
                        message: error.to_string(),
                    },
                )?;
                Ok(CompactReport {
                    outcome: CompactOutcome::Failed,
                    usage,
                    estimate_before: before,
                    estimate_after: self.estimate(),
                    summarized_items: 0,
                    kept_items: 0,
                })
            }
            Err(error) => Err(error),
        }
    }

    pub(super) fn push_item(&mut self, item: Item) {
        self.added_bytes = self.added_bytes.saturating_add(conversation::item_bytes(
            &item,
            self.config.capabilities.reasoning_replay,
            &self.config.destination,
        ));
        self.conversation.items.push(item);
    }

    pub(super) fn note_usage(&mut self, usage: &Usage) {
        if let Some(input) = usage.input_tokens {
            self.largest_input = Some(self.largest_input.unwrap_or(0).max(input));
            self.last_usage = Some(usage.clone());
            self.added_bytes = 0;
        }
    }

    fn total_bytes(&self) -> u64 {
        let fixed = self.system.system_prompt.len().saturating_add(
            context::tool_definitions_json(&self.system.tools)
                .to_string()
                .len(),
        );
        self.conversation.items.iter().fold(
            u64::try_from(fixed).unwrap_or(u64::MAX),
            |total, item| {
                total.saturating_add(conversation::item_bytes(
                    item,
                    self.config.capabilities.reasoning_replay,
                    &self.config.destination,
                ))
            },
        )
    }

    fn estimate(&self) -> u64 {
        context::estimate_tokens(
            self.last_usage.as_ref(),
            self.total_bytes(),
            self.added_bytes,
        )
    }

    pub(super) fn threshold_reached(&self) -> bool {
        self.config
            .capabilities
            .context_window
            .is_some_and(|window| {
                let reserve = self
                    .config
                    .capabilities
                    .max_output_tokens
                    .unwrap_or(32_000)
                    .saturating_add(16_384)
                    .min(window / 2);
                self.estimate() > window.saturating_sub(reserve)
            })
    }

    fn overflow_window(&self) -> u64 {
        self.config
            .capabilities
            .context_window
            .or(self.largest_input)
            .unwrap_or_else(|| self.estimate() / 2)
    }

    pub(super) fn perform_compaction(
        &mut self,
        options: CompactOptions,
        cancelled: &AtomicBool,
        events: &SyncSender<AgentEvent>,
        summary: &mut Summary,
        usage: &mut Usage,
    ) -> Result<CompactReport, AgentError> {
        let before = self.estimate();
        let prior = conversation::previous_summary(&self.conversation);
        let window = if options.reason == CompactionReason::Overflow {
            Some(self.overflow_window())
        } else {
            self.config.capabilities.context_window
        };
        let budget = window
            .map(|window| window / 4)
            .unwrap_or(20_000)
            .min(20_000);
        let end = conversation::tail_start(
            &self.conversation,
            budget,
            self.config.capabilities.reasoning_replay,
            &self.config.destination,
            options.keep_recent,
            self.config.kind,
        );
        let ordinary = end
            .saturating_sub(1)
            .saturating_add(conversation::first_request_count(&self.conversation));
        let count = ordinary.saturating_add(usize::from(prior.is_some()));
        let kept = self.conversation.items.len().saturating_sub(end);
        let mut report = CompactReport {
            outcome: CompactOutcome::NothingToCompact,
            usage: Usage::default(),
            estimate_before: before,
            estimate_after: before,
            summarized_items: count,
            kept_items: kept,
        };
        if cancelled.load(Ordering::Acquire) {
            report.outcome = CompactOutcome::Cancelled;
            return Ok(report);
        }
        if ordinary == 0 {
            report.summarized_items = 0;
            return Ok(report);
        }
        let continuing = !options.separate_request
            && options.reason != CompactionReason::Overflow
            && self.config.capabilities.tool_choice_none;
        let mut result = None;
        let mut shorten = options.reason == CompactionReason::Overflow;
        if continuing {
            let mut conversation = self.conversation.clone();
            conversation.items.push(user(prompt(
                options.instructions.as_deref(),
                kept > 0,
                Some(end.saturating_sub(1)),
            )));
            let request = self.adapter.request_with(
                &conversation,
                &self.system,
                &self.config.effort,
                self.previous_last_item,
                &self.session.id().to_string(),
                self.config.kind != crate::provider::AdapterKind::AnthropicMessages,
            );
            // 再開した会話などで元の要求を組み立てられない場合も、文字化して作り直す。
            if let Ok(request) = request {
                match self.receive_summary(&request, cancelled, events, summary, usage)? {
                    SummaryResponse::Text(text) if !text.is_empty() => result = Some(text),
                    SummaryResponse::Text(_) => {}
                    SummaryResponse::Overflow => shorten = true,
                    SummaryResponse::Cancelled => report.outcome = CompactOutcome::Cancelled,
                }
            }
        }
        if result.is_none() && report.outcome != CompactOutcome::Cancelled {
            if self.token_limit_reached(summary) {
                return Err(AgentError::Limit(text::TOKEN_LIMIT));
            }
            let content = self.separate_content(
                end,
                prior.as_deref(),
                options.instructions.as_deref(),
                shorten,
                options.reason,
            )?;
            let conversation = Conversation {
                items: vec![user(content)],
            };
            let system = SystemContext {
                system_prompt: SYSTEM.into(),
                tools: Vec::new(),
            };
            let request = self
                .adapter
                .request_with(
                    &conversation,
                    &system,
                    &self.config.effort,
                    None,
                    &self.session.id().to_string(),
                    false,
                )
                .map_err(|_| AgentError::Compaction)?;
            match self.receive_summary(&request, cancelled, events, summary, usage)? {
                SummaryResponse::Text(text) if !text.is_empty() => result = Some(text),
                SummaryResponse::Cancelled => report.outcome = CompactOutcome::Cancelled,
                SummaryResponse::Text(_) => return Err(AgentError::Compaction),
                SummaryResponse::Overflow => return Err(overflow_failure(options.reason)),
            }
        }
        report.usage = usage.clone();
        if cancelled.load(Ordering::Acquire) || report.outcome == CompactOutcome::Cancelled {
            report.outcome = CompactOutcome::Cancelled;
            return Ok(report);
        }
        let result = result.ok_or(AgentError::Compaction)?;
        let conversation = conversation::rebuild(
            &self.conversation,
            end,
            &result,
            &self.files,
            self.config.capabilities.reasoning_replay,
        );
        // 書き込みに失敗した場合は元の会話を保ち、replay とメモリの起点を揃える。
        self.writer.append(&Record::Compaction {
            conversation: conversation.clone(),
            reason: options.reason,
            summarized_items: count,
            kept_items: kept,
        })?;
        if let Some(recorder) = &self.tool_context.checkpoints {
            recorder.advance_generation()?;
        }
        self.conversation = conversation;
        self.last_usage = None;
        self.added_bytes = 0;
        self.previous_last_item = None;
        summary.compactions = summary.compactions.saturating_add(1);
        report.outcome = CompactOutcome::Compacted;
        report.estimate_after = self.estimate();
        self.emit(
            events,
            AgentEvent::Compaction {
                reason: options.reason,
                summarized_items: count,
                kept_items: kept,
                estimate_before: before,
                estimate_after: report.estimate_after,
            },
        )?;
        Ok(report)
    }

    fn separate_content(
        &self,
        end: usize,
        prior: Option<&str>,
        instructions: Option<&str>,
        shorten: bool,
        reason: CompactionReason,
    ) -> Result<String, AgentError> {
        const OPEN: &str = "<conversation>\n";
        const CLOSE: &str = "</conversation>\n\n";
        let mut transcript = conversation::Transcript::new(&self.conversation, end, prior, shorten);
        let request = prompt(instructions, false, None);
        let fixed = OPEN
            .len()
            .saturating_add(CLOSE.len())
            .saturating_add(request.len());
        let mut omitted = 0_usize;
        let mut omitted_prior = false;
        let mut note = String::new();
        loop {
            let note_bytes = if note.is_empty() {
                0
            } else {
                note.len().saturating_add(2)
            };
            let bytes = fixed
                .saturating_add(transcript.len())
                .saturating_add(note_bytes);
            if !shorten
                || u64::try_from(bytes).unwrap_or(u64::MAX).div_ceil(3)
                    <= self.overflow_window() / 2
            {
                let mut content = String::with_capacity(bytes);
                content.push_str(OPEN);
                transcript.append_to(&mut content);
                content.push_str(CLOSE);
                if !note.is_empty() {
                    content.push_str(&note);
                    content.push_str("\n\n");
                }
                content.push_str(&request);
                return Ok(content);
            }
            if transcript.remove_oldest() {
                omitted = omitted.saturating_add(1);
            } else if transcript.remove_prior() {
                omitted_prior = true;
            } else {
                return Err(overflow_failure(reason));
            }
            // 依頼の長さだけ更新し、会話を再び文字にしない。前の要約は件数に数えない。
            let mut notes = Vec::new();
            if omitted > 0 {
                notes.push(text::omitted_items(omitted));
            }
            if omitted_prior {
                notes.push(text::OMITTED_PREVIOUS_SUMMARY.to_owned());
            }
            note = notes.join("\n\n");
        }
    }

    fn receive_summary(
        &mut self,
        request: &super::transport::Request,
        cancelled: &AtomicBool,
        events: &SyncSender<AgentEvent>,
        summary: &mut Summary,
        usage: &mut Usage,
    ) -> Result<SummaryResponse, AgentError> {
        self.suppress_deltas = true;
        let received = self.receive_with_retry(request, cancelled, events);
        self.suppress_deltas = false;
        match received {
            Ok(Some(response)) => {
                summary.add_usage(&response.usage);
                add_usage(usage, &response.usage);
                if let Some(input) = response.usage.input_tokens {
                    self.largest_input = Some(self.largest_input.unwrap_or(0).max(input));
                }
                if matches!(
                    response.finish_reason,
                    FinishReason::Refusal(_) | FinishReason::OutputLimit
                ) {
                    return Ok(SummaryResponse::Text(String::new()));
                }
                // 道具は実行せず、文だけを採用する（設計書 01-04「圧縮」）。
                let text = response
                    .parts
                    .into_iter()
                    .filter_map(|part| match part {
                        AssistantPart::Text(text) => Some(text),
                        AssistantPart::ToolCall(_) | AssistantPart::Reasoning(_) => None,
                    })
                    .collect::<String>();
                Ok(SummaryResponse::Text(if text.trim().is_empty() {
                    String::new()
                } else {
                    text
                }))
            }
            Ok(None) => Ok(SummaryResponse::Cancelled),
            Err(AgentError::Provider(ProviderError::ContextOverflow)) => {
                Ok(SummaryResponse::Overflow)
            }
            Err(AgentError::Provider(_)) => Err(AgentError::Compaction),
            Err(error) => Err(error),
        }
    }
}

enum SummaryResponse {
    Text(String),
    Overflow,
    Cancelled,
}

fn user(text: String) -> Item {
    Item::UserMessage(UserMessage {
        parts: vec![UserPart {
            kind: UserPartKind::Request,
            text,
        }],
    })
}

fn overflow_failure(reason: CompactionReason) -> AgentError {
    if reason == CompactionReason::Overflow {
        ProviderError::ContextOverflow.into()
    } else {
        AgentError::Compaction
    }
}

fn prompt(instructions: Option<&str>, retained: bool, scope: Option<usize>) -> String {
    let mut prompt = PROMPT.trim_end().to_owned();
    if retained {
        prompt = format!("{}\n\n{prompt}", text::RECENT_RETAINED);
    }
    if let Some(items) = scope {
        prompt = format!("{}\n\n{prompt}", text::summary_scope(items));
    }
    if let Some(instructions) = instructions {
        prompt.push_str(&text::additional_instructions(instructions));
    }
    prompt
}

fn add_usage(total: &mut Usage, usage: &Usage) {
    fn add(left: Option<u64>, right: Option<u64>) -> Option<u64> {
        [left, right]
            .into_iter()
            .flatten()
            .reduce(u64::saturating_add)
    }
    total.input_tokens = add(total.input_tokens, usage.input_tokens);
    total.output_tokens = add(total.output_tokens, usage.output_tokens);
    total.cache_read_tokens = add(total.cache_read_tokens, usage.cache_read_tokens);
    total.cache_write_tokens = add(total.cache_write_tokens, usage.cache_write_tokens);
    total.reasoning_tokens = add(total.reasoning_tokens, usage.reasoning_tokens);
    if !usage.raw.is_null() {
        if total.raw.is_null() {
            total.raw = serde_json::json!([]);
        }
        if let Some(reports) = total.raw.as_array_mut() {
            reports.push(usage.raw.clone());
        }
    }
}
