//! 圧縮から独立して、成功したファイル操作を記録から復元する（設計書 01-04「圧縮」）。

use std::collections::{BTreeMap, BTreeSet};

use crate::provider::ToolCall;

use super::Record;

/// セッション全体で読んだファイルと変えたファイル。圧縮でも空にしない。
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct FileHistory {
    pub read: BTreeSet<String>,
    pub modified: BTreeSet<String>,
}

impl FileHistory {
    /// 成功した呼び出しの引数だけを覚える。bash の効果は推測しない。
    pub fn add_call(&mut self, call: &ToolCall) {
        let Some(path) = call
            .arguments
            .get("path")
            .and_then(serde_json::Value::as_str)
        else {
            return;
        };
        match call.name.as_str() {
            "read" => {
                self.read.insert(path.into());
            }
            "write" | "edit" => {
                self.modified.insert(path.into());
            }
            _ => {}
        }
    }

    /// ID で呼び出しと結果を結び、圧縮前の操作も含めて復元する。
    pub fn from_records(records: &[Record]) -> Self {
        let mut calls = BTreeMap::new();
        let mut files = Self::default();
        for record in records {
            if let Record::ToolCall { call, .. } = record {
                calls.insert(&call.id, call);
            }
            if let Record::ToolResult { result, .. } = record
                && let Some(call) = calls.remove(&result.id)
                && !result.is_error
            {
                files.add_call(call);
            }
        }
        files
    }
}
