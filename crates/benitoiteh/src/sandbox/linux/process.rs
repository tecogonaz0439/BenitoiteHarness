//! 子の終了を確認するまでマウントの場所を保持する（設計書 03-02「Linux のファイルシステムの見え方」）。

use std::ffi::OsStr;
use std::path::Path;
use std::process::{Child, Command, ExitStatus, Output, Stdio};

use super::mounts::{self, Cleanup};
use crate::sandbox::{SandboxCommand, SandboxError, policy, text};

/// 起動するプログラムと引数、および Linux の後始末の所有権。
///
/// 起動したスレッドは子の終了まで生存させること。`--die-with-parent` は
/// プロセス全体でなく、起動したスレッドが終わったときにも働く。
pub struct PreparedSandbox {
    command: Command,
    cleanup: Cleanup,
}

/// 終了を待った後にだけ、塞ぐための空のディレクトリと覆いを片付ける子。
///
/// `Drop` は子を待たず、後始末もしない。中のプロセスが動いている間の
/// `rmdir` はマウントを外しうるためである（設計書 03-02）。必ず wait すること。
pub struct SandboxChild {
    child: Child,
    cleanup: Cleanup,
}

impl SandboxCommand {
    /// 固定の `/usr/bin/bwrap` での起動を準備する。
    /// `scratch` は、秘密のファイルを覆う空のファイルを置く、書ける場所の外のディレクトリ。
    pub fn prepare(&self, helper: &Path, scratch: &Path) -> Result<PreparedSandbox, SandboxError> {
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
        let plan = mounts::build(&self.policy, &self.cwd, scratch)?;
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
        Ok(PreparedSandbox {
            command,
            cleanup: plan.cleanup,
        })
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
        match self.command.spawn() {
            Ok(child) => Ok(SandboxChild {
                child,
                cleanup: std::mem::take(&mut self.cleanup),
            }),
            Err(error) => {
                self.cleanup.finish().map_err(SandboxError::Io)?;
                Err(SandboxError::Io(error))
            }
        }
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
        // 起動する前のディレクトリを消すと、別の子のマウントを外しうる（BH-095）。
        if let Some(path) = self.cleanup.cover.take() {
            let _cleanup = std::fs::remove_file(path);
        }
    }
}

impl SandboxChild {
    /// 標準入出力の取得、終了要求、プロセス番号の参照に使う。
    /// 終了待ちはこの型の `wait` または `wait_with_output` で行うこと。
    pub fn child_mut(&mut self) -> &mut Child {
        &mut self.child
    }

    /// 終了を確認してから後始末する。待機に失敗した場合は後始末しない。
    pub fn wait(&mut self) -> Result<ExitStatus, SandboxError> {
        let status = self.child.wait().map_err(SandboxError::Io)?;
        self.cleanup.finish().map_err(SandboxError::Io)?;
        Ok(status)
    }

    /// 出力の回収と終了の確認を終えてから後始末する。
    pub fn wait_with_output(mut self) -> Result<Output, SandboxError> {
        let output = self.child.wait_with_output().map_err(SandboxError::Io)?;
        self.cleanup.finish().map_err(SandboxError::Io)?;
        Ok(output)
    }
}
