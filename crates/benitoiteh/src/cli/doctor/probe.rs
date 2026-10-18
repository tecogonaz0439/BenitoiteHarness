//! 専用の一時領域だけで制限モードの正負の起動を試す（設計書 05-01「doctor」、03-02「サンドボックスの掛け方」）。

use std::collections::BTreeMap;
use std::fs::{self, DirBuilder};
use std::io::{self, Write};
use std::os::unix::fs::DirBuilderExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use crate::sandbox::run::{self, Leftovers, RunEnd, RunLimits, RunOutcome};
use crate::sandbox::{PolicyConfig, ProgramAccess, SandboxCommand, SandboxPolicy, SandboxStdin};

use super::{Item, Status, processor::Located, text};
use crate::config::paths::DataPaths;

/// 診断用の一時領域を、片付けまで所有する。
pub(super) struct Probe {
    root: PathBuf,
    pub(super) write: PathBuf,
    pub(super) helper: PathBuf,
}

impl Probe {
    /// 問い合わせも doctor 専用の一時領域を使い、保存先を作らない。
    pub(super) fn scratch(&self) -> PathBuf {
        self.root.join("scratch")
    }

    /// Linux の覆いも doctor の終了時に片付ける。
    pub(super) fn placeholders(&self) -> PathBuf {
        self.root.join("placeholders")
    }

    /// プロジェクトとデータを変えない場所にだけ、一時領域を作る。
    pub(super) fn new(paths: &DataPaths, project: &Path) -> io::Result<Self> {
        let temporary_root = fs::canonicalize(&paths.temporary_root)?;
        let data = match fs::canonicalize(&paths.data_dir) {
            Ok(data) => Some(data),
            Err(error) if error.kind() == io::ErrorKind::NotFound => None,
            Err(error) => return Err(error),
        };
        let mut random = [0; 16];
        getrandom::fill(&mut random).map_err(io::Error::other)?;
        let root = temporary_root.join(format!(
            "benitoiteh-doctor-{:032x}",
            u128::from_ne_bytes(random)
        ));
        // TMPDIR も利用者の入力である。作ってから書ける場所を検査すると、
        // 結局消した場合でもプロジェクトや記録を一度変えてしまう（実装プラン P4-05）。
        if root.starts_with(project) || data.is_some_and(|data| root.starts_with(data)) {
            return Err(io::Error::other(text::TEMPORARY_LOCATION));
        }
        DirBuilder::new().mode(0o700).create(&root)?;
        // 作成直後から所有し、正規化や子ディレクトリの作成の失敗でも片付ける。
        let mut probe = Self {
            write: root.join("write"),
            helper: PathBuf::new(),
            root,
        };
        probe.root = fs::canonicalize(&probe.root)?;
        probe.write = probe.root.join("write");
        probe.helper = std::env::current_exe().and_then(fs::canonicalize)?;
        for name in ["write", "scratch", "placeholders"] {
            DirBuilder::new()
                .mode(0o700)
                .create(probe.root.join(name))?;
        }
        Ok(probe)
    }

    /// 必須機能と正負の起動の結果を項目に加え、制限モードの可否を返す。
    pub(super) fn check(&self, items: &mut Vec<Item>, processor: Option<&Located>) -> bool {
        let (status, detail) = match self.features(items) {
            Ok(true) => self
                .rule(processor)
                .unwrap_or_else(|error| (Status::Unavailable, error)),
            Ok(false) => (Status::Unchecked, text::RULE_REQUIRED.to_owned()),
            Err(error) => (Status::Unavailable, error),
        };
        let detail = if processor.is_none() {
            format!("{detail} {}", text::FALLBACK_RULE)
        } else {
            detail
        };
        items.push(Item::new("sandbox.restricted_rule", status, detail));
        #[cfg(target_os = "linux")]
        items.push(Item::new(
            "sandbox.linux_scope",
            Status::Warning,
            text::LINUX_SCOPE,
        ));
        status == Status::Ok
    }

    #[cfg(target_os = "macos")]
    fn features(&self, items: &mut Vec<Item>) -> Result<bool, String> {
        let available = Path::new("/usr/bin/sandbox-exec").is_file();
        items.push(Item::new(
            "sandbox.sandbox_exec",
            if available {
                Status::Ok
            } else {
                Status::Unavailable
            },
            "/usr/bin/sandbox-exec",
        ));
        Ok(available)
    }

