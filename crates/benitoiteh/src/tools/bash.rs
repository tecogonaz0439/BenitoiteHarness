//! 通常モードのシェルを確認後に起動し、出力の末尾と保存を管理する（設計書 01-03「bash（通常モード）」）。

use std::ffi::{OsStr, OsString};
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::os::unix::fs::MetadataExt;
use std::os::unix::process::ExitStatusExt;
use std::path::{Component, Path, PathBuf};

use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use serde_json::Value;

use super::process_output::Output;
use super::{
    ApprovalRequest, ApprovalStatus, Invocation, ShellContext, ToolContext, ToolError, ToolOutput,
    text,
};
use crate::approval::{ApprovalFacts, ApprovalKind};
use crate::sandbox::run::{self, Leftovers, RunCommand, RunEnd, RunLimits};
use crate::sandbox::{SandboxCommand, SandboxStdin};

const MAX_BYTES: usize = 50 * 1024;
const MAX_LINES: usize = 2000;

fn failure(error: &dyn std::fmt::Display) -> ToolError {
    ToolError::InvalidArguments(text::bash_failure(error))
}

pub(super) fn execute(
    context: &ToolContext,
    arguments: &Value,
    approval: ApprovalStatus,
    invocation: Invocation<'_>,
) -> Result<ToolOutput, ToolError> {
    let command = arguments
        .get("command")
        .and_then(Value::as_str)
        .ok_or_else(|| ToolError::InvalidArguments(text::missing_argument("command")))?;
    let timeout = match arguments.get("timeout") {
        None => 120,
        Some(value) => value
            .as_u64()
            .filter(|n| (1..=600).contains(n))
            .ok_or_else(|| ToolError::InvalidArguments(text::BASH_TIMEOUT.into()))?,
    };
    let escalate = arguments
        .get("escalate")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    if approval == ApprovalStatus::Unapproved {
        return Ok(ToolOutput::needs_approval(ApprovalRequest {
            kind: if escalate {
                ApprovalKind::Escalate
            } else {
                ApprovalKind::Command
            },
            facts: ApprovalFacts {
                command: Some(command.to_owned()),
                cwd: Some(context.project.real_path().to_owned()),
                sandbox_policy: Some(
                    context
                        .shell
                        .as_ref()
                        .map_or(crate::config::Sandbox::WorkspaceWrite, |shell| {
                            shell.sandbox
                        })
                        .name()
                        .to_owned(),
                ),
                ..ApprovalFacts::default()
            },
            justification: arguments
                .get("justification")
                .and_then(Value::as_str)
                .map(str::to_owned),
            summary: text::bash_approval(command, escalate),
        }));
    }
    let (Some(shell), Some(session)) = (&context.shell, &context.session) else {
        return Err(ToolError::InvalidArguments(text::BASH_CONTEXT.into()));
    };
    let sandboxed = shell.policy.is_some() && !escalate;
    let mediated = !sandboxed
        && (shell.sandbox == crate::config::Sandbox::DangerFullAccess
            || (escalate && shell.policy.is_some() && approval == ApprovalStatus::Approved));
    let mut changed_directory = shell
        .askpass
        .as_ref()
        .is_some_and(|broker| !broker.valid_directory());
    let secret_call = if mediated && !changed_directory {
        shell
            .askpass
            .as_ref()
            .map(|broker| {
                broker.begin(crate::askpass::Facts {
                    call_id: invocation.call_id.into(),
                    command: command.into(),
                    cwd: context.project.real_path().into(),
                    mode: if shell.sandbox == crate::config::Sandbox::DangerFullAccess {
                        crate::askpass::text::NORMAL_FULL
                    } else {
                        crate::askpass::text::NORMAL_ESCALATED
                    },
                })
            })
            .transpose()
            .or_else(|error| {
                if matches!(error, crate::askpass::AskpassError::ChangedDirectory) {
                    changed_directory = true;
                    Ok(None)
                } else {
                    Err(error)
                }
            })
            .map_err(|error| failure(&error))?
            .flatten()
    } else {
        None
    };
    let (launch, proxy, denial_log) = prepare(
        shell,
        context.project.real_path(),
        command,
        sandboxed,
        secret_call.as_ref(),
        &mut changed_directory,
    )?;
    let captured = if let Some(call) = &secret_call {
        Output::mediated(MAX_BYTES, MAX_LINES, call.redactor())
    } else {
        Output::new(
            session
                .spill_writer(invocation.call_id)
                .map_err(|e| failure(&e))?,
            MAX_BYTES,
            MAX_LINES,
        )
    };
    let sink = Arc::new(Mutex::new(captured));
    let stdout = Arc::new(Mutex::new(super::process_output::StreamOutput::new(
        Arc::clone(&sink),
        secret_call.as_ref().map(|call| call.redactor()),
    )));
    let stderr = Arc::new(Mutex::new(super::process_output::StreamOutput::new(
        Arc::clone(&sink),
        secret_call.as_ref().map(|call| call.redactor()),
    )));
    context.checkpoint_before(crate::rewind::CheckpointReason::Bash, None, &invocation)?;
    let outcome = run::run_with_pause(
        launch,
        Vec::new(),
        Arc::clone(&stdout),
        Arc::clone(&stderr),
        RunLimits {
            timeout: Duration::from_secs(timeout),
            ..RunLimits::default()
        },
        invocation.cancelled,
        secret_call.as_ref().map(|call| call as &dyn run::RunPause),
    );
    if let Some(call) = &secret_call {
        call.finish();
    }
    // 流れごとの保留を先に流し、混合列の二段目も切り詰め前に確定する。
    stdout
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .finish()?;
    stderr
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .finish()?;
    // パイプの読み手が残っていても、この錠の中で書き手を抜いた後は出力を捨てる。
    // （設計書 03-02「子プロセスの後始末」）。
    let mut output = sink.lock().unwrap_or_else(PoisonError::into_inner);
    output.finish_redaction()?;
    output.closed = true;
    let writer = output.writer.take();
    let outcome = match outcome {
        Ok(outcome) => outcome,
        Err(error) => {
            if let Some(writer) = writer {
                let _removed = writer.discard();
            }
            return Err(failure(&error));
        }
    };
    let (mut contents, first, total, truncated) = output.tail();
    let spill = match writer {
        Some(writer) if truncated => writer.finish().ok(),
        Some(writer) => {
            writer.discard().map_err(|e| failure(&e))?;
            None
        }
        None => None,
    };
    if truncated {
        append(
            &mut contents,
            &text::bash_truncated(
                first,
                total,
                total,
                spill.as_ref().map(|info| (info, session.directory())),
            ),
        );
    }
    let signal = outcome.status.signal();
    let code = outcome
        .status
        .code()
        .unwrap_or_else(|| 128_i32.saturating_add(signal.unwrap_or(0)));
    let blocked = sandbox_denied(
        code,
        signal,
        &String::from_utf8_lossy(&output.bytes),
        sandboxed,
        cfg!(target_os = "linux"),
    );
    if changed_directory {
        append(&mut contents, crate::askpass::text::CHANGED_DIRECTORY);
    }
    if secret_call.is_some() {
        append(&mut contents, crate::askpass::text::SPILL_DISCARDED);
    }
    if let Some(call) = &secret_call {
        for event in call.events() {
            append(&mut contents, &event);
        }
    }
    append(&mut contents, &text::bash_exit(code));
    match outcome.end {
        RunEnd::TimedOut => append(&mut contents, &text::bash_timed_out(timeout)),
        RunEnd::Cancelled => append(&mut contents, text::BASH_CANCELLED),
        RunEnd::Exited => {}
    }
    if outcome.leftovers == Leftovers::PossiblyRemaining {
        append(&mut contents, text::BASH_LEFTOVERS);
    }
    if blocked {
        append(&mut contents, text::BASH_SANDBOX);
        // macOS では、推定でなく、Seatbelt が記録した拒否の操作と対象を添える。読むのに 1 秒ほどかかるので、
        // 拒否と見なしたときだけ読む（設計書 03-02「拒否の検出」）。
        if let Some(log) = &denial_log {
            let denials = log.read();
            for denial in &denials.shown {
                append(&mut contents, &text::bash_denial(denial));
            }
            if denials.omitted > 0 {
                append(&mut contents, &text::bash_denials_omitted(denials.omitted));
            }
        }
    }
    // 中継が断った宛先と理由を、推定でなく事実として返す（設計書 03-02「通信の中継」）。
    if let Some(proxy) = &proxy {
        for denial in proxy.run.denials() {
            append(&mut contents, &denial);
        }
    }
    Ok(ToolOutput {
        screen: Some(text::screen_exit(code)),
        spill,
        is_error: code != 0,
        ..ToolOutput::success(contents)
    })
}

