//! Agent の所有権を仕事のスレッドに置き、画面は錠なしでイベントを読む（設計書 06-01「スレッドと非同期」）。

use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
use std::thread::{self, JoinHandle};

use crate::agent::{Agent, AgentError, AgentEvent};
use crate::session::{Record, Session};

use super::Screen;

// 後の操縦と後続は仕事ごとの制御に加え、/compact はこの列挙に加える。
// Agent を Mutex で包まず、依頼の間に見積もりを返す（実装プラン P4-06）。
enum Job {
    Run {
        request: String,
        control: RunControl,
    },
}

struct Completion {
    result: Result<(), AgentError>,
    estimate: u64,
}

/// 仕事ごとの制御。終える印とは別に作り、後の二段階の取り消しもここへ加える。
#[derive(Clone, Default)]
pub struct RunControl {
    cancelled: Arc<AtomicBool>,
}
impl RunControl {
    /// 現在の仕事だけを止める。
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
    }
}

/// 依頼ごとの取り消しと、上限付きイベント列の受け口。
pub struct Worker {
    jobs: Option<mpsc::SyncSender<Job>>,
    events: mpsc::Receiver<AgentEvent>,
    completions: mpsc::Receiver<Completion>,
    control: Option<RunControl>,
    thread: Option<JoinHandle<()>>,
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
                        .run(request, Arc::clone(&control.cancelled), events.clone())
                        .and_then(|report| {
                            session.writer()?.append(&Record::Summary {
                                summary: report.summary,
                            })?;
                            Ok(())
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
            thread: Some(thread),
        })
    }

    /// 各依頼に新しい取り消しの印を作る。実行中に二つ目を送らない。
    pub fn submit(&mut self, request: String, control: RunControl) -> Result<(), AgentError> {
        if self.is_running() {
            return Err(AgentError::Worker);
        }
        self.jobs
            .as_ref()
            .ok_or(AgentError::Worker)?
            .send(Job::Run {
                request,
                control: control.clone(),
            })
            .map_err(|_| AgentError::Worker)?;
        self.control = Some(control);
        Ok(())
    }

    /// 現在の仕事だけを取り消す。終了の印とは共有しない。
    pub fn cancel(&self) {
        if let Some(control) = &self.control {
            control.cancel();
        }
    }

    /// 処理中かどうか。
    pub fn is_running(&self) -> bool {
        self.control.is_some()
    }

    /// その時点で届いたイベントをすべて読む。終了前にも必ず呼び続ける。
    pub fn drain(&mut self, screen: &mut Screen) -> Result<bool, AgentError> {
        let mut changed = false;
        while let Ok(event) = self.events.try_recv() {
            screen.event(&event);
            changed = true;
        }
        match self.completions.try_recv() {
            Ok(completion) => {
                // 別のチャネルの完了が、最後のイベントの受信より先に見える場合を処理する。
                while let Ok(event) = self.events.try_recv() {
                    screen.event(&event);
                }
                screen.complete(completion.estimate);
                self.control = None;
                completion.result?;
                changed = true;
            }
            Err(mpsc::TryRecvError::Empty) => {}
            Err(mpsc::TryRecvError::Disconnected) if self.is_running() => {
                self.control = None;
                return Err(AgentError::Worker);
            }
            Err(mpsc::TryRecvError::Disconnected) => {}
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
