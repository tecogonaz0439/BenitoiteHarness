//! 書いた後の検査を、問い合わせと同じ方針のサンドボックスで測る（実装プラン P2-07、BH-028）。

use benitoiteh::config::{child_environment, paths::DataPaths, writable::WritableLocations};
use benitoiteh::sandbox::run::{self, RunCommand, RunEnd, RunLimits, StopSignal};
use benitoiteh::sandbox::{self, ReadAccess, RestrictedAccess, SandboxCommand, SandboxStdin};
use benitoiteh::session::SessionId;
use std::error::Error;
use std::fs;
use std::os::unix::fs::DirBuilderExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};

struct Workspace(PathBuf);
impl Drop for Workspace {
    fn drop(&mut self) {
        let _removed = fs::remove_dir_all(&self.0);
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    let mut args = std::env::args_os().skip(1);
    let processor = fs::canonicalize(args.next().ok_or(text::USAGE)?)?;
    let work = fs::canonicalize(args.next().ok_or(text::USAGE)?)?;
    let helper = fs::canonicalize(
        std::env::current_exe()?
            .parent()
            .and_then(Path::parent)
            .ok_or(text::USAGE)?
            .join("benitoiteh"),
    )?;
    let root = Workspace(work.join(format!(
        "bench-write-check-{}",
        SessionId::generate(SystemTime::now())?
    )));
    fs::DirBuilder::new().mode(0o700).create(&root.0)?;
    for directory in ["temporary", "scratch", "placeholders"] {
        fs::DirBuilder::new()
            .mode(0o700)
            .create(root.0.join(directory))?;
    }
    let version = std::process::Command::new(&processor)
        .arg("--version")
        .output()?;
    if !version.status.success() {
        return Err(text::VERSION.into());
    }
    println!("{}", String::from_utf8_lossy(&version.stdout).trim());
    let os = std::process::Command::new("uname").arg("-a").output()?;
    println!("OS: {}", String::from_utf8_lossy(&os.stdout).trim());
    #[cfg(target_os = "macos")]
    {
        let cpu = std::process::Command::new("/usr/sbin/sysctl")
            .args(["-n", "machdep.cpu.brand_string"])
            .output()?;
        if cpu.status.success() {
            println!("CPU: {}", String::from_utf8_lossy(&cpu.stdout).trim());
        } else {
            println!(
                "CPU: {} ({})",
                std::env::consts::ARCH,
                text::CPU_UNAVAILABLE
            );
        }
    }
    #[cfg(target_os = "linux")]
    println!(
        "CPU: {}",
        fs::read_to_string("/proc/cpuinfo")?
            .lines()
            .find(|line| line.starts_with("model name"))
            .unwrap_or(std::env::consts::ARCH)
    );
    let paths = DataPaths::from_env()?;
    let mut denied = vec![paths.config_dir, paths.data_dir];
    denied.extend([".ssh", ".gnupg", ".aws"].map(|name| paths.home.join(name)));
    println!("| Entries | Median seconds (5 runs, sandboxed) |\n|---:|---:|");
    for entries in [10, 100, 300, 500, 1000] {
        let project = root.0.join(format!("project-{entries}"));
        fs::create_dir(&project)?;
        for n in 0..entries {
            fs::write(project.join(format!("entry{n}.bnt")), text::SCRIPT)?;
        }
        let locations = WritableLocations {
            project: project.clone(),
            temporary: root.0.join("temporary"),
            additional: vec![],
        };
        let policy = sandbox::restricted_policy(
            &RestrictedAccess {
                write_project: false,
                allow_ip: false,
                programs: vec![processor.clone()],
                readable: ReadAccess::All,
            },
            &locations,
            &denied,
        )?;
        let env = child_environment(
            std::env::vars_os(),
            &Default::default(),
            &locations.temporary,
            Some(("restricted", false)),
        );
        let mut times = Vec::new();
        for _ in 0..5 {
            let command = SandboxCommand {
                policy: policy.clone(),
                program: processor.clone(),
                args: vec![
                    "test".into(),
                    "--check".into(),
                    "--diagnostics=json".into(),
                    project.as_os_str().into(),
                ],
                cwd: project.clone(),
                env: env.clone(),
                stdin: SandboxStdin::Null,
            };
            let command = prepare(command, &helper, &root.0)?;
            let stdout = Arc::new(Mutex::new(Vec::new()));
            let stderr = Arc::new(Mutex::new(Vec::new()));
            let started = Instant::now();
            let outcome = run::run(
                command,
                vec![],
                stdout,
                Arc::clone(&stderr),
                RunLimits {
                    timeout: Duration::from_secs(30),
                    stop_signal: StopSignal::Interrupt,
                    ..RunLimits::default()
                },
                &AtomicBool::new(false),
            )?;
            let elapsed = started.elapsed().as_secs_f64();
            if outcome.end != RunEnd::Exited || !outcome.status.success() {
                eprintln!(
                    "{}",
                    String::from_utf8_lossy(&stderr.lock().map_err(|_| text::FAILED)?)
                );
                return Err(text::FAILED.into());
            }
            times.push(elapsed);
        }
        times.sort_by(f64::total_cmp);
        println!("| {entries} | {:.6} |", times.get(2).ok_or(text::FAILED)?);
    }
    Ok(())
}

fn prepare(
    command: SandboxCommand,
    helper: &Path,
    root: &Path,
) -> Result<RunCommand, Box<dyn Error>> {
    #[cfg(target_os = "linux")]
    {
        Ok(command
            .prepare(helper, &root.join("scratch"), &root.join("placeholders"))?
            .into())
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = root;
        Ok(command.to_command(helper)?.into())
    }
}
mod text {
    //! 測定の引数と失敗の英文。
    pub const USAGE: &str = "Usage: bench_write_check <benitoite> <work-directory>; build benitoiteh in the same target directory first.";
    #[cfg(target_os = "macos")]
    pub const CPU_UNAVAILABLE: &str = "CPU model query was denied";
    pub const VERSION: &str = "Could not query the processor version.";
    pub const FAILED: &str = "The sandboxed check failed; no timing was recorded.";
    pub const SCRIPT: &str = "function main() -> Unit\n  return ()\nend function\n";
}