fn append(output: &mut String, line: &str) {
    if !output.is_empty() && !output.ends_with('\n') {
        output.push('\n');
    }
    output.push_str(line);
}

fn shell_arguments(program: &Path, command: &str) -> Vec<OsString> {
    let mut args = Vec::new();
    if !program
        .file_name()
        .is_some_and(|name| ["bash", "sh", "dash"].iter().any(|known| name == *known))
    {
        args.push("sh".into());
    }
    args.extend([OsString::from("-c"), OsString::from(command)]);
    args
}

fn prepare(
    shell: &ShellContext,
    cwd: &Path,
    command: &str,
    sandboxed: bool,
    secret_call: Option<&crate::askpass::CallGuard>,
    changed_directory: &mut bool,
) -> Result<
    (
        RunCommand,
        Option<ProxyGuard>,
        Option<crate::sandbox::DenialLog>,
    ),
    ToolError,
> {
    let path = if Path::new("/bin/bash").is_file() {
        Path::new("/bin/bash")
    } else {
        Path::new("/bin/sh")
    };
    let mut env = shell.env.clone();
    if *changed_directory
        || shell
            .askpass
            .as_ref()
            .is_some_and(|broker| !broker.valid_directory())
    {
        *changed_directory = true;
        crate::askpass::clear_environment(&mut env);
    } else if let Some(call) = secret_call {
        call.environment(&mut env);
    }
    if sandboxed {
        let program = path.canonicalize()?;
        let policy = shell
            .policy
            .clone()
            .ok_or_else(|| ToolError::InvalidArguments(text::BASH_CONTEXT.into()))?;
        let (policy, guard) = proxy(shell, cwd, command, policy, &mut env)?;
        let sandbox = SandboxCommand {
            policy,
            args: shell_arguments(&program, command),
            program,
            cwd: cwd.to_owned(),
            env,
            stdin: SandboxStdin::Null,
        };
        #[cfg(target_os = "linux")]
        {
            let prepared = sandbox
                .prepare(&shell.helper, &shell.scratch, &shell.placeholders)
                .map_err(|e| failure(&e))?;
            let prepared = match &guard {
                Some(guard) => prepared.with_proxy(Arc::clone(&guard.run)),
                None => prepared,
            };
            Ok((prepared.into(), guard, None))
        }
        #[cfg(target_os = "macos")]
        {
            let (launch, denial_log) = sandbox
                .to_command_reporting(&shell.helper)
                .map_err(|e| failure(&e))?;
            Ok((launch.into(), guard, Some(denial_log)))
        }
        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        {
            let launch = sandbox.to_command(&shell.helper).map_err(|e| failure(&e))?;
            Ok((launch.into(), guard, None))
        }
    } else {
        env.remove(std::ffi::OsStr::new("BENITOITEH_SANDBOX"));
        env.remove(std::ffi::OsStr::new("BENITOITEH_SANDBOX_NETWORK_DISABLED"));
        // 承認を受けて外で動かす一回では、サンドボックスの中から書ける場所に届く PATH の項目を除く。
        // 中のコマンドが置いた偽の git などを、利用者が承認した外の実行に拾わせない
        // （設計書 03-02「通常モードの方針」）。
        if shell.policy.is_some()
            && let Some(value) = env.get_mut(std::ffi::OsStr::new("PATH"))
        {
            *value = outside_path(value, &shell.writable_roots);
        }
        let mut launch = crate::sandbox::session::command(&shell.helper, path);
        launch
            .arg("-c")
            .arg(command)
            .current_dir(cwd)
            .env_clear()
            .envs(env);
        Ok((launch.into(), None, None))
    }
}

