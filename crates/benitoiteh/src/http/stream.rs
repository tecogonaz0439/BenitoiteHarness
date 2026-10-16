//! 有限長のチャネルで応答を渡し、呼び出す側で時間を見張る（設計書 02-01「HTTP の通信」、06-01「スレッドと非同期」）。

use std::io::{self, Read};
use std::net::{Shutdown, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender, TrySendError};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use super::{
    HttpClient, HttpError, HttpTimeouts, ResponseHead, TimeoutKind, classify, make_agent,
    status_error,
};

const POLL_INTERVAL: Duration = Duration::from_millis(10);
const CHANNEL_CAPACITY: usize = 8;
const CHUNK_SIZE: usize = 8192;

/// ストリームを受け取るときのイベント。
#[derive(Debug)]
pub enum StreamEvent {
    /// 成功した応答の状態とヘッダ。本文より前に一度だけ届く。
    Headers(ResponseHead),
    /// 読み取った本文の一部分。
    Data(Vec<u8>),
    /// 正常な EOF。以降の poll も Finished を返す。
    Finished,
    /// poll に指定した待ち時間が過ぎた。HTTP の上限には達していない。
    Pending,
}

/// 別のスレッドにも渡せる要求ごとの取り消しの印。
#[derive(Clone, Debug)]
pub struct Cancellation(Arc<Control>);

impl Cancellation {
    /// 印を立て、接続済みなら下の TCP の読み書きを shutdown する。
    pub fn cancel(&self) {
        self.0.stop(HttpError::Cancelled);
    }

    /// 打ち切りが要求されたかを返す。
    pub fn is_cancelled(&self) -> bool {
        self.0.stopped.load(Ordering::Acquire)
    }
}

/// ヘッダと本文の受け取り口。Drop は取り消しを要求する。
///
/// データが届かない時間は `poll` または `check_timeouts` を呼ぶ側が見張る。
/// `join` で読み取りのスレッドの終了を待てる。接続前の DNS と TCP 接続は
/// 即時には打ち切れず、設定した接続の上限まで待つことがある。
pub struct HttpStream {
    events: Receiver<StreamEvent>,
    completion: Receiver<Result<(), HttpError>>,
    outcome: Option<Result<(), HttpError>>,
    control: Arc<Control>,
    worker: Option<JoinHandle<()>>,
    started: Instant,
    timeouts: HttpTimeouts,
}

impl std::fmt::Debug for HttpStream {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HttpStream")
            .field("finished", &self.is_finished())
            .finish_non_exhaustive()
    }
}

impl HttpStream {
    /// 別のスレッドからも取り消せるハンドルを返す。
    pub fn cancellation(&self) -> Cancellation {
        Cancellation(Arc::clone(&self.control))
    }

    /// この要求を打ち切る。
    pub fn cancel(&self) {
        self.control.stop(HttpError::Cancelled);
    }

    /// 最後に本文のデータを読み取った時刻。本文がまだならヘッダの到着時刻。
    /// ヘッダもまだなら要求の開始時刻を返す。
    pub fn last_received(&self) -> Instant {
        self.control.lock().last_received
    }

    /// 無通信と全体の上限を判定し、超過していれば接続を打ち切る。
    pub fn check_timeouts(&self) -> Result<(), HttpError> {
        let state = self.control.lock();
        if let Some(error) = &state.stop {
            return Err(error.clone());
        }
        if state.completed {
            return Ok(());
        }
        let error = if self.started.elapsed() >= self.timeouts.total {
            Some(HttpError::Timeout(TimeoutKind::Total))
        } else if state.body_started && state.last_received.elapsed() >= self.timeouts.idle {
            Some(HttpError::Timeout(TimeoutKind::Idle))
        } else {
            None
        };
        drop(state);
        if let Some(error) = error {
            self.control.stop(error.clone());
            return Err(error);
        }
        Ok(())
    }

    /// 最大 `wait` 待ち、イベントか Pending を返す。途中でも時間の上限を見張る。
    /// 打ち切りの後は、チャネルに残っている本文を返さない。
    pub fn poll(&mut self, wait: Duration) -> Result<StreamEvent, HttpError> {
        let started = Instant::now();
        loop {
            self.check_timeouts()?;
            if let Some(outcome) = &self.outcome {
                return outcome.clone().map(|()| StreamEvent::Finished);
            }
            let duration = wait.saturating_sub(started.elapsed()).min(POLL_INTERVAL);
            match self.events.recv_timeout(duration) {
                Ok(event) => {
                    self.check_timeouts()?;
                    return Ok(event);
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    let outcome = self.completion.recv().unwrap_or(Err(HttpError::Worker));
                    self.outcome = Some(outcome.clone());
                    return outcome.map(|()| StreamEvent::Finished);
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    if started.elapsed() >= wait {
                        self.check_timeouts()?;
                        return Ok(StreamEvent::Pending);
                    }
                }
            }
        }
    }

    /// 読み取りのスレッドが終了しているかを返す。
    pub fn is_finished(&self) -> bool {
        self.worker.as_ref().is_none_or(JoinHandle::is_finished)
    }

    /// 読み取りのスレッドの終了を待つ。HTTP の結果は poll で受け取る。
    /// 終了前に呼ぶと待つので、途中で待つ場合は先に cancel する。
    pub fn join(&mut self) -> Result<(), HttpError> {
        if let Some(worker) = self.worker.take() {
            worker.join().map_err(|_| HttpError::Worker)?;
        }
        Ok(())
    }
}

