//! Agent の所有権を仕事のスレッドに置き、画面は錠なしでイベントを読む（設計書 06-01「スレッドと非同期」）。

use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
use std::thread::{self, JoinHandle};

use crate::agent::{
    Agent, AgentError, AgentEvent, AgentStatus, BranchPreview, BranchTarget, BranchTreeRow,
    CompactOutcome, CompactReport, Outcome, RewindPreview, RunReport,
};
use crate::rewind::{CheckpointInfo, RewindError, RewindReport, RewindScope};
use crate::session::{Record, Session};
use crate::session::{Summary, TokenCounts};

use super::{Screen, selection::PendingMove};

// Agent の変更はすべて同じ働き手で直列に行う（設計書 06-01「スレッドと非同期」）。
enum Job {
    Run {
        request: String,
        control: crate::agent::RunControl,
    },
    Compact {
        instructions: Option<String>,
        cancelled: Arc<AtomicBool>,
    },
    Status,
    ListCheckpoints,
    ListBranches,
    ListForkRequests,
    PreviewBranch {
        target: BranchTarget,
        scope: RewindScope,
    },
    SwitchBranch {
        target: BranchTarget,
        checkpoint: Option<String>,
        scope: RewindScope,
        allow_incomplete: bool,
    },
    PreviewRewind {
        checkpoint: String,
        scope: RewindScope,
    },
    Rewind {
        checkpoint: String,
        scope: RewindScope,
        allow_incomplete: bool,
    },
    Pause {
        ready: mpsc::SyncSender<Session>,
        resume: mpsc::Receiver<()>,
    },
    End,
}
enum Reply {
    Run(RunReport),
    Compact(CompactReport),
    Status(AgentStatus, TokenCounts),
    ListCheckpoints(Vec<CheckpointInfo>),
    ListBranches(Vec<BranchTreeRow>),
    ListForkRequests(Vec<BranchTreeRow>),
    PreviewBranch(BranchPreview),
    SwitchBranch(RewindReport, RewindScope),
    PreviewRewind(RewindPreview),
    Rewind(RewindReport),
    End,
}
struct Completion {
    result: Result<Reply, AgentError>,
    estimate: u64,
    command: bool,
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
    status: AgentStatus,
    rewinding: bool,
    ended: bool,
}

