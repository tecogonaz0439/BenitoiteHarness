//! 圧縮の前に、長い道具の結果を頭と尾だけにする（設計書 01-04「道具の結果の刈り込み」）。
//!
//! LLM を呼ばずに文脈を減らす。DeepSeek Harness の `dsh-compaction-tool-result-pruner`
//! （`ce7073d` の `packages/bundle/base/cordis.patch.yml`）の既定の値に倣う。

use std::sync::mpsc::SyncSender;

use crate::provider::Item;
use crate::session::{PrunedResult, Record};

use super::super::{Agent, AgentError, AgentEvent, text};

/// これより多くの文字の結果を刈り込む。
const THRESHOLD_CHARS: usize = 8192;
/// 残す頭の文字の数。
const HEAD_CHARS: usize = 4096;
/// 残す尾の文字の数。
const TAIL_CHARS: usize = 1024;

/// 文字（Unicode のスカラー値）で数えて長ければ、頭と尾の間を印に替えた文を返す。
pub(super) fn prune(text: &str) -> Option<String> {
    let count = text.chars().count();
    if count <= THRESHOLD_CHARS {
        return None;
    }
    let head_end = text.char_indices().nth(HEAD_CHARS)?.0;
    let tail_start = text.char_indices().nth(count.saturating_sub(TAIL_CHARS))?.0;
    let omitted = count.saturating_sub(HEAD_CHARS).saturating_sub(TAIL_CHARS);
    Some(format!(
        "{}{}{}",
        text.get(..head_end)?,
        text::pruned_tool_result(omitted),
        text.get(tail_start..)?
    ))
}

impl Agent {
    /// 最後の応答より前の道具の結果を刈り込み、記録してから会話に当てる。刈り込んだら真を返す。
    ///
    /// 最後の応答の後の結果は、LLM がまだ読んでいないので残す。
    pub(in crate::agent) fn prune_tool_results(
        &mut self,
        events: &SyncSender<AgentEvent>,
    ) -> Result<bool, AgentError> {
        let Some(last_response) = self
            .conversation
            .items
            .iter()
            .rposition(|item| matches!(item, Item::AssistantTurn(_)))
        else {
            return Ok(false);
        };
        let mut removed = 0_u64;
        let results = self
            .conversation
            .items
            .iter()
            .take(last_response)
            .filter_map(|item| {
                let Item::ToolResult(result) = item else {
                    return None;
                };
                let text = prune(&result.text)?;
                let before = u64::try_from(result.text.chars().count()).unwrap_or(u64::MAX);
                let after = u64::try_from(text.chars().count()).unwrap_or(u64::MAX);
                removed = removed.saturating_add(before.saturating_sub(after));
                Some(PrunedResult {
                    id: result.id.clone(),
                    text,
                })
            })
            .collect::<Vec<_>>();
        if results.is_empty() {
            return Ok(false);
        }
        let estimate_before = self.estimate();
        // 書き込みに失敗したら会話を変えず、記録と再生の起点を揃える。
        self.append_node(Record::ToolResultsPruned {
            node: 0,
            results: results.clone(),
        })?;
        crate::session::apply_pruned(&mut self.conversation, &results);
        // 前の要求の使用量は刈り込む前の会話のものなので、見積もりをバイト数からやり直す。
        self.last_usage = None;
        self.added_bytes = 0;
        self.previous_last_item = None;
        let estimate_after = self.estimate();
        self.emit(
            events,
            AgentEvent::ToolResultsPruned {
                results: results.into_iter().map(|result| result.id).collect(),
                removed_chars: removed,
                estimate_before,
                estimate_after,
            },
        )?;
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // 作成時の関門: 字の数え方（Unicode のスカラー値）と境界。バイトで数える退行では、
    // 日本語の結果を早く刈り込み、文字の途中で切る。ループの境界のテストは ASCII だけを使う。
    #[test]
    fn pruning_counts_characters_and_keeps_head_and_tail() {
        assert_eq!(prune(&"日".repeat(THRESHOLD_CHARS)), None);
        let text = format!(
            "{}{}{}",
            "頭".repeat(HEAD_CHARS),
            "中".repeat(THRESHOLD_CHARS),
            "尾".repeat(TAIL_CHARS)
        );
        let expected = format!(
            "{}{}{}",
            "頭".repeat(HEAD_CHARS),
            text::pruned_tool_result(THRESHOLD_CHARS),
            "尾".repeat(TAIL_CHARS)
        );
        assert_eq!(prune(&text), Some(expected));
    }
}
