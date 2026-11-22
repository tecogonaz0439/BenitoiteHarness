//! ACP 版 1（Agent Client Protocol）の通信層・確認台帳・終了手順の本体
//! （設計書 05-03「ACP（エディタとの連携）」）。

mod approval;
mod prompt;
mod protocol;
pub mod text;
mod updates;

use std::collections::BTreeMap;
use std::io::{self, Read, Write};
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender, TryRecvError};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use serde_json::{Value, json};

use crate::agent::{Agent, AgentError, AgentEvent, RunReport, StopReason};
use crate::approval::{ApprovalAnswer, ApprovalOutcome, ApprovalPrompt};
use crate::session::{Record, Session};

pub use approval::AcpApprover;
use approval::PermissionLedger;
use protocol::{
    ERR_INTERNAL, ERR_INVALID_PARAMS, ERR_INVALID_REQUEST, ERR_METHOD_NOT_FOUND, ERR_PARSE,
    ERR_SERVER, InputItem, OutputWriter,
};
use updates::ToolState;

/// 宣伝する認証の方法（`initialize` の `authMethods`）。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AuthMethod {
    pub id: &'static str,
    pub name: &'static str,
    pub description: &'static str,
}

/// `initialize` の応答に必要なエージェント能力と認証の方法。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InitializeInfo {
    pub image_input: bool,
    pub auth_methods: Vec<AuthMethod>,
}

/// `session/new`・`session/load` で準備したセッション一式。
pub struct PreparedSession {
    pub agent: Agent,
    pub session: Session,
    pub image_input: bool,
    pub supports_effort: bool,
    pub context_window: Option<u64>,
    pub interactive_approvals: bool,
    pub diagnostics: Vec<String>,
    pub on_activate: Option<Box<dyn FnOnce()>>,
}

/// バックエンドからの準備・認証の誤り。
#[derive(Debug)]
pub struct BackendError {
    pub code: i64,
    pub message: String,
    pub diagnostics: Vec<String>,
}

impl BackendError {
    pub fn invalid_params(message: impl Into<String>) -> Self {
        Self {
            code: ERR_INVALID_PARAMS,
            message: message.into(),
            diagnostics: Vec::new(),
        }
    }

    pub fn server(message: impl Into<String>, diagnostics: Vec<String>) -> Self {
        Self {
            code: ERR_SERVER,
            message: message.into(),
            diagnostics,
        }
    }
}

/// CLI 依存（設定・データパス・起動準備・panic hook）を ACP 通信層から切り離す境界。
pub trait AcpBackend {
    fn initialize_info(&mut self) -> Result<InitializeInfo, BackendError>;
    fn authenticate(&mut self, method_id: &str) -> Result<(), BackendError>;
    fn new_session(&mut self, cwd: &Path) -> Result<PreparedSession, BackendError>;
    fn load_session(
        &mut self,
        session_id: &str,
        cwd: &Path,
    ) -> Result<PreparedSession, BackendError>;
    fn on_session_closed(&mut self) {}
}

struct PromptDone {
    agent: Agent,
    result: Result<RunReport, AgentError>,
    estimate: u64,
}

struct RunningPrompt {
    prompt_id: Value,
    cancel_step: Arc<AtomicBool>,
    cancel_run: Arc<AtomicBool>,
    cancelled: bool,
    events_rx: Receiver<AgentEvent>,
    done_rx: Receiver<PromptDone>,
    handle: Option<JoinHandle<()>>,
    tool_states: BTreeMap<String, ToolState>,
}

enum SessionState {
    Idle(Box<Agent>),
    Running(RunningPrompt),
}

struct ActiveSession {
    session_id: String,
    session: Session,
    state: SessionState,
    approvals_rx: Receiver<ApprovalPrompt>,
    image_input: bool,
    supports_effort: bool,
    context_window: Option<u64>,
    current_estimate: u64,
}

impl ActiveSession {
    fn is_running(&self) -> bool {
        matches!(self.state, SessionState::Running(_))
    }
}

