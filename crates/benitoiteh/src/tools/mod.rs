//! 道具の検証と実行をまとめ、範囲と結果の上限を守る（設計書 01-03「道具の定義の共通の規則」）。

mod definitions;
pub mod exclude;
mod paths;
mod read;
mod search;
mod text;
pub mod truncate;

use std::collections::BTreeMap;
use std::fmt;
use std::fs::Metadata;
use std::io;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde_json::Value;

use crate::config::Mode;
use crate::provider::ToolSpec;
use crate::safe_fs::{SafeFsError, ScopeRoot};
use exclude::ExcludeRules;

/// セッション中に順序を変えない道具の識別子。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ToolId {
    /// 行番号を付けたテキストの読み取り。
    Read,
    /// パターンに合うパスの列挙。
    Glob,
    /// 正規表現による行の検索。
    Grep,
    /// ファイルの作成と置き換え。実行は P1-07 で加える。
    Write,
    /// 文字列の置き換え。実行は P1-07 で加える。
    Edit,
}

impl ToolId {
    /// 提供者とプロンプトで共通に使う名前。
    pub const fn name(self) -> &'static str {
        match self {
            Self::Read => "read",
            Self::Glob => "glob",
            Self::Grep => "grep",
            Self::Write => "write",
            Self::Edit => "edit",
        }
    }
}

/// 道具が実行前に行う確認の分類。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ApprovalRequirement {
    /// 読み取りには確認を求めない。
    Never,
    /// 書く先とモードによって確認を求める。
    WriteScope,
}

/// JSON の引数から独立した、ループが渡す確認の状態。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ApprovalStatus {
    /// まだ確認していない。
    Unapproved,
    /// ループでこの呼び出しの確認を受けた。
    Approved,
}

/// 一つの道具の定義と、ループとプロンプトが使う情報。
#[derive(Clone, Debug)]
pub struct ToolDefinition {
    pub id: ToolId,
    pub spec: ToolSpec,
    /// P1-08 が prompts/ の英文を選ぶための識別子。
    pub prompt_key: &'static str,
    pub summary: &'static str,
    pub rules: &'static [&'static str],
    pub parallel: bool,
    pub approval: ApprovalRequirement,
}

/// 修復と検証を終えた呼び出し。未検証の引数では作れない。
#[derive(Clone, Debug)]
pub struct ValidatedCall {
    pub(crate) id: ToolId,
    pub(crate) arguments: Value,
}

impl ValidatedCall {
    /// 検証済みの道具の識別子。
    pub fn id(&self) -> ToolId {
        self.id
    }

    /// 修復した後の引数。
    pub fn arguments(&self) -> &Value {
        &self.arguments
    }
}

/// セッションの開始時に固定する道具の一覧。
#[derive(Debug)]
pub struct ToolSet {
    mode: Mode,
    definitions: Vec<ToolDefinition>,
}

impl ToolSet {
    /// P1 の両モードで同じ五つを、決まった順序で用意する。
    pub fn new(mode: Mode) -> Self {
        Self {
            mode,
            definitions: definitions::definitions(),
        }
    }

    /// プロンプトとスケジューラが共有する定義。
    pub fn definitions(&self) -> &[ToolDefinition] {
        &self.definitions
    }

    /// 提供者に渡す引数の定義を、同じ順序で返す。
    pub fn specs(&self) -> Vec<ToolSpec> {
        self.definitions
            .iter()
            .map(|tool| tool.spec.clone())
            .collect()
    }

    /// 知らない道具を拒否し、引数を修復して型と必須を確かめる。
    pub fn validate(&self, name: &str, mut arguments: Value) -> Result<ValidatedCall, ToolError> {
        let tool = self
            .definitions
            .iter()
            .find(|tool| tool.spec.name == name)
            .ok_or_else(|| ToolError::InvalidArguments(text::unknown_tool(name)))?;
        repair_and_validate(&tool.spec, &mut arguments)?;
        Ok(ValidatedCall {
            id: tool.id,
            arguments,
        })
    }

    /// JSON の道具呼び出しを検証して実行する、持ち主の境界。
    pub fn execute(&self, context: &ToolContext, name: &str, arguments: Value) -> ToolOutput {
        match self.validate(name, arguments) {
            Ok(call) => self.execute_validated(context, &call, ApprovalStatus::Unapproved),
            Err(error) => ToolOutput::error(error.to_string()),
        }
    }

    /// ループが検証済みの呼び出しを共有の文脈で実行する。
    pub fn execute_validated(
        &self,
        context: &ToolContext,
        call: &ValidatedCall,
        _approval: ApprovalStatus,
    ) -> ToolOutput {
        if self.mode != context.mode {
            return ToolOutput::error(text::MODE_MISMATCH);
        }
        let result = match call.id {
            ToolId::Read => read::execute(context, &call.arguments),
            ToolId::Glob => search::glob(context, &call.arguments),
            ToolId::Grep => search::grep(context, &call.arguments),
            ToolId::Write | ToolId::Edit => Err(ToolError::Unavailable(call.id)),
        };
        match result {
            Ok(output) => ToolOutput::success(output),
            Err(error) => ToolOutput::error(error.to_string()),
        }
    }
}

