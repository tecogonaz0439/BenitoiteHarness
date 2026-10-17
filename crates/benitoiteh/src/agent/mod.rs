//! 要求、応答、道具、記録を順序付けるループ（設計書 01-01、01-04、01-05）。

mod events;
mod execution;
mod retry;
mod text;
mod transport;

pub use events::{AgentEvent, Outcome};

use std::fmt;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{SyncSender, TrySendError};
use std::thread;
use std::time::{Duration, Instant};

use crate::approval::NonInteractiveApprover;
use crate::config::{Limits, Mode, ResolvedExec};
use crate::context::{self, ContextError, Environment, SystemContext};
use crate::http::{HttpClient, HttpTimeouts, Uri};
use crate::provider::{
    AdapterKind, AssistantPart, AssistantTurn, Capabilities, Conversation, Destination,
    FinishReason, Item, ProviderError, StreamEvent, UserMessage, UserPart, UserPartKind,
};
use crate::session::{
    self, Header, Record, RecordedProvider, Session, SessionError, SessionWriter, Summary, UserKind,
};
use crate::tools::{ToolContext, ToolSet};
use transport::{Adapter, Request, Stream};

const TICK: Duration = Duration::from_millis(10);

/// exec が解決した設定と、通信・やり直しの時間の上限。
#[derive(Clone, Debug)]
pub struct AgentConfig {
    pub mode: Mode,
    pub destination: Destination,
    pub kind: AdapterKind,
    pub capabilities: Capabilities,
    pub effort: String,
    /// max_check_fix は P2 に渡す。P1 では数えない。
    pub limits: Limits,
    pub http_timeouts: HttpTimeouts,
    /// 既定 1 秒。試験や呼び出す側の待ち時間の方針で短くできる。
    pub retry_initial: Duration,
}

impl From<ResolvedExec> for AgentConfig {
    fn from(config: ResolvedExec) -> Self {
        Self {
            mode: config.mode,
            destination: config.destination,
            kind: config.kind,
            capabilities: config.capabilities,
            effort: config.effort,
            limits: config.limits,
            http_timeouts: HttpTimeouts::default(),
            retry_initial: Duration::from_secs(1),
        }
    }
}

/// P4 で受け取り口を加える操縦と後続の入力の形。
#[derive(Clone, Debug, PartialEq)]
pub enum AgentInput {
    /// 一連の処理を始める依頼。
    Request(String),
    /// 今の番の道具の後に加える入力。
    Steering(String),
    /// 一連の処理の後に始める依頼。
    FollowUp(String),
}

/// 一連の処理の結果。exec は summary を最後の行として記録する。
#[derive(Clone, Debug, PartialEq)]
pub struct RunReport {
    pub outcome: Outcome,
    pub summary: Summary,
}

/// ループの失敗。提供者の本文と認証情報は持たない。
#[derive(Debug)]
pub enum AgentError {
    /// 記録の失敗。
    Session(SessionError),
    /// 文脈の構築の失敗。
    Context(ContextError),
    /// 分類済みの提供者の失敗。
    Provider(ProviderError),
    /// 提供者の拒否の分類。
    Refusal(String),
    /// 番数またはトークンの上限。
    Limit(&'static str),
    /// 設定と実行器の不一致。
    InvalidConfiguration,
    /// イベントの受け手が終了した。
    ReceiverClosed,
    /// 道具または通信のスレッドの失敗。
    Worker,
    /// やり直しのゆらぎ用の乱数が得られない。
    Random,
}

impl fmt::Display for AgentError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Session(error) => error.fmt(f),
            Self::Context(error) => error.fmt(f),
            Self::Provider(ProviderError::ContextOverflow) => f.write_str(text::CONTEXT_OVERFLOW),
            Self::Provider(error) => error.fmt(f),
            Self::Refusal(category) => f.write_str(&text::refusal(category)),
            Self::Limit(message) => f.write_str(message),
            Self::InvalidConfiguration => f.write_str(text::INVALID_CONFIGURATION),
            Self::ReceiverClosed => f.write_str(text::CHANNEL_CLOSED),
            Self::Worker => f.write_str(text::WORKER_FAILED),
            Self::Random => f.write_str(text::RANDOM_FAILED),
        }
    }
}

impl std::error::Error for AgentError {}
impl From<SessionError> for AgentError {
    fn from(error: SessionError) -> Self {
        Self::Session(error)
    }
}
impl From<ContextError> for AgentError {
    fn from(error: ContextError) -> Self {
        Self::Context(error)
    }
}
impl From<ProviderError> for AgentError {
    fn from(error: ProviderError) -> Self {
        Self::Provider(error)
    }
}

