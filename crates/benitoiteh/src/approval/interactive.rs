//! 確認ごとに独立した返事の口で画面と受け渡す（設計書 03-03「確認の共通の規則」）。

use std::sync::mpsc::{self, Receiver, Sender, SyncSender};

use super::{ApprovalAnswer, ApprovalOutcome, ApprovalRecord, Approver};

/// 画面へ渡す一回の確認。返事を次の要求と共有しない。
pub struct ApprovalPrompt {
    pub request: ApprovalRecord,
    pub reply: SyncSender<ApprovalAnswer>,
}

/// Agent から画面に依存せずに確認を求める受け手。
pub struct InteractiveApprover {
    requests: Sender<ApprovalPrompt>,
}

impl InteractiveApprover {
    /// セッションを作り直すたびに、新しい受け口を作る。
    pub fn channel() -> (Self, Receiver<ApprovalPrompt>) {
        let (requests, receiver) = mpsc::channel();
        (Self { requests }, receiver)
    }
}

impl Approver for InteractiveApprover {
    fn approve(&mut self, request: &ApprovalRecord) -> ApprovalAnswer {
        let (reply, response) = mpsc::sync_channel(1);
        if self
            .requests
            .send(ApprovalPrompt {
                request: request.clone(),
                reply,
            })
            .is_ok()
            && let Ok(answer) = response.recv()
        {
            return answer;
        }
        ApprovalAnswer {
            outcome: ApprovalOutcome::Cancelled,
            message: None,
        }
    }
}