    #[cfg(target_os = "linux")]
    fn features(&self, items: &mut Vec<Item>) -> Result<bool, String> {
        use crate::sandbox::{CapabilityStatus, bubblewrap_version, probe_linux};
        let version = bubblewrap_version();
        let bubblewrap_detail = match &version {
            Ok(version) => text::bubblewrap(version),
            Err(CapabilityStatus::Unavailable(reason)) => {
                text::at(Path::new("/usr/bin/bwrap"), reason)
            }
            Err(CapabilityStatus::NotChecked | CapabilityStatus::Available) => {
                text::REQUIRES_BWRAP.to_owned()
            }
        };
        // 探査自身の不明な失敗は、後続の機能を使えたと推測せずに示す。
        let availability = match probe_linux(&self.helper, &self.root) {
            Ok(availability) => availability,
            Err(error) => {
                items.push(Item::new(
                    "sandbox.bubblewrap",
                    if version.is_ok() {
                        Status::Ok
                    } else {
                        Status::Unavailable
                    },
                    bubblewrap_detail,
                ));
                for name in [
                    "sandbox.user_namespaces",
                    "sandbox.landlock",
                    "sandbox.seccomp",
                    "sandbox.capabilities",
                ] {
                    items.push(Item::new(name, Status::Unchecked, text::RULE_REQUIRED));
                }
                return Err(error.to_string());
            }
        };
        let required_namespaces = availability.user_namespaces != CapabilityStatus::Available;
        let required_capabilities = availability.capabilities != CapabilityStatus::Available;
        let namespace_reason = if version.is_err() {
            text::REQUIRES_BWRAP
        } else {
            text::RULE_REQUIRED
        };
        let capability_reason = if required_namespaces {
            text::REQUIRES_NAMESPACES
        } else {
            text::RULE_REQUIRED
        };
        let landlock_reason = if required_namespaces {
            text::REQUIRES_NAMESPACES
        } else if required_capabilities {
            text::REQUIRES_CAPABILITIES
        } else {
            text::RULE_REQUIRED
        };
        let mut available = true;
        for (name, status, detail, requires) in [
            (
                "sandbox.bubblewrap",
                availability.bubblewrap,
                bubblewrap_detail.as_str(),
                text::REQUIRES_BWRAP,
            ),
            (
                "sandbox.user_namespaces",
                availability.user_namespaces,
                text::USER_NAMESPACES,
                namespace_reason,
            ),
            (
                "sandbox.landlock",
                availability.landlock_v3,
                text::LANDLOCK,
                landlock_reason,
            ),
            (
                "sandbox.seccomp",
                availability.seccomp,
                text::SECCOMP,
                if required_namespaces || required_capabilities {
                    landlock_reason
                } else {
                    text::REQUIRES_LANDLOCK
                },
            ),
            (
                "sandbox.capabilities",
                availability.capabilities,
                text::CAPABILITIES,
                capability_reason,
            ),
        ] {
            let (status, detail) = match status {
                CapabilityStatus::Available => (Status::Ok, detail.to_owned()),
                CapabilityStatus::Unavailable(reason) => (
                    Status::Unavailable,
                    if name == "sandbox.bubblewrap" {
                        text::at(Path::new("/usr/bin/bwrap"), &reason)
                    } else {
                        reason
                    },
                ),
                CapabilityStatus::NotChecked => (Status::Unchecked, requires.to_owned()),
            };
            available &= status == Status::Ok;
            items.push(Item::new(name, status, detail));
        }
        Ok(available)
    }