/// ACP 版 1 の主スレッドのイベントループを実行する（設計書 05-03）。
pub fn run<R, W, D, B>(
    input: R,
    output: W,
    mut diagnostics: D,
    backend: &mut B,
    external_cancel: Arc<AtomicBool>,
) -> io::Result<()>
where
    R: Read + Send + 'static,
    W: Write + Send + 'static,
    D: Write,
    B: AcpBackend,
{
    let (wake_tx, wake_rx) = mpsc::sync_channel::<()>(1);
    let input_rx = protocol::spawn_reader(input, wake_tx.clone())?;
    let mut writer = OutputWriter::spawn(output, wake_tx.clone())?;
    let mut active: Option<ActiveSession> = None;
    let mut ledger = PermissionLedger::default();
    let mut generation: u64 = 0;

    'main: loop {
        if external_cancel.load(Ordering::Acquire) || writer.is_failed() {
            break 'main;
        }

        // 1. 実行中のエージェントのイベントと確認要求を処理する。
        if let Some(session) = active.as_mut() {
            drain_running_events(session, &mut writer, &mut diagnostics);
            if writer.is_failed() {
                break 'main;
            }
            drain_approvals(
                session,
                generation,
                &mut ledger,
                &mut writer,
                &mut diagnostics,
            );
            if writer.is_failed() {
                break 'main;
            }
            let finished = poll_completion(session, &mut ledger, &mut writer, &mut diagnostics);
            if finished == CompletionState::Panicked {
                active = None;
                backend.on_session_closed();
            }
            if writer.is_failed() {
                break 'main;
            }
        }

        // 2. クライアントからの入力（要求・通知・確認応答・EOF）を処理する。
        let mut saw_input = false;
        loop {
            match input_rx.try_recv() {
                Ok(InputItem::Eof) => break 'main,
                Ok(item) => {
                    saw_input = true;
                    handle_input(
                        item,
                        &mut active,
                        &mut ledger,
                        &mut generation,
                        &wake_tx,
                        &mut writer,
                        &mut diagnostics,
                        backend,
                    );
                    if writer.is_failed() || external_cancel.load(Ordering::Acquire) {
                        break 'main;
                    }
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => break 'main,
            }
        }

        if !saw_input {
            let wait = if active.as_ref().is_some_and(ActiveSession::is_running) {
                Duration::from_millis(5)
            } else {
                Duration::from_millis(20)
            };
            let _woken = wake_rx.recv_timeout(wait);
            while wake_rx.try_recv().is_ok() {}
        }
    }

    // 5 段の終了手順（設計書 05-03「スレッドの構成と終了の手順」）。
    // 1. 新しい受付を止めた（主ループを脱出済み）。
    // 2. 取り消しを設定し、保留中の全確認（通常確認と通信確認）を即時 Cancelled で解除する。
    ledger.cancel_all();
    if let Some(mut session) = active.take() {
        shutdown_session(&mut session, &mut ledger);
        backend.on_session_closed();
    }
    // 5. 出力の終了処理。出力障害や詰まった stdout、開いたままの stdin を無期限に join しない。
    writer.finish();
    Ok(())
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum CompletionState {
    StillRunning,
    Completed,
    Panicked,
}

fn write_diagnostic(diagnostics: &mut impl Write, message: &str) {
    let safe = crate::ui::terminal_text(message, false);
    let _written = writeln!(diagnostics, "{safe}");
}

fn drain_running_events(
    session: &mut ActiveSession,
    writer: &mut OutputWriter,
    diagnostics: &mut impl Write,
) {
    let SessionState::Running(running) = &mut session.state else {
        return;
    };
    while let Ok(event) = running.events_rx.try_recv() {
        let translated = updates::translate_event(
            &event,
            &mut running.tool_states,
            session.context_window,
            &mut session.current_estimate,
        );
        for line in translated.diagnostics {
            write_diagnostic(diagnostics, &line);
        }
        for update in translated.updates {
            if !writer.send_notification(
                "session/update",
                json!({
                    "sessionId": session.session_id,
                    "update": update,
                }),
            ) {
                return;
            }
        }
    }
}

fn drain_approvals(
    session: &mut ActiveSession,
    generation: u64,
    ledger: &mut PermissionLedger,
    writer: &mut OutputWriter,
    diagnostics: &mut impl Write,
) {
    while let Ok(prompt) = session.approvals_rx.try_recv() {
        // 確認要求より先に送られた ToolCall 等のイベントを必ず先に送る（TUI の Worker と同じ順序）。
        drain_running_events(session, writer, diagnostics);
        let cancelled = match &session.state {
            SessionState::Running(running) => running.cancelled,
            SessionState::Idle(_) => true,
        };
        if cancelled || writer.is_failed() {
            let _sent = prompt
                .reply
                .send(ApprovalAnswer::new(ApprovalOutcome::Cancelled, None));
            continue;
        }
        let (request_id, params) = ledger.register(&session.session_id, generation, prompt);
        if !writer.send_request(&request_id, "session/request_permission", params) {
            return;
        }
    }
}

fn poll_completion(
    session: &mut ActiveSession,
    ledger: &mut PermissionLedger,
    writer: &mut OutputWriter,
    diagnostics: &mut impl Write,
) -> CompletionState {
    let done = match &mut session.state {
        SessionState::Running(running) => match running.done_rx.try_recv() {
            Ok(done) => Ok(done),
            Err(TryRecvError::Empty) => return CompletionState::StillRunning,
            Err(TryRecvError::Disconnected) => Err(()),
        },
        SessionState::Idle(_) => return CompletionState::StillRunning,
    };
    // 残イベントをすべて抜き終えてから完了応答を一度だけ返す（設計書 05-03）。
    drain_running_events(session, writer, diagnostics);
    ledger.cancel_all();
    while let Ok(prompt) = session.approvals_rx.try_recv() {
        let _sent = prompt
            .reply
            .send(ApprovalAnswer::new(ApprovalOutcome::Cancelled, None));
    }
    match done {
        Ok(done) => {
            let previous =
                std::mem::replace(&mut session.state, SessionState::Idle(Box::new(done.agent)));
            let SessionState::Running(mut running) = previous else {
                return CompletionState::Completed;
            };
            if let Some(handle) = running.handle.take() {
                let _joined = handle.join();
            }
            // join 後にチャネルへ残った最終イベントも漏れなく送る。
            while let Ok(event) = running.events_rx.try_recv() {
                let translated = updates::translate_event(
                    &event,
                    &mut running.tool_states,
                    session.context_window,
                    &mut session.current_estimate,
                );
                for line in translated.diagnostics {
                    write_diagnostic(diagnostics, &line);
                }
                for update in translated.updates {
                    if !writer.send_notification(
                        "session/update",
                        json!({
                            "sessionId": session.session_id,
                            "update": update,
                        }),
                    ) {
                        break;
                    }
                }
            }
            session.current_estimate = done.estimate;
            if let Ok(report) = &done.result
                && let Ok(mut record_writer) = session.session.writer()
            {
                let _appended = record_writer.append(&Record::Summary {
                    summary: report.summary.clone(),
                });
            }
            send_prompt_final_response(writer, running.prompt_id, running.cancelled, done.result);
            CompletionState::Completed
        }
        Err(()) => {
            if let SessionState::Running(mut running) = std::mem::replace(
                &mut session.state,
                SessionState::Running(dummy_closed_running()),
            ) {
                if let Some(handle) = running.handle.take() {
                    let _joined = handle.join();
                }
                let _sent =
                    writer.send_error(running.prompt_id, ERR_INTERNAL, text::INTERNAL_ERROR);
            }
            CompletionState::Panicked
        }
    }
}

fn dummy_closed_running() -> RunningPrompt {
    let (_, events_rx) = mpsc::sync_channel(1);
    let (_, done_rx) = mpsc::sync_channel(1);
    RunningPrompt {
        prompt_id: Value::Null,
        cancel_step: Arc::new(AtomicBool::new(true)),
        cancel_run: Arc::new(AtomicBool::new(true)),
        cancelled: true,
        events_rx,
        done_rx,
        handle: None,
        tool_states: BTreeMap::new(),
    }
}

/// `session/prompt` の最終応答を、残イベント回収後に一か所で一度だけ決めて返す
/// （設計書 05-03「依頼の受け取りと終わりの理由」「イベントから `session/update` への変換」）。
fn send_prompt_final_response(
    writer: &mut OutputWriter,
    prompt_id: Value,
    was_cancelled: bool,
    result: Result<RunReport, AgentError>,
) {
    if was_cancelled {
        // 取消の後に下位の処理が誤りを返したときも cancelled とする（設計書 05-03）。
        let _ = writer.send_result(prompt_id, json!({ "stopReason": "cancelled" }));
        return;
    }
    match result {
        Ok(report) => match report.stop_reason {
            StopReason::Completed => {
                let _ = writer.send_result(prompt_id, json!({ "stopReason": "end_turn" }));
            }
            StopReason::Cancelled => {
                let _ = writer.send_result(prompt_id, json!({ "stopReason": "cancelled" }));
            }
            StopReason::MaxTurns => {
                let _ = writer.send_result(prompt_id, json!({ "stopReason": "max_turn_requests" }));
            }
            StopReason::MaxRunTokens => {
                let _ = writer.send_result(prompt_id, json!({ "stopReason": "max_tokens" }));
            }
            StopReason::Failed { kind, message } => {
                let code = if kind == "attachment" {
                    ERR_INVALID_PARAMS
                } else {
                    ERR_SERVER
                };
                let _ = writer.send_error(prompt_id, code, &message);
            }
        },
        Err(error) => {
            let code = if matches!(error, AgentError::Attachment(_)) {
                ERR_INVALID_PARAMS
            } else {
                ERR_INTERNAL
            };
            let _ = writer.send_error(prompt_id, code, &error.to_string());
        }
    }
}

/// 終了手順の段 2〜4: 実行中なら取り消し、AgentEvent チャネルを完了まで抜き続け、
/// 子プロセス停止・セッション記録書き出しを終えてから閉じる（設計書 05-03）。
fn shutdown_session(session: &mut ActiveSession, ledger: &mut PermissionLedger) {
    ledger.cancel_all();
    if let SessionState::Running(running) = &mut session.state {
        running.cancelled = true;
        running.cancel_run.store(true, Ordering::Release);
        running.cancel_step.store(true, Ordering::Release);
        // 段 3: エージェントスレッドが終わるまで有界の AgentEvent チャネルを抜き続ける。
        let done = loop {
            while running.events_rx.try_recv().is_ok() {}
            while let Ok(prompt) = session.approvals_rx.try_recv() {
                let _sent = prompt
                    .reply
                    .send(ApprovalAnswer::new(ApprovalOutcome::Cancelled, None));
            }
            match running.done_rx.recv_timeout(Duration::from_millis(5)) {
                Ok(done) => break Some(done),
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => break None,
            }
        };
        if let Some(handle) = running.handle.take() {
            let _joined = handle.join();
        }
        while running.events_rx.try_recv().is_ok() {}
        while let Ok(prompt) = session.approvals_rx.try_recv() {
            let _sent = prompt
                .reply
                .send(ApprovalAnswer::new(ApprovalOutcome::Cancelled, None));
        }
        if let Some(mut done) = done {
            if let Ok(report) = &done.result
                && let Ok(mut writer) = session.session.writer()
            {
                let _appended = writer.append(&Record::Summary {
                    summary: report.summary.clone(),
                });
            }
            let _ended = done.agent.end(text::SESSION_CLOSED_REASON);
            return;
        }
    }
    if let SessionState::Idle(agent) = &mut session.state {
        let _ended = agent.end(text::SESSION_CLOSED_REASON);
    }
}

#[allow(clippy::too_many_arguments)]
fn handle_input<B: AcpBackend>(
    item: InputItem,
    active: &mut Option<ActiveSession>,
    ledger: &mut PermissionLedger,
    generation: &mut u64,
    wake_tx: &SyncSender<()>,
    writer: &mut OutputWriter,
    diagnostics: &mut impl Write,
    backend: &mut B,
) {
    match item {
        InputItem::ParseError => {
            let _ = writer.send_error(Value::Null, ERR_PARSE, text::PARSE_ERROR);
        }
        InputItem::InvalidRequest { id } => {
            let _ = writer.send_error(id, ERR_INVALID_REQUEST, text::INVALID_REQUEST);
        }
        InputItem::LineTooLong => {
            let _ = writer.send_error(Value::Null, ERR_INVALID_PARAMS, text::LINE_TOO_LONG);
        }
        InputItem::Response { id, result, error } => {
            let active_id = active.as_ref().map(|session| session.session_id.as_str());
            ledger.resolve(active_id, *generation, &id, result.as_ref(), error.as_ref());
        }
        InputItem::Notification { method, params } => {
            if method == "session/cancel" {
                handle_cancel(None, &params, active, ledger, writer);
            }
            // 知らないメソッドの通知には応答を返さない（JSON-RPC 2.0 §4.1、設計書 05-03）。
        }
        InputItem::Request { id, method, params } => match method.as_str() {
            "initialize" => handle_initialize(id, &params, writer, diagnostics, backend),
            "authenticate" => handle_authenticate(id, &params, writer, diagnostics, backend),
            "session/new" => handle_session_new(
                id,
                &params,
                active,
                ledger,
                generation,
                wake_tx,
                writer,
                diagnostics,
                backend,
            ),
            "session/load" => handle_session_load(
                id,
                &params,
                active,
                ledger,
                generation,
                wake_tx,
                writer,
                diagnostics,
                backend,
            ),
            "session/set_config_option" => {
                handle_set_config_option(id, &params, active, writer);
            }
            "session/set_mode" => {
                let _ = writer.send_error(id, ERR_METHOD_NOT_FOUND, text::METHOD_NOT_FOUND);
            }
            "session/prompt" => {
                handle_prompt(id, &params, active, generation, wake_tx, writer);
            }
            "session/cancel" => {
                handle_cancel(Some(id), &params, active, ledger, writer);
            }
            _ => {
                let _ = writer.send_error(id, ERR_METHOD_NOT_FOUND, text::METHOD_NOT_FOUND);
            }
        },
        InputItem::Eof => {}
    }
}

fn handle_initialize<B: AcpBackend>(
    id: Value,
    params: &Value,
    writer: &mut OutputWriter,
    diagnostics: &mut impl Write,
    backend: &mut B,
) {
    let version_ok = params
        .get("protocolVersion")
        .and_then(Value::as_u64)
        .is_some_and(|v| v >= 1);
    if !version_ok {
        let _ = writer.send_error(id, ERR_INVALID_PARAMS, text::INVALID_PROTOCOL_VERSION);
        return;
    }
    let info = match backend.initialize_info() {
        Ok(info) => info,
        Err(error) => {
            for line in error.diagnostics {
                write_diagnostic(diagnostics, &line);
            }
            let _ = writer.send_error(id, error.code, &error.message);
            return;
        }
    };
    let auth_methods = info
        .auth_methods
        .iter()
        .map(|method| {
            json!({
                "id": method.id,
                "name": method.name,
                "description": method.description,
            })
        })
        .collect::<Vec<_>>();
    // クライアントが版 2 を送ってきても result.protocolVersion = 1 を返して版 1 として交渉する
    // （設計書 05-03「ACP 版 1 のメソッドと通信の契約」）。
    let _ = writer.send_result(
        id,
        json!({
            "protocolVersion": 1,
            "agentInfo": {
                "name": "benitoiteh",
                "version": env!("CARGO_PKG_VERSION"),
            },
            "agentCapabilities": {
                "loadSession": true,
                "promptCapabilities": {
                    "image": info.image_input,
                    "embeddedContext": true,
                }
            },
            "authMethods": auth_methods,
        }),
    );
}

fn handle_authenticate<B: AcpBackend>(
    id: Value,
    params: &Value,
    writer: &mut OutputWriter,
    diagnostics: &mut impl Write,
    backend: &mut B,
) {
    let Some(method_id) = params
        .get("methodId")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
    else {
        let _ = writer.send_error(id, ERR_INVALID_PARAMS, text::INVALID_METHOD_ID);
        return;
    };
    match backend.authenticate(method_id) {
        Ok(()) => {
            let _ = writer.send_result(id, json!({}));
        }
        Err(error) => {
            for line in error.diagnostics {
                write_diagnostic(diagnostics, &line);
            }
            let _ = writer.send_error(id, error.code, &error.message);
        }
    }
}

fn validate_cwd_and_mcp<'a>(
    params: &'a Value,
    diagnostics: &mut impl Write,
) -> Result<&'a Path, &'static str> {
    let Some(cwd_str) = params
        .get("cwd")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
    else {
        return Err(text::INVALID_CWD);
    };
    let cwd = Path::new(cwd_str);
    if !cwd.is_absolute() {
        return Err(text::INVALID_CWD);
    }
    let Some(mcp_servers) = params.get("mcpServers").and_then(Value::as_array) else {
        return Err(text::INVALID_MCP_SERVERS);
    };
    // 非空の mcpServers は接続せず、資格情報や URL を一切出さずに件数だけを一度診断する（設計書 05-03）。
    if !mcp_servers.is_empty() {
        write_diagnostic(diagnostics, &text::ignored_mcp_servers(mcp_servers.len()));
    }
    Ok(cwd)
}

