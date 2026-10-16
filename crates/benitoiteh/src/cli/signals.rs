//! シグナルの通知をループの取り消しの印へつなぐ（設計書 05-01「終了状態」）。

use std::io;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use signal_hook::SigId;
use signal_hook::consts::signal::{SIGINT, SIGTERM};

pub(super) struct Signals {
    pub cancelled: Arc<AtomicBool>,
    registrations: Vec<SigId>,
}

impl Signals {
    /// 両方のシグナルを同じ印へ登録し、一部の登録の失敗でも後始末する。
    pub fn new() -> io::Result<Self> {
        let mut signals = Self {
            cancelled: Arc::new(AtomicBool::new(false)),
            registrations: Vec::new(),
        };
        for signal in [SIGINT, SIGTERM] {
            signals.registrations.push(signal_hook::flag::register(
                signal,
                Arc::clone(&signals.cancelled),
            )?);
        }
        Ok(signals)
    }
}

impl Drop for Signals {
    fn drop(&mut self) {
        for registration in self.registrations.drain(..) {
            signal_hook::low_level::unregister(registration);
        }
    }
}
