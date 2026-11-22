//! ACP 版 1 の標準入出力 JSON-RPC 2.0（NDJSON）の読み書きと非ブロッキング出力キュー
//! （設計書 05-03「コマンドラインと入出力」「スレッドの構成と終了の手順」）。

use std::io::{self, BufRead, BufReader, Read, Write};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender, TrySendError};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use serde_json::{Value, json};

/// 1 行の JSON-RPC メッセージの最大バイト数（16 MiB。設計書 05-03）。
pub const MAX_LINE_BYTES: usize = 16 * 1024 * 1024;
/// 主スレッドから出力スレッドへの有界チャネルの容量（256 行。設計書 05-03）。
pub const OUTPUT_QUEUE_CAPACITY: usize = 256;

/// JSON-RPC 2.0 のエラーコード。
pub const ERR_PARSE: i64 = -32700;
pub const ERR_INVALID_REQUEST: i64 = -32600;
pub const ERR_METHOD_NOT_FOUND: i64 = -32601;
pub const ERR_INVALID_PARAMS: i64 = -32602;
pub const ERR_INTERNAL: i64 = -32603;
pub const ERR_SERVER: i64 = -32000;

/// 入力スレッドから主スレッドへ渡す分類済みのメッセージ。
#[derive(Debug)]
pub(super) enum InputItem {
    Request {
        id: Value,
        method: String,
        params: Value,
    },
    Notification {
        method: String,
        params: Value,
    },
    Response {
        id: Value,
        result: Option<Value>,
        error: Option<Value>,
    },
    InvalidRequest {
        id: Value,
    },
    ParseError,
    LineTooLong,
    Eof,
}

/// 入力スレッドを起動し、1 行ずつ JSON-RPC 2.0 を分類して主スレッドへ送る。
/// 出力障害やシグナルで終了するときは、クライアントが stdin を閉じなくても無期限に join しない
/// （設計書 05-03「スレッドの構成と終了の手順」）。
pub(super) fn spawn_reader<R: Read + Send + 'static>(
    input: R,
    wake: SyncSender<()>,
) -> io::Result<Receiver<InputItem>> {
    let (tx, rx) = mpsc::channel();
    let _handle = thread::Builder::new()
        .name("acp-stdin".into())
        .spawn(move || {
            let mut reader = BufReader::new(input);
            let mut line = Vec::new();
            loop {
                match read_bounded_line(&mut reader, &mut line, MAX_LINE_BYTES) {
                    Ok(LineRead::Eof) => {
                        let _sent = tx.send(InputItem::Eof);
                        let _woken = wake.try_send(());
                        break;
                    }
                    Ok(LineRead::TooLong) => {
                        if tx.send(InputItem::LineTooLong).is_err() {
                            break;
                        }
                        let _woken = wake.try_send(());
                    }
                    Ok(LineRead::Line) => {
                        if line.iter().all(u8::is_ascii_whitespace) {
                            continue;
                        }
                        let item = classify_line(&line);
                        if tx.send(item).is_err() {
                            break;
                        }
                        let _woken = wake.try_send(());
                    }
                    Err(_) => {
                        let _sent = tx.send(InputItem::Eof);
                        let _woken = wake.try_send(());
                        break;
                    }
                }
            }
        })?;
    Ok(rx)
}

enum LineRead {
    Line,
    TooLong,
    Eof,
}

/// 16 MiB を超える行でメモリを枯渇させないよう、上限を超えたら次の改行まで読み捨てる。
fn read_bounded_line<R: BufRead>(
    reader: &mut R,
    out: &mut Vec<u8>,
    limit: usize,
) -> io::Result<LineRead> {
    out.clear();
    let mut exceeded = false;
    let mut saw_any = false;
    loop {
        let available = match reader.fill_buf() {
            Ok(buf) => buf,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(error),
        };
        if available.is_empty() {
            return Ok(if !saw_any {
                LineRead::Eof
            } else if exceeded {
                LineRead::TooLong
            } else {
                trim_trailing_cr(out);
                LineRead::Line
            });
        }
        saw_any = true;
        if let Some(newline) = available.iter().position(|&b| b == b'\n') {
            let chunk = available.get(..newline).unwrap_or_default();
            if !exceeded {
                if out.len().saturating_add(chunk.len()) > limit {
                    exceeded = true;
                    out.clear();
                } else {
                    out.extend_from_slice(chunk);
                }
            }
            let consume = newline.saturating_add(1);
            reader.consume(consume);
            if exceeded {
                return Ok(LineRead::TooLong);
            }
            trim_trailing_cr(out);
            return Ok(LineRead::Line);
        }
        let len = available.len();
        if !exceeded {
            if out.len().saturating_add(len) > limit {
                exceeded = true;
                out.clear();
            } else {
                out.extend_from_slice(available);
            }
        }
        reader.consume(len);
    }
}

fn trim_trailing_cr(buf: &mut Vec<u8>) {
    if buf.last() == Some(&b'\r') {
        let _ = buf.pop();
    }
}

