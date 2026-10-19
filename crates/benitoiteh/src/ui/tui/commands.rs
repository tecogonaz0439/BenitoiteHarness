//! 入力欄から送った文だけをコマンドへ変える（設計書 05-02「`/` で始まるコマンド」）。

use super::{Action, Screen, Status, text};
use crate::agent::Agent;
use crate::session::Session;

/// 画面の入力だけから作る操作。AgentInput と出力のイベントでは作らない。
#[derive(Debug, PartialEq)]
pub enum Command {
    Help,
    Status,
    Compact(Option<String>),
    Rewind,
    Tree,
    New,
    /// 選択を終えた時点と範囲を、働き手へ渡す。
    PreviewRewind(String, crate::rewind::RewindScope),
    /// 木で選んだ移動先と範囲を、働き手で確かめる。
    PreviewBranch(crate::agent::BranchTarget, crate::rewind::RewindScope),
}

/// 新しいセッションの準備。端末と panic の書き先は CLI が設定する。
pub struct NewSession {
    pub agent: Agent,
    pub session: Session,
    pub status: Status,
    pub diagnostics: Vec<String>,
    pub(super) activate: Option<Box<dyn FnOnce()>>,
}
impl NewSession {
    /// 設定の読み直しに成功したセッションを渡す。
    pub fn new(agent: Agent, session: Session, status: Status, diagnostics: Vec<String>) -> Self {
        Self {
            agent,
            session,
            status,
            diagnostics,
            activate: None,
        }
    }
    /// 前の Agent の破棄を確かめた後に、panic の書き先を切り替える。
    pub fn on_start(mut self, activate: impl FnOnce() + 'static) -> Self {
        self.activate = Some(Box::new(activate));
        self
    }
}

impl Screen {
    pub(super) fn command_action(&mut self, action: Action, running: bool) -> Action {
        let (raw, channel) = match action {
            Action::Submit(value) => (value, 0),
            Action::Steer(value) => (value, 1),
            Action::FollowUp(value) => (value, 2),
            other @ (Action::None
            | Action::Command(_)
            | Action::CancelStep
            | Action::Cancel
            | Action::Quit) => return other,
        };
        self.remember(&raw);
        self.original_submission = Some(raw.clone());
        let send = |value| match channel {
            0 => Action::Submit(value),
            1 => Action::Steer(value),
            _ => Action::FollowUp(value),
        };
        if let Some(value) = raw.strip_prefix("//") {
            return send(format!("/{value}"));
        }
        let Some(rest) = raw.strip_prefix('/') else {
            return send(raw);
        };
        if running && channel == 2 {
            self.notice(text::COMMAND_RUNNING);
            return Action::None;
        }
        let boundary = rest.find(char::is_whitespace).unwrap_or(rest.len());
        let name = rest.get(..boundary).unwrap_or_default();
        let arguments = rest
            .get(boundary..)
            .unwrap_or_default()
            .trim_start_matches(char::is_whitespace);
        if !name.is_empty() && !name.bytes().all(|byte| byte.is_ascii_lowercase()) {
            return send(raw);
        }
        if running && (name != "quit" || !arguments.trim().is_empty()) {
            self.notice(text::COMMAND_RUNNING);
            return Action::None;
        }
        if matches!(name, "help" | "status" | "rewind" | "tree" | "new" | "quit")
            && !arguments.trim().is_empty()
        {
            self.notice(text::COMMAND_ARGUMENTS);
            return Action::None;
        }
        match name {
            "help" => Action::Command(Command::Help),
            "status" => Action::Command(Command::Status),
            "compact" => Action::Command(Command::Compact(
                (!arguments.trim().is_empty()).then(|| arguments.to_owned()),
            )),
            "rewind" => Action::Command(Command::Rewind),
            "tree" => Action::Command(Command::Tree),
            "new" => Action::Command(Command::New),
            "quit" => Action::Quit,
            _ => {
                self.notice(text::UNKNOWN_COMMAND);
                Action::None
            }
        }
    }

    pub(super) fn submitted(&mut self, text: String) -> super::SubmittedInput {
        let original = self
            .original_submission
            .take()
            .unwrap_or_else(|| text.clone());
        super::SubmittedInput { text, original }
    }

    pub(super) fn command_status(
        &mut self,
        status: &crate::agent::AgentStatus,
        tokens: &crate::session::TokenCounts,
    ) {
        for line in text::session_status(status, tokens).lines() {
            self.notice(line);
        }
    }

    pub(super) fn compact_result(&mut self, report: &crate::agent::CompactReport) {
        self.notice(&text::compact_result(report));
    }
}
