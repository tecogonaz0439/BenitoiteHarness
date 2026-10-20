//! 検証した方針だけを生成し、JSON の読み戻しにも同じ検査を掛ける（設計書 03-02「方針の要素」）。

use std::fs;
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Deserializer, Serialize};

use super::{SandboxError, text};

/// 既定の守る名前。一時ディレクトリの削除もこの名前の項目を残す
/// （設計書 01-05「一時ディレクトリ」）。
pub const PROTECTED_NAMES: [&str; 2] = [".git", ".benitoiteh"];

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
    /// 配下を含まず、そのパスだけの読み取り。Linux では通常のファイルに限る。
    #[serde(default)]
    pub readable_files: Vec<PathBuf>,
    /// 配下のデータの読み取りだけを許し、実行可能なマッピングは許さない場所。
    #[serde(default)]
    pub readable_data_roots: Vec<PathBuf>,
    /// macOS で配下を含まずメタデータだけを許すパス。Linux の stat は制限されない。
    #[serde(default)]
    pub readable_metadata: Vec<PathBuf>,
    /// 読み取りを拒む実のパス。
    pub denied_read: Vec<PathBuf>,
    /// 書き込みを許すディレクトリ。require_writable が偽なら空に限る。
    pub writable: Vec<PathBuf>,
    /// 書ける場所を必須にするか。省略時も必須とする。
    #[serde(default = "default_require_writable")]
    pub require_writable: bool,
    /// 子のプロセスの作成を許すか（macOS の方針）。
    pub allow_fork: bool,
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

fn default_require_writable() -> bool {
    true
}

fn default_deny_exec_in_writable() -> bool {
    // 方針の JSON で省略されても、制限モードの拒否を弱めない（設計書 03-02「制限モードの方針」）。
    true
}

impl PolicyConfig {
    /// 書ける場所を持たず、子のプロセスの作成も許さない入力を作る。
    pub fn without_writable() -> Self {
        Self {
            require_writable: false,
            allow_fork: false,
            ..Self::new(Vec::new())
        }
    }

    /// 全体を読め、指定した場所にだけ書け、IP 通信と書ける場所での実行を拒む入力を作る。
    /// 守る名前は [`PROTECTED_NAMES`] とする。
    pub fn new(writable: Vec<PathBuf>) -> Self {
        Self {
            readable: ReadAccess::All,
            readable_files: Vec::new(),
            readable_data_roots: Vec::new(),
            readable_metadata: Vec::new(),
            denied_read: Vec::new(),
            writable,
            require_writable: true,
            allow_fork: true,
            protected_names: PROTECTED_NAMES.iter().map(|&name| name.into()).collect(),
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
    /// 正規の絶対パスと、指定した書ける場所の有無を確かめる。
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
        if config.require_writable == config.writable.is_empty() {
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
        for path in &config.readable_data_roots {
            canonical_path(path)?;
            directory(path)?;
        }
        for path in &config.readable_files {
            canonical_path(path)?;
            #[cfg(target_os = "linux")]
            if !fs::metadata(path).map_err(SandboxError::Io)?.is_file() {
                return Err(SandboxError::InvalidPolicy(text::INVALID_PATH));
            }
        }
        for path in &config.readable_metadata {
            // /etc や /var のリンク自身にもメタデータを許す必要がある。
            // 読み取りの根と違い、リンク先の内容を読む権限は与えない（実装プラン P2-09）。
            canonical_syntax(path)?;
            let kind = fs::symlink_metadata(path)
                .map_err(SandboxError::Io)?
                .file_type();
            if !kind.is_symlink() && !kind.is_file() && !kind.is_dir() {
                return Err(SandboxError::InvalidPolicy(text::INVALID_PATH));
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
