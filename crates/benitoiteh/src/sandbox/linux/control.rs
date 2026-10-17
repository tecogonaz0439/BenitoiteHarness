//! 標準入力の記述子を受け取り、pid 1 として停止と回収を行う（設計書 03-02「子プロセスの後始末」）。

use std::io::{self, IoSliceMut, Read};
use std::mem::MaybeUninit;
use std::os::fd::OwnedFd;
use std::os::unix::net::UnixStream;
use std::process::Child;
use std::sync::{Arc, Mutex, PoisonError};

use rustix::net::{RecvAncillaryBuffer, RecvAncillaryMessage, RecvFlags, ReturnFlags, recvmsg};
use rustix::process::{Pid, Signal, WaitId, WaitIdOptions, WaitOptions, wait, waitid};

use super::{SandboxError, failure, text};

pub(super) fn receive_stdin() -> io::Result<OwnedFd> {
    let stdin = rustix::stdio::stdin();
    let stat = rustix::fs::fstat(stdin)?;
    if rustix::fs::FileType::from_raw_mode(stat.st_mode) != rustix::fs::FileType::Socket {
        return Err(io::Error::other(text::CONTROL));
    }
    let mut space = [MaybeUninit::uninit(); rustix::cmsg_space!(ScmRights(2))];
    let mut ancillary = RecvAncillaryBuffer::new(&mut space);
    let mut byte = [0];
    let message = loop {
        match recvmsg(
            stdin,
            &mut [IoSliceMut::new(&mut byte)],
            &mut ancillary,
            RecvFlags::CMSG_CLOEXEC,
        ) {
            Err(rustix::io::Errno::INTR) => continue,
            result => break result?,
        }
    };
    let mut received = Vec::new();
    for message in ancillary.drain() {
        if let RecvAncillaryMessage::ScmRights(fds) = message {
            received.extend(fds);
        }
    }
    if message.bytes != 1 || message.flags.contains(ReturnFlags::CTRUNC) || received.len() != 1 {
        return Err(io::Error::other(text::CONTROL));
    }
    let control = rustix::io::fcntl_dupfd_cloexec(stdin, 3)?;
    let fd = received
        .pop()
        .ok_or_else(|| io::Error::other(text::CONTROL))?;
    // stdin が閉じられていた場合の fd=0 は上でソケットと確認済みなので生じない。
    rustix::stdio::dup2_stdin(&fd)?;
    Ok(control)
}

pub(super) fn supervise(
    control: OwnedFd,
    child: Child,
) -> Result<std::convert::Infallible, SandboxError> {
    let pid = Pid::from_child(&child);
    // 回収と合図を同じ錠の中に置き、回収後の番号へ送る隙間をなくす。
    // 待機は錠の外の NOWAIT で行う。回収だけを錠の中に置き、停止要求を妨げない
    // （設計書 03-02）。
    let gate = Arc::new(Mutex::new(()));
    let control_gate = Arc::clone(&gate);
    std::thread::Builder::new()
        .spawn(move || {
            let mut socket = UnixStream::from(control);
            loop {
                let mut byte = [0];
                let result = socket.read(&mut byte);
                let _guard = control_gate.lock().unwrap_or_else(PoisonError::into_inner);
                let signal = match (result, byte) {
                    (Ok(1), [b'I']) => Signal::INT,
                    (Ok(1), [b'T']) => Signal::TERM,
                    (Ok(1), [b'K']) | (Ok(0), _) => std::process::exit(137),
                    (Err(error), _) if error.kind() == io::ErrorKind::Interrupted => continue,
                    (Err(_), _) => std::process::exit(137),
                    (Ok(_), _) => continue,
                };
                let _sent = rustix::process::kill_process_group(pid, signal);
            }
        })
        .map_err(|e| failure(text::EXEC, e))?;
    loop {
        match waitid(WaitId::All, WaitIdOptions::EXITED | WaitIdOptions::NOWAIT) {
            Ok(_) => {}
            Err(rustix::io::Errno::INTR) => continue,
            Err(error) => return Err(failure(text::EXEC, error)),
        }
        let _guard = gate.lock().unwrap_or_else(PoisonError::into_inner);
        match wait(WaitOptions::NOHANG) {
            Ok(Some((reaped, status))) if reaped == pid => {
                let code = status.exit_status().unwrap_or_else(|| {
                    status
                        .terminating_signal()
                        .map(|signal| 128_i32.saturating_add(signal))
                        .unwrap_or(125)
                });
                std::process::exit(code);
            }
            Ok(Some(_)) | Ok(None) | Err(rustix::io::Errno::INTR) => {}
            Err(error) => return Err(failure(text::EXEC, error)),
        }
    }
}
