//! 子孫の終了の観測と、テスト用ディレクトリの後始末（実装プラン P2-01）。

use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub type TestResult = Result<(), Box<dyn std::error::Error>>;
static NEXT: AtomicU64 = AtomicU64::new(0);

pub struct ProcessFixture {
    pub root: PathBuf,
    pub probe: PathBuf,
    pub marker: PathBuf,
}
impl ProcessFixture {
    pub fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let stamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
        let sequence = NEXT.fetch_add(1, Ordering::Relaxed);
        let root = fs::canonicalize(std::env::temp_dir())?.join(format!(
            "bh-run-{}-{stamp:x}-{sequence:x}",
            std::process::id()
        ));
        fs::create_dir(&root)?;
        let probe = std::env::current_exe()?
            .parent()
            .and_then(std::path::Path::parent)
            .ok_or("Missing target directory")?
            .join("sandbox-probe");
        if !probe.is_file() {
            return Err("Build sandbox-probe before testing".into());
        }
        Ok(Self {
            marker: root.join("unique-descendant"),
            root,
            probe,
        })
    }
    pub fn pid(&self) -> Result<rustix::process::Pid, Box<dyn std::error::Error>> {
        let raw = fs::read_to_string(&self.marker)?.parse::<i32>()?;
        rustix::process::Pid::from_raw(raw).ok_or_else(|| "Invalid child PID".into())
    }
    pub fn wait_until(&self, predicate: impl Fn() -> bool) -> TestResult {
        let start = Instant::now();
        while !predicate() {
            if start.elapsed() > Duration::from_secs(5) {
                return Err("Timed out waiting for process readiness or exit".into());
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        Ok(())
    }
    pub fn stopped(&self, pid: rustix::process::Pid) -> bool {
        #[cfg(target_os = "linux")]
        {
            let Ok(stat) = fs::read_to_string(format!("/proc/{}/stat", pid.as_raw_nonzero()))
            else {
                return true;
            };
            stat.rsplit_once(") ")
                .is_some_and(|(_, tail)| tail.starts_with('Z'))
        }
        #[cfg(not(target_os = "linux"))]
        {
            rustix::process::test_kill_process(pid) == Err(rustix::io::Errno::SRCH)
        }
    }
}

impl Drop for ProcessFixture {
    fn drop(&mut self) {
        // 探査の子は 30 秒で自分で終わる。外から番号を使って止めないため、
        // ps による照合と、番号の使い回しの危険を避けられる（実装プラン P2-01）。
        let _removed = fs::remove_dir_all(&self.root);
    }
}