fn close_idle_session<B: AcpBackend>(
    active: &mut Option<ActiveSession>,
    ledger: &mut PermissionLedger,
    generation: &mut u64,
    backend: &mut B,
) {
    ledger.cancel_all();
    *generation = generation.saturating_add(1);
    if let Some(mut previous) = active.take() {
        if let SessionState::Idle(agent) = &mut previous.state {
            let _ended = agent.end(text::SESSION_SWITCH_REASON);
        }
        drop(previous);
        backend.on_session_closed();
    }
}

fn activate_prepared(mut prepared: PreparedSession, wake_tx: &SyncSender<()>) -> ActiveSession {
    let wake = wake_tx.clone();
    let (approver, approvals_rx) = AcpApprover::channel();
    let approver = approver.with_wake(Arc::new(move || {
        let _woken = wake.try_send(());
    }));
    if prepared.interactive_approvals {
        prepared
            .agent
            .connect_network_prompts(approver.prompts(), approver.wake());
        prepared.agent = prepared.agent.with_approver(Box::new(approver));
    }
    if let Some(on_activate) = prepared.on_activate.take() {
        on_activate();
    }
    let session_id = prepared.session.id().to_string();
    let current_estimate = prepared.agent.context_estimate();
    ActiveSession {
        session_id,
        session: prepared.session,
        state: SessionState::Idle(Box::new(prepared.agent)),
        approvals_rx,
        image_input: prepared.image_input,
        supports_effort: prepared.supports_effort,
        context_window: prepared.context_window,
        current_estimate,
    }
}

