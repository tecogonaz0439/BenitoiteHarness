//! `benitoiteh` が起動する子を、プロセスの中で一つずつ起動する（設計書 03-02「方針の要素」、BH-124）。
//!
//! macOS の Rust の標準のライブラリは、パイプを `pipe()` で作った後に close-on-exec の印を付ける
//! （Linux などは `pipe2` で一度に付ける）。`Command` の標準入出力のパイプは起動の中で作られるので、
//! 別の糸の起動がその間に入ると、その子が印のないパイプを継ぎ、継いだ子が終わるまで、読み手は出力の
//! 終わりを得られない。起動の呼び出しを一つの錠で囲み、起動どうしを重ねない。錠を持つのは起動の
//! 間だけで、子が動く間は持たない。ソケットなど、起動の外で作る記述子との重なりは塞がない。

use std::io;
use std::process::{Child, Command, ExitStatus, Output, Stdio};
use std::sync::{Mutex, PoisonError};

static SPAWN: Mutex<()> = Mutex::new(());

/// 錠を持って子を起動する。
pub fn spawn(command: &mut Command) -> io::Result<Child> {
    let _guard = SPAWN.lock().unwrap_or_else(PoisonError::into_inner);
    command.spawn()
}

/// [`Command::output`] と同じく、標準入力を閉じ、標準出力と標準エラー出力を受け取る。
/// 起動だけを錠で囲み、終わりを待つ間は錠を持たない。
pub fn output(command: &mut Command) -> io::Result<Output> {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    spawn(command)?.wait_with_output()
}

/// [`Command::status`] と同じく、標準入出力を受け継いで起動し、終わりを待つ。
pub fn status(command: &mut Command) -> io::Result<ExitStatus> {
    spawn(command)?.wait()
}
