//! 記録の木を、再帰せず表示用の行へ変える（設計書 01-05「セッションの木」）。

use std::collections::BTreeMap;

use super::{Record, SessionTree, replay::request_text_ref};

/// 根、依頼、先端、今の位置の表示行。印は同じ行に併記できる。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TreeRow {
    pub node: Option<u64>,
    pub depth: usize,
    /// 依頼の先頭 60 文字と、続きがある場合の省略印。全文は保持しない。
    pub request: Option<String>,
    /// 依頼の全文の行数。依頼以外の行では 0。
    pub request_lines: usize,
    pub is_tip: bool,
    pub is_current: bool,
}

/// 兄弟を番号順に前順で辿り、分岐する祖先だけを字下げの深さに数える。
pub fn tree_rows(records: &[Record], tree: &SessionTree) -> Vec<TreeRow> {
    let mut children: BTreeMap<Option<u64>, Vec<u64>> = BTreeMap::new();
    for (node, parent, _) in tree.nodes() {
        children.entry(parent).or_default().push(node);
    }
    if children.is_empty() {
        return Vec::new();
    }
    let mut rows = Vec::new();
    let mut stack = vec![(None, 0usize)];
    while let Some((node, depth)) = stack.pop() {
        let descendants = children.get(&node).map(Vec::as_slice).unwrap_or_default();
        let request = node
            .and_then(|node| tree.entry(node))
            .and_then(|(_, index)| records.get(index))
            .and_then(request_text_ref);
        let is_tip = node.is_some() && descendants.is_empty();
        let is_current = tree.tip == node;
        if node.is_none() || request.is_some() || is_tip || is_current {
            rows.push(TreeRow {
                node,
                depth,
                request: request.map(request_heading),
                request_lines: request.map_or(0, |request| request.lines().count().max(1)),
                is_tip,
                is_current,
            });
        }
        let child_depth = depth.saturating_add(usize::from(descendants.len() >= 2));
        stack.extend(
            descendants
                .iter()
                .rev()
                .map(|node| (Some(*node), child_depth)),
        );
    }
    rows
}

/// 一覧と確認で同じ範囲の見出しを使い、全文の可視化を繰り返さない。
pub(crate) fn request_heading(request: &str) -> String {
    let mut chars = request.chars();
    let mut heading: String = chars.by_ref().take(60).collect();
    if chars.next().is_some() {
        heading.push_str(super::text::REQUEST_ELLIPSIS);
    }
    heading
}