pub use definitions::repair_and_validate;

/// ファイルの同一性と、read の後の変更を検知する版。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FileVersion {
    pub device: u64,
    pub inode: u64,
    pub size: u64,
    pub modified_seconds: i64,
    pub modified_nanoseconds: i64,
}

impl FileVersion {
    /// 開いたファイルのメタデータから版を作る。
    pub fn from_metadata(metadata: &Metadata) -> Self {
        Self {
            device: metadata.dev(),
            inode: metadata.ino(),
            size: metadata.len(),
            modified_seconds: metadata.mtime(),
            modified_nanoseconds: metadata.mtime_nsec(),
        }
    }
}

/// 最大八つの道具が共有する文脈。変わる版の記録だけを Mutex で守る。
#[derive(Debug)]
pub struct ToolContext {
    pub mode: Mode,
    pub project: ScopeRoot,
    pub temporary: ScopeRoot,
    pub spill: ScopeRoot,
    pub exclude: ExcludeRules,
    read_versions: Mutex<BTreeMap<PathBuf, FileVersion>>,
}

impl ToolContext {
    /// 起動時に開いた範囲と、除外の規則を受け取る。
    pub fn new(
        mode: Mode,
        project: ScopeRoot,
        temporary: ScopeRoot,
        spill: ScopeRoot,
        exclude: ExcludeRules,
    ) -> Self {
        Self {
            mode,
            project,
            temporary,
            spill,
            exclude,
            read_versions: Mutex::new(BTreeMap::new()),
        }
    }

    /// read または書き込みを終えた実のパスの版を記録する。
    pub fn record_read_version(
        &self,
        path: PathBuf,
        version: FileVersion,
    ) -> Result<(), ToolError> {
        self.read_versions
            .lock()
            .map_err(|_| ToolError::State)?
            .insert(path, version);
        Ok(())
    }

    /// 書く道具が、read の記録と現在の版を突き合わせる。
    pub fn read_version(&self, path: &Path) -> Result<Option<FileVersion>, ToolError> {
        Ok(self
            .read_versions
            .lock()
            .map_err(|_| ToolError::State)?
            .get(path)
            .cloned())
    }
}

/// 確認を受ける側へ渡す、実行前の操作の要約。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ApprovalRequest {
    pub summary: String,
}

/// LLM の結果、画面用の文、実行前の確認の要求。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ToolOutput {
    pub text: String,
    pub is_error: bool,
    pub display: Option<String>,
    pub approval: Option<ApprovalRequest>,
}

impl ToolOutput {
    /// 成功の結果にも共通の 64 KiB の上限を掛ける。
    pub fn success(text: impl Into<String>) -> Self {
        Self {
            text: truncate::cap_middle(&text.into()),
            is_error: false,
            display: None,
            approval: None,
        }
    }

    /// 誤りもセッションを止めずに LLM へ返す。
    pub fn error(text: impl Into<String>) -> Self {
        Self {
            is_error: true,
            ..Self::success(text)
        }
    }

    /// 実行せずに確認の要求を返す。許可後に同じ呼び出しを再実行する。
    pub fn needs_approval(summary: impl Into<String>) -> Self {
        let summary = summary.into();
        Self {
            approval: Some(ApprovalRequest {
                summary: summary.clone(),
            }),
            ..Self::success(summary)
        }
    }
}

/// 道具の失敗。LLM に示す文は text の関数で組み立てる。
#[derive(Debug)]
pub enum ToolError {
    InvalidArguments(String),
    SafeFs(SafeFsError),
    Io(io::Error),
    Excluded(String),
    OutsideScope,
    NotText,
    Offset { offset: u64, total: usize },
    Pattern(String),
    State,
    Changed,
    Unavailable(ToolId),
}

impl fmt::Display for ToolError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidArguments(message) | Self::Pattern(message) => f.write_str(message),
            Self::SafeFs(error) => write!(f, "{} {error}", text::SAFE_FS),
            Self::Io(error) => write!(f, "{} {error}", text::IO),
            Self::Excluded(rule) => f.write_str(&text::excluded(rule)),
            Self::OutsideScope => f.write_str(text::OUTSIDE_SCOPE),
            Self::NotText => f.write_str(text::NOT_TEXT),
            Self::Offset { offset, total } => f.write_str(&text::offset(*offset, *total)),
            Self::State => f.write_str(text::STATE),
            Self::Changed => f.write_str(text::CHANGED),
            Self::Unavailable(id) => f.write_str(&text::unavailable(id.name())),
        }
    }
}

impl std::error::Error for ToolError {}

impl From<SafeFsError> for ToolError {
    fn from(error: SafeFsError) -> Self {
        Self::SafeFs(error)
    }
}

impl From<io::Error> for ToolError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}
