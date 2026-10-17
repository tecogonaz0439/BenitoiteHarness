//! 専用の一時領域だけで制限モードの正負の起動を試す（設計書 05-01「doctor」、03-02「サンドボックスの掛け方」）。

use std::collections::BTreeMap;
use std::fs::{self, DirBuilder};
use std::io;
use std::os::unix::fs::DirBuilderExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use crate::sandbox::run::{self, Leftovers, RunEnd, RunLimits, RunOutcome};
use crate::sandbox::{PolicyConfig, ProgramAccess, SandboxCommand, SandboxPolicy, SandboxStdin};

use super::{Item, Status, text};
use crate::config::paths::DataPaths;

/// 診断用の一時領域を、片付けまで所有する。
pub(super) struct Probe {
    root: PathBuf,
    pub(super) write: PathBuf,
    pub(super) helper: PathBuf,
}

impl Probe {
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
    pub(super) fn check(&self, items: &mut Vec<Item>) -> bool {
        let (status, detail) = match self.features(items) {
            Ok(true) => self
                .rule()
                .unwrap_or_else(|error| (Status::Unavailable, error)),
            Ok(false) => (Status::Unchecked, text::RULE_REQUIRED.to_owned()),
            Err(error) => (Status::Unavailable, error),
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

    fn rule(&self) -> Result<(Status, String), String> {
        let mut config = PolicyConfig::new(vec![self.write.clone()]);
        config.programs = ProgramAccess::Only(vec![self.helper.clone()]);
        let policy = SandboxPolicy::new(config).map_err(|error| error.to_string())?;
        let positive = self.run(&policy, &self.helper, &["--version"])?;
        if !positive.completed()
            || !positive.outcome.status.success()
            || positive.stdout != super::super::text::version().as_bytes()
        {
            #[cfg(target_os = "linux")]
            let reason = if positive.stderr.contains("os error 13") {
                text::DYNAMIC
            } else {
                text::POSITIVE_FAILED
            };
            #[cfg(not(target_os = "linux"))]
            let reason = text::POSITIVE_FAILED;
            return Ok((
                Status::Unavailable,
                text::probe_failed(reason, &positive.stderr),
            ));
        }
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
        Ok(if denied {
            (Status::Ok, text::RULE_OK.to_owned())
        } else {
            (
                Status::Unavailable,
                text::probe_failed(text::RULE_FAILED, &negative.stderr),
            )
        })
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
        let stdout = Arc::new(Mutex::new(Vec::new()));
        let stderr = Arc::new(Mutex::new(Vec::new()));
        let outcome = run::run(
            command,
            Vec::new(),
            Arc::clone(&stdout),
            Arc::clone(&stderr),
            RunLimits {
                timeout: Duration::from_secs(10),
                ..RunLimits::default()
            },
            &AtomicBool::new(false),
        )
        .map_err(|error| error.to_string())?;
        let stdout = stdout
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        let stderr =
            String::from_utf8_lossy(&stderr.lock().unwrap_or_else(PoisonError::into_inner))
                .into_owned();
        Ok(Captured {
            outcome,
            stdout,
            stderr,
        })
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
