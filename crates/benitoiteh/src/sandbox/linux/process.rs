//! 子の終了を確認するまでマウントの場所を保持する（設計書 03-02「Linux のファイルシステムの見え方」）。

use std::ffi::OsStr;
use std::fs::File;
use std::io::{self, IoSlice, PipeWriter, Write};
use std::mem::MaybeUninit;
use std::os::fd::{AsFd, OwnedFd};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::process::{Child, Command, ExitStatus, Output, Stdio};

use super::mounts::{self, Cleanup};
use crate::sandbox::run::StopSignal;
use crate::sandbox::{SandboxCommand, SandboxError, SandboxStdin, policy, text};

/// 起動するプログラムと引数、および Linux の後始末の所有権。
///
/// 起動したスレッドは子の終了まで生存させること。`--die-with-parent` は
/// プロセス全体でなく、起動したスレッドが終わったときにも働く。
pub struct PreparedSandbox {
    command: Command,
    inherited: Option<UnixStream>,
    cleanup: Cleanup,
    control: UnixStream,
    stdin: Option<PipeWriter>,
}

/// 終了を待った後にだけ、塞ぐための空のディレクトリと覆いを片付ける子。
///
/// `Drop` は制御ソケットを閉じて停止を要求するが、子を待たず、後始末もしない。
/// 中のプロセスが動いている間の
/// `rmdir` はマウントを外しうるためである（設計書 03-02）。必ず wait すること。
/// 回収せずに落とすと、場所の錠は親プロセスが終わるまで保持する。
pub struct SandboxChild {
    child: Child,
    cleanup: Cleanup,
    control: UnixStream,
    stdin: Option<PipeWriter>,
}

impl SandboxCommand {
    /// 固定の `/usr/bin/bwrap` での起動を準備する。
    /// `scratch` は、秘密のファイルを覆う空のファイルを置く、書ける場所の外のディレクトリ。
    /// `placeholders` は、書ける場所の外の、利用者だけが使える錠と記録のディレクトリ。
    pub fn prepare(
        &self,
        helper: &Path,
        scratch: &Path,
        placeholders: &Path,
    ) -> Result<PreparedSandbox, SandboxError> {
        policy::canonical_path(&self.program)?;
        policy::canonical_path(&self.cwd)?;
        policy::directory(&self.cwd)?;
        policy::canonical_path(helper)?;
        if self
            .policy
            .config()
            .writable
            .iter()
            .any(|root| helper.starts_with(root))
        {
            return Err(SandboxError::InvalidPolicy(text::WRITABLE_HELPER));
        }
        let json = serde_json::to_string(&self.policy).map_err(|error| SandboxError::Setup {
            stage: text::POLICY,
            detail: error.to_string(),
        })?;
        let (control, inherited) = UnixStream::pair().map_err(SandboxError::Io)?;
        let plan = mounts::build(&self.policy, &self.cwd, scratch, placeholders)?;
        let mut command = Command::new("/usr/bin/bwrap");
        command
            .args(plan.args)
            .arg("--")
            .arg(helper)
            .arg("__sandbox")
            .arg(json)
            .arg("--")
            .arg(&self.program)
            .args(&self.args);
        let command = self.configure(command)?;
        let mut prepared = PreparedSandbox {
            command,
            inherited: Some(inherited),
            cleanup: plan.cleanup,
            control,
            stdin: None,
        };
        let (target_stdin, stdin): (OwnedFd, _) = match self.stdin {
            SandboxStdin::Null => (
                File::open("/dev/null").map_err(SandboxError::Io)?.into(),
                None,
            ),
            SandboxStdin::Inherit => (
                rustix::io::fcntl_dupfd_cloexec(rustix::stdio::stdin(), 3)
                    .map_err(|e| SandboxError::Io(e.into()))?,
                None,
            ),
            SandboxStdin::Piped => {
                let (reader, writer) = io::pipe().map_err(SandboxError::Io)?;
                (reader.into(), Some(writer))
            }
        };
        send_stdin(&prepared.control, &target_stdin).map_err(SandboxError::Io)?;
        prepared.stdin = stdin;
        Ok(prepared)
    }
}

impl PreparedSandbox {
    /// 包むプログラムに渡す、固定の bubblewrap のパス。
    pub fn get_program(&self) -> &OsStr {
        self.command.get_program()
    }

    /// 包むプログラムに渡す、マウントと補助のプロセスの引数。
    pub fn get_args(&self) -> impl Iterator<Item = &OsStr> {
        self.command.get_args()
    }

    /// 子を起動し、後始末の所有権を子に移す。
    pub fn spawn(self) -> Result<SandboxChild, SandboxError> {
        self.spawn_wrapped_inner(None)
    }

    /// 呼び出し元が作ったラッパーを起動し、その終了まで後始末を保留する。
    /// ラッパーは `get_program`・`get_args` の子を起動し、子の終了まで待つこと。
    /// 標準入力は渡された制御ソケットのまま子へ受け継がせること。
    pub fn spawn_wrapped(self, wrapper: Command) -> Result<SandboxChild, SandboxError> {
        self.spawn_wrapped_inner(Some(wrapper))
    }

