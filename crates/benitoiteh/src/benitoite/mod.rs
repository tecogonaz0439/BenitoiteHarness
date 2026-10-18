//! 処理系を探して機能を確かめ、スクリプトを実行しない問い合わせを起動する（設計書 04-01）。

pub mod diagnostics;
pub(crate) mod display;
pub mod effects;
mod executable;
pub mod report;
pub(crate) mod test_results;
pub use effects::Effect;
mod query;
pub mod skill;
pub(crate) mod text;

use crate::sandbox;
use serde_json::Value;
use std::fmt;
use std::io;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;

pub use executable::{discover, validate_executable};
pub(crate) use query::{QueryContext, QueryLauncher};
pub use query::{QueryLimits, QueryOutput, StderrLimit};

/// スクリプトを実行しない検査の問い合わせの口。
pub type CheckQuery = dyn Fn(&[std::ffi::OsString], QueryLimits, &AtomicBool) -> Result<QueryOutput, BenitoiteError>
    + Send
    + Sync;

/// システムプロンプトとの対応を確かめた処理系の版。
pub const PROMPT_VERIFIED_VERSION: &str = "0.0.7";

/// 問い合わせと探索で起きた誤り。
#[derive(Debug)]
pub enum BenitoiteError {
    /// ファイルの読み取りの失敗。
    Io(io::Error),
    /// 形、名前、出力の契約の違反。
    Invalid(&'static str),
    /// 方針の作成または起動の準備の失敗。
    Sandbox(sandbox::SandboxError),
    /// 子プロセスの起動または回収の失敗。
    Run(sandbox::run::RunError),
    /// 終了状態が成功でない。
    Exit {
        code: Option<i32>,
        stdout: Vec<u8>,
        stderr: Vec<u8>,
    },
    /// 時間の上限。
    TimedOut,
    /// 呼び出し元の取り消し。
    Cancelled,
    /// 標準出力の上限。
    OutputLimit,
    /// 検査の標準エラー出力の上限。
    DiagnosticsLimit,
}
impl fmt::Display for BenitoiteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(f, "{} {error}", text::IO),
            Self::Invalid(message) => f.write_str(message),
            Self::Sandbox(error) => error.fmt(f),
            Self::Run(error) => error.fmt(f),
            Self::Exit { code, .. } => write!(f, "{} ({code:?})", text::FAILED),
            Self::TimedOut => f.write_str(text::TIMED_OUT),
            Self::Cancelled => f.write_str(text::CANCELLED),
            Self::OutputLimit => f.write_str(text::OUTPUT_LIMIT),
            Self::DiagnosticsLimit => f.write_str(text::DIAGNOSTICS_LIMIT),
        }
    }
}
impl std::error::Error for BenitoiteError {}
impl From<io::Error> for BenitoiteError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}
impl From<sandbox::SandboxError> for BenitoiteError {
    fn from(error: sandbox::SandboxError) -> Self {
        Self::Sandbox(error)
    }
}
impl From<sandbox::run::RunError> for BenitoiteError {
    fn from(error: sandbox::run::RunError) -> Self {
        Self::Run(error)
    }
}

/// 検査済みの処理系と、起動時に固定した問い合わせの文脈。
#[derive(Debug)]
pub struct Processor {
    /// シンボリックリンクを解いた絶対パス。
    pub path: PathBuf,
    /// 処理系が申告した版。
    pub version: String,
    /// 処理系が申告したビルドの情報。
    pub build: Value,
    /// 形を確かめた記述子から計算した SHA-256。
    pub sha256: String,
    context: QueryContext,
}
impl Processor {
    pub(crate) fn inspect(
        configured: Option<&std::path::Path>,
        path: Option<&std::ffi::OsStr>,
        context: QueryContext,
    ) -> Result<Self, BenitoiteError> {
        let path = discover(configured, path, &context.locations)?;
        let (path, sha256) = validate_executable(&path, &context.locations, false)?;
        let mut processor = Self {
            path,
            sha256,
            version: String::new(),
            build: Value::Null,
            context,
        };
        let output = processor.query(
            &["--version=json".into()],
            QueryLimits::default(),
            &AtomicBool::new(false),
        )?;
        let (version, build) = version_report(&output.stdout)?;
        processor.version = version;
        processor.build = build;
        Ok(processor)
    }

    /// 未検証の版であれば起動時の警告を返す。
    pub fn version_warning(&self) -> Option<String> {
        (self.version != PROMPT_VERIFIED_VERSION).then(|| text::version_warning(&self.version))
    }
}

/// 標準出力全体を一つの JSON オブジェクトとして読み、形式の版と種類を照合する。
pub fn json_object(
    bytes: &[u8],
    kind: &str,
) -> Result<serde_json::Map<String, Value>, BenitoiteError> {
    let value: Value =
        serde_json::from_slice(bytes).map_err(|_| BenitoiteError::Invalid(text::INVALID_JSON))?;
    let Value::Object(object) = value else {
        return Err(BenitoiteError::Invalid(text::INVALID_JSON));
    };
    if object.get("schemaVersion").and_then(Value::as_u64) != Some(1)
        || object.get("kind").and_then(Value::as_str) != Some(kind)
    {
        return Err(BenitoiteError::Invalid(text::INVALID_JSON));
    }
    Ok(object)
}

fn version_report(bytes: &[u8]) -> Result<(String, Value), BenitoiteError> {
    let object = json_object(bytes, "version")?;
    let supported = [
        "diagnostics",
        "testResults",
        "checkReport",
        "testCheckReport",
        "effectLimit",
        "skillList",
        "skillDocument",
    ]
    .iter()
    .all(|key| {
        object
            .get("formats")
            .and_then(|formats| formats.get(key))
            .and_then(Value::as_array)
            .is_some_and(|versions| versions.iter().any(|version| version.as_u64() == Some(1)))
    });
    if !supported {
        return Err(BenitoiteError::Invalid(text::UNSUPPORTED));
    }
    let version = object
        .get("version")
        .and_then(Value::as_str)
        .ok_or(BenitoiteError::Invalid(text::INVALID_JSON))?;
    let build = object
        .get("build")
        .ok_or(BenitoiteError::Invalid(text::INVALID_JSON))?;
    Ok((version.into(), build.clone()))
}

#[cfg(test)]
mod tests;

/// 制限モードの実行用に起動時に固定した値（設計書 03-02、04-01）。
#[derive(Clone, Debug)]
pub struct RunContext {
    pub test_timeout: std::time::Duration,
    pub processor: PathBuf,
    pub helper: PathBuf,
    pub scratch: PathBuf,
    pub placeholders: PathBuf,
    pub env: std::collections::BTreeMap<std::ffi::OsString, std::ffi::OsString>,
    pub locations: crate::config::writable::WritableLocations,
    pub denied_candidates: Vec<PathBuf>,
    pub allowed_programs: Vec<PathBuf>,
}
