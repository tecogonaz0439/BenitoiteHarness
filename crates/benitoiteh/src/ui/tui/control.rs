//! 信頼した端末の命令だけを組み立てる（設計書 05-02「画面の構成」「出力の示し方」）。

use std::io::{self, Write};

use crossterm::{cursor, event, queue, style, terminal};

/// 制御文字を可視化し、タブを空白にした文字列。生の文字列からの抜け道を設けない。
#[derive(Clone, Debug, Default)]
pub(super) struct SafeText(String);

impl SafeText {
    pub fn new(value: &str, multiline: bool) -> Self {
        Self(super::super::terminal_text(value, multiline).replace('\t', "    "))
    }

    pub fn strict(value: &str, multiline: bool) -> Self {
        Self(
            super::unicode::visible(&super::super::terminal_text(value, multiline))
                .replace('\t', "    "),
        )
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(super) enum Decoration {
    #[default]
    Plain,
    Bold,
    Dim,
    Code,
    Added,
    Removed,
    Error,
}

pub(super) type Line = Vec<(SafeText, Decoration)>;

pub(super) fn plain(value: &str, decoration: Decoration) -> Line {
    vec![(SafeText::new(value, false), decoration)]
}

/// 書き込みを一回にまとめるためのフレーム。制御の列はここだけで作る。
#[derive(Default)]
pub(super) struct Frame(Vec<u8>);

impl Frame {
    pub fn begin(&mut self) -> io::Result<()> {
        queue!(self.0, terminal::BeginSynchronizedUpdate, cursor::Hide)
    }

    pub fn clear(&mut self, rows_above: u16) -> io::Result<()> {
        self.up(rows_above)?;
        self.column(0)?;
        queue!(self.0, terminal::Clear(terminal::ClearType::FromCursorDown))
    }

    pub fn wrap(&mut self, enabled: bool) -> io::Result<()> {
        if enabled {
            queue!(self.0, terminal::EnableLineWrap)
        } else {
            queue!(self.0, terminal::DisableLineWrap)
        }
    }

    pub fn column(&mut self, column: u16) -> io::Result<()> {
        queue!(self.0, cursor::MoveToColumn(column))
    }

    pub fn up(&mut self, rows: u16) -> io::Result<()> {
        if rows > 0 {
            queue!(self.0, cursor::MoveUp(rows))?;
        }
        Ok(())
    }

    pub fn newline(&mut self) {
        self.0.extend_from_slice(b"\r\n");
    }

    pub fn line(&mut self, line: &Line) -> io::Result<()> {
        for (text, decoration) in line {
            queue!(self.0, style::SetAttribute(style::Attribute::Reset))?;
            match decoration {
                Decoration::Plain => {}
                Decoration::Bold => queue!(self.0, style::SetAttribute(style::Attribute::Bold))?,
                Decoration::Dim => queue!(self.0, style::SetAttribute(style::Attribute::Dim))?,
                Decoration::Code => queue!(self.0, style::SetForegroundColor(style::Color::Cyan))?,
                Decoration::Added => {
                    queue!(self.0, style::SetForegroundColor(style::Color::Green))?
                }
                Decoration::Removed | Decoration::Error => {
                    queue!(self.0, style::SetForegroundColor(style::Color::Red))?;
                }
            }
            self.0.extend_from_slice(text.as_str().as_bytes());
        }
        queue!(self.0, style::SetAttribute(style::Attribute::Reset))
    }

    pub fn finish(&mut self, writer: &mut impl Write) -> io::Result<()> {
        queue!(
            self.0,
            terminal::EnableLineWrap,
            cursor::Show,
            terminal::EndSynchronizedUpdate
        )?;
        // File の write 一回でフレーム全体を渡す。部分書き込みは継ぎ足さず誤りにする。
        // 復元済みの書き先は全体を捨て、panic の復元後にモードを変えない（実装プラン P4-06）。
        if writer.write(&self.0)? != self.0.len() {
            return Err(io::ErrorKind::WriteZero.into());
        }
        writer.flush()
    }
}

/// 問い合わせをせずに端末の入力の拡張を有効にする。
pub(super) fn setup(writer: &mut impl Write) -> io::Result<()> {
    let mut bytes = Vec::new();
    queue!(
        bytes,
        event::EnableBracketedPaste,
        event::PushKeyboardEnhancementFlags(
            event::KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES
        )
    )?;
    writer.write_all(&bytes)?;
    writer.flush()
}

/// 正常・異常・シグナル・panic のすべてで同じ復元の列を使う。
pub fn restoration(rows_below_cursor: u16) -> io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    queue!(
        bytes,
        terminal::EndSynchronizedUpdate,
        style::SetAttribute(style::Attribute::Reset),
        cursor::Show,
        terminal::EnableLineWrap,
        event::DisableBracketedPaste,
        event::PopKeyboardEnhancementFlags
    )?;
    for _ in 0..=rows_below_cursor {
        bytes.extend_from_slice(b"\r\n");
    }
    Ok(bytes)
}