    fn spawn_wrapped_inner(
        mut self,
        wrapper: Option<Command>,
    ) -> Result<SandboxChild, SandboxError> {
        if let Some(wrapper) = wrapper {
            self.command = wrapper;
        }
        if let Some(inherited) = self.inherited.take() {
            self.command.stdin(Stdio::from(OwnedFd::from(inherited)));
        }
        let control = self.control.try_clone().map_err(SandboxError::Io)?;
        let spawned = self.command.spawn();
        self.command.stdin(Stdio::null());
        match spawned {
            Ok(child) => {
                self.cleanup.retain_locks = true;
                Ok(SandboxChild {
                    child,
                    cleanup: std::mem::take(&mut self.cleanup),
                    control,
                    stdin: self.stdin.take(),
                })
            }
            Err(error) => {
                self.cleanup.finish()?;
                Err(SandboxError::Io(error))
            }
        }
    }

    /// 実行の部分が出力を読み続けられるよう、パイプを設定する。
    pub(crate) fn capture_output(&mut self) {
        self.command.stdout(Stdio::piped()).stderr(Stdio::piped());
    }

    /// 標準出力と標準エラー出力を回収し、終了後に片付ける。
    pub fn output(mut self) -> Result<Output, SandboxError> {
        self.command.stdout(Stdio::piped()).stderr(Stdio::piped());
        self.spawn()?.wait_with_output()
    }

    /// ラッパーの標準出力と標準エラー出力を回収し、終了後に片付ける。
    pub fn output_wrapped(self, mut wrapper: Command) -> Result<Output, SandboxError> {
        wrapper.stdout(Stdio::piped()).stderr(Stdio::piped());
        self.spawn_wrapped(wrapper)?.wait_with_output()
    }
}

impl Drop for PreparedSandbox {
    fn drop(&mut self) {
        // まだ起動していないので、親が作った覆いだけは消せる。
        // 錠は解放し、残った記録は次の prepare の排他錠の下で片付ける。
        if let Some(path) = self.cleanup.cover.take() {
            let _cleanup = std::fs::remove_file(path);
        }
    }
}

impl SandboxChild {
    /// 目的のプログラムの標準入力への書き口を一度だけ取得する。
    pub fn take_stdin(&mut self) -> Option<PipeWriter> {
        self.stdin.take()
    }

    /// 名前空間の中の目的のプログラムのグループへ停止の合図を渡す。
    pub fn stop(&mut self, signal: StopSignal) -> Result<(), SandboxError> {
        self.send_control(match signal {
            StopSignal::Interrupt => b'I',
            StopSignal::Terminate => b'T',
        })
    }

    /// 補助を直ちに終え、名前空間全体を止める。
    pub fn terminate(&mut self) -> Result<(), SandboxError> {
        self.send_control(b'K')
    }

    fn send_control(&mut self, byte: u8) -> Result<(), SandboxError> {
        match self.control.write_all(&[byte]) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == io::ErrorKind::BrokenPipe => Ok(()),
            Err(error) => Err(SandboxError::Io(error)),
        }
    }

    /// 標準入出力の取得、終了要求、プロセス番号の参照に使う。
    /// 終了待ちはこの型の `wait` または `wait_with_output` で行うこと。
    pub fn child_mut(&mut self) -> &mut Child {
        &mut self.child
    }

    /// 回収を確認した実行の部分からだけ呼ぶ。
    pub(crate) fn finish_cleanup(&mut self) -> Result<(), SandboxError> {
        self.cleanup.finish()
    }

    /// 終了を確認してから後始末する。待機に失敗した場合は後始末しない。
    pub fn wait(&mut self) -> Result<ExitStatus, SandboxError> {
        drop(self.stdin.take());
        let status = self.child.wait().map_err(SandboxError::Io)?;
        self.cleanup.finish()?;
        Ok(status)
    }

    /// 出力の回収と終了の確認を終えてから後始末する。
    pub fn wait_with_output(mut self) -> Result<Output, SandboxError> {
        drop(self.stdin.take());
        let output = self.child.wait_with_output().map_err(SandboxError::Io)?;
        self.cleanup.finish()?;
        Ok(output)
    }
}

fn send_stdin(socket: &UnixStream, fd: &OwnedFd) -> io::Result<()> {
    use rustix::net::{SendAncillaryBuffer, SendAncillaryMessage, SendFlags, sendmsg};
    let mut space = [MaybeUninit::uninit(); rustix::cmsg_space!(ScmRights(1))];
    let mut ancillary = SendAncillaryBuffer::new(&mut space);
    let descriptors = [fd.as_fd()];
    if !ancillary.push(SendAncillaryMessage::ScmRights(&descriptors)) {
        return Err(io::Error::other(text::CONTROL));
    }
    sendmsg(
        socket,
        &[IoSlice::new(b"S")],
        &mut ancillary,
        SendFlags::NOSIGNAL,
    )?;
    Ok(())
}
