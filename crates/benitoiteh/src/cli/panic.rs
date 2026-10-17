//! panic の情報から秘密を除き、abort より先に終了する（設計書 06-01「ビルドの設定」）。

use std::fs::File;
use std::io::{self, Write};
use std::sync::Arc;

use super::text;

pub(super) fn install(
    record: Option<File>,
    terminal: Option<Arc<crate::ui::tui::TerminalRestore>>,
) {
    std::panic::set_hook(Box::new(move |_| {
        // abort では Drop が走らず、描画中は stdout の錠を持つ場合がある。
        if let Some(terminal) = &terminal {
            terminal.panic_message(text::INTERNAL_ERROR);
        }
        // 記録中の panic でも同じ Mutex を取って停止しないよう、専用の追記記述子を使う。
        // payload と位置には秘密が入り得るため、一定の一行だけを残す。
        if let Some(file) = &record {
            let mut file = file;
            let _written = file.write_all(text::PANIC_RECORD);
            let _flushed = file.flush();
        }
        if terminal.is_none() {
            let _shown = writeln!(io::stderr().lock(), "{}", text::INTERNAL_ERROR);
        }
        std::process::exit(3);
    }));
}