impl AgentError {
    fn kind(&self) -> &'static str {
        match self {
            Self::Session(_) => "session",
            Self::Context(_) => "context",
            Self::Provider(ProviderError::ContextOverflow) => "context_overflow",
            Self::Provider(_) => "provider",
            Self::Refusal(_) => "refusal",
            Self::Limit(_) => "limit",
            Self::InvalidConfiguration => "configuration",
            Self::ReceiverClosed => "event_receiver",
            Self::Worker => "worker",
            Self::Random => "random",
        }
    }
}

/// セッションの状態を持つエージェント。呼び出す側がスレッドへ移して run を呼ぶ。
/// システムプロンプトと道具を作成時に固定し、別のハッシュの RawItems を混ぜない。
pub struct Agent {
    config: AgentConfig,
    adapter: Adapter,
    client: HttpClient,
    tools: ToolSet,
    tool_context: ToolContext,
    system: SystemContext,
    environment: Environment,
    session: Session,
    writer: SessionWriter,
    conversation: Conversation,
    previous_last_item: Option<usize>,
    detached: Vec<Stream>,
    approver: NonInteractiveApprover,
    started: bool,
    turn: u64,
    repeated: Option<(String, serde_json::Value, u64)>,
}

impl Agent {
    /// 新しいセッションのヘッダを書き、会話を空で始める。キーはアダプタだけに渡す。
    pub fn new(
        config: AgentConfig,
        api_key: String,
        tool_context: ToolContext,
        environment: Environment,
        session: Session,
    ) -> Result<Self, AgentError> {
        if config.mode != tool_context.mode {
            return Err(AgentError::InvalidConfiguration);
        }
        provider_host(&config.destination)?;
        let tools = ToolSet::new(config.mode);
        let system = SystemContext::new(&tools)?;
        let adapter = Adapter::new(&config, api_key)?;
        let client = HttpClient::new(config.http_timeouts);
        let mut writer = session.writer()?;
        writer.append(&Record::Header(Header {
            format: session::FORMAT_VERSION,
            session_id: session.id().to_string(),
            started_at: session.id().started_at_utc(),
            benitoiteh_version: env!("CARGO_PKG_VERSION").into(),
            mode: config.mode.into(),
            project_dir: tool_context.project.real_path().into(),
            provider: RecordedProvider {
                name: config.destination.provider.clone(),
                base_url: config.destination.base_url.clone(),
                model: config.destination.model.clone(),
                adapter: config.kind,
            },
            benitoite: None,
            skill_version: context::BENITOITE_DOCUMENT_VERSION.into(),
            system_prompt_sha256: context::system_prompt_hash(&system.system_prompt),
            tools_sha256: context::tool_definitions_hash(&system.tools)?,
        }))?;
        Ok(Self {
            config,
            adapter,
            client,
            tools,
            tool_context,
            system,
            environment,
            session,
            writer,
            conversation: Conversation::default(),
            previous_last_item: None,
            detached: Vec::new(),
            approver: NonInteractiveApprover,
            started: false,
            turn: 0,
            repeated: None,
        })
    }

    /// 画面や後続の処理が読む会話。途中で切れた応答は含まない。
    pub fn conversation(&self) -> &Conversation {
        &self.conversation
    }

    /// 依頼を処理する。処理の失敗は Failed の報告、イベントや終了の記録を渡せないときは Err。
    /// events は sync_channel の送信側とし、呼び出す側は並行して受け取る。
    pub fn run(
        &mut self,
        request: String,
        cancelled: Arc<AtomicBool>,
        events: SyncSender<AgentEvent>,
    ) -> Result<RunReport, AgentError> {
        let started = Instant::now();
        let mut summary = Summary::default();
        if !self.started {
            let host = provider_host(&self.config.destination)?;
            self.emit(
                &events,
                AgentEvent::SessionStart {
                    session_id: self.session.id().to_string(),
                    mode: self.config.mode.into(),
                    provider: self.config.destination.provider.clone(),
                    model: self.config.destination.model.clone(),
                    host: host.clone(),
                    benitoiteh_version: env!("CARGO_PKG_VERSION").into(),
                    instruction_files: self
                        .environment
                        .project_instructions()
                        .iter()
                        .map(|file| file.path.clone())
                        .collect(),
                    skipped_instructions: self.environment.skipped_instructions().to_vec(),
                    notice: Some(context::transmission_notice(
                        &self.config.destination.provider,
                        &self.config.destination.model,
                        &host,
                    )),
                },
            )?;
        }
        let mut parts = Vec::new();
        if !self.started {
            parts.push(UserPart {
                kind: UserPartKind::Environment,
                text: self.environment.render(),
            });
        }
        parts.push(UserPart {
            kind: UserPartKind::Request,
            text: request,
        });
        let message = UserMessage { parts };
        self.writer.append(&Record::User {
            kind: UserKind::Request,
            message: message.clone(),
        })?;
        self.conversation.items.push(Item::UserMessage(message));
        self.started = true;
        self.repeated = None;
        let (outcome, reason) = match self.run_inner(&cancelled, &events, &mut summary) {
            Ok(Outcome::Completed) => (Outcome::Completed, text::COMPLETED.into()),
            Ok(Outcome::Cancelled) => (Outcome::Cancelled, text::RUN_CANCELLED.into()),
            Ok(Outcome::Failed) => return Err(AgentError::Worker),
            Err(error) => {
                let message = error.to_string();
                self.emit(
                    &events,
                    AgentEvent::Error {
                        kind: error.kind().into(),
                        message: message.clone(),
                    },
                )?;
                (Outcome::Failed, message)
            }
        };
        summary.elapsed_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
        summary.reason = reason.clone();
        self.emit(&events, AgentEvent::SessionEnd { outcome, reason })?;
        Ok(RunReport { outcome, summary })
    }