/// 一回のコマンドの中継。断った記録を結果に添えるまで保つ。macOS では待ち受けもここで持つ。
/// 落とすときにコマンドが終わった印を立て、残った接続に尋ねさせない（設計書 03-02「通信の中継」）。
struct ProxyGuard {
    run: Arc<crate::sandbox::proxy::ProxyRun>,
    #[cfg(not(target_os = "linux"))]
    _server: crate::sandbox::proxy::ProxyServer,
}

impl Drop for ProxyGuard {
    fn drop(&mut self) {
        self.run.finish();
    }
}

/// 中継の方式なら、このコマンドの中継を用意し、子にプロキシの変数を渡す（設計書 03-02「通信の中継」）。
/// `NO_PROXY` は除く。受け継いだ例外で中継を飛ばすと、つながらずに失敗するだけだからである。
fn proxy(
    shell: &ShellContext,
    cwd: &Path,
    command: &str,
    policy: crate::sandbox::SandboxPolicy,
    env: &mut std::collections::BTreeMap<OsString, OsString>,
) -> Result<(crate::sandbox::SandboxPolicy, Option<ProxyGuard>), ToolError> {
    let Some(gate) = shell
        .network
        .as_ref()
        .filter(|_| policy.config().network_proxy)
    else {
        return Ok((policy, None));
    };
    #[cfg(target_os = "linux")]
    let (policy, guard, port) = (
        policy,
        ProxyGuard {
            run: crate::sandbox::proxy::ProxyRun::new(Arc::clone(gate), command.into(), cwd.into()),
        },
        crate::sandbox::proxy::LINUX_PORT,
    );
    // macOS では、主のプロセスがループバックで待ち受け、方針でそのポートへの接続だけを許す。
    // 待ち受けは手元のほかのプロセスからも届くので、呼び出しごとの合言葉を求める。
    #[cfg(not(target_os = "linux"))]
    let (policy, guard, port) = {
        let (listeners, port) = loopback_listeners()?;
        let mut random = [0_u8; 16];
        getrandom::fill(&mut random).map_err(|e| failure(&e))?;
        let run = crate::sandbox::proxy::ProxyRun::with_token(
            Arc::clone(gate),
            command.into(),
            cwd.into(),
            format!("{:032x}", u128::from_ne_bytes(random)),
        );
        let mut config = policy.config().clone();
        config.network_proxy = false;
        config.allow_loopback_port = std::num::NonZeroU16::new(port);
        let policy = crate::sandbox::SandboxPolicy::new(config).map_err(|e| failure(&e))?;
        let server =
            crate::sandbox::proxy::serve(listeners, Arc::clone(&run)).map_err(|e| failure(&e))?;
        (
            policy,
            ProxyGuard {
                run,
                _server: server,
            },
            port,
        )
    };
    let url = OsString::from(guard.run.proxy_url(port));
    for name in [
        "HTTP_PROXY",
        "HTTPS_PROXY",
        "ALL_PROXY",
        "http_proxy",
        "https_proxy",
        "all_proxy",
    ] {
        env.insert(name.into(), url.clone());
    }
    for name in ["NO_PROXY", "no_proxy"] {
        env.remove(std::ffi::OsStr::new(name));
    }
    Ok((policy, Some(guard)))
}

