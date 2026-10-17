//! 停止の合図と、グループを抜ける子孫を作る探査（設計書 03-02「子プロセスの後始末」、実装プラン P2-01）。

use std::ffi::OsString;
use std::fs;
use std::io::{self, Read, Write};
use std::os::unix::net::UnixStream;
#[cfg(target_os = "linux")]
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use super::text;

pub(super) fn run(operation: &str, args: &[OsString]) -> io::Result<Option<u8>> {
    match (operation, args) {
        ("noop", []) => Ok(Some(0)),
        ("exit", [code]) => Ok(code.to_str().and_then(|code| code.parse::<u8>().ok())),
        ("echo-stdin", []) => {
            io::copy(&mut io::stdin().lock(), &mut io::stdout().lock())?;
            // 共有の受け取り先で stdout が本文と改行の間に入らないよう、まとめて渡す。
            // （設計書 03-02「子プロセスの後始末」、実装プラン P2-01）
            io::stderr().write_all(text::ECHO_STDERR.as_bytes())?;
            Ok(Some(0))
        }
        ("sleep", [millis, ..]) => {
            let Some(millis) = millis.to_str().and_then(|s| s.parse::<u64>().ok()) else {
                return Ok(None);
            };
            std::thread::sleep(Duration::from_millis(millis));
            Ok(Some(0))
        }
        ("signal", [mode, marker, ..]) => {
            let interrupt = Arc::new(AtomicBool::new(false));
            let terminate = Arc::new(AtomicBool::new(false));
            signal_hook::flag::register(signal_hook::consts::SIGINT, Arc::clone(&interrupt))?;
            signal_hook::flag::register(signal_hook::consts::SIGTERM, Arc::clone(&terminate))?;
            // 受け手の準備の前に止めるテストを避けるため、ファイルで準備を知らせる。
            fs::write(marker, text::READY)?;
            loop {
                if mode == "handle" {
                    if interrupt.load(Ordering::Acquire) {
                        fs::write(marker, b"SIGINT")?;
                        return Ok(Some(0));
                    }
                    if terminate.load(Ordering::Acquire) {
                        fs::write(marker, b"SIGTERM")?;
                        return Ok(Some(0));
                    }
                }
                std::thread::sleep(Duration::from_millis(5));
            }
        }
        ("descendant", [session, pipes, marker]) => {
            // 子の stdin のソケットで準備を同期する。stdout/stderr の保持の検査を邪魔しない。
            let (mut parent, inherited) = UnixStream::pair()?;
            let fd = std::os::fd::OwnedFd::from(inherited);
            let mut command = Command::new(std::env::current_exe()?);
            command.args([
                OsString::from("descendant-child"),
                session.clone(),
                marker.clone(),
            ]);
            command.stdin(Stdio::from(fd));
            if pipes == "closed" {
                command.stdout(Stdio::null()).stderr(Stdio::null());
            }
            let child = command.spawn()?;
            let mut ready = [0];
            parent.read_exact(&mut ready)?;
            fs::write(marker, child.id().to_string())?;
            Ok(Some(0))
        }
        ("descendant-child", [session, _marker]) => {
            if session == "session" {
                rustix::process::setsid()?;
            }
            // 記述子 0 はソケットなので、unsafe を使わずに準備の通知を書ける。
            let fd = rustix::io::fcntl_dupfd_cloexec(rustix::stdio::stdin(), 3)?;
            let mut socket = UnixStream::from(fd);
            socket.write_all(b"R")?;
            // グループを抜けた子も有限時間で終え、テストの片付けで外から
            // 使い回されうる番号へ送る必要をなくす（実装プラン P2-01）。
            std::thread::sleep(Duration::from_secs(30));
            Ok(Some(0))
        }
        #[cfg(target_os = "linux")]
        ("check-control-fds", []) => {
            let entries = fs::read_dir("/proc/self/fd")?;
            let mut count = 0_u32;
            for entry in entries {
                let entry = entry?;
                // read_dir 自身が使う一つの記述子は symlink から区別する。
                let target = fs::read_link(entry.path())?;
                if target == Path::new(&format!("/proc/{}/fd", std::process::id())) {
                    continue;
                }
                count = count.saturating_add(1);
            }
            if count != 3 {
                return Err(io::Error::other(text::EXTRA_DESCRIPTOR));
            }
            if fs::read_dir("/proc/1/fd").is_ok() {
                return Err(io::Error::other(text::CONTROL_ACCESS));
            }
            Ok(Some(0))
        }
        _ => Ok(None),
    }
}
