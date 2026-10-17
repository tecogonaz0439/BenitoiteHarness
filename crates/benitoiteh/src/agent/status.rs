//! 対話のコマンドへ認証情報を含まない状態の写しを渡す（設計書 05-02「`/` で始まるコマンド」）。

use super::{Agent, AgentError};
use crate::config::{Approval, Mode, Sandbox};
use crate::session::{Record, SkippedInstruction};
use std::path::PathBuf;

/// 設定を読み直しても置き換えない、解決済みの安全設定（実装プラン P4-08）。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SessionSafety {
    pub network: bool,
    /// プロジェクトとセッションの一時領域を除く、追加の書ける場所。
    pub writable_roots: Vec<PathBuf>,
    pub allowed_programs: Vec<PathBuf>,
}

/// セッションの設定と文脈。使用量はプロセス内の働き手が別に集計する。
#[derive(Clone, Debug)]
pub struct AgentStatus {
    pub session_id: String,
    pub mode: Mode,
    pub project: PathBuf,
    pub provider: String,
    pub model: String,
    pub host: String,
    pub sandbox: Sandbox,
    pub sandbox_disabled: bool,
    pub approval: Approval,
    pub safety: SessionSafety,
    pub estimate: u64,
    pub context_window: Option<u64>,
    pub instruction_files: Vec<PathBuf>,
    pub skipped_instructions: Vec<SkippedInstruction>,
}

impl Agent {
    /// URL の認証情報とパスを画面へ渡さず、ホストだけを返す。
    pub fn status(&self) -> AgentStatus {
        AgentStatus {
            session_id: self.session.id().to_string(),
            mode: self.config.mode,
            project: self.project_dir().to_owned(),
            provider: self.config.destination.provider.clone(),
            model: self.config.destination.model.clone(),
            host: self
                .config
                .destination
                .base_url
                .parse::<crate::http::Uri>()
                .ok()
                .and_then(|uri| uri.host().map(str::to_owned))
                .unwrap_or_default(),
            sandbox: self.approval_policy.sandbox,
            sandbox_disabled: !self.approval_policy.sandboxed
                && self.approval_policy.sandbox != Sandbox::DangerFullAccess,
            approval: self.approval_policy.approval,
            safety: self.session_safety.clone(),
            estimate: self.context_estimate(),
            context_window: self.context_window(),
            instruction_files: self
                .environment
                .project_instructions()
                .iter()
                .map(|file| file.path.clone())
                .collect(),
            skipped_instructions: self.environment.skipped_instructions().to_vec(),
        }
    }

    /// 起動側が実パスで検査した値を、次の /new へ渡すために保持する。
    pub fn with_session_safety(mut self, safety: SessionSafety) -> Self {
        self.session_safety = safety;
        self
    }

    /// 新しいセッションの準備が済んだ後だけ、前の記録を閉じる。
    pub fn end(&mut self, reason: &str) -> Result<(), AgentError> {
        self.writer.append(&Record::SessionEnd {
            outcome: super::text::SESSION_COMPLETED.into(),
            reason: reason.to_owned(),
        })?;
        Ok(())
    }
}