/// 同じ番号で、IPv4 と IPv6 のループバックの両方に待ち受けを作る。Seatbelt の `localhost:<番号>` の規則は
/// `127.0.0.1` と `::1` の両方への接続を許すので、片方だけを持つと、ほかのプロセスがもう片方の同じ番号で
/// 待ち受けたときに、中継を通らずにそこへ届く（BH-119）。IPv6 のループバックがない機械では、そこで待ち受ける
/// ものもないので、IPv4 だけでよい。
#[cfg(not(target_os = "linux"))]
fn loopback_listeners() -> Result<(Vec<std::net::TcpListener>, u16), ToolError> {
    let mut last = None;
    for _ in 0..16 {
        let v4 = std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))?;
        let port = v4.local_addr()?.port();
        match std::net::TcpListener::bind((std::net::Ipv6Addr::LOCALHOST, port)) {
            Ok(v6) => return Ok((vec![v4, v6], port)),
            Err(error) if error.kind() == std::io::ErrorKind::AddrNotAvailable => {
                return Ok((vec![v4], port));
            }
            // 同じ番号の IPv6 がほかで使われている。両方を閉じて別の番号で取り直す。
            Err(error) => last = Some(error),
        }
    }
    Err(last
        .unwrap_or_else(|| std::io::Error::from(std::io::ErrorKind::AddrInUse))
        .into())
}

/// `PATH` の項目のうち、書ける場所に届くものを除いて並べ直す。
fn outside_path(path: &OsStr, roots: &[PathBuf]) -> OsString {
    let roots: Vec<Root> = roots.iter().map(|root| Root::new(root)).collect();
    let kept: Vec<&[u8]> = path
        .as_bytes()
        .split(|byte| *byte == b':')
        .filter(|entry| !reaches_writable(Path::new(OsStr::from_bytes(entry)), &roots))
        .collect();
    OsString::from_vec(kept.join(&b':'))
}