    fn rule(&self, processor: Option<&Located>) -> Result<(Status, String), String> {
        let (policy, program, args) = if let Some(processor) = processor {
            let policy = crate::sandbox::restricted_policy(
                &crate::sandbox::RestrictedAccess {
                    write_project: false,
                    allow_ip: false,
                    programs: vec![processor.path.clone()],
                    readable: crate::sandbox::ReadAccess::All,
                },
                &processor.context.locations,
                &processor.context.denied_candidates,
            )
            .map_err(|error| error.to_string())?;
            (policy, processor.path.as_path(), "--version=json")
        } else {
            let mut config = PolicyConfig::new(vec![self.write.clone()]);
            config.programs = ProgramAccess::Only(vec![self.helper.clone()]);
            let policy = SandboxPolicy::new(config).map_err(|error| error.to_string())?;
            (policy, self.helper.as_path(), "--version")
        };
        let shell = match fs::canonicalize("/bin/sh") {
            Ok(shell) => shell,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Ok((Status::Unchecked, text::SHELL_MISSING.to_owned()));
            }
            Err(error) => return Err(error.to_string()),
        };
        // Linux は実のパスを要求する。macOS でも同じファイルを起動する。
        let negative = self.run(
            &policy,
            &shell,
            &["-c", "printf benitoiteh-doctor-shell-ran"],
        )?;
        let denied = negative.completed()
            && {
                #[cfg(target_os = "linux")]
                {
                    negative.outcome.status.code() == Some(125)
                        && negative.stderr.starts_with("Sandbox setup failed at exec:")
                        && negative.stderr.contains("os error 13")
                }
                #[cfg(not(target_os = "linux"))]
                {
                    !negative.outcome.status.success()
                }
            }
            && !negative
                .stdout
                .windows(b"benitoiteh-doctor-shell-ran".len())
                .any(|bytes| bytes == b"benitoiteh-doctor-shell-ran");
        if !denied {
            return Ok((
                Status::Unavailable,
                text::probe_failed(text::RULE_FAILED, &negative.stderr),
            ));
        }
        // シェルを拒めると確かめる前には、利用者が選んだ処理系を起動しない。
        let positive = self.run(&policy, program, &[args])?;
        if !positive.completed()
            || !positive.outcome.status.success()
            || (processor.is_none() && positive.stdout != super::super::text::version().as_bytes())
        {
            let reason = if processor.is_some() {
                text::PROCESSOR_POSITIVE_FAILED
            } else {
                #[cfg(target_os = "linux")]
                let reason = if positive.stderr.contains("os error 13") {
                    text::DYNAMIC
                } else {
                    text::POSITIVE_FAILED
                };
                #[cfg(not(target_os = "linux"))]
                let reason = text::POSITIVE_FAILED;
                reason
            };
            return Ok((
                Status::Unavailable,
                text::probe_failed(reason, &positive.stderr),
            ));
        }
        Ok((
            Status::Ok,
            if processor.is_some() {
                text::PROCESSOR_RULE_OK
            } else {
                text::RULE_OK
            }
            .to_owned(),
        ))
    }

    fn run(
        &self,
        policy: &SandboxPolicy,
        program: &Path,
        args: &[&str],
    ) -> Result<Captured, String> {
        let command = SandboxCommand {
            policy: policy.clone(),
            program: program.to_path_buf(),
            args: args.iter().map(Into::into).collect(),
            cwd: self.write.clone(),
            env: BTreeMap::new(),
            stdin: SandboxStdin::Null,
        };
        #[cfg(target_os = "linux")]
        let command = command.prepare(
            &self.helper,
            &self.root.join("scratch"),
            &self.root.join("placeholders"),
        );
        #[cfg(not(target_os = "linux"))]
        let command = command.to_command(&self.helper);
        let command = command.map_err(|error| error.to_string())?;
        capture(command)
    }
}

fn capture(command: impl Into<run::RunCommand>) -> Result<Captured, String> {
    let exceeded = Arc::new(AtomicBool::new(false));
    let stdout = Arc::new(Mutex::new(Output {
        bytes: Vec::new(),
        cap: 1024 * 1024,
        exceeded: Some(Arc::clone(&exceeded)),
    }));
    let stderr = Arc::new(Mutex::new(Output {
        bytes: Vec::new(),
        cap: 64 * 1024,
        exceeded: None,
    }));
    let outcome = run::run(
        command,
        Vec::new(),
        Arc::clone(&stdout),
        Arc::clone(&stderr),
        RunLimits {
            timeout: Duration::from_secs(10),
            ..RunLimits::default()
        },
        &exceeded,
    )
    .map_err(|error| error.to_string())?;
    if exceeded.load(Ordering::Acquire) {
        return Err(text::PROBE_OUTPUT_LIMIT.to_owned());
    }
    let stdout = std::mem::take(&mut stdout.lock().unwrap_or_else(PoisonError::into_inner).bytes);
    let stderr =
        String::from_utf8_lossy(&stderr.lock().unwrap_or_else(PoisonError::into_inner).bytes)
            .into_owned();
    Ok(Captured {
        outcome,
        stdout,
        stderr,
    })
}