#[allow(clippy::too_many_arguments)]
fn handle_session_new<B: AcpBackend>(
    id: Value,
    params: &Value,
    active: &mut Option<ActiveSession>,
    ledger: &mut PermissionLedger,
    generation: &mut u64,
    wake_tx: &SyncSender<()>,
    writer: &mut OutputWriter,
    diagnostics: &mut impl Write,
    backend: &mut B,
) {
    if active.as_ref().is_some_and(ActiveSession::is_running) {
        let _ = writer.send_error(id, ERR_SERVER, text::PROMPT_IN_PROGRESS);
        return;
    }
    let cwd = match validate_cwd_and_mcp(params, diagnostics) {
        Ok(cwd) => cwd,
        Err(message) => {
            let _ = writer.send_error(id, ERR_INVALID_PARAMS, message);
            return;
        }
    };
    close_idle_session(active, ledger, generation, backend);
    let prepared = match backend.new_session(cwd) {
        Ok(prepared) => prepared,
        Err(error) => {
            for line in error.diagnostics {
                write_diagnostic(diagnostics, &line);
            }
            let _ = writer.send_error(id, error.code, &error.message);
            return;
        }
    };
    for line in &prepared.diagnostics {
        write_diagnostic(diagnostics, line);
    }
    let session = activate_prepared(prepared, wake_tx);
    let (model, effort) = match &session.state {
        SessionState::Idle(agent) => (agent.status().model, agent.effort().to_owned()),
        SessionState::Running(_) => (String::new(), String::new()),
    };
    let config_options = updates::config_options(&model, &effort, session.supports_effort);
    let session_id = session.session_id.clone();
    let context_window = session.context_window;
    let estimate = session.current_estimate;
    *active = Some(session);
    if !writer.send_result(
        id,
        json!({
            "sessionId": session_id,
            "configOptions": config_options,
        }),
    ) {
        return;
    }
    if let Some(size) = context_window {
        let _ = writer.send_notification(
            "session/update",
            json!({
                "sessionId": session_id,
                "update": updates::usage_update(estimate, size),
            }),
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn handle_session_load<B: AcpBackend>(
    id: Value,
    params: &Value,
    active: &mut Option<ActiveSession>,
    ledger: &mut PermissionLedger,
    generation: &mut u64,
    wake_tx: &SyncSender<()>,
    writer: &mut OutputWriter,
    diagnostics: &mut impl Write,
    backend: &mut B,
) {
    if active.as_ref().is_some_and(ActiveSession::is_running) {
        let _ = writer.send_error(id, ERR_SERVER, text::PROMPT_IN_PROGRESS);
        return;
    }
    let Some(requested_id) = params
        .get("sessionId")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
    else {
        let _ = writer.send_error(id, ERR_INVALID_PARAMS, text::INVALID_SESSION_ID);
        return;
    };
    let cwd = match validate_cwd_and_mcp(params, diagnostics) {
        Ok(cwd) => cwd,
        Err(message) => {
            let _ = writer.send_error(id, ERR_INVALID_PARAMS, message);
            return;
        }
    };
    close_idle_session(active, ledger, generation, backend);
    let prepared = match backend.load_session(requested_id, cwd) {
        Ok(prepared) => prepared,
        Err(error) => {
            for line in error.diagnostics {
                write_diagnostic(diagnostics, &line);
            }
            let _ = writer.send_error(id, error.code, &error.message);
            return;
        }
    };
    for line in &prepared.diagnostics {
        write_diagnostic(diagnostics, line);
    }
    let session = activate_prepared(prepared, wake_tx);
    let (model, effort, replay) = match &session.state {
        SessionState::Idle(agent) => (
            agent.status().model,
            agent.effort().to_owned(),
            updates::replay_conversation(agent.conversation()),
        ),
        SessionState::Running(_) => (String::new(), String::new(), Vec::new()),
    };
    let session_id = session.session_id.clone();
    let config_options = updates::config_options(&model, &effort, session.supports_effort);
    let context_window = session.context_window;
    let estimate = session.current_estimate;
    *active = Some(session);
    // 現在の枝の会話を session/update で順に送り終えてから session/load の応答を返す（設計書 05-03）。
    for update in replay {
        if !writer.send_notification(
            "session/update",
            json!({
                "sessionId": session_id,
                "update": update,
            }),
        ) {
            return;
        }
    }
    if !writer.send_result(
        id,
        json!({
            "configOptions": config_options,
        }),
    ) {
        return;
    }
    if let Some(size) = context_window {
        let _ = writer.send_notification(
            "session/update",
            json!({
                "sessionId": session_id,
                "update": updates::usage_update(estimate, size),
            }),
        );
    }
}

fn handle_set_config_option(
    id: Value,
    params: &Value,
    active: &mut Option<ActiveSession>,
    writer: &mut OutputWriter,
) {
    let Some(requested_id) = params.get("sessionId").and_then(Value::as_str) else {
        let _ = writer.send_error(id, ERR_INVALID_PARAMS, text::UNKNOWN_SESSION);
        return;
    };
    let Some(session) = active
        .as_mut()
        .filter(|session| session.session_id == requested_id)
    else {
        let _ = writer.send_error(id, ERR_INVALID_PARAMS, text::UNKNOWN_SESSION);
        return;
    };
    if session.is_running() {
        let _ = writer.send_error(id, ERR_SERVER, text::PROMPT_IN_PROGRESS);
        return;
    }
    let Some(config_id) = params.get("configId").and_then(Value::as_str) else {
        let _ = writer.send_error(id, ERR_INVALID_PARAMS, text::INVALID_CONFIG_OPTION);
        return;
    };
    let Some(value) = params.get("value").and_then(Value::as_str).filter(|value| {
        !value.is_empty() && value.chars().all(|c| !c.is_control() && !c.is_whitespace())
    }) else {
        let _ = writer.send_error(id, ERR_INVALID_PARAMS, text::INVALID_CONFIG_OPTION);
        return;
    };
    let SessionState::Idle(agent) = &mut session.state else {
        let _ = writer.send_error(id, ERR_SERVER, text::PROMPT_IN_PROGRESS);
        return;
    };
    match config_id {
        "model" => {
            agent.switch_model(value, None);
        }
        "effort" => {
            if !session.supports_effort {
                let _ = writer.send_error(id, ERR_INVALID_PARAMS, text::EFFORT_UNSUPPORTED);
                return;
            }
            let current_model = agent.status().model;
            agent.switch_model(&current_model, Some(value));
        }
        _ => {
            let _ = writer.send_error(id, ERR_INVALID_PARAMS, text::INVALID_CONFIG_OPTION);
            return;
        }
    }
    let config_options = updates::config_options(
        &agent.status().model,
        agent.effort(),
        session.supports_effort,
    );
    let _ = writer.send_result(id, json!({ "configOptions": config_options }));
}

fn handle_prompt(
    id: Value,
    params: &Value,
    active: &mut Option<ActiveSession>,
    generation: &mut u64,
    wake_tx: &SyncSender<()>,
    writer: &mut OutputWriter,
) {
    let Some(requested_id) = params.get("sessionId").and_then(Value::as_str) else {
        let _ = writer.send_error(id, ERR_INVALID_PARAMS, text::UNKNOWN_SESSION);
        return;
    };
    let Some(session) = active
        .as_mut()
        .filter(|session| session.session_id == requested_id)
    else {
        let _ = writer.send_error(id, ERR_INVALID_PARAMS, text::UNKNOWN_SESSION);
        return;
    };
    if session.is_running() {
        let _ = writer.send_error(id, ERR_SERVER, text::PROMPT_IN_PROGRESS);
        return;
    }
    let Some(prompt_value) = params.get("prompt") else {
        let _ = writer.send_error(id, ERR_INVALID_PARAMS, text::INVALID_PROMPT);
        return;
    };
    let parsed = match prompt::parse_prompt(prompt_value, session.image_input) {
        Ok(parsed) => parsed,
        Err(message) => {
            let _ = writer.send_error(id, ERR_INVALID_PARAMS, message);
            return;
        }
    };
    *generation = generation.saturating_add(1);
    let cancel_step = Arc::new(AtomicBool::new(false));
    let cancel_run = Arc::new(AtomicBool::new(false));
    let (events_tx, events_rx) = mpsc::sync_channel::<AgentEvent>(64);
    let (done_tx, done_rx) = mpsc::sync_channel::<PromptDone>(1);

    let previous = std::mem::replace(
        &mut session.state,
        SessionState::Running(RunningPrompt {
            prompt_id: id,
            cancel_step: Arc::clone(&cancel_step),
            cancel_run: Arc::clone(&cancel_run),
            cancelled: false,
            events_rx,
            done_rx,
            handle: None,
            tool_states: BTreeMap::new(),
        }),
    );
    let SessionState::Idle(mut agent) = previous else {
        return;
    };
    let mut control = crate::agent::RunControl::without_steering(cancel_step, cancel_run);
    control.inline_images = parsed.inline_images;
    let agent_wake = wake_tx.clone();
    match thread::Builder::new()
        .name("acp-agent".into())
        .spawn(move || {
            let result = agent.run_controlled(parsed.text, control, events_tx);
            let estimate = agent.context_estimate();
            let _sent = done_tx.send(PromptDone {
                agent: *agent,
                result,
                estimate,
            });
            let _woken = agent_wake.try_send(());
        }) {
        Ok(handle) => {
            if let SessionState::Running(running) = &mut session.state {
                running.handle = Some(handle);
            }
        }
        Err(_) => {
            let failed_id = match std::mem::replace(
                &mut session.state,
                SessionState::Running(dummy_closed_running()),
            ) {
                SessionState::Running(running) => running.prompt_id,
                SessionState::Idle(_) => Value::Null,
            };
            let _ = writer.send_error(failed_id, ERR_INTERNAL, text::INTERNAL_ERROR);
        }
    }
}

fn handle_cancel(
    request_id: Option<Value>,
    params: &Value,
    active: &mut Option<ActiveSession>,
    ledger: &mut PermissionLedger,
    writer: &mut OutputWriter,
) {
    let requested_id = params.get("sessionId").and_then(Value::as_str);
    let matches_active = active
        .as_ref()
        .is_some_and(|session| requested_id == Some(session.session_id.as_str()));
    if !matches_active {
        // 正規の id なし通知は JSON-RPC 2.0 §4.1 に従い応答せず無視し、
        // id 付き要求は -32602 を返す（設計書 05-03「セッションの多重度」）。
        if let Some(id) = request_id {
            let _ = writer.send_error(id, ERR_INVALID_PARAMS, text::UNKNOWN_SESSION);
        }
        return;
    }
    if let Some(session) = active.as_mut() {
        if let SessionState::Running(running) = &mut session.state {
            running.cancelled = true;
            running.cancel_run.store(true, Ordering::Release);
            running.cancel_step.store(true, Ordering::Release);
        }
        ledger.cancel_all();
        while let Ok(prompt) = session.approvals_rx.try_recv() {
            let _sent = prompt
                .reply
                .send(ApprovalAnswer::new(ApprovalOutcome::Cancelled, None));
        }
    }
    if let Some(id) = request_id {
        let _ = writer.send_result(id, json!({}));
    }
}
