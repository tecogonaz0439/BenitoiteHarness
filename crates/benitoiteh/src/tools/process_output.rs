//! 子プロセスの出力を排出しつつ末尾と保存を管理する（設計書 01-03「出力の切り詰め」）。
use crate::session::SpillWriter;
use std::io::{self, Write};
use std::sync::{Arc, Mutex, PoisonError};
pub(crate) struct Output {
    redactor: Option<crate::askpass::Redactor>,
    max_bytes: usize,
    max_lines: usize,
    pub(super) writer: Option<SpillWriter>,
    pub(super) bytes: Vec<u8>,
    total_bytes: usize,
    partial_first: bool,
    newlines: usize,
    retained_newlines: usize,
    ends_newline: bool,
    pub(super) closed: bool,
}
impl Output {
    pub(super) fn new(writer: SpillWriter, max_bytes: usize, max_lines: usize) -> Self {
        Self {
            redactor: None,
            writer: Some(writer),
            max_bytes,
            max_lines,
            bytes: Vec::new(),
            total_bytes: 0,
            partial_first: false,
            newlines: 0,
            retained_newlines: 0,
            ends_newline: false,
            closed: false,
        }
    }
    pub(crate) fn memory(max_bytes: usize, max_lines: usize) -> Self {
        Self {
            redactor: None,
            writer: None,
            max_bytes,
            max_lines,
            bytes: Vec::new(),
            total_bytes: 0,
            partial_first: false,
            newlines: 0,
            retained_newlines: 0,
            ends_newline: false,
            closed: false,
        }
    }
    pub(crate) fn mediated(
        max_bytes: usize,
        max_lines: usize,
        redactor: crate::askpass::Redactor,
    ) -> Self {
        let mut output = Self::memory(max_bytes, max_lines);
        output.redactor = Some(redactor);
        output
    }
    pub(crate) fn finish_redaction(&mut self) -> io::Result<()> {
        if let Some(mut redactor) = self.redactor.take() {
            let bytes = redactor.push(&[], true);
            self.write_plain(&bytes)?;
        }
        Ok(())
    }
    // テスト結果の保持の予算には、捨てた出力でなく確保した末尾の領域を数える。
    pub(crate) fn retained_bytes(&self) -> usize {
        self.bytes.capacity()
    }
    pub(crate) fn tail(&self) -> (String, usize, usize, bool) {
        let total = self
            .newlines
            .saturating_add(usize::from(self.total_bytes != 0 && !self.ends_newline));
        let mut bytes = self.bytes.as_slice();
        let mut truncated = self.total_bytes > bytes.len();
        let mut cut_in_line = self.partial_first;
        // バイト上限で最初の行が欠けたときは、最後の一行だけの場合を除き返さない。
        // （設計書 01-03「出力の切り詰め」）。
        if self.partial_first
            && let Some(index) = bytes.iter().position(|b| *b == b'\n')
            && index.saturating_add(1) < bytes.len()
        {
            bytes = bytes.get(index.saturating_add(1)..).unwrap_or_default();
            cut_in_line = false;
        }
        let mut lines = bytes
            .iter()
            .filter(|b| **b == b'\n')
            .count()
            .saturating_add(usize::from(!bytes.is_empty() && !bytes.ends_with(b"\n")));
        while lines > self.max_lines {
            let Some(index) = bytes.iter().position(|b| *b == b'\n') else {
                break;
            };
            bytes = bytes.get(index.saturating_add(1)..).unwrap_or_default();
            lines = lines.saturating_sub(1);
            truncated = true;
            cut_in_line = false;
        }
        if cut_in_line {
            while bytes.first().is_some_and(|b| b & 0xc0 == 0x80) {
                bytes = bytes.get(1..).unwrap_or_default();
            }
        }
        let mut contents = String::from_utf8_lossy(bytes).into_owned();
        if contents.len() > self.max_bytes {
            let mut start = contents.len().saturating_sub(self.max_bytes);
            while !contents.is_char_boundary(start) {
                start = start.saturating_add(1);
            }
            if let Some(index) = contents.get(start..).and_then(|s| s.find('\n')) {
                let next = start.saturating_add(index).saturating_add(1);
                if next < contents.len() {
                    start = next;
                }
            }
            contents = contents.get(start..).unwrap_or_default().to_owned();
            lines = contents.lines().count();
            truncated = true;
        }
        (
            contents,
            total.saturating_sub(lines).saturating_add(1),
            total,
            truncated,
        )
    }
}
impl Write for Output {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if self.closed {
            return Ok(bytes.len());
        }
        if self.redactor.is_some() {
            for chunk in bytes.chunks(8192) {
                let filtered = self
                    .redactor
                    .as_mut()
                    .map(|redactor| redactor.push(chunk, false))
                    .unwrap_or_default();
                self.write_plain(&filtered)?;
            }
            Ok(bytes.len())
        } else {
            self.write_plain(bytes)
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

// 準備や巻き戻しの記録の段で止まった場合も、空の spill を残さない。
impl Drop for Output {
    fn drop(&mut self) {
        if let Some(writer) = self.writer.take() {
            let _removed = writer.discard();
        }
    }
}

impl Output {
    fn write_plain(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if let Some(writer) = &mut self.writer
            && writer.write_all(bytes).is_err()
            && let Some(writer) = self.writer.take()
        {
            let _removed = writer.discard();
        }
        self.total_bytes = self.total_bytes.saturating_add(bytes.len());
        let added_newlines = bytes.iter().filter(|b| **b == b'\n').count();
        self.newlines = self.newlines.saturating_add(added_newlines);
        self.retained_newlines = self.retained_newlines.saturating_add(added_newlines);
        if let Some(last) = bytes.last() {
            self.ends_newline = *last == b'\n';
        }
        self.bytes.extend_from_slice(bytes);
        let excess = self.bytes.len().saturating_sub(self.max_bytes);
        if excess != 0 {
            self.partial_first = self.bytes.get(excess.saturating_sub(1)) != Some(&b'\n');
            let removed = self
                .bytes
                .get(..excess)
                .unwrap_or_default()
                .iter()
                .filter(|b| **b == b'\n')
                .count();
            self.retained_newlines = self.retained_newlines.saturating_sub(removed);
            self.bytes.drain(..excess);
        }
        let mut lines = self
            .retained_newlines
            .saturating_add(usize::from(!self.ends_newline));
        let mut start = 0;
        while lines > self.max_lines {
            let Some(index) = self
                .bytes
                .get(start..)
                .and_then(|rest| rest.iter().position(|b| *b == b'\n'))
            else {
                break;
            };
            start = start.saturating_add(index).saturating_add(1);
            lines = lines.saturating_sub(1);
            self.retained_newlines = self.retained_newlines.saturating_sub(1);
        }
        if start != 0 {
            self.bytes.drain(..start);
            self.partial_first = false;
        }
        Ok(bytes.len())
    }
}

/// それぞれのパイプで一致を確定してから共通の出力へ渡す。
/// 別の流れの書き込みが秘密の途中へ割り込んでも、一致を失わない。
pub(crate) struct StreamOutput {
    output: Arc<Mutex<Output>>,
    redactor: Option<crate::askpass::Redactor>,
    closed: bool,
}
impl StreamOutput {
    pub(crate) fn new(
        output: Arc<Mutex<Output>>,
        redactor: Option<crate::askpass::Redactor>,
    ) -> Self {
        Self {
            output,
            redactor,
            closed: false,
        }
    }
    pub(crate) fn finish(&mut self) -> io::Result<()> {
        self.closed = true;
        if let Some(mut redactor) = self.redactor.take() {
            self.output
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .write_all(&redactor.push(&[], true))?;
        }
        Ok(())
    }
}
impl Write for StreamOutput {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if !self.closed {
            for chunk in bytes.chunks(8192) {
                let mut output = self.output.lock().unwrap_or_else(PoisonError::into_inner);
                if let Some(redactor) = self.redactor.as_mut() {
                    output.write_all(&redactor.push(chunk, false))?;
                } else {
                    output.write_all(chunk)?;
                }
            }
        }
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
