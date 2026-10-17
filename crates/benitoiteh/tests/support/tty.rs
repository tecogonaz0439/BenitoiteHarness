//! 制御端末を持つ子を、本物の擬似端末で確かめる（設計書 03-01「保証しないこと」、BH-097）。

use std::fs::File;
use std::io::{self, Read};
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::time::{Duration, Instant};

use rustix::fs::{Mode, OFlags};
use rustix::io::{FdFlags, fcntl_getfd, fcntl_setfd};
use rustix::pty::{OpenptFlags, grantpt, openpt, ptsname, unlockpt};

pub struct Pty {
    master: File,
    slave: File,
    path: PathBuf,
}
impl Pty {
    pub fn new() -> io::Result<Self> {
        let master = openpt(OpenptFlags::RDWR | OpenptFlags::NOCTTY)?;
        fcntl_setfd(&master, fcntl_getfd(&master)? | FdFlags::CLOEXEC)?;
        grantpt(&master)?;
        unlockpt(&master)?;
        let path = PathBuf::from(std::ffi::OsStr::from_bytes(
            ptsname(&master, Vec::new())?.to_bytes(),
        ));
        let slave = File::from(rustix::fs::open(
            &path,
            OFlags::RDWR | OFlags::NOCTTY | OFlags::CLOEXEC,
            Mode::empty(),
        )?);
        rustix::termios::tcsetwinsize(
            &slave,
            rustix::termios::Winsize {
                ws_row: 24,
                ws_col: 120,
                ws_xpixel: 0,
                ws_ypixel: 0,
            },
        )?;
        let flags = rustix::fs::fcntl_getfl(&master)?;
        rustix::fs::fcntl_setfl(&master, flags | OFlags::NONBLOCK)?;
        Ok(Self {
            master: master.into(),
            slave,
            path,
        })
    }

    pub fn slave(&self) -> io::Result<File> {
        self.slave.try_clone()
    }

    pub fn input(&self) -> io::Result<File> {
        self.master.try_clone()
    }

    pub fn wrapper(&self, probe: &Path) -> Command {
        let mut wrapper = Command::new(probe);
        wrapper.arg("with-ctty").arg(&self.path).arg("--");
        wrapper
            .env_clear()
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        wrapper
    }

    /// 終了待ちと読み取りを交互に行う。端末の出力待ちで子の exit が止まっても排出を続ける。
    pub fn capture(&mut self, child: &mut Child) -> io::Result<(Output, Vec<u8>)> {
        self.capture_until(child, Duration::from_secs(10))
    }

    pub fn capture_until(
        &mut self,
        child: &mut Child,
        limit: Duration,
    ) -> io::Result<(Output, Vec<u8>)> {
        self.interact(child, limit, |_, _, _| Ok(()))
    }

    pub fn interact(
        &mut self,
        child: &mut Child,
        limit: Duration,
        mut action: impl FnMut(&mut Child, &[u8], Duration) -> io::Result<()>,
    ) -> io::Result<(Output, Vec<u8>)> {
        let mut guard = ChildGuard {
            child,
            master: &mut self.master,
            reaped: false,
        };
        let mut stdout = guard.child.stdout.take();
        let mut stderr = guard.child.stderr.take();
        if let Some(fd) = &stdout {
            nonblocking(fd)?;
        }
        if let Some(fd) = &stderr {
            nonblocking(fd)?;
        }
        let mut out = Vec::new();
        let mut err = Vec::new();
        let mut terminal = Vec::new();
        let started = Instant::now();
        let mut killed = false;
        loop {
            drain(&mut *guard.master, &mut terminal)?;
            if let Some(reader) = &mut stdout {
                drain(reader, &mut out)?;
            }
            if let Some(reader) = &mut stderr {
                drain(reader, &mut err)?;
            }
            action(guard.child, &terminal, started.elapsed())?;
            if let Some(status) = guard.child.try_wait()? {
                guard.reaped = true;
                drain(&mut *guard.master, &mut terminal)?;
                if let Some(reader) = &mut stdout {
                    drain(reader, &mut out)?;
                }
                if let Some(reader) = &mut stderr {
                    drain(reader, &mut err)?;
                }
                if killed {
                    return Err(io::Error::new(
                        io::ErrorKind::TimedOut,
                        format!(
                            "pty child exceeded its deadline; stdout: {}; stderr: {}; terminal: {}",
                            String::from_utf8_lossy(&out),
                            String::from_utf8_lossy(&err),
                            String::from_utf8_lossy(&terminal)
                        ),
                    ));
                }
                return Ok((
                    Output {
                        status,
                        stdout: out,
                        stderr: err,
                    },
                    terminal,
                ));
            }
            if started.elapsed() >= limit && !killed {
                // with-ctty の setsid が作ったグループ全体を止める。setsid 前の失敗にも備える。
                if let Some(pid) = rustix::process::Pid::from_raw(
                    i32::try_from(guard.child.id()).map_err(io::Error::other)?,
                ) {
                    let _group =
                        rustix::process::kill_process_group(pid, rustix::process::Signal::KILL);
                }
                let _child = guard.child.kill();
                killed = true;
            }
            // kill 後の回収にも上限を設け、カーネルの終了処理で検査全体を止めない。
            if killed && started.elapsed() >= limit.saturating_add(Duration::from_secs(1)) {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "pty child did not exit after SIGKILL",
                ));
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    pub fn control(&mut self, probe: &Path) -> io::Result<()> {
        for operation in ["tty-open", "tty-write", "tty-ioctl"] {
            let mut child = self.wrapper(probe).arg(probe).arg(operation).spawn()?;
            let (output, terminal) = self.capture(&mut child)?;
            assert!(output.status.success(), "{output:?}");
            assert_eq!(
                terminal,
                if operation == "tty-write" {
                    b"sandbox-probe-tty\r\n".as_slice()
                } else {
                    b""
                }
            );
        }
        Ok(())
    }
}

fn drain(reader: &mut impl Read, bytes: &mut Vec<u8>) -> io::Result<()> {
    // 一周の読み取りも有限にし、出力し続ける子でも期限を調べる。
    let mut buffer = [0; 8192];
    for _ in 0..32 {
        match reader.read(&mut buffer) {
            Ok(0) => break,
            Ok(count) => bytes.extend_from_slice(buffer.get(..count).unwrap_or_default()),
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => break,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

fn nonblocking(fd: &impl std::os::fd::AsFd) -> io::Result<()> {
    let flags = rustix::fs::fcntl_getfl(fd)?;
    rustix::fs::fcntl_setfl(fd, flags | OFlags::NONBLOCK)?;
    Ok(())
}

// 失敗の出口ごとに kill を書くと、読み取りや action の ? から漏れる。
// 回収前の番号だけを保持し、Drop でも主側を読みながら有限時間で回収する。
struct ChildGuard<'a> {
    child: &'a mut Child,
    master: &'a mut File,
    reaped: bool,
}
impl Drop for ChildGuard<'_> {
    fn drop(&mut self) {
        if self.reaped {
            return;
        }
        let pid = rustix::process::Pid::from_child(self.child);
        let _group = rustix::process::kill_process_group(pid, rustix::process::Signal::KILL);
        let _child = self.child.kill();
        let started = Instant::now();
        let mut ignored = Vec::new();
        loop {
            ignored.clear();
            let _drain = drain(self.master, &mut ignored);
            match self.child.try_wait() {
                Ok(Some(_)) => {
                    self.reaped = true;
                    break;
                }
                Err(_) => break,
                Ok(None) => {}
            }
            if started.elapsed() >= Duration::from_secs(1) {
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }
}
