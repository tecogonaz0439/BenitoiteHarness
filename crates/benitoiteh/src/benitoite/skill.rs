//! 処理系の Skill の文書を検証し、セッションの間だけ共有する（設計書 04-01「取り込む文書と版」）。

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fmt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use serde::Deserialize;
use sha2::{Digest, Sha256};

use super::{BenitoiteError, Processor, QueryLimits, QueryOutput, json_object, text};

/// 名前、長さ、SHA-256 を照合した処理系の文書。
#[derive(Debug)]
pub struct SkillDocument {
    text: String,
    content_hash: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Entry {
    name: String,
    byte_length: u64,
    content_hash: ContentHash,
}
#[derive(Deserialize)]
struct ContentHash {
    algorithm: String,
    value: String,
}

fn invalid() -> BenitoiteError {
    BenitoiteError::Invalid(text::INVALID_SKILL)
}

impl SkillDocument {
    /// skill show の標準出力全体を検証する。終了状態は問い合わせの側が検証する。
    pub fn from_json(requested_name: &str, stdout: &[u8]) -> Result<Self, BenitoiteError> {
        let object = json_object(stdout, "skillDocument")?;
        let entry: Entry = serde_json::from_value(serde_json::Value::Object(object.clone()))
            .map_err(|_| invalid())?;
        let body = object
            .get("text")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(invalid)?;
        let hash = Sha256::digest(body.as_bytes())
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        if entry.name != requested_name
            || entry.byte_length != u64::try_from(body.len()).map_err(|_| invalid())?
            || entry.content_hash.algorithm != "sha256"
            || entry.content_hash.value != hash
        {
            return Err(invalid());
        }
        Ok(Self {
            text: body.into(),
            content_hash: hash,
        })
    }

    /// 注釈と属性を含む、処理系が返した原文。
    pub fn text(&self) -> &str {
        &self.text
    }

    /// 原文の UTF-8 のバイト列の SHA-256。
    pub fn content_hash(&self) -> &str {
        &self.content_hash
    }
}

/// LLM に渡す属性と、属性および先頭の注釈を除いた本文。
#[derive(Debug)]
pub struct SkillParts<'a> {
    /// Skill の属性の名前。
    pub name: &'a str,
    /// Skill の属性の説明。
    pub description: &'a str,
    /// 先頭の注釈と前後の空白を除いた本文。
    pub body: &'a str,
}

/// 属性と先頭の注釈を解析し、プロンプトへ入れる本文の境界と量を確かめる。版の置換は行わない。
pub fn skill_parts(skill: &str) -> Result<SkillParts<'_>, BenitoiteError> {
    let (attributes, body) = skill
        .strip_prefix("---\n")
        .and_then(|skill| skill.split_once("\n---\n"))
        .ok_or_else(invalid)?;
    let attribute = |key: &str| {
        attributes
            .lines()
            .find_map(|line| {
                line.strip_prefix(key)
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
            })
            .ok_or_else(invalid)
    };
    let mut start = body;
    while let Some((line, rest)) = start.split_once('\n') {
        if !line.trim().is_empty() {
            break;
        }
        start = rest;
    }
    // 本文の途中の注釈は原文の一部として残す（設計書 01-04「Benitoite の Skill の置き方」）。
    let body = if let Some(comment) = start.strip_prefix("<!--") {
        comment.split_once("-->").ok_or_else(invalid)?.1
    } else {
        body
    };
    let body = body.trim();
    // プロンプトの節を本文で閉じさせず、起動時に受け入れる本文の量を制限する。
    // 本文の書き換えで対処しない（設計書 01-04「Benitoite の Skill の置き方」）。
    if body.len() > 256 * 1024
        || body
            .as_bytes()
            .windows(b"</benitoite>".len())
            .any(|window| window.eq_ignore_ascii_case(b"</benitoite>"))
    {
        return Err(invalid());
    }
    Ok(SkillParts {
        name: attribute("name:")?,
        description: attribute("description:")?,
        body,
    })
}

/// Skill の取得に合う理由を返し、検査の報告に固有の案内を混ぜない。
pub(crate) fn query_reason(error: &BenitoiteError) -> String {
    match error {
        BenitoiteError::OutputLimit => text::SKILL_OUTPUT_LIMIT.into(),
        BenitoiteError::Exit { code, .. } => text::skill_exit(*code),
        error @ (BenitoiteError::Io(_)
        | BenitoiteError::Invalid(_)
        | BenitoiteError::Sandbox(_)
        | BenitoiteError::Run(_)
        | BenitoiteError::TimedOut
        | BenitoiteError::Cancelled) => error.to_string(),
    }
}

