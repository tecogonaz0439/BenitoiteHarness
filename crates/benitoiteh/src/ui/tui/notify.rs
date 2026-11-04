//! 端末が前面にないときに、確認を待つことと一連の処理の終わりを知らせる（設計書 05-02「通知」）。

use std::io::{self, Write};
use std::process::{Command, Stdio};

use crate::config::NotificationsConfig;

/// 知らせること。文は決まったものだけとし、LLM の文やファイルの名前を含めない。
/// 理由: 端末の制御の列とコマンドの引数に、信頼しない文字列を入れない。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Notification {
    Approval,
    Finished,
}

impl Notification {
    pub fn message(self) -> &'static str {
        match self {
            Self::Approval => super::text::NOTIFY_APPROVAL,
            Self::Finished => super::text::NOTIFY_FINISHED,
        }
    }
}

/// 利用者の設定で選んだ知らせ方。
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Notifier {
    terminal: bool,
    command: Vec<String>,
}

impl Notifier {
    pub fn new(config: &NotificationsConfig) -> Self {
        Self {
            terminal: config.terminal,
            command: config.command.clone(),
        }
    }

    /// 端末に知らせの列を書き、コマンドを待たずに起動する。コマンドの失敗は画面を止めない。
    pub(super) fn send(
        &self,
        output: &mut impl Write,
        notification: Notification,
    ) -> io::Result<()> {
        if self.terminal {
            // OSC 9 は知らせを出す端末の拡張である。知らない端末は列ごと読み捨てる。
            // 続く BEL は、多くの端末で窓やタブに印を付ける。
            write!(output, "\x1b]9;{}\x07\x07", notification.message())?;
            output.flush()?;
        }
        if let Some((program, arguments)) = self.command.split_first() {
            let spawned = Command::new(program)
                .args(arguments)
                .arg(notification.message())
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn();
            if let Ok(mut child) = spawned {
                // 終わりを待つ糸を分け、ゾンビを残さない。
                let _waiter = std::thread::Builder::new()
                    .name("notify".into())
                    .spawn(move || {
                        let _status = child.wait();
                    });
            }
        }
        Ok(())
    }
}
