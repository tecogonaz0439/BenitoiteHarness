//! 提供者の分類に従うやり直しと取り消し可能な待機（設計書 01-01「やり直し」）。

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::SyncSender;
use std::thread;
use std::time::{Duration, Instant};

use crate::provider::{AssistantTurn, ProviderError};

use super::{Agent, AgentError, AgentEvent, TICK, transport::Request};

impl Agent {
    pub(super) fn receive_with_retry(
        &mut self,
        request: &Request,
        cancelled: &AtomicBool,
        events: &SyncSender<AgentEvent>,
    ) -> Result<Option<AssistantTurn>, AgentError> {
        let mut attempt = 0_u64;
        loop {
            let error = match self.receive(request, cancelled, events) {
                Ok(response) => return Ok(response),
                Err(AgentError::Provider(error)) => error,
                Err(error) => return Err(error),
            };
            if cancelled.load(Ordering::Acquire) {
                return Ok(None);
            }
            let max_retries = match error {
                ProviderError::ContextOverflow | ProviderError::NonRetryable { .. } => {
                    return Err(error.into());
                }
                // 形の違いで課金済みの応答を繰り返し捨てない（設計書 01-01「やり直し」）。
                ProviderError::InvalidResponse => 1,
                ProviderError::Retryable { .. }
                | ProviderError::StreamDisconnected
                | ProviderError::EmptyResponse => 4,
            };
            if attempt >= max_retries {
                return Err(error.into());
            }
            let delay = self.retry_delay(attempt, &error)?;
            attempt = attempt.saturating_add(1);
            self.emit(
                events,
                AgentEvent::Retry {
                    attempt,
                    delay_ms: u64::try_from(delay.as_millis()).unwrap_or(u64::MAX),
                    reason: error,
                },
            )?;
            let started = Instant::now();
            while started.elapsed() < delay {
                if cancelled.load(Ordering::Acquire) {
                    return Ok(None);
                }
                thread::sleep(delay.saturating_sub(started.elapsed()).min(TICK));
            }
        }
    }

    fn retry_delay(&self, attempt: u64, error: &ProviderError) -> Result<Duration, AgentError> {
        if let ProviderError::Retryable {
            retry_after_secs: Some(seconds),
        } = error
        {
            return Ok(Duration::from_secs((*seconds).min(60)));
        }
        let mut base = self.config.retry_initial.min(Duration::from_secs(30));
        for _ in 0..attempt {
            base = base.saturating_mul(2).min(Duration::from_secs(30));
        }
        let mut random = [0; 2];
        getrandom::fill(&mut random).map_err(|_| AgentError::Random)?;
        // 800〜1200 の千分率にし、浮動小数や収まらない数値の変換を避ける。
        let permille = u32::from(u16::from_ne_bytes(random) % 401).saturating_add(800);
        Ok(base
            .saturating_mul(permille)
            .checked_div(1000)
            .unwrap_or(Duration::ZERO))
    }
}