fn allowed_name(name: &str) -> bool {
    name == "SKILL.md"
        || name
            .strip_prefix("references/")
            .and_then(|name| name.strip_suffix(".md"))
            .is_some_and(|name| {
                !name.contains('\\') && name.split('/').all(|part| !matches!(part, "" | "." | ".."))
            })
}

fn document_names(stdout: &[u8]) -> Result<Vec<String>, BenitoiteError> {
    let object = json_object(stdout, "skillList")?;
    let documents: Vec<Entry> =
        serde_json::from_value(object.get("documents").cloned().ok_or_else(invalid)?)
            .map_err(|_| invalid())?;
    let mut names = Vec::new();
    for entry in documents {
        if entry.content_hash.algorithm != "sha256"
            || entry.content_hash.value.len() != 64
            || !entry
                .content_hash
                .value
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit())
        {
            return Err(invalid());
        }
        if allowed_name(&entry.name) && !names.contains(&entry.name) {
            names.push(entry.name);
        }
    }
    Ok(names)
}

type Query = dyn Fn(&[OsString], &AtomicBool) -> Result<QueryOutput, BenitoiteError> + Send + Sync;
type DocumentSlot = Arc<Mutex<Option<Arc<SkillDocument>>>>;

/// 一覧と名前ごとの本文を、成功したときだけ一度保持する。
pub struct SkillDocuments {
    query: Box<Query>,
    names: Mutex<Option<Arc<Vec<String>>>>,
    documents: Mutex<BTreeMap<String, DocumentSlot>>,
}
impl fmt::Debug for SkillDocuments {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SkillDocuments").finish_non_exhaustive()
    }
}
impl SkillDocuments {
    /// 問い合わせの実行口を固定する。失敗と取り消しは保持しない。
    pub fn new(
        query: impl Fn(&[OsString], &AtomicBool) -> Result<QueryOutput, BenitoiteError>
        + Send
        + Sync
        + 'static,
    ) -> Self {
        Self {
            query: Box::new(query),
            names: Mutex::new(None),
            documents: Mutex::new(BTreeMap::new()),
        }
    }

    /// 起動時に検証した処理系を、既定の時間制限で問い合わせる。
    pub fn from_processor(processor: Arc<Processor>) -> Self {
        Self::new(move |args, cancelled| processor.query(args, QueryLimits::default(), cancelled))
    }

    /// 一覧の順序を保ち、ライセンスとパスの形が不正な名前を除く。
    pub fn names(&self, cancelled: &AtomicBool) -> Result<Arc<Vec<String>>, BenitoiteError> {
        // 問い合わせの間も枠を保持し、同時に求めたスレッドからの再問い合わせを防ぐ。
        let mut slot = self.names.lock().map_err(|_| invalid())?;
        if let Some(names) = &*slot {
            return Ok(Arc::clone(names));
        }
        let output = self.query(
            &["skill".into(), "list".into(), "--format=json".into()],
            cancelled,
        )?;
        let names = Arc::new(document_names(&output.stdout)?);
        *slot = Some(Arc::clone(&names));
        Ok(names)
    }

    /// 一覧にある許可した名前だけを問い合わせる。同名の並列取得も一回にする。
    pub fn document(
        &self,
        name: &str,
        cancelled: &AtomicBool,
    ) -> Result<Arc<SkillDocument>, BenitoiteError> {
        if !allowed_name(name) || !self.names(cancelled)?.iter().any(|item| item == name) {
            return Err(invalid());
        }
        let slot = Arc::clone(
            self.documents
                .lock()
                .map_err(|_| invalid())?
                .entry(name.into())
                .or_insert_with(|| Arc::new(Mutex::new(None))),
        );
        let mut document = slot.lock().map_err(|_| invalid())?;
        if let Some(document) = &*document {
            return Ok(Arc::clone(document));
        }
        let output = self.query(
            &[
                "skill".into(),
                "show".into(),
                "--format=json".into(),
                name.into(),
            ],
            cancelled,
        )?;
        let result = Arc::new(SkillDocument::from_json(name, &output.stdout)?);
        *document = Some(Arc::clone(&result));
        Ok(result)
    }

    fn query(
        &self,
        args: &[OsString],
        cancelled: &AtomicBool,
    ) -> Result<QueryOutput, BenitoiteError> {
        if cancelled.load(Ordering::Acquire) {
            return Err(BenitoiteError::Cancelled);
        }
        let output = (self.query)(args, cancelled)?;
        if cancelled.load(Ordering::Acquire) {
            return Err(BenitoiteError::Cancelled);
        }
        Ok(output)
    }
}

#[cfg(test)]
mod tests;
