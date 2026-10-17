//! 行ごとの追記と、上限を分けた spill の書き込み（設計書 01-05「セッションの記録」、01-03「出力の切り詰め」）。

use std::fs::File;
use std::io::{self, Write};
use std::path::PathBuf;

use rustix::fs::OFlags;
use sha2::{Digest, Sha256};

use super::{
    FORMAT_VERSION, MAX_BYTES, Record, Session, SessionError, SpillInfo, create_file, id, text,
};

/// 追記の途中で失敗した後には追記を止め、壊れた行の後へ記録を連ねない。
pub struct SessionWriter {
    session: Session,
}

// 書き手が複数のスレッドへ渡っても、行とヘッダと失敗の状態を一緒に保つ。
// （設計書 01-05「セッションの記録」）。
#[derive(Debug)]
pub(super) struct LogState {
    file: File,
    has_header: bool,
    failed: bool,
}

impl LogState {
    pub(super) fn new(file: File) -> Self {
        Self {
            file,
            has_header: false,
            failed: false,
        }
    }
}

impl SessionWriter {
    pub(super) fn open(session: Session) -> Self {
        Self { session }
    }

    /// JSON と改行を追記して flush する。大きい記録は欠落させずに分割して保存する。
    pub fn append(&mut self, record: &Record) -> Result<(), SessionError> {
        let mut log = self
            .session
            .inner
            .log
            .lock()
            .map_err(|_| SessionError::Invalid(text::WRITER_FAILED))?;
        if log.failed {
            return Err(SessionError::Invalid(text::WRITER_FAILED));
        }
        if let Record::Header(header) = record {
            if log.has_header
                || header.format != FORMAT_VERSION
                || header.session_id != self.session.id().as_str()
            {
                return Err(SessionError::Invalid(text::INVALID_FORMAT));
            }
        } else if !log.has_header {
            return Err(SessionError::Invalid(text::INVALID_FORMAT));
        }
        if matches!(record, Record::Spilled { .. }) {
            return Err(SessionError::Invalid(text::INVALID_SPILL));
        }
        let mut bytes = serde_json::to_vec(record)?;
        if bytes.len() >= MAX_BYTES {
            let mut random = [0; 8];
            getrandom::fill(&mut random).map_err(SessionError::Random)?;
            let prefix = id::hex(&random);
            let mut parts = Vec::new();
            // 出力の切り詰めとは区別し、署名や元の項目は欠かさず復元する。
            // 各ファイル自体は一つの出力と同じ上限以下にする（設計書 01-05「再開」）。
            for (index, chunk) in bytes.chunks(MAX_BYTES).enumerate() {
                let name = format!("record-{prefix}-{index}.json");
                let mut file = create_file(&self.session.inner.spill, &name, OFlags::WRONLY)?;
                file.write_all(chunk)?;
                file.flush()?;
                parts.push(PathBuf::from("spill").join(name));
            }
            bytes = serde_json::to_vec(&Record::Spilled { parts })?;
        }
        if bytes.len() >= MAX_BYTES {
            return Err(SessionError::Invalid(text::RECORD_TOO_LARGE));
        }
        bytes.push(b'\n');
        log.failed = true;
        log.file.write_all(&bytes)?;
        log.file.flush()?;
        log.failed = false;
        log.has_header = true;
        Ok(())
    }
}

/// 子プロセスの出力を読み続けながら、保存量だけを制限する書き手。
pub struct SpillWriter {
    _session: Session,
    file: File,
    info: SpillInfo,
}

impl SpillWriter {
    /// 書き手を閉じ、不要になった出力を保持したディレクトリから消す。
    pub fn discard(self) -> Result<(), SessionError> {
        let name = self
            .info
            .path
            .file_name()
            .ok_or(SessionError::Invalid(text::INVALID_SPILL))?;
        drop(self.file);
        rustix::fs::unlinkat(
            &self._session.inner.spill,
            name,
            rustix::fs::AtFlags::empty(),
        )?;
        Ok(())
    }

    pub(super) fn open(session: Session, tool_id: &str) -> Result<Self, SessionError> {
        // 未検証の ID をパスの要素にせず、同じ ID の出力を上書きしない。
        // （設計書 01-03「道具の定義の共通の規則」）。
        let name = format!(
            "tool-{}.output",
            id::hex(&Sha256::digest(tool_id.as_bytes()))
        );
        let file = create_file(&session.inner.spill, &name, OFlags::WRONLY)?;
        Ok(Self {
            _session: session,
            file,
            info: SpillInfo {
                path: PathBuf::from("spill").join(name),
                written_bytes: 0,
                discarded_bytes: 0,
            },
        })
    }

    /// 超過があったときだけ、保存量と捨てた量を末尾に付けて書き終える。
    pub fn finish(mut self) -> Result<SpillInfo, SessionError> {
        let result = self.finish_output();
        if let Err(error) = result {
            let _removed = self.discard();
            return Err(error);
        }
        Ok(self.info)
    }

    fn finish_output(&mut self) -> Result<(), SessionError> {
        if self.info.discarded_bytes != 0 {
            self.file.write_all(
                text::spill_footer(self.info.written_bytes, self.info.discarded_bytes).as_bytes(),
            )?;
        }
        self.file.flush()?;
        Ok(())
    }
}

impl Write for SpillWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let written = usize::try_from(self.info.written_bytes)
            .map_err(|_| io::Error::other(text::COUNTER_OVERFLOW))?;
        let keep = MAX_BYTES.saturating_sub(written).min(bytes.len());
        let stored = bytes
            .get(..keep)
            .ok_or_else(|| io::Error::other(text::COUNTER_OVERFLOW))?;
        let saved = self.file.write(stored)?;
        let saved_count =
            u64::try_from(saved).map_err(|_| io::Error::other(text::COUNTER_OVERFLOW))?;
        self.info.written_bytes = self
            .info
            .written_bytes
            .checked_add(saved_count)
            .ok_or_else(|| io::Error::other(text::COUNTER_OVERFLOW))?;
        if saved != stored.len() {
            return Ok(saved);
        }
        let dropped = u64::try_from(bytes.len().saturating_sub(stored.len()))
            .map_err(|_| io::Error::other(text::COUNTER_OVERFLOW))?;
        self.info.discarded_bytes = self
            .info
            .discarded_bytes
            .checked_add(dropped)
            .ok_or_else(|| io::Error::other(text::COUNTER_OVERFLOW))?;
        // 捨てた分も消費したと伝え、読み手が上限で子プロセスのパイプを止めないようにする。
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        self.file.flush()
    }
}
