//! 確認ごとに独立した返事の口で画面と受け渡す（設計書 03-03「確認の共通の規則」）。

use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, Sender, SyncSender};

use super::{ApprovalAnswer, ApprovalOutcome, ApprovalRecord, Approver};

/// 確認を送った直後に画面やイベントループの待ちを起こす通知先。
pub type WakeCallback = Arc<dyn Fn() + Send + Sync>;

/// 画面へ渡す一回の確認。返事を次の要求と共有しない。
pub struct ApprovalPrompt {
    pub request: ApprovalRecord,
    pub reply: SyncSender<ApprovalAnswer>,
}

/// Agent から画面に依存せずに確認を求める受け手。
pub struct InteractiveApprover {
    requests: Sender<ApprovalPrompt>,
    wake: Option<WakeCallback>,
}

impl InteractiveApprover {
    /// セッションを作り直すたびに、新しい受け口を作る。
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

    /// 確認を送った直後に画面の待ちを起こす通知先を添える。
    pub fn with_wake(mut self, wake: WakeCallback) -> Self {
        self.wake = Some(wake);
        self
    }

    /// 同じ画面へ確認を送る口。通信の中継が、コマンドの実行の途中に尋ねるときに使う。
    pub fn prompts(&self) -> Sender<ApprovalPrompt> {
        self.requests.clone()
    }

    pub(crate) fn wake(&self) -> Option<WakeCallback> {
        self.wake.as_ref().map(Arc::clone)
    }
}

impl Approver for InteractiveApprover {
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
