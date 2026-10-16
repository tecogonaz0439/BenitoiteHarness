//! 別の送り先へ送る道具の ID を作る（設計書 02-01「共通の形」）。

use sha2::{Digest, Sha256};

/// 元の ID 全体を SHA-256 の小文字の16進数に変える。
///
/// 64 字の英数字であり、Anthropic の ID の形にも合う。削った文字だけが異なる
/// ID も区別するため、接頭辞だけのハッシュや文字の除去を用いない。
/// 呼び出す側は元の ID と戻り値の対応を会話の変換の間保持する。
pub fn remap_tool_call_id(original: &str) -> String {
    Sha256::digest(original.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}
