//! OS の拒否を終了状態で返すテスト用の探査（設計書 07-01「安全の性質のテスト」、実装プラン P0-04）。

mod process;
mod text;

use std::ffi::{OsStr, OsString};
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::net::TcpStream;
use std::os::fd::AsRawFd;
use std::os::unix::net::{UnixDatagram, UnixStream};
use std::path::PathBuf;
use std::process::{Command, ExitCode};

use rustix::io::{FdFlags, fcntl_getfd, fcntl_setfd};

fn main() -> ExitCode {
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    let Some((operation, rest)) = args.split_first() else {
        eprintln!("{}", text::USAGE);
        return ExitCode::from(2);
    };
    let result = run(operation, rest);
    let code = match result {
        Ok(Some(code)) => code,
        Ok(None) => {
            eprintln!("{}", text::USAGE);
            2
        }
        Err(error) => {
            eprintln!(
                "{}: {:?}: {error}",
                operation.to_string_lossy(),
                error.kind()
            );
            if error.raw_os_error().is_some_and(|code| {
                code == rustix::io::Errno::PERM.raw_os_error()
                    || code == rustix::io::Errno::ACCESS.raw_os_error()
            }) {
                10
            } else if operation == "hardlink"
                && error.raw_os_error() == Some(rustix::io::Errno::XDEV.raw_os_error())
            {
                // ハードリンクの EXDEV だけを区別し、ほかの操作の終了状態を保つ。
                // （実装プラン P2-02「作るもの」）
                14
            } else {
                11
            }
        }
    };
    ExitCode::from(code)
}

fn run(operation: &OsStr, args: &[OsString]) -> io::Result<Option<u8>> {
    let Some(operation) = operation.to_str() else {
        return Ok(None);
    };
    match (operation, args) {
        ("read", [path]) => fs::read(path).map(|_| ()),
        ("read-dir", [path]) => fs::read_dir(path).map(|_| ()),
        ("wait-stdin", []) => {
            // 親はマウントの準備完了を見てから、EOF で子の終了を指示する。
            writeln!(io::stdout(), "{}", text::READY)?;
            io::stdout().flush()?;
            io::stdin().read_to_end(&mut Vec::new()).map(|_| ())
        }
        ("write", [path]) => fs::write(path, b"sandbox-probe\n"),
        ("mkdir", [path]) => fs::create_dir(path),
        ("remove-file", [path]) => fs::remove_file(path),
        ("remove-dir", [path]) => fs::remove_dir(path),
        ("rename", [from, to]) => fs::rename(from, to),
        ("hardlink", [from, to]) => fs::hard_link(from, to),
        #[cfg(target_os = "linux")]
        ("umount", [path]) => {
            rustix::mount::unmount(path, rustix::mount::UnmountFlags::empty()).map_err(Into::into)
        }
        #[cfg(target_os = "linux")]
        ("bind-mount", [from, to]) => rustix::mount::mount_bind(from, to).map_err(Into::into),
        ("unix-connect", [path]) => UnixStream::connect(path).map(|_| ()),
        ("unix-dgram-send", [path]) => UnixDatagram::unbound()?.send_to(b"probe", path).map(|_| ()),
        ("tcp-connect", [address]) => {
            let Some(address) = address.to_str() else {
                return Ok(None);
            };
            TcpStream::connect(address).map(|_| ())
        }
        ("socketpair", []) => UnixStream::pair().map(|_| ()),
        ("fd-open", [number]) => return descriptor(number),
        ("exec", [program, rest @ ..]) => {
            let status = Command::new(program).args(rest).status()?;
            let code = if status.success() { 0 } else { 12 };
            eprintln!(
                "{operation}: {}",
                if code == 0 {
                    text::OK
                } else {
                    text::CHILD_FAILED
                }
            );
            return Ok(Some(code));
        }
        ("spawn-with-fd", [path, separator, program, rest @ ..]) if separator == "--" => {
            // 印の変更をテストの本体から隔離して、並行する別の子へ記述子が漏れるのを防ぐ。
            let file = File::open(path)?;
            let flags = fcntl_getfd(&file)?;
            fcntl_setfd(&file, flags & !FdFlags::CLOEXEC)?;
            let status = Command::new(program)
                .args(rest)
                .stdin(std::process::Stdio::inherit())
                .env("PROBE_FD", file.as_raw_fd().to_string())
                .status()?;
            // file は子の終了まで保持する。
            drop(file);
            let code = status
                .code()
                .and_then(|code| u8::try_from(code).ok())
                .unwrap_or(12);
            eprintln!(
                "{operation}: {}",
                if code == 0 {
                    text::OK
                } else {
                    text::CHILD_FAILED
                }
            );
            return Ok(Some(code));
        }
        _ => return process::run(operation, args),
    }?;
    eprintln!("{operation}: {}", text::OK);
    Ok(Some(0))
}

fn descriptor(number: &OsStr) -> io::Result<Option<u8>> {
    let Some(number) = number.to_str().and_then(|value| value.parse::<u32>().ok()) else {
        return Ok(None);
    };
    #[cfg(target_os = "macos")]
    let path = PathBuf::from(format!("/dev/fd/{number}"));
    #[cfg(not(target_os = "macos"))]
    let path = PathBuf::from(format!("/proc/self/fd/{number}"));
    match fs::metadata(path) {
        Ok(_) => {
            eprintln!("fd-open: {}", text::OK);
            Ok(Some(0))
        }
        // macOS の /dev/fd は、閉じた番号の stat に ENOENT でなく EBADF を返す。
        // Linux の /proc/self/fd と同じ終了状態に揃える（実装プラン P0-04「作るもの」）。
        Err(error)
            if error.kind() == io::ErrorKind::NotFound
                || error.raw_os_error() == Some(rustix::io::Errno::BADF.raw_os_error()) =>
        {
            eprintln!("fd-open: {}: {:?}", text::FD_CLOSED, error.kind());
            Ok(Some(13))
        }
        Err(error) => Err(error),
    }
}
