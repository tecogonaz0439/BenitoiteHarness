//! テストが作って手放したセッションを開く準備の補助（設計書 07-01）。

use benitoiteh::config::paths::DataPaths;
use benitoiteh::session::{Session, SessionError, SessionId};
use std::thread;
use std::time::Duration;

/// 前もって作ったセッションを、Locked の場合だけ最大 100 回やり直して開く。
pub(super) fn open_created_session(
    paths: &DataPaths,
    id: &SessionId,
) -> Result<Session, SessionError> {
    // 並行するテストの fork で継承された錠が exec まで残る場合がある。
    // 本物の使用中の錠や、ほかの誤りを隠さないよう、待つ回数と誤りを限る。
    for _attempt in 0..100 {
        match Session::open(paths, id) {
            Err(SessionError::Locked) => thread::sleep(Duration::from_millis(10)),
            result => return result,
        }
    }
    Session::open(paths, id)
}