fn classify_line(line: &[u8]) -> InputItem {
    let Ok(value) = serde_json::from_slice::<Value>(line) else {
        return InputItem::ParseError;
    };
    let Some(object) = value.as_object() else {
        return InputItem::InvalidRequest { id: Value::Null };
    };
    if let Some(version) = object.get("jsonrpc")
        && version.as_str() != Some("2.0")
    {
        let id = valid_id(object.get("id")).unwrap_or(Value::Null);
        return InputItem::InvalidRequest { id };
    }
    let has_id = object.contains_key("id");
    let id = object.get("id");
    let method = object.get("method");
    match (method, has_id) {
        (Some(Value::String(method)), true) => {
            if let Some(id) = valid_id(id) {
                let params = object.get("params").cloned().unwrap_or_else(|| json!({}));
                InputItem::Request {
                    id,
                    method: method.clone(),
                    params,
                }
            } else {
                InputItem::InvalidRequest { id: Value::Null }
            }
        }
        (Some(Value::String(method)), false) => {
            let params = object.get("params").cloned().unwrap_or_else(|| json!({}));
            InputItem::Notification {
                method: method.clone(),
                params,
            }
        }
        (Some(_), _) => InputItem::InvalidRequest {
            id: valid_id(id).unwrap_or(Value::Null),
        },
        (None, true) => {
            let result = object.get("result").cloned();
            let error = object.get("error").cloned();
            if let Some(id) = valid_id(id)
                && (result.is_some() || error.is_some())
            {
                InputItem::Response { id, result, error }
            } else {
                InputItem::InvalidRequest {
                    id: valid_id(id).unwrap_or(Value::Null),
                }
            }
        }
        (None, false) => InputItem::InvalidRequest { id: Value::Null },
    }
}

fn valid_id(value: Option<&Value>) -> Option<Value> {
    match value? {
        v @ (Value::String(_) | Value::Number(_)) => Some(v.clone()),
        Value::Null | Value::Bool(_) | Value::Array(_) | Value::Object(_) => None,
    }
}

/// 非ブロッキングの有界出力キュー（容量 256 行）と出力スレッドの管理
/// （設計書 05-03「スレッドの構成と終了の手順」）。
pub(super) struct OutputWriter {
    tx: Option<SyncSender<Vec<u8>>>,
    failed: Arc<AtomicBool>,
    done_rx: Receiver<()>,
    handle: Option<JoinHandle<()>>,
}

impl OutputWriter {
    pub(super) fn spawn<W: Write + Send + 'static>(
        mut output: W,
        wake: SyncSender<()>,
    ) -> io::Result<Self> {
        let (tx, rx) = mpsc::sync_channel::<Vec<u8>>(OUTPUT_QUEUE_CAPACITY);
        let failed = Arc::new(AtomicBool::new(false));
        let writer_failed = Arc::clone(&failed);
        let (done_tx, done_rx) = mpsc::sync_channel(1);
        let handle = thread::Builder::new()
            .name("acp-stdout".into())
            .spawn(move || {
                while let Ok(line) = rx.recv() {
                    if output.write_all(&line).is_err() || output.flush().is_err() {
                        writer_failed.store(true, Ordering::Release);
                        let _woken = wake.try_send(());
                        while rx.recv().is_ok() {}
                        break;
                    }
                }
                let _done = done_tx.try_send(());
            })?;
        Ok(Self {
            tx: Some(tx),
            failed,
            done_rx,
            handle: Some(handle),
        })
    }

    pub(super) fn is_failed(&self) -> bool {
        self.failed.load(Ordering::Acquire)
    }

    /// 主スレッドを絶対にブロックしない非ブロッキング送信。
    /// キュー満杯または切断のときは出力の障害フラグを立てて false を返す。
    pub(super) fn send(&mut self, message: &Value) -> bool {
        if self.is_failed() {
            return false;
        }
        let Some(tx) = &self.tx else {
            return false;
        };
        let Ok(mut bytes) = serde_json::to_vec(message) else {
            return false;
        };
        bytes.push(b'\n');
        match tx.try_send(bytes) {
            Ok(()) => true,
            Err(TrySendError::Full(_)) | Err(TrySendError::Disconnected(_)) => {
                self.failed.store(true, Ordering::Release);
                false
            }
        }
    }

    pub(super) fn send_result(&mut self, id: Value, result: Value) -> bool {
        self.send(&json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": result,
        }))
    }

    pub(super) fn send_error(&mut self, id: Value, code: i64, message: &str) -> bool {
        self.send(&json!({
            "jsonrpc": "2.0",
            "id": id,
            "error": {
                "code": code,
                "message": message,
            }
        }))
    }

    pub(super) fn send_notification(&mut self, method: &str, params: Value) -> bool {
        self.send(&json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
        }))
    }

    pub(super) fn send_request(&mut self, id: &str, method: &str, params: Value) -> bool {
        self.send(&json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        }))
    }

    /// 正常な EOF では送信済みの行がフラッシュされるのを短時間だけ待ち、
    /// 出力障害や詰まった stdout のときは無期限に join せずに終える（設計書 05-03）。
    pub(super) fn finish(mut self) {
        self.tx.take();
        if self.is_failed() {
            return;
        }
        if self
            .done_rx
            .recv_timeout(Duration::from_millis(200))
            .is_ok()
            && let Some(handle) = self.handle.take()
        {
            let _joined = handle.join();
        }
    }
}
