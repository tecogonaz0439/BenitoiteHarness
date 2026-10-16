//! キーの参照を起動時に解決し、診断から値を除く（設計書 02-01「認証の情報」）。

use std::fmt;
use std::process::{Command, Stdio};

use serde::Deserialize;

use super::{ConfigError, text};

/// 設定に保存してよい、キーそのものを含まない参照。
#[derive(Clone, Deserialize)]
#[serde(try_from = "RawApiKeyRef")]
pub enum ApiKeyRef {
    /// 環境変数の名前。
    Env(String),
    /// シェルを挟まず実行するプログラムと引数。
    Command(Vec<String>),
}

impl fmt::Debug for ApiKeyRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Env(name) => f.debug_tuple("Env").field(name).finish(),
            // コマンドの引数にも秘密が含まれ得るため、参照も伏せる（設計書 02-01「認証の情報」）。
            Self::Command(_) => f.debug_tuple("Command").field(&text::REDACTED).finish(),
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawApiKeyRef {
    env: Option<String>,
    command: Option<Vec<String>>,
}

impl TryFrom<RawApiKeyRef> for ApiKeyRef {
    type Error = ConfigError;

    fn try_from(value: RawApiKeyRef) -> Result<Self, Self::Error> {
        let reference = match (value.env, value.command) {
            (Some(name), None) => Self::Env(name),
            (None, Some(command)) => Self::Command(command),
            (Some(_), Some(_)) | (None, None) => {
                return Err(ConfigError::Invalid(text::INVALID_KEY_REF));
            }
        };
        reference.validate()?;
        Ok(reference)
    }
}

impl ApiKeyRef {
    /// 外から組み立てられた参照にも同じ制約を掛ける。
    pub fn validate(&self) -> Result<(), ConfigError> {
        let valid = match self {
            Self::Env(name) => !name.is_empty() && !name.contains(['=', '\0']),
            Self::Command(command) => {
                command.first().is_some_and(|program| !program.is_empty())
                    && command.iter().all(|argument| !argument.contains('\0'))
            }
        };
        if valid {
            Ok(())
        } else {
            Err(ConfigError::Invalid(text::INVALID_KEY_REF))
        }
    }

    /// 子プロセスの環境から除く変数の名前。
    pub fn env_name(&self) -> Option<&str> {
        match self {
            Self::Env(name) => Some(name),
            Self::Command(_) => None,
        }
    }

    /// 起動時に一度呼び、返ったキーをアダプタへ渡して保持する。
    pub fn resolve(&self) -> Result<ApiKey, CredentialError> {
        self.validate()
            .map_err(|_| CredentialError::InvalidReference)?;
        match self {
            Self::Env(name) => {
                let value = std::env::var(name).map_err(|_| CredentialError::Environment)?;
                if value.is_empty() {
                    return Err(CredentialError::Environment);
                }
                Ok(ApiKey(value))
            }
            Self::Command(arguments) => {
                let mut arguments = arguments.iter();
                let program = arguments.next().ok_or(CredentialError::InvalidReference)?;
                let output = Command::new(program)
                    .args(arguments)
                    .stdin(Stdio::null())
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped())
                    .output()
                    .map_err(CredentialError::Spawn)?;
                let first = output
                    .stdout
                    .split(|byte| *byte == b'\n')
                    .next()
                    .unwrap_or_default();
                let first = first.strip_suffix(b"\r").unwrap_or(first);
                if !output.status.success() {
                    // 失敗時も stdout の一行目は秘密として扱い、stderr 内の同じ値を伏せる。
                    let candidate = String::from_utf8_lossy(first);
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    let stderr = if candidate.is_empty() {
                        stderr.into_owned()
                    } else {
                        stderr.replace(candidate.as_ref(), text::REDACTED)
                    };
                    return Err(CredentialError::Command {
                        status: output.status.code(),
                        stderr: stderr.escape_debug().to_string(),
                    });
                }
                let key = std::str::from_utf8(first).map_err(|_| CredentialError::Output)?;
                if key.is_empty() {
                    return Err(CredentialError::Output);
                }
                Ok(ApiKey(key.to_owned()))
            }
        }
    }
}

/// Debug では常に値を伏せる API キー。
#[derive(Clone)]
pub struct ApiKey(String);

impl ApiKey {
    /// 提供者の認証ヘッダを作るときだけ借りる。
    pub fn expose_secret(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for ApiKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(text::REDACTED)
    }
}

/// 秘密の値やコマンドの引数を持たない、参照の解決の誤り。
#[derive(Debug)]
pub enum CredentialError {
    /// 参照の形が不正。
    InvalidReference,
    /// 環境変数が使えない。
    Environment,
    /// プログラムを起動できない。
    Spawn(std::io::Error),
    /// 失敗した終了状態と秘密を伏せた標準エラー出力。
    Command { status: Option<i32>, stderr: String },
    /// 一行目をキーにできない。
    Output,
}

impl fmt::Display for CredentialError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidReference => f.write_str(text::INVALID_KEY_REF),
            Self::Environment => f.write_str(text::KEY_UNAVAILABLE),
            Self::Spawn(error) => write!(f, "{}: {error}", text::KEY_COMMAND),
            Self::Command { status, stderr } => {
                write!(f, "{} (status {status:?}): {stderr}", text::KEY_COMMAND)
            }
            Self::Output => f.write_str(text::KEY_OUTPUT),
        }
    }
}
impl std::error::Error for CredentialError {}
