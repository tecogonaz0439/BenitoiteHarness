//! `AcpApprover`、確認の台帳、および `session/request_permission` の組み立てと応答の解釈
//! （設計書 03-03、05-03「実行の前の確認（`session/request_permission`）」）。

use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender, SyncSender};

use serde_json::{Value, json};

use crate::approval::{
    ApprovalAnswer, ApprovalKind, ApprovalOutcome, ApprovalPrompt, ApprovalRecord, Approver,
    WakeCallback,
};
use crate::ui::tui::{approval_fact_entries, sanitize_fact_text};

use super::text;

/// プロセス内で単調増加し再利用しない確認の要求 ID の連番（設計書 05-03）。
static NEXT_PERMISSION_ID: AtomicU64 = AtomicU64::new(1);

const OPT_ALLOW_ONCE: &str = "allow-once";
const OPT_ALLOW_SESSION: &str = "allow-session";
const OPT_REJECT_ONCE: &str = "reject-once";

/// ACP クライアントへ確認の要求を仲介する受け手。
/// `Agent::with_approver` と `Agent::connect_network_prompts` の両方へ同じ口を繋ぐ。
pub struct AcpApprover {
    requests: Sender<ApprovalPrompt>,
    wake: Option<WakeCallback>,
}

impl AcpApprover {
    /// 確認の受け手と主スレッド側の受信口を作る。
    pub fn channel() -> (Self, Receiver<ApprovalPrompt>) {
        let (requests, receiver) = mpsc::channel();
        (
            Self {
                requests,
                wake: None,
            },
            receiver,
        )
    }

    /// 確認を送った直後に主スレッドを起こす通知先を添える。
    pub fn with_wake(mut self, wake: WakeCallback) -> Self {
        self.wake = Some(wake);
        self
    }

    /// `NetworkGate`（通信の中継）の確認も同じ台帳へ繋ぐための送信口。
    pub fn prompts(&self) -> Sender<ApprovalPrompt> {
        self.requests.clone()
    }

    /// 通信の中継から主スレッドを起こす通知先。
    pub fn wake(&self) -> Option<WakeCallback> {
        self.wake.as_ref().map(Arc::clone)
    }
}

impl Approver for AcpApprover {
    fn approve(&mut self, request: &ApprovalRecord) -> ApprovalAnswer {
        let (reply, response) = mpsc::sync_channel(1);
        let sent = self
            .requests
            .send(ApprovalPrompt {
                request: request.clone(),
                reply,
            })
            .is_ok();
        if sent && let Some(wake) = &self.wake {
            wake();
        }
        if sent && let Ok(answer) = response.recv() {
            return answer;
        }
        ApprovalAnswer::new(ApprovalOutcome::Cancelled, None)
    }
}

struct PendingPermission {
    session_id: String,
    generation: u64,
    kind: ApprovalKind,
    presented_options: Vec<&'static str>,
    reply: SyncSender<ApprovalAnswer>,
}

/// 主スレッドが単一の持ち主として管理する保留中の確認の台帳（設計書 05-03）。
#[derive(Default)]
pub(super) struct PermissionLedger {
    pending: BTreeMap<String, PendingPermission>,
}

impl PermissionLedger {
    /// 新しい確認要求を台帳へ登録し、単調増加の要求 ID と `session/request_permission` の params を返す。
    pub(super) fn register(
        &mut self,
        session_id: &str,
        generation: u64,
        prompt: ApprovalPrompt,
    ) -> (String, Value) {
        let number = NEXT_PERMISSION_ID.fetch_add(1, Ordering::Relaxed);
        let request_id = format!("bh-perm-{number}");
        let options = permission_options(&prompt.request);
        let presented_options = options.iter().map(|opt| opt.id).collect::<Vec<_>>();
        let params = build_permission_params(session_id, &prompt.request, &options);
        self.pending.insert(
            request_id.clone(),
            PendingPermission {
                session_id: session_id.to_owned(),
                generation,
                kind: prompt.request.kind,
                presented_options,
                reply: prompt.reply,
            },
        );
        (request_id, params)
    }