// 読み手は EOF まで排出するが、上限より後は保持しない。stdout の超過だけで
// 子を停止し、stderr は先頭を診断に残す（設計書 05-01「doctor」）。
struct Output {
    bytes: Vec<u8>,
    cap: usize,
    exceeded: Option<Arc<AtomicBool>>,
}

impl Write for Output {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let remaining = self.cap.saturating_sub(self.bytes.len());
        if bytes.len() > remaining
            && let Some(exceeded) = &self.exceeded
        {
            exceeded.store(true, Ordering::Release);
        }
        self.bytes
            .extend_from_slice(bytes.get(..bytes.len().min(remaining)).unwrap_or_default());
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Drop for Probe {
    fn drop(&mut self) {
        // Session を作らず、成功と失敗の両方で doctor 自身の場所だけを消す。
        let _cleanup = fs::remove_dir_all(&self.root);
    }
}

struct Captured {
    outcome: RunOutcome,
    stdout: Vec<u8>,
    stderr: String,
}

impl Captured {
    fn completed(&self) -> bool {
        self.outcome.end == RunEnd::Exited && self.outcome.leftovers == Leftovers::None
    }
}

#[cfg(test)]
mod tests {
    //! OS の方針とは独立に、実際の子から保持する出力量と停止を確かめる。

    use super::*;
    use std::error::Error;
    use std::os::unix::process::CommandExt;
    use std::process::Command;

    struct Directory(PathBuf);
    impl Directory {
        fn new() -> Result<Self, Box<dyn Error>> {
            let mut random = [0; 16];
            getrandom::fill(&mut random)?;
            let directory = Self(std::env::temp_dir().join(format!(
                "bh-doctor-capture-{:032x}",
                u128::from_ne_bytes(random)
            )));
            fs::create_dir(&directory.0)?;
            Ok(directory)
        }

        fn command(
            &self,
            stdout: &str,
            stderr: &str,
            delay: &str,
        ) -> Result<Command, Box<dyn Error>> {
            let program = std::env::current_exe()?
                .parent()
                .and_then(Path::parent)
                .ok_or("Missing target directory")?
                .join("sandbox-probe");
            let mut command = Command::new(program);
            command
                .args(["query-output", stdout, stderr, delay])
                .current_dir(&self.0)
                .process_group(0);
            Ok(command)
        }
    }
    impl Drop for Directory {
        fn drop(&mut self) {
            let _cleanup = fs::remove_dir_all(&self.0);
        }
    }

    // 作成時の関門: query の上限は doctor に適用されない。実際に出力する子で
    // 無制限の保持と、上限超過後に子を動かし続ける退行を捕まえる。
    #[test]
    fn stdout_overflow_stops_the_child() -> Result<(), Box<dyn Error>> {
        let directory = Directory::new()?;
        let started = std::time::Instant::now();
        // stdout の利用者空間のバッファに最後の一バイトが留まっても、
        // 上限を超す量が親へ届くようにする。
        let result = capture(directory.command("2097152", "0", "30000")?);
        assert!(matches!(result, Err(ref reason) if reason == text::PROBE_OUTPUT_LIMIT));
        assert!(started.elapsed() < Duration::from_secs(5));
        Ok(())
    }

    // 作成時の関門: stderr は超過しても停止させず、先頭だけを保持して排出を続ける。
    #[test]
    fn stderr_keeps_only_the_first_64_kib_without_stopping_the_child() -> Result<(), Box<dyn Error>>
    {
        let directory = Directory::new()?;
        let result = capture(directory.command("3", "131072", "0")?)?;
        assert!(result.completed() && result.outcome.status.success());
        assert_eq!(result.stdout, b"xxx");
        assert_eq!(result.stderr.len(), 65536);
        assert!(result.stderr.bytes().all(|byte| byte == b'e'));
        Ok(())
    }
}
