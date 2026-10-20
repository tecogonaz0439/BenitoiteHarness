//! openat2 の `RESOLVE_BENEATH` が返す `EAGAIN` の開き直し（設計書 01-03「道具の定義の共通の規則」）。

use rustix::io::Errno;

/// `EAGAIN` のときに同じ引数で開き直す最大の回数。
pub(super) const EAGAIN_RETRIES: u32 = 8;

/// すでに `retried` 回開き直した後の誤り `error` で、もう一度開き直すかを決める。
pub(super) fn should_retry(error: Errno, retried: u32) -> bool {
    error == Errno::AGAIN && retried < EAGAIN_RETRIES
}

/// `call` を呼び、`EAGAIN` なら 8 回まで呼び直す。それでも `EAGAIN` ならその誤りを返す。
/// 範囲の判定は毎回カーネルが行うので、開き直しで範囲の外へ出る道は増えない。
pub(super) fn retry_eagain<T>(mut call: impl FnMut() -> Result<T, Errno>) -> Result<T, Errno> {
    let mut retried = 0;
    loop {
        match call() {
            Err(error) if should_retry(error, retried) => retried = retried.saturating_add(1),
            other => return other,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eagain_is_retried_and_a_later_success_is_returned() {
        let mut calls = 0;
        let result = retry_eagain(|| {
            calls += 1;
            if calls <= 8 {
                Err(Errno::AGAIN)
            } else {
                Ok(calls)
            }
        });
        assert_eq!(result, Ok(9));
        assert_eq!(calls, 9);
    }

    #[test]
    fn eagain_after_eight_retries_is_returned_as_the_error() {
        let mut calls = 0;
        let result: Result<(), Errno> = retry_eagain(|| {
            calls += 1;
            Err(Errno::AGAIN)
        });
        assert_eq!(result, Err(Errno::AGAIN));
        assert_eq!(calls, 1 + EAGAIN_RETRIES);
    }

    #[test]
    fn other_errors_are_returned_without_retrying() {
        for error in [Errno::XDEV, Errno::LOOP, Errno::NOENT, Errno::INTR] {
            let mut calls = 0;
            let result: Result<(), Errno> = retry_eagain(|| {
                calls += 1;
                Err(error)
            });
            assert_eq!(result, Err(error));
            assert_eq!(calls, 1);
        }
    }

    #[test]
    fn retry_decision_stops_at_the_limit() {
        assert!(should_retry(Errno::AGAIN, 0));
        assert!(should_retry(Errno::AGAIN, EAGAIN_RETRIES - 1));
        assert!(!should_retry(Errno::AGAIN, EAGAIN_RETRIES));
        assert!(!should_retry(Errno::XDEV, 0));
    }
}