    /// クライアントからの JSON-RPC 応答を台帳と照合し、保留中の要求へ反映する。
    /// 保留のない ID への遅延・重複応答は無視する（設計書 05-03）。
    pub(super) fn resolve(
        &mut self,
        active_session_id: Option<&str>,
        current_generation: u64,
        id: &Value,
        result: Option<&Value>,
        error: Option<&Value>,
    ) {
        let Some(request_id) = id.as_str() else {
            return;
        };
        let Some(entry) = self.pending.remove(request_id) else {
            return;
        };
        if active_session_id != Some(entry.session_id.as_str())
            || entry.generation != current_generation
        {
            let _sent = entry
                .reply
                .send(ApprovalAnswer::new(ApprovalOutcome::Cancelled, None));
            return;
        }
        let answer = parse_permission_response(&entry, result, error);
        let _sent = entry.reply.send(answer);
    }

    /// 保留中の全確認（通常確認と通信確認）を即時に Cancelled で完了させ、台帳を空にする
    /// （設計書 05-03「スレッドの構成と終了の手順」の段 2）。
    pub(super) fn cancel_all(&mut self) {
        let entries = std::mem::take(&mut self.pending);
        for (_, entry) in entries {
            let _sent = entry
                .reply
                .send(ApprovalAnswer::new(ApprovalOutcome::Cancelled, None));
        }
    }
}

struct OptionSpec {
    id: &'static str,
    name: &'static str,
    kind: &'static str,
}

/// 設計書 05-03「確認の種類ごとの選択肢と記憶の範囲の対応表」に従い選択肢を決める。
fn permission_options(request: &ApprovalRecord) -> Vec<OptionSpec> {
    let allow_session = match request.kind {
        ApprovalKind::Effects | ApprovalKind::TestEffects => {
            !request.facts.agents_write && request.facts.unread_imports == 0
        }
        ApprovalKind::Command | ApprovalKind::WebFetch => !request.facts.agents_write,
        ApprovalKind::Network => true,
        ApprovalKind::Escalate | ApprovalKind::WriteScope | ApprovalKind::CreateProject => false,
    };
    let allow_once_name = if request.kind == ApprovalKind::Network {
        text::OPTION_ALLOW_COMMAND
    } else {
        text::OPTION_ALLOW_ONCE
    };
    let allow_session_name = if request.kind == ApprovalKind::Network {
        text::OPTION_ALLOW_DOMAIN_SESSION
    } else {
        text::OPTION_ALLOW_SESSION
    };
    let mut options = vec![OptionSpec {
        id: OPT_ALLOW_ONCE,
        name: allow_once_name,
        kind: "allow_once",
    }];
    if allow_session {
        options.push(OptionSpec {
            id: OPT_ALLOW_SESSION,
            name: allow_session_name,
            kind: "allow_always",
        });
    }
    options.push(OptionSpec {
        id: OPT_REJECT_ONCE,
        name: text::OPTION_DECLINE,
        kind: "reject_once",
    });
    options
}

/// 確認要求の `toolCall.kind`。T3Code で自動承認される read・search・think は絶対に使わない
/// （設計書 05-03「実行の前の確認」）。
fn permission_tool_kind(kind: ApprovalKind) -> &'static str {
    match kind {
        ApprovalKind::Effects
        | ApprovalKind::TestEffects
        | ApprovalKind::Command
        | ApprovalKind::Escalate => "execute",
        ApprovalKind::WriteScope | ApprovalKind::CreateProject => "edit",
        ApprovalKind::WebFetch | ApprovalKind::Network => "fetch",
    }
}

/// `benitoiteh` が確かめた事実の全文を組み立てる。LLM の `justification` は一切含めない
/// （設計書 05-03「実行の前の確認」）。
pub(super) fn format_permission_facts(request: &ApprovalRecord) -> String {
    let title = text::permission_title(request);
    let mut lines = vec![title.to_owned()];
    let mut entries = approval_fact_entries(request);
    if request.kind == ApprovalKind::Escalate {
        entries.push((text::SCOPE_LABEL, text::ESCALATE_SCOPE.to_owned()));
    }
    if request.kind == ApprovalKind::Network
        && let Some(domain) = request.facts.domain.as_deref()
    {
        entries.push((text::SCOPE_LABEL, text::network_scope(domain)));
    }
    for (label, value) in entries {
        lines.push(format!("{label}:"));
        let sanitized = sanitize_fact_text(&value);
        for line in sanitized.split('\n') {
            lines.push(format!("  {line}"));
        }
    }
    lines.join("\n")
}