impl Drop for HttpStream {
    fn drop(&mut self) {
        self.cancel();
    }
}

struct State {
    socket: Option<TcpStream>,
    stop: Option<HttpError>,
    last_received: Instant,
    body_started: bool,
    completed: bool,
}

pub(super) struct Control {
    stopped: AtomicBool,
    state: Mutex<State>,
    started: Instant,
    total: Duration,
}

impl std::fmt::Debug for Control {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Control")
            .field("stopped", &self.stopped.load(Ordering::Acquire))
            .finish_non_exhaustive()
    }
}

impl Control {
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn stop(&self, error: HttpError) {
        let mut state = self.lock();
        if state.stop.is_none() {
            state.stop = Some(error);
            self.stopped.store(true, Ordering::Release);
            if let Some(socket) = &state.socket {
                // shutdown は同じ OS の接続を共有する複製にも作用する。失敗時も印は残す。
                if socket.shutdown(Shutdown::Both).is_err() { /* 既に切断された接続も打ち切り済み。 */
                }
            }
        }
    }

    pub(super) fn check_io(&self) -> io::Result<()> {
        if self.stopped.load(Ordering::Acquire) {
            Err(io::ErrorKind::Interrupted.into())
        } else {
            Ok(())
        }
    }

    pub(super) fn attach(&self, socket: &TcpStream) -> io::Result<()> {
        let mut state = self.lock();
        self.check_io()?;
        state.socket = Some(socket.try_clone()?);
        Ok(())
    }

    fn received(&self) {
        let mut state = self.lock();
        state.body_started = true;
        state.last_received = Instant::now();
    }
}

pub(super) fn start(
    client: &HttpClient,
    request: ureq::http::Request<Vec<u8>>,
) -> Result<HttpStream, HttpError> {
    let started = Instant::now();
    let control = Arc::new(Control {
        stopped: AtomicBool::new(false),
        started,
        total: client.timeouts.total,
        state: Mutex::new(State {
            socket: None,
            stop: None,
            last_received: started,
            body_started: false,
            completed: false,
        }),
    });
    // 接続の複製と要求を一対一にするため、ストリームだけプールを使わない（実装プラン P0-03「作るもの」）。
    let agent = make_agent(
        client.timeouts,
        client.tls.clone(),
        None,
        Some(Arc::clone(&control)),
    );
    let (sender, events) = mpsc::sync_channel(CHANNEL_CAPACITY);
    let (complete, completion) = mpsc::sync_channel(1);
    let worker_control = Arc::clone(&control);
    let worker = thread::Builder::new()
        .spawn(move || {
            let result = read_response(agent, request, &sender, &worker_control);
            let mut state = worker_control.lock();
            state.completed = true;
            state.socket = None;
            let result = state.stop.clone().map_or(result, Err);
            drop(state);
            // 完了の通知は本文とは別の一件のチャネルなので、本文が満杯でもスレッドを終えられる。
            if complete.send(result).is_err() { /* 受け手は既に終了した。 */ }
        })
        .map_err(|_| HttpError::Worker)?;
    Ok(HttpStream {
        events,
        completion,
        outcome: None,
        control,
        worker: Some(worker),
        started,
        timeouts: client.timeouts,
    })
}

fn send(
    sender: &SyncSender<StreamEvent>,
    mut event: StreamEvent,
    control: &Control,
) -> Result<(), HttpError> {
    loop {
        // チャネルの背圧で ureq の read に戻れなくても、要求全体の上限で終える。
        if control.started.elapsed() >= control.total {
            control.stop(HttpError::Timeout(TimeoutKind::Total));
        }
        if control.stopped.load(Ordering::Acquire) {
            return Err(HttpError::Cancelled);
        }
        match sender.try_send(event) {
            Ok(()) => return Ok(()),
            Err(TrySendError::Disconnected(_)) => return Err(HttpError::Cancelled),
            Err(TrySendError::Full(value)) => {
                event = value;
                thread::sleep(POLL_INTERVAL);
            }
        }
    }
}

fn read_response(
    agent: ureq::Agent,
    request: ureq::http::Request<Vec<u8>>,
    sender: &SyncSender<StreamEvent>,
    control: &Control,
) -> Result<(), HttpError> {
    let response = agent.run(request).map_err(classify)?;
    let (parts, body) = response.into_parts();
    let head = ResponseHead {
        status: parts.status.as_u16(),
        headers: parts.headers,
    };
    let mut reader = ProgressReader {
        reader: body.into_reader(),
        control,
    };
    control.received();
    if !(200..300).contains(&head.status) {
        return Err(status_error(head, &mut reader));
    }
    send(sender, StreamEvent::Headers(head), control)?;
    let mut buffer = [0; CHUNK_SIZE];
    loop {
        control.check_io().map_err(|_| HttpError::Cancelled)?;
        let amount = reader.read(&mut buffer).map_err(|e| classify(e.into()))?;
        if amount == 0 {
            return Ok(());
        }
        let data = buffer.get(..amount).ok_or(HttpError::Worker)?.to_vec();
        send(sender, StreamEvent::Data(data), control)?;
    }
}

// 誤りの応答の接頭辞を読む間も、本文が届いた時刻を更新する。
struct ProgressReader<'a, R> {
    reader: R,
    control: &'a Control,
}

impl<R: Read> Read for ProgressReader<'_, R> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        self.control.check_io()?;
        let amount = self.reader.read(buffer)?;
        if amount > 0 {
            self.control.received();
        }
        Ok(amount)
    }
}
