//! Agent の所有権を仕事のスレッドに置き、画面は錠なしでイベントを読む（設計書 06-01「スレッドと非同期」）。

use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
use std::thread::{self, JoinHandle};

use crate::agent::{Agent, AgentError, AgentEvent, Outcome, RunReport};
use crate::session::{Record, Session};

use super::Screen;

// 後の操縦と後続は仕事ごとの制御に加え、/compact はこの列挙に加える。
// Agent を Mutex で包まず、依頼の間に見積もりを返す（実装プラン P4-06）。
enum Job {
    Run {
        request: String,
        control: crate::agent::RunControl,
    },
}

struct Completion {
    result: Result<RunReport, AgentError>,
    estimate: u64,
}

/// 仕事ごとの制御。終える印とは別に作り、後の二段階の取り消しもここへ加える。
#[derive(Clone, Default)]
pub struct RunControl {
    cancel_step: Arc<AtomicBool>,
    cancel_run: Arc<AtomicBool>,
}
impl RunControl {
    /// 現在の仕事だけを止める。
    pub fn cancel(&self) {
        self.cancel_run.store(true, Ordering::Release);
        self.cancel_step.store(true, Ordering::Release);
    }
}

/// 依頼ごとの取り消しと、上限付きイベント列の受け口。
pub struct Worker {
    jobs: Option<mpsc::SyncSender<Job>>,
    events: mpsc::Receiver<AgentEvent>,
    completions: mpsc::Receiver<Completion>,
    control: Option<RunControl>,
    steering: Option<mpsc::Sender<String>>,
    pending_steering: std::collections::VecDeque<String>,
    thread: Option<JoinHandle<()>>,
    approvals: Option<mpsc::Receiver<crate::approval::ApprovalPrompt>>,
}

impl Worker {
    /// Agent とセッションを移し、仕事が届くまで待つ。
    pub fn start(mut agent: Agent, session: Session) -> io::Result<Self> {
        let (jobs, requests) = mpsc::sync_channel(1);
        let (events, receiver) = mpsc::sync_channel(64);
        let (done, completions) = mpsc::sync_channel(1);
        let thread = thread::Builder::new().name("agent".into()).spawn(move || {
            while let Ok(job) = requests.recv() {
                let result = match job {
                    Job::Run { request, control } => agent
                        .run_controlled(request, control, events.clone())
                        .and_then(|report| {
                            session.writer()?.append(&Record::Summary {
                                summary: report.summary.clone(),
                            })?;
                            Ok(report)
                        }),
                };
                let completion = Completion {
                    result,
                    estimate: agent.context_estimate(),
                };
                if done.send(completion).is_err() {
                    break;
                }
            }
        })?;
        Ok(Self {
            jobs: Some(jobs),
            events: receiver,
            completions,
            control: None,
            steering: None,
            pending_steering: std::collections::VecDeque::new(),
            thread: Some(thread),
            approvals: None,
        })
    }

    /// Agent に設定した確認の受け口を画面側へ渡す。
    pub fn with_approval_requests(
        mut self,
        requests: mpsc::Receiver<crate::approval::ApprovalPrompt>,
    ) -> Self {
        self.approvals = Some(requests);
        self
    }

    /// 各依頼に新しい取り消しの印を作る。実行中に二つ目を送らない。
    pub fn submit(&mut self, request: String, control: RunControl) -> Result<(), AgentError> {
        if self.is_running() {
            return Err(AgentError::Worker);
        }
        let (steering, receiver) = mpsc::channel();
        self.jobs
            .as_ref()
            .ok_or(AgentError::Worker)?
            .send(Job::Run {
                request,
                control: crate::agent::RunControl {
                    cancel_step: Arc::clone(&control.cancel_step),
                    cancel_run: Arc::clone(&control.cancel_run),
                    steering: receiver,
                },
            })
            .map_err(|_| AgentError::Worker)?;
        self.control = Some(control);
        self.steering = Some(steering);
        Ok(())
    }

    /// 現在の仕事だけを取り消す。終了の印とは共有しない。
    pub fn cancel(&self) {
        if let Some(control) = &self.control {
            control.cancel();
        }
    }

    /// 一度の ESC は、道具の段階だけに印を渡す。
    pub fn cancel_step(&self) {
        if let Some(control) = &self.control {
            control.cancel_step.store(true, Ordering::Release);
        }
    }

    /// 会話へ加えた通知は Agent が返す。
    pub fn steer(&mut self, text: String) -> Result<(), String> {
        let Some(sender) = &self.steering else {
            return Err(text);
        };
        sender.send(text.clone()).map_err(|error| error.0)?;
        self.pending_steering.push_back(text);
        Ok(())
    }

    fn event(&mut self, screen: &mut Screen, event: AgentEvent) {
        if matches!(event, AgentEvent::Steered { .. }) {
            self.pending_steering.pop_front();
        }
        screen.event(&event);
    }

    /// 処理中かどうか。
    pub fn is_running(&self) -> bool {
        self.control.is_some()
    }

    /// その時点で届いたイベントをすべて読む。終了前にも必ず呼び続ける。
    pub fn drain(&mut self, screen: &mut Screen) -> Result<bool, AgentError> {
        let mut changed = false;
        while let Ok(event) = self.events.try_recv() {
            self.event(screen, event);
            changed = true;
        }
        match self.completions.try_recv() {
            Ok(completion) => {
                // 別のチャネルの完了が、最後のイベントの受信より先に見える場合を処理する。
                while let Ok(event) = self.events.try_recv() {
                    self.event(screen, event);
                }
                screen.complete(completion.estimate);
                self.control = None;
                // 受け口が閉じる前に UI の送り手を外す。
                self.steering = None;
                let report = completion.result?;
                // 受け口を閉じる直前に送れた文も、加えた通知がなければ入力へ戻す。
                screen.finished(
                    report.outcome == Outcome::Cancelled,
                    self.pending_steering.drain(..).collect(),
                );
                changed = true;
            }
            Err(mpsc::TryRecvError::Empty) => {}
            Err(mpsc::TryRecvError::Disconnected) if self.is_running() => {
                self.control = None;
                return Err(AgentError::Worker);
            }
            Err(mpsc::TryRecvError::Disconnected) => {}
        }
        if !screen.confirming()
            && let Some(requests) = &self.approvals
            && let Ok(prompt) = requests.try_recv()
        {
            // 確認の要求より先に送られたイベントを必ず表示へ反映する。
            while let Ok(event) = self.events.try_recv() {
                self.event(screen, event);
            }
            screen.open_approval(prompt);
            changed = true;
        }
        Ok(changed)
    }

    /// 仕事の完了とイベントの受信を終えてからスレッドを閉じる。
    pub fn close(mut self) -> Result<(), AgentError> {
        self.jobs.take();
        if let Some(thread) = self.thread.take() {
            thread.join().map_err(|_| AgentError::Worker)?;
        }
        Ok(())
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        self.cancel();
        // receiver の破棄で send の待ちを解く。通常の終了では close で join 済み。
        self.jobs.take();
    }
}
