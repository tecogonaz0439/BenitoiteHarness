//! 端末の状態を記述子とともに保存し、panic でも錠を取らずに戻す（設計書 06-01「ビルドの設定」）。

use std::fs::File;
use std::io::{self, Write};
use std::os::fd::{AsFd, OwnedFd};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU16, Ordering};

use rustix::termios::{OptionalActions, Termios, tcgetattr, tcsetattr};

/// panic hook が通常の描画の錠を取らずに使う、複製した端末の記述子。
pub struct TerminalRestore {
    input: OwnedFd,
    output: OwnedFd,
    error: OwnedFd,
    original: Termios,
    rows_below: AtomicU16,
    restored: Arc<AtomicBool>,
}

impl TerminalRestore {
    /// raw のモードにするより前に保存する。問い合わせの制御の列は送らない。
    pub fn capture() -> io::Result<Arc<Self>> {
        let input = rustix::io::dup(io::stdin())?;
        let output = rustix::io::dup(io::stdout())?;
        let error = rustix::io::dup(io::stderr())?;
        let original = tcgetattr(&input)?;
        Ok(Arc::new(Self {
            input,
            output,
            error,
            original,
            rows_below: AtomicU16::new(0),
            restored: Arc::new(AtomicBool::new(false)),
        }))
    }

    /// 描画の後のカーソル位置を hook へ知らせる。
    pub fn set_rows_below(&self, rows: u16) {
        self.rows_below.store(rows, Ordering::Release);
    }

    /// LineWriter を経ず、複製した記述子へ一回で書く出力先を作る。
    pub fn output(&self) -> io::Result<TerminalOutput> {
        Ok(TerminalOutput(RestoredWriter::new(
            File::from(rustix::io::dup(&self.output)?),
            Arc::clone(&self.restored),
        )))
    }

    /// 正常終了・誤り・シグナル・panic で共有する。Drop に依存しない。
    pub fn restore(&self) {
        if self.restored.swap(true, Ordering::AcqRel) {
            return;
        }
        if let Ok(bytes) = super::restoration(self.rows_below.load(Ordering::Acquire)) {
            write_descriptor(&self.output, &bytes);
        }
        let _restored = tcsetattr(&self.input, OptionalActions::Now, &self.original);
    }

    /// termios を戻した後に固定の文だけを書く。標準エラーの錠を取らない。
    pub fn panic_message(&self, message: &str) {
        self.restore();
        let message = super::control::SafeText::new(message, false);
        write_descriptor(&self.error, message.as_str().as_bytes());
        write_descriptor(&self.error, b"\n");
    }
}

fn write_descriptor(fd: impl AsFd, mut bytes: &[u8]) {
    while !bytes.is_empty() {
        match rustix::io::write(&fd, bytes) {
            Ok(0) => break,
            Ok(count) => bytes = bytes.get(count..).unwrap_or_default(),
            Err(rustix::io::Errno::INTR) => continue,
            Err(_) => break,
        }
    }
}

/// 通常の終了と Err の帰り道でも端末を戻す所有者。
pub struct TerminalGuard(Arc<TerminalRestore>);

impl TerminalGuard {
    /// hook を先に登録してから呼ぶ。raw の開始の失敗にも復元を掛ける。
    pub fn enter(restore: Arc<TerminalRestore>) -> io::Result<Self> {
        let guard = Self(restore);
        crossterm::terminal::enable_raw_mode()?;
        super::control::setup(&mut guard.0.output()?)?;
        Ok(guard)
    }
}

/// 復元後の描画を捨てる、バッファを持たない端末の書き先。
pub struct TerminalOutput(RestoredWriter<File>);

impl Write for TerminalOutput {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0.write(bytes)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.0.flush()
    }
}

// panic hook は錠を取らずに復元する。進行中の一回の write はフレーム末尾でも
// 折り返しを戻し、復元の印の後の write と flush は書き先で捨てる。
pub(super) struct RestoredWriter<W> {
    writer: W,
    restored: Arc<AtomicBool>,
}

impl<W> RestoredWriter<W> {
    pub(super) fn new(writer: W, restored: Arc<AtomicBool>) -> Self {
        Self { writer, restored }
    }
}

impl<W: Write> Write for RestoredWriter<W> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if self.restored.load(Ordering::Acquire) {
            Ok(bytes.len())
        } else {
            self.writer.write(bytes)
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        if self.restored.load(Ordering::Acquire) {
            Ok(())
        } else {
            self.writer.flush()
        }
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        self.0.restore();
        // crossterm が保持する raw の状態も解く。panic の道は上の tcsetattr だけを使う。
        let _disabled = crossterm::terminal::disable_raw_mode();
    }
}
