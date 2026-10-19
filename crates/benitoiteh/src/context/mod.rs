//! 会話の前に置く固定の文脈と環境の情報を組み立てる（設計書 01-04「文脈の構成」）。

mod environment;
pub mod instructions;
pub mod skills;
mod text;

use std::fmt;

use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::benitoite::skill::{SkillDocument, skill_parts};
use crate::config::Mode;
use crate::provider::{SchemaKind, ToolSpec, Usage};
use crate::tools::ToolSet;

pub use environment::Environment;

/// 文脈の組み立てで起きた誤り。
#[derive(Debug)]
pub enum ContextError {
    /// 処理系の Skill の属性が欠けているか、先頭の注釈が閉じていない。
    InvalidSkill,
    /// 道具と Skill の有無が一致しない。
    SkillMismatch,
    /// 環境のパスが UTF-8 の絶対パスでない。
    InvalidPath,
    /// UTC の日付を 1970〜9999 年で表せない。
    InvalidDate,
    /// 道具の JSON を作れなかった。
    Json(serde_json::Error),
}

impl fmt::Display for ContextError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidSkill => f.write_str(text::INVALID_SKILL),
            Self::SkillMismatch => f.write_str(text::SKILL_MISMATCH),
            Self::InvalidPath => f.write_str(text::INVALID_PATH),
            Self::InvalidDate => f.write_str(text::INVALID_DATE),
            Self::Json(error) => write!(f, "{}: {error}", text::JSON_ERROR),
        }
    }
}

impl std::error::Error for ContextError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Json(error) => Some(error),
            Self::SkillMismatch | Self::InvalidSkill | Self::InvalidPath | Self::InvalidDate => {
                None
            }
        }
    }
}

/// セッションの間変えず、アダプタに渡すシステムプロンプトと道具の定義。
#[derive(Clone, Debug)]
pub struct SystemContext {
    /// 環境の情報を含まないシステムプロンプト。
    pub system_prompt: String,
    /// ToolSet と同じ順序の道具の定義。
    pub tools: Vec<ToolSpec>,
}

impl SystemContext {
    /// 同じ ToolSet から、モード、道具の説明、引数の定義を揃えて作る。
    pub fn new(tools: &ToolSet, skill: Option<&SkillDocument>) -> Result<Self, ContextError> {
        if tools.has_benitoite() != skill.is_some()
            || (tools.mode() == Mode::Restricted && skill.is_none())
        {
            return Err(ContextError::SkillMismatch);
        }
        let (preamble, mode) = match tools.mode() {
            Mode::Restricted => (
                include_str!("../../prompts/preamble-restricted.md"),
                include_str!("../../prompts/mode-restricted.md"),
            ),
            Mode::Normal => (
                include_str!("../../prompts/preamble-normal.md"),
                include_str!("../../prompts/mode-normal.md"),
            ),
        };
        let summaries = tools
            .definitions()
            .iter()
            .map(|tool| format!("{}: {}", tool.spec.name, tool.summary))
            .collect::<Vec<_>>()
            .join("\n");
        let mut rules = Vec::new();
        for tool in tools.definitions() {
            for rule in tool.rules.iter().filter(|rule| !rule.is_empty()) {
                rules.push(format!("{}: {rule}", tool.spec.name));
            }
        }
        rules.push(
            include_str!("../../prompts/rules-common.md")
                .trim_end()
                .into(),
        );
        let mut system = preamble.trim_end().to_owned();
        append_section(&mut system, "tools", &summaries);
        append_section(&mut system, "rules", &rules.join("\n"));
        append_section(&mut system, "mode", mode.trim_end());
        if let Some(skill) = skill {
            let parts = skill_parts(skill.text()).map_err(|_| ContextError::InvalidSkill)?;
            let benitoite = match tools.mode() {
                Mode::Restricted => format!(
                    "{}\n\n{}",
                    include_str!("../../prompts/benitoite-mapping.md").trim_end(),
                    parts.body
                ),
                Mode::Normal => format!(
                    "{}: {}\n{}",
                    parts.name,
                    parts.description,
                    include_str!("../../prompts/benitoite-index.md").trim_end()
                ),
            };
            append_section(&mut system, "benitoite", &benitoite);
        }
        system.push('\n');
        Ok(Self {
            system_prompt: system,
            tools: tools.specs(),
        })
    }
}

fn append_section(output: &mut String, tag: &str, contents: &str) {
    output.push_str(&format!("\n\n<{tag}>\n{contents}\n</{tag}>"));
}

/// 道具を提供者固有の strict 指定を含まない JSON の配列にする。
///
/// オブジェクトのキーは serde_json の既定の辞書順、配列は ToolSet の順序とする。
pub fn tool_definitions_json(tools: &[ToolSpec]) -> Value {
    Value::Array(
        tools
            .iter()
            .map(|tool| {
                json!({
                    "name": tool.name,
                    "description": tool.description,
                    "input_schema": tool.input_schema(SchemaKind::Plain),
                })
            })
            .collect(),
    )
}

/// システムプロンプトの UTF-8 のバイト列の SHA-256 を小文字の十六進で返す。
pub fn system_prompt_hash(system: &str) -> String {
    sha256(system.as_bytes())
}

/// 普通の形の道具の JSON（空白なし）の SHA-256 を返す。
pub fn tool_definitions_hash(tools: &[ToolSpec]) -> Result<String, ContextError> {
    let json = serde_json::to_vec(&tool_definitions_json(tools)).map_err(ContextError::Json)?;
    Ok(sha256(&json))
}

fn sha256(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// 最後の応答の入力全体と、その後の追加分からトークン数を見積もる。
///
/// 報告がなければ total_bytes を使う。各バイト数は UTF-8 で数え、
/// 入力の報告にキャッシュの量を重ねて足さない（設計書 01-04「文脈の大きさの見積もり」）。
pub fn estimate_tokens(last_usage: Option<&Usage>, total_bytes: u64, added_bytes: u64) -> u64 {
    match last_usage.and_then(|usage| usage.input_tokens) {
        Some(input) => input.saturating_add(added_bytes.div_ceil(3)),
        None => total_bytes.div_ceil(3),
    }
}

/// session_start に入れる、送り先と送る内容の一行の表示。
///
/// host には URL 全体ではなく、設定を検証した側が取り出したホスト名を渡す。
pub fn transmission_notice(provider: &str, model: &str, host: &str) -> String {
    text::transmission_notice(provider, model, host)
}
