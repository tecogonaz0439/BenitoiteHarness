//! シグナルの通知をループの取り消しの印へつなぐ（設計書 05-01「終了状態」）。

use std::io::{self, Read};
use std::os::unix::net::UnixStream;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use signal_hook::SigId;
use signal_hook::consts::signal::{SIGHUP, SIGINT, SIGTERM};

pub(super) struct Signals {
    pub cancelled: Arc<AtomicBool>,
    registrations: Vec<SigId>,
    received: Option<UnixStream>,
    received_count: usize,
}

impl Signals {
    /// 両方のシグナルを同じ印へ登録し、一部の登録の失敗でも後始末する。
    pub fn new() -> io::Result<Self> {
        Self::register(false)
    }

    /// raw のモードでは外から来た INT と HUP も終了の印にする。
    pub fn interactive() -> io::Result<Self> {
        Self::register(true)
    }

    fn register(interactive: bool) -> io::Result<Self> {
        let pipe = if interactive {
            let (read, write) = UnixStream::pair()?;
            read.set_nonblocking(true)?;
            Some((read, write))
        } else {
            None
        };
        let mut signals = Self {
            cancelled: Arc::new(AtomicBool::new(false)),
            registrations: Vec::new(),
            received: None,
            received_count: 0,
        };
        let mut numbers = vec![SIGINT, SIGTERM];
        if interactive {
            numbers.push(SIGHUP);
        }
        for signal in numbers {
            signals.registrations.push(signal_hook::flag::register(
                signal,
                Arc::clone(&signals.cancelled),
            )?);
            // 同じ真偽値への二度目の通知を失わないよう、通知ごとに一バイトも残す。
            // flag を先に立てる順序は signal-hook 0.4.5 の self-pipe の規則に従う。
            if let Some((_, write)) = &pipe {
                signals
                    .registrations
                    .push(signal_hook::low_level::pipe::register(
                        signal,
                        write.try_clone()?,
                    )?);
            }
        }
        signals.received = pipe.map(|(read, _)| read);
        Ok(signals)
    }

    /// ハンドラが残した通知を読み、同じシグナルの二度目も数える。
    pub fn termination_count(&mut self) -> io::Result<usize> {
        if let Some(read) = &mut self.received {
            let mut bytes = [0; 64];
            loop {
                match read.read(&mut bytes) {
                    Ok(0) => break,
                    Ok(count) => {
                        self.received_count = self.received_count.saturating_add(count);
                        if self.received_count >= 2 {
                            break;
                        }
                    }
                    Err(error) if error.kind() == io::ErrorKind::WouldBlock => break,
                    Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                    Err(error) => return Err(error),
                }
            }
        }
        Ok(self.received_count)
    }
}

impl Drop for Signals {
    fn drop(&mut self) {
        for registration in self.registrations.drain(..) {
            signal_hook::low_level::unregister(registration);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // 作成時の関門: 同じシグナルを真偽値だけで扱って二度目を失う退行を、本物の通知で検出する。
    // 他の並列テストへ SIGTERM を送らないよう、専用の子プロセスで登録して raise する。
    #[test]
    fn repeated_termination_signals_are_counted_without_a_terminal()
    -> Result<(), Box<dyn std::error::Error>> {
        const CHILD: &str = "BENITOITEH_SIGNAL_COUNT_CHILD";
        if std::env::var_os(CHILD).is_some() {
            let mut signals = Signals::interactive()?;
            signal_hook::low_level::raise(SIGTERM)?;
            signal_hook::low_level::raise(SIGTERM)?;
            assert!(signals.cancelled.load(std::sync::atomic::Ordering::Acquire));
            assert_eq!(signals.termination_count()?, 2);
            assert_eq!(signals.termination_count()?, 2);
            signal_hook::low_level::raise(SIGHUP)?;
            assert_eq!(signals.termination_count()?, 3);
            return Ok(());
        }
        let directory = std::env::temp_dir().join(format!(
            "benitoiteh-signals-{}",
            crate::session::SessionId::generate(std::time::SystemTime::now())?
        ));
        std::fs::create_dir(&directory)?;
        let output = std::process::Command::new(std::env::current_exe()?)
            .args([
                "--exact",
                "cli::signals::tests::repeated_termination_signals_are_counted_without_a_terminal",
                "--nocapture",
            ])
            .env(CHILD, "1")
            .current_dir(&directory)
            .output();
        std::fs::remove_dir_all(directory)?;
        let output = output?;
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        Ok(())
    }
}