fn build_permission_params(
    session_id: &str,
    request: &ApprovalRecord,
    options: &[OptionSpec],
) -> Value {
    let title = text::permission_title(request);
    let full_facts = format_permission_facts(request);
    let options_json = options
        .iter()
        .map(|opt| {
            json!({
                "optionId": opt.id,
                "name": opt.name,
                "kind": opt.kind,
            })
        })
        .collect::<Vec<_>>();
    // T3Code の承認カードは rawInput.command を最優先で表示するため、
    // 事実の全文を rawInput.command と content の両方に入れる（設計書 05-03）。
    json!({
        "sessionId": session_id,
        "toolCall": {
            "toolCallId": request.call_id,
            "title": title,
            "kind": permission_tool_kind(request.kind),
            "status": "pending",
            "rawInput": {
                "command": full_facts,
            },
            "content": [
                {
                    "type": "content",
                    "content": {
                        "type": "text",
                        "text": full_facts,
                    }
                }
            ]
        },
        "options": options_json,
    })
}

fn parse_permission_response(
    entry: &PendingPermission,
    result: Option<&Value>,
    error: Option<&Value>,
) -> ApprovalAnswer {
    if error.is_some() {
        return ApprovalAnswer::new(ApprovalOutcome::Denied, None);
    }
    let Some(outcome_obj) = result
        .and_then(|value| value.get("outcome"))
        .and_then(Value::as_object)
    else {
        return ApprovalAnswer::new(ApprovalOutcome::Denied, None);
    };
    match outcome_obj.get("outcome").and_then(Value::as_str) {
        Some("cancelled") => ApprovalAnswer::new(ApprovalOutcome::Cancelled, None),
        Some("selected") => {
            let Some(option_id) = outcome_obj.get("optionId").and_then(Value::as_str) else {
                return ApprovalAnswer::new(ApprovalOutcome::Denied, None);
            };
            // 未提示の optionId（一回限りの確認に対する allow-session など）は拒否にする（設計書 05-03）。
            if !entry.presented_options.contains(&option_id) {
                return ApprovalAnswer::new(ApprovalOutcome::Denied, None);
            }
            match option_id {
                OPT_ALLOW_ONCE => match entry.kind {
                    ApprovalKind::Effects | ApprovalKind::TestEffects => {
                        ApprovalAnswer::approved_once()
                    }
                    ApprovalKind::Command
                    | ApprovalKind::Escalate
                    | ApprovalKind::WriteScope
                    | ApprovalKind::CreateProject
                    | ApprovalKind::WebFetch
                    | ApprovalKind::Network => ApprovalAnswer::new(ApprovalOutcome::Approved, None),
                },
                OPT_ALLOW_SESSION => match entry.kind {
                    ApprovalKind::Effects | ApprovalKind::TestEffects => {
                        ApprovalAnswer::new(ApprovalOutcome::Approved, None)
                    }
                    ApprovalKind::Command | ApprovalKind::WebFetch | ApprovalKind::Network => {
                        ApprovalAnswer::new(ApprovalOutcome::ApprovedForSession, None)
                    }
                    ApprovalKind::Escalate
                    | ApprovalKind::WriteScope
                    | ApprovalKind::CreateProject => {
                        ApprovalAnswer::new(ApprovalOutcome::Denied, None)
                    }
                },
                OPT_REJECT_ONCE => ApprovalAnswer::new(ApprovalOutcome::Denied, None),
                _ => ApprovalAnswer::new(ApprovalOutcome::Denied, None),
            }
        }
        _ => ApprovalAnswer::new(ApprovalOutcome::Denied, None),
    }
}