impl Worker {
    /// Agent とセッションを移し、仕事が届くまで待つ。
    pub fn start(mut agent: Agent, session: Session) -> io::Result<Self> {
        let (jobs, requests) = mpsc::sync_channel(1);
        let (events, receiver) = mpsc::sync_channel(64);
        let (done, completions) = mpsc::sync_channel(1);
        let status = agent.status();
        let thread = thread::Builder::new().name("agent".into()).spawn(move || {
            let mut tokens = TokenCounts::default();
            while let Ok(job) = requests.recv() {
                if let Job::Pause { ready, resume } = job {
                    if ready.send(session.clone()).is_err() {
                        break;
                    }
                    let _resumed = resume.recv();
                    continue;
                }
                let command = !matches!(job, Job::Run { .. });
                if matches!(job, Job::End) {
                    let result = agent
                        .end(super::text::NEW_SESSION_REASON)
                        .map(|()| Reply::End);
                    // 確認の集合と一時ディレクトリを捨ててから、切り替えを許す（設計書 01-05）。
                    drop(agent);
                    drop(session);
                    let _sent = done.send(Completion {
                        result,
                        estimate: 0,
                        command: false,
                    });
                    return;
                }
                let result = match job {
                    Job::Run { request, control } => agent
                        .run_controlled(request, control, events.clone())
                        .and_then(|report| {
                            add_tokens(&mut tokens, &report.summary.tokens);
                            session.writer()?.append(&Record::Summary {
                                summary: report.summary.clone(),
                            })?;
                            Ok(Reply::Run(report))
                        }),
                    Job::Compact {
                        instructions,
                        cancelled,
                    } => {
                        let result = agent.compact(instructions, cancelled, events.clone());
                        let mut summary = Summary::default();
                        if let Ok(report) = &result {
                            summary.add_usage(&report.usage);
                            summary.compactions =
                                u64::from(report.outcome == CompactOutcome::Compacted);
                        }
                        add_tokens(&mut tokens, &summary.tokens);
                        // 取消し・失敗・空の会話も、手動の試行を一行残す（実装プラン P4-08）。
                        session
                            .writer()
                            .and_then(|mut writer| writer.append(&Record::Summary { summary }))
                            .map_err(AgentError::from)
                            .and(result.map(Reply::Compact))
                    }
                    Job::Status => Ok(Reply::Status(agent.status(), tokens.clone())),
                    Job::ListCheckpoints => agent.checkpoints().map(Reply::ListCheckpoints),
                    Job::ListBranches => agent.branch_tree().map(Reply::ListBranches),
                    Job::ListForkRequests => agent.branch_tree().map(Reply::ListForkRequests),
                    Job::PreviewBranch { target, scope } => agent
                        .branch_preview(target, scope)
                        .map(Reply::PreviewBranch),
                    Job::SwitchBranch {
                        target,
                        checkpoint,
                        scope,
                        allow_incomplete,
                    } => agent
                        .switch_branch(
                            target,
                            checkpoint.as_deref(),
                            scope,
                            allow_incomplete,
                            events.clone(),
                        )
                        .map(|report| Reply::SwitchBranch(report, scope)),
                    Job::PreviewRewind { checkpoint, scope } => agent
                        .rewind_preview(&checkpoint, scope)
                        .map(Reply::PreviewRewind),
                    Job::Rewind {
                        checkpoint,
                        scope,
                        allow_incomplete,
                    } => agent
                        .rewind(&checkpoint, scope, allow_incomplete, events.clone())
                        .map(Reply::Rewind),
                    Job::End | Job::Pause { .. } => return,
                };
                let completion = Completion {
                    result,
                    estimate: agent.context_estimate(),
                    command,
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
            status,
            rewinding: false,
            ended: false,
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

    fn command(&mut self, job: Job, control: RunControl) -> Result<(), AgentError> {
        if self.is_running() {
            return Err(AgentError::Worker);
        }
        self.rewinding = matches!(job, Job::Rewind { .. } | Job::SwitchBranch { .. });
        self.jobs
            .as_ref()
            .ok_or(AgentError::Worker)?
            .send(job)
            .map_err(|_| AgentError::Worker)?;
        self.control = Some(control);
        Ok(())
    }
    pub(super) fn compact(&mut self, instructions: Option<String>) -> Result<(), AgentError> {
        let control = RunControl::default();
        self.command(
            Job::Compact {
                instructions,
                cancelled: Arc::clone(&control.cancel_step),
            },
            control,
        )
    }
    pub(super) fn status(&mut self) -> Result<(), AgentError> {
        self.command(Job::Status, RunControl::default())
    }
    pub(super) fn checkpoints(&mut self) -> Result<(), AgentError> {
        self.command(Job::ListCheckpoints, RunControl::default())
    }
    pub(super) fn branches(&mut self) -> Result<(), AgentError> {
        self.command(Job::ListBranches, RunControl::default())
    }
    pub(super) fn fork_requests(&mut self) -> Result<(), AgentError> {
        self.command(Job::ListForkRequests, RunControl::default())
    }
    /// 元の Agent を破棄せずに待たせ、UI 側へロックを保つハンドルを渡す。
    pub(super) fn pause(&mut self) -> Result<PausedWorker, AgentError> {
        if self.is_running() {
            return Err(AgentError::Worker);
        }
        let (ready, receive) = mpsc::sync_channel(1);
        let (resume, wait) = mpsc::sync_channel(1);
        self.jobs
            .as_ref()
            .ok_or(AgentError::Worker)?
            .send(Job::Pause {
                ready,
                resume: wait,
            })
            .map_err(|_| AgentError::Worker)?;
        let session = receive.recv().map_err(|_| AgentError::Worker)?;
        Ok(PausedWorker {
            session,
            _resume: resume,
        })
    }
    pub(super) fn preview_branch(
        &mut self,
        target: BranchTarget,
        scope: RewindScope,
    ) -> Result<(), AgentError> {
        self.command(Job::PreviewBranch { target, scope }, RunControl::default())
    }
    pub(super) fn switch_branch(
        &mut self,
        target: BranchTarget,
        checkpoint: Option<String>,
        scope: RewindScope,
        allow_incomplete: bool,
    ) -> Result<(), AgentError> {
        self.command(
            Job::SwitchBranch {
                target,
                checkpoint,
                scope,
                allow_incomplete,
            },
            RunControl::default(),
        )
    }
    pub(super) fn preview(
        &mut self,
        checkpoint: String,
        scope: RewindScope,
    ) -> Result<(), AgentError> {
        self.command(
            Job::PreviewRewind { checkpoint, scope },
            RunControl::default(),
        )
    }
    pub(super) fn rewind(
        &mut self,
        checkpoint: String,
        scope: RewindScope,
        allow_incomplete: bool,
    ) -> Result<(), AgentError> {
        self.command(
            Job::Rewind {
                checkpoint,
                scope,
                allow_incomplete,
            },
            RunControl::default(),
        )
    }
    pub(super) fn end(&mut self) -> Result<(), AgentError> {
        self.command(Job::End, RunControl::default())
    }
    pub(super) fn current_status(&self) -> &AgentStatus {
        &self.status
    }
    pub(super) fn ended(&self) -> bool {
        self.ended
    }
    /// 時点や枝を記録する移動は、範囲によらず途中で取り消さない。
    pub fn is_rewinding(&self) -> bool {
        self.rewinding
    }

    /// 現在の仕事だけを取り消す。終了の印とは共有しない。
    pub fn cancel(&self) {
        if !self.rewinding
            && let Some(control) = &self.control
        {
            control.cancel();
        }
    }

    /// 一度の ESC は、道具の段階だけに印を渡す。
    pub fn cancel_step(&self) {
        if !self.rewinding
            && let Some(control) = &self.control
        {
            control.cancel_step.store(true, Ordering::Release);
        }
    }

    /// 会話へ加えた通知は Agent が返す。未反映の操縦を戻すため、入力した原文も保つ。
    pub fn steer(&mut self, text: String, original: String) -> Result<(), String> {
        let Some(sender) = &self.steering else {
            return Err(original);
        };
        if sender.send(text).is_err() {
            return Err(original);
        }
        self.pending_steering.push_back(original);
        Ok(())
    }

    fn event(&mut self, screen: &mut Screen, event: AgentEvent) {
        if matches!(event, AgentEvent::Steered { .. }) {
            self.pending_steering.pop_front();
        }
        if let AgentEvent::ProjectDirChanged { project_dir } = &event {
            self.status.project = project_dir.clone();
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
                self.rewinding = false;
                screen.before_rewind = None;
                screen.operation = None;
                match completion.result {
                    Ok(Reply::Run(report)) => screen.finished(
                        report.outcome == Outcome::Cancelled,
                        self.pending_steering.drain(..).collect(),
                    ),
                    Ok(Reply::Compact(report)) => {
                        if report.outcome == CompactOutcome::Cancelled {
                            screen.finished(true, Vec::new());
                        }
                        screen.compact_result(&report);
                    }
                    Ok(Reply::Status(status, tokens)) => {
                        screen.command_status(&status, &tokens);
                        self.status = status;
                    }
                    Ok(Reply::ListCheckpoints(points)) => screen.select_checkpoints(points),
                    Ok(Reply::ListBranches(rows)) => screen.select_branches(rows),
                    Ok(Reply::ListForkRequests(rows)) => screen.select_fork_requests(rows),
                    Ok(Reply::PreviewBranch(preview)) => {
                        screen.move_confirmation(PendingMove::Branch(preview), None)
                    }
                    Ok(Reply::PreviewRewind(preview)) => screen.rewind_confirmation(preview, None),
                    Ok(Reply::Rewind(report)) => {
                        screen.move_result(&report, None);
                        screen.pending_move = None;
                    }
                    Ok(Reply::SwitchBranch(report, scope)) => {
                        screen.move_result(&report, Some(scope));
                        screen.pending_move = None;
                    }
                    Ok(Reply::End) => self.ended = true,
                    Err(AgentError::Rewind(RewindError::IncompleteBefore {
                        checkpoint,
                        incomplete,
                    })) => {
                        if let Some(preview) = screen.pending_move.take() {
                            screen.move_confirmation(preview, Some((checkpoint, incomplete)));
                        }
                    }
                    Err(error) if completion.command => {
                        screen.notice(&error.to_string());
                        screen.pending_move = None;
                    }
                    Err(error) => return Err(error),
                }
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

// 失敗や早期終了でも送り手の drop で働き手の待ちを解く。
pub(super) struct PausedWorker {
    pub session: Session,
    _resume: mpsc::SyncSender<()>,
}

fn add_tokens(total: &mut TokenCounts, value: &TokenCounts) {
    total.input = total.input.saturating_add(value.input);
    total.cache_read = total.cache_read.saturating_add(value.cache_read);
    total.cache_write = total.cache_write.saturating_add(value.cache_write);
    total.output = total.output.saturating_add(value.output);
}