/// 書ける場所の根。名前と、ファイルシステムの上の同一性（デバイスと inode）の両方で比べる。
/// 同一性で比べるのは、大小文字を区別しないファイルシステムの綴りの違う名前や、別の場所の
/// bind マウントから根に入る道を捕まえるため。
struct Root {
    path: PathBuf,
    identity: Option<(u64, u64)>,
}
impl Root {
    fn new(path: &Path) -> Self {
        let path = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_owned());
        let identity = std::fs::metadata(&path)
            .ok()
            .map(|metadata| (metadata.dev(), metadata.ino()));
        Self { path, identity }
    }
}

enum Part {
    Parent,
    Name(OsString),
}

fn parts(path: &Path) -> impl DoubleEndedIterator<Item = Part> + '_ {
    path.components().filter_map(|component| match component {
        Component::ParentDir => Some(Part::Parent),
        Component::Normal(name) => Some(Part::Name(name.to_owned())),
        Component::RootDir | Component::CurDir | Component::Prefix(_) => None,
    })
}

/// 項目を根から一段ずつ辿り、通るディレクトリかリンクの先が書ける場所に入るかを調べる。
/// 相対の項目（空の項目を含む）は作業ディレクトリに依るので、いつも届くとみなす。
/// 調べられないときも届くとみなす（nono の broker_path と同じく、リンクの各段を調べる）。
fn reaches_writable(entry: &Path, roots: &[Root]) -> bool {
    if !entry.is_absolute() {
        return true;
    }
    let inside = |path: &Path, metadata: Option<&std::fs::Metadata>| {
        roots.iter().any(|root| {
            path.starts_with(&root.path)
                || metadata
                    .is_some_and(|metadata| root.identity == Some((metadata.dev(), metadata.ino())))
        })
    };
    let mut pending: Vec<Part> = parts(entry).rev().collect();
    let mut current = PathBuf::from("/");
    let mut links = 0_u8;
    while let Some(part) = pending.pop() {
        let name = match part {
            Part::Parent => {
                current.pop();
                continue;
            }
            Part::Name(name) => name,
        };
        let next = current.join(name);
        match std::fs::symlink_metadata(&next) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                if inside(&next, None) {
                    return true;
                }
                links = links.saturating_add(1);
                let Ok(target) = std::fs::read_link(&next) else {
                    return true;
                };
                if links > 40 {
                    return true;
                }
                if target.is_absolute() {
                    current = PathBuf::from("/");
                }
                pending.extend(parts(&target).rev());
            }
            Ok(metadata) => {
                if inside(&next, Some(&metadata)) {
                    return true;
                }
                current = next;
            }
            // まだない場所は、書ける場所の中でなければ中から作れない。書ける場所の中かは、
            // 残りを名前のまま辿った後の最後の判定で見る。
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => current = next,
            Err(_) => return true,
        }
    }
    inside(&current, None)
}

fn sandbox_denied(
    code: i32,
    signal: Option<i32>,
    tail: &str,
    sandboxed: bool,
    linux: bool,
) -> bool {
    if !sandboxed || [0, 2, 126, 127].contains(&code) {
        return false;
    }
    let tail = tail.to_ascii_lowercase();
    (linux && (code == 159 || signal == Some(31)))
        || [
            "operation not permitted",
            "permission denied",
            "read-only file system",
            "sandbox",
        ]
        .iter()
        .any(|needle| tail.contains(needle))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn shell_launch_supports_multicall_executables() {
        for name in ["bash", "sh", "dash", "busybox"] {
            let args = shell_arguments(&Path::new("/bin").join(name), "echo ok");
            let expected: Vec<OsString> = if name == "busybox" {
                vec!["sh".into(), "-c".into(), "echo ok".into()]
            } else {
                vec!["-c".into(), "echo ok".into()]
            };
            assert_eq!(args, expected);
        }
    }
    #[test]
    fn denial_detection_excludes_shell_errors_and_unsandboxed_commands() {
        for linux in [false, true] {
            for code in [0, 1, 2, 125, 126, 127, 159] {
                for sandboxed in [false, true] {
                    assert_eq!(
                        sandbox_denied(code, None, "PERMISSION DENIED", sandboxed, linux),
                        sandboxed && ![0, 2, 126, 127].contains(&code)
                    );
                    assert_eq!(
                        sandbox_denied(code, None, "", sandboxed, linux),
                        sandboxed && linux && code == 159
                    );
                }
            }
        }
        assert!(sandbox_denied(128, Some(31), "", true, true));
    }
}
