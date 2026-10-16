//! バイト列の塊から SSE を読む。イベントの上限でメモリを制限する（設計書 02-01「HTTP の通信」）。

use std::fmt;

use serde::{Deserialize, Serialize};

use super::text;

/// 一つのイベントに許すバイト数。フィールド名、注釈、改行と区切りも含む。
pub const MAX_EVENT_BYTES: usize = 16 * 1024 * 1024;

/// 空行で受け取りを終えた SSE のイベント。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SseEvent {
    /// event の値。省略または空なら SSE の既定の message。
    pub event: String,
    /// data の値。複数行は改行でつなぎ、末尾に改行を足さない。
    pub data: String,
}

/// SSE の読み取りを続けられない理由。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SseError {
    /// 空行までのバイト数が上限を超えた。
    EventTooLarge,
    /// フィールドに不正な UTF-8 がある。
    InvalidUtf8,
    /// finish の後にデータが渡された。
    Closed,
}

impl fmt::Display for SseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::EventTooLarge => text::SSE_EVENT_LIMIT,
            Self::InvalidUtf8 => text::SSE_INVALID_UTF8,
            Self::Closed => text::SSE_CLOSED,
        })
    }
}

impl std::error::Error for SseError {}

/// 行や UTF-8 の文字の途中で分かれた塊も受け取る SSE の読み取り器。
///
/// LF と CRLF を扱う。誤りの後はその誤りを返し続け、再開しない。
/// 注釈と未知のフィールドも上限に数え、改行のない入力も制限する。
#[derive(Debug, Default)]
pub struct SseDecoder {
    line: Vec<u8>,
    event: String,
    data: String,
    has_data: bool,
    event_bytes: usize,
    stopped: Option<SseError>,
}

impl SseDecoder {
    /// 空の読み取り器を作る。
    pub fn new() -> Self {
        Self::default()
    }

    /// 次のバイト列の塊を受け取り、空行まで届いたイベントを順に返す。
    ///
    /// 誤りが出た塊に含まれるイベントは返さない。アダプタはその要求を打ち切る。
    pub fn push(&mut self, bytes: &[u8]) -> Result<Vec<SseEvent>, SseError> {
        if let Some(error) = self.stopped {
            return Err(error);
        }
        let mut events = Vec::new();
        for &byte in bytes {
            if self.event_bytes >= MAX_EVENT_BYTES {
                return Err(self.stop(SseError::EventTooLarge));
            }
            self.event_bytes = self.event_bytes.saturating_add(1);
            if byte == b'\n' {
                if self.line.last() == Some(&b'\r') {
                    self.line.pop();
                }
                if let Err(error) = self.end_line(&mut events) {
                    return Err(self.stop(error));
                }
            } else {
                self.line.push(byte);
            }
        }
        Ok(events)
    }

    /// 入力の終わり。空行で終わらなかったイベントを捨てる。
    ///
    /// SSE の EOF を提供者の正常な終わりとは見なさない。アダプタは提供者の
    /// 終わりのイベントがなければ切断とする（設計書 01-01「番の流れ」）。
    pub fn finish(&mut self) {
        if self.stopped.is_none() {
            self.stop(SseError::Closed);
        }
    }

    fn stop(&mut self, error: SseError) -> SseError {
        self.stopped = Some(error);
        self.line = Vec::new();
        self.data = String::new();
        self.event = String::new();
        error
    }

    fn end_line(&mut self, events: &mut Vec<SseEvent>) -> Result<(), SseError> {
        if self.line.is_empty() {
            if self.has_data {
                events.push(SseEvent {
                    event: if self.event.is_empty() {
                        "message".into()
                    } else {
                        std::mem::take(&mut self.event)
                    },
                    data: std::mem::take(&mut self.data),
                });
            }
            self.event.clear();
            self.has_data = false;
            self.event_bytes = 0;
            return Ok(());
        }
        if self.line.first() != Some(&b':') {
            let line = std::str::from_utf8(&self.line).map_err(|_| SseError::InvalidUtf8)?;
            let (field, value) = line.split_once(':').unwrap_or((line, ""));
            let value = value.strip_prefix(' ').unwrap_or(value);
            match field {
                "event" => {
                    self.event.clear();
                    self.event.push_str(value);
                }
                "data" => {
                    if self.has_data {
                        self.data.push('\n');
                    }
                    self.data.push_str(value);
                    self.has_data = true;
                }
                // id と retry による自動の再接続は行わない（設計書 01-01「やり直し」）。
                _ => {}
            }
        }
        self.line.clear();
        Ok(())
    }
}