    fn run_inner(
        &mut self,
        cancelled: &AtomicBool,
        events: &SyncSender<AgentEvent>,
        summary: &mut Summary,
    ) -> Result<Outcome, AgentError> {
        loop {
            if cancelled.load(Ordering::Acquire) {
                return Ok(Outcome::Cancelled);
            }
            if summary.turns >= self.config.limits.max_turns {
                return Err(AgentError::Limit(text::TURN_LIMIT));
            }
            if self.token_limit_reached(summary) {
                return Err(AgentError::Limit(text::TOKEN_LIMIT));
            }
            self.turn = self.turn.saturating_add(1);
            self.emit(events, AgentEvent::TurnStart { turn: self.turn })?;
            // 要求は番ごとに一度だけ作る。Retry-After と切断のどちらでも本文は変えない。
            // （設計書 01-01「やり直し」）。
            let request = self.adapter.request(
                &self.conversation,
                &self.system,
                &self.config.effort,
                self.previous_last_item,
            )?;
            let Some(mut response) = self.receive_with_retry(&request, cancelled, events)? else {
                return Ok(Outcome::Cancelled);
            };
            self.previous_last_item = self.conversation.items.len().checked_sub(1);
            // 提供者が同じ ID を別の番で再利用しても、内部の対応が衝突しない。
            for (index, part) in response.parts.iter_mut().enumerate() {
                if let AssistantPart::ToolCall(call) = part {
                    call.id = format!("turn-{}-call-{index}", self.turn);
                }
            }
            summary.add_turn(&response.usage);
            self.writer.append(&Record::Assistant {
                turn: self.turn,
                response: response.clone(),
            })?;
            self.conversation
                .items
                .push(Item::AssistantTurn(response.clone()));
            let calls = response
                .parts
                .iter()
                .filter_map(|part| match part {
                    AssistantPart::ToolCall(call) => Some(call.clone()),
                    AssistantPart::Text(_) | AssistantPart::Reasoning(_) => None,
                })
                .collect::<Vec<_>>();
            let refusal = match &response.finish_reason {
                FinishReason::Refusal(category) => Some(category.clone()),
                FinishReason::Completed
                | FinishReason::ToolCalls
                | FinishReason::OutputLimit
                | FinishReason::Error
                | FinishReason::Cancelled
                | FinishReason::Disconnected => None,
            };
            self.execute_calls(&calls, &response.finish_reason, cancelled, events, summary)?;
            let was_cancelled = cancelled.load(Ordering::Acquire);
            self.emit(
                events,
                AgentEvent::TurnEnd {
                    turn: self.turn,
                    finish_reason: if was_cancelled {
                        FinishReason::Cancelled
                    } else {
                        response.finish_reason.clone()
                    },
                    usage: response.usage,
                },
            )?;
            if was_cancelled {
                return Ok(Outcome::Cancelled);
            }
            if self.token_limit_reached(summary) {
                return Err(AgentError::Limit(text::TOKEN_LIMIT));
            }
            if let Some(category) = refusal {
                return Err(AgentError::Refusal(category));
            }
            if calls.is_empty() && response.finish_reason != FinishReason::OutputLimit {
                return Ok(Outcome::Completed);
            }
        }
    }

    fn token_limit_reached(&self, summary: &Summary) -> bool {
        self.config.limits.max_run_tokens.is_some_and(|limit| {
            summary.tokens.input.saturating_add(summary.tokens.output) >= limit
        })
    }

