//! 検証した方針だけを生成し、JSON の読み戻しにも同じ検査を掛ける（設計書 03-02「方針の要素」）。

use std::fs;
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Deserializer, Serialize};

use super::{SandboxError, text};

/// 読み取りを許す範囲。どちらも `denied_read` を除く。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReadAccess {
    /// ファイルシステムの全体。
    All,
    /// 列挙したディレクトリの下だけ。
    Roots(Vec<PathBuf>),
}

/// 子孫にも適用するプログラムの起動の制限。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProgramAccess {
    /// ファイルの一覧では制限しない。書ける場所での実行の拒否は別の要素である。
    Unrestricted,
    /// 列挙した実のパスだけ。
    Only(Vec<PathBuf>),
}

/// 方針の入力。`SandboxPolicy::new` で検証して使う。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PolicyConfig {
    /// 読み取りを許す範囲。
    pub readable: ReadAccess,
    /// 読み取りを拒む実のパス。
    pub denied_read: Vec<PathBuf>,
    /// 書き込みを許すディレクトリ。空にはできない。
    pub writable: Vec<PathBuf>,
    /// 各書ける場所の下で、作成と変更を拒む名前。
    pub protected_names: Vec<String>,
    /// IP の通信を許すか。外部の Unix IPC は常に拒む。
    pub allow_ip: bool,
    /// 起動を許すプログラム。
    pub programs: ProgramAccess,
    /// 書ける場所での実行と実行可能なマッピングを拒むか。省略時も拒む。
    #[serde(default = "default_deny_exec_in_writable")]
    pub deny_exec_in_writable: bool,
}

fn default_deny_exec_in_writable() -> bool {
    // 方針の JSON で省略されても、制限モードの拒否を弱めない（設計書 03-02「制限モードの方針」）。
    true
}

impl PolicyConfig {
    /// 全体を読め、指定した場所にだけ書け、IP 通信と書ける場所での実行を拒む入力を作る。
    pub fn new(writable: Vec<PathBuf>) -> Self {
        Self {
            readable: ReadAccess::All,
            denied_read: Vec::new(),
            writable,
            protected_names: vec![".git".into(), ".benitoiteh".into()],
            allow_ip: false,
            programs: ProgramAccess::Unrestricted,
            deny_exec_in_writable: default_deny_exec_in_writable(),
        }
    }
}

/// 検証済みの方針。書ける場所での実行と実行可能なマッピングの可否を含む。
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct SandboxPolicy(PolicyConfig);

impl SandboxPolicy {
    /// 正規の絶対パスと、空でない書ける場所を確かめる。
    pub fn new(config: PolicyConfig) -> Result<Self, SandboxError> {
        let policy = Self(config);
        policy.validate()?;
        Ok(policy)
    }

    /// 検証済みの要素を参照する。
    pub fn config(&self) -> &PolicyConfig {
        &self.0
    }

    pub(super) fn validate(&self) -> Result<(), SandboxError> {
        let config = self.config();
        if config.writable.is_empty() {
            return Err(SandboxError::InvalidPolicy(text::EMPTY_WRITABLE));
        }
        for name in &config.protected_names {
            if name.is_empty()
                || name == "."
                || name == ".."
                || !name
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_'))
            {
                return Err(SandboxError::InvalidPolicy(text::INVALID_PROTECTED));
            }
        }
        if ![".git", ".benitoiteh"]
            .iter()
            .all(|required| config.protected_names.iter().any(|name| name == required))
        {
            return Err(SandboxError::InvalidPolicy(text::INVALID_PROTECTED));
        }
        for path in &config.writable {
            canonical_path(path)?;
            directory(path)?;
        }
        if let ReadAccess::Roots(roots) = &config.readable {
            for path in roots {
                canonical_path(path)?;
                directory(path)?;
            }
        }
        for path in &config.denied_read {
            canonical_path(path)?;
            if config.writable.iter().any(|root| root.starts_with(path)) {
                return Err(SandboxError::InvalidPolicy(text::DENIED_WRITABLE));
            }
        }
        if let ProgramAccess::Only(paths) = &config.programs {
            for path in paths {
                canonical_path(path)?;
            }
        }
        Ok(())
    }
}

impl<'de> Deserialize<'de> for SandboxPolicy {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Self::new(PolicyConfig::deserialize(deserializer)?).map_err(serde::de::Error::custom)
    }
}

pub(super) fn canonical_path(path: &Path) -> Result<(), SandboxError> {
    // Path の比較は途中の「.」などを無視するので、元の OS 文字列を比較する。
    // Seatbelt の照合先と入力が食い違う別名を許さない（設計書 03-02「サンドボックスの掛け方」）。
    canonical_syntax(path)?;
    let real = fs::canonicalize(path).map_err(SandboxError::Io)?;
    if real.as_os_str() != path.as_os_str() {
        return Err(SandboxError::InvalidPolicy(text::INVALID_PATH));
    }
    Ok(())
}

pub(super) fn canonical_syntax(path: &Path) -> Result<(), SandboxError> {
    let normalized: PathBuf = path.components().collect();
    if !path.is_absolute()
        || path.to_str().is_none()
        || normalized.as_os_str() != path.as_os_str()
        || path
            .components()
            .any(|component| matches!(component, Component::ParentDir | Component::CurDir))
    {
        return Err(SandboxError::InvalidPolicy(text::INVALID_PATH));
    }
    Ok(())
}

pub(super) fn directory(path: &Path) -> Result<(), SandboxError> {
    if !fs::metadata(path).map_err(SandboxError::Io)?.is_dir() {
        return Err(SandboxError::InvalidPolicy(text::INVALID_DIRECTORY));
    }
    Ok(())
}