    fn emit(
        &mut self,
        events: &SyncSender<AgentEvent>,
        event: AgentEvent,
    ) -> Result<(), AgentError> {
        if let Some(record) = event.record() {
            self.writer.append(&record)?;
        }
        events.send(event).map_err(|_| AgentError::ReceiverClosed)
    }

    fn delta(
        &mut self,
        events: &SyncSender<AgentEvent>,
        mut event: AgentEvent,
        cancelled: &AtomicBool,
    ) -> Result<(), AgentError> {
        loop {
            if cancelled.load(Ordering::Acquire) {
                return Ok(());
            }
            match events.try_send(event) {
                Ok(()) => return Ok(()),
                Err(TrySendError::Disconnected(_)) => return Err(AgentError::ReceiverClosed),
                Err(TrySendError::Full(value)) => {
                    event = value;
                    thread::sleep(TICK);
                }
            }
        }
    }

    fn reap_streams(&mut self) -> Result<(), AgentError> {
        let mut pending = Vec::new();
        for mut stream in self.detached.drain(..) {
            if stream.is_finished() {
                stream.join().map_err(|_| AgentError::Worker)?;
            } else {
                pending.push(stream);
            }
        }
        self.detached = pending;
        Ok(())
    }

    fn receive(
        &mut self,
        request: &Request,
        cancelled: &AtomicBool,
        events: &SyncSender<AgentEvent>,
    ) -> Result<Option<AssistantTurn>, AgentError> {
        self.reap_streams()?;
        while self.detached.len() >= 2 {
            if cancelled.load(Ordering::Acquire) {
                return Ok(None);
            }
            thread::sleep(TICK);
            self.reap_streams()?;
        }
        if cancelled.load(Ordering::Acquire) {
            return Ok(None);
        }
        let mut stream = self.adapter.stream(&self.client, request)?;
        let result = self.read_stream(&mut stream, cancelled, events);
        stream.cancel();
        match result {
            Ok(true) => {
                // 正常な終わりのイベントは接続後なので、shutdown して回収できる。
                stream.join().map_err(|_| AgentError::Worker)?;
                let response = stream.finish()?;
                match response.finish_reason {
                    FinishReason::Disconnected | FinishReason::Error => {
                        Err(ProviderError::StreamDisconnected.into())
                    }
                    FinishReason::Cancelled => Ok(None),
                    FinishReason::Completed
                    | FinishReason::ToolCalls
                    | FinishReason::OutputLimit
                    | FinishReason::Refusal(_) => Ok(Some(response)),
                }
            }
            Ok(false) => {
                self.detached.push(stream);
                self.reap_streams()?;
                Ok(None)
            }
            Err(error) => {
                self.detached.push(stream);
                self.reap_streams()?;
                Err(error)
            }
        }
    }

    fn read_stream(
        &mut self,
        stream: &mut Stream,
        cancelled: &AtomicBool,
        events: &SyncSender<AgentEvent>,
    ) -> Result<bool, AgentError> {
        loop {
            if cancelled.load(Ordering::Acquire) {
                return Ok(false);
            }
            let polled = stream.poll(TICK);
            if cancelled.load(Ordering::Acquire) {
                return Ok(false);
            }
            for event in polled? {
                match event {
                    StreamEvent::TextDelta { text, .. } => self.delta(
                        events,
                        AgentEvent::TextDelta {
                            turn: self.turn,
                            text,
                        },
                        cancelled,
                    )?,
                    StreamEvent::ReasoningDelta { text, .. } => self.delta(
                        events,
                        AgentEvent::ReasoningDelta {
                            turn: self.turn,
                            text,
                        },
                        cancelled,
                    )?,
                    StreamEvent::Finished { .. } => return Ok(true),
                    StreamEvent::ToolCallStart { .. }
                    | StreamEvent::ToolArgumentsDelta { .. }
                    | StreamEvent::ToolCallEnd { .. }
                    | StreamEvent::Usage { .. } => {}
                }
            }
        }
    }
}

fn provider_host(destination: &Destination) -> Result<String, AgentError> {
    let uri = destination
        .base_url
        .parse::<Uri>()
        .map_err(|_| AgentError::InvalidConfiguration)?;
    if !matches!(uri.scheme_str(), Some("http" | "https"))
        || uri.authority().is_none_or(|a| a.as_str().contains('@'))
    {
        return Err(AgentError::InvalidConfiguration);
    }
    uri.host()
        .map(str::to_owned)
        .ok_or(AgentError::InvalidConfiguration)
}
