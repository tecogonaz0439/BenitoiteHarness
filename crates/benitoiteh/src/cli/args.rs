//! exec の引数を解釈し、P1 で未対応の指定を拒む（設計書 05-01「オプション」）。

use std::collections::BTreeSet;
use std::ffi::OsString;
use std::fmt;
use std::io::{self, Read};
use std::path::PathBuf;

use super::text;
use crate::config::{Approval, Mode, Sandbox};

/// 依頼の入力元。
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Request {
    /// 一つの引数に書いた依頼。
    Text(String),
    /// `-` による標準入力。
    Stdin,
}

impl Request {
    /// 標準入力を使うときだけ読み取る。
    pub fn read(&self, stdin: &mut impl Read) -> io::Result<String> {
        match self {
            Self::Text(text) => Ok(text.clone()),
            Self::Stdin => {
                let mut text = String::new();
                stdin.read_to_string(&mut text)?;
                Ok(text)
            }
        }
    }
}

/// 後の段階で有効にするオプション。現段階では指定を保持して誤りにする。
#[derive(Clone, Debug, Default)]
pub struct DeferredOptions {
    pub allow: Option<Vec<String>>,
    pub project_parent: Option<PathBuf>,
    pub resume: Option<String>,
    pub continue_session: bool,
}

/// P1 の exec が使う明示的な指定。既定の値は config で解決する。
#[derive(Clone, Debug)]
pub struct ExecArgs {
    pub sandbox: Option<Sandbox>,
    pub approval: Option<Approval>,
    pub no_sandbox: bool,
    pub mode: Option<Mode>,
    pub project: Option<PathBuf>,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub effort: Option<String>,
    pub max_turns: Option<u64>,
    pub max_check_fix: Option<u64>,
    pub max_run_tokens: Option<u64>,
    pub json: bool,
    pub output_last_message: Option<PathBuf>,
    pub request: Request,
}

/// 使い方の誤り。呼び出し元は終了状態 2 にする。
#[derive(Debug)]
pub enum ArgsError {
    /// 知らない引数、欠けた値、または不正な値。
    Invalid,
    /// 同じオプションを二度指定した。
    Duplicate(String),
    /// 解釈したが、まだ使えないオプション。
    Unavailable {
        option: String,
        values: Box<DeferredOptions>,
    },
}

impl fmt::Display for ArgsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Invalid => f.write_str(text::INVALID_ARGUMENTS),
            Self::Duplicate(option) => f.write_str(&text::duplicate_option(option)),
            Self::Unavailable { option, .. } => f.write_str(&text::unavailable_option(option)),
        }
    }
}
impl std::error::Error for ArgsError {}

impl ExecArgs {
    /// `exec` の後ろだけを渡して解釈する。パスの値は非 UTF-8 でも保つ。
    pub fn parse(args: impl IntoIterator<Item = impl Into<OsString>>) -> Result<Self, ArgsError> {
        let mut result = Self {
            sandbox: None,
            approval: None,
            no_sandbox: false,
            mode: None,
            project: None,
            provider: None,
            model: None,
            effort: None,
            max_turns: None,
            max_check_fix: None,
            max_run_tokens: None,
            json: false,
            output_last_message: None,
            request: Request::Stdin,
        };
        let mut deferred = DeferredOptions::default();
        let mut unavailable = None;
        let mut seen = BTreeSet::new();
        let mut request = None;
        let mut options = true;
        let mut args = args.into_iter().map(Into::into);
        while let Some(argument) = args.next() {
            if options && argument == "--" {
                options = false;
                continue;
            }
            let Some(argument_text) = argument.to_str() else {
                return Err(ArgsError::Invalid);
            };
            if !options || !argument_text.starts_with('-') || argument_text == "-" {
                if request.is_some() || argument_text.is_empty() {
                    return Err(ArgsError::Invalid);
                }
                request = Some(if argument_text == "-" {
                    Request::Stdin
                } else {
                    Request::Text(argument_text.to_owned())
                });
                continue;
            }
            let (option, attached) = argument_text
                .split_once('=')
                .map_or((argument_text, None), |(option, value)| {
                    (option, Some(value))
                });
            if !seen.insert(option.to_owned()) {
                return Err(ArgsError::Duplicate(option.to_owned()));
            }
            let flag = matches!(option, "--json" | "--continue" | "--no-sandbox");
            let known = matches!(
                option,
                "--mode"
                    | "--project"
                    | "--provider"
                    | "--model"
                    | "--effort"
                    | "--max-turns"
                    | "--max-check-fix"
                    | "--max-run-tokens"
                    | "--json"
                    | "--output-last-message"
                    | "--sandbox"
                    | "--approval"
                    | "--allow"
                    | "--project-parent"
                    | "--resume"
                    | "--continue"
                    | "--no-sandbox"
            );
            if !known || (flag && attached.is_some()) {
                return Err(ArgsError::Invalid);
            }
            let value = if flag {
                OsString::new()
            } else {
                attached
                    .map(OsString::from)
                    .or_else(|| args.next())
                    .filter(|value| !value.is_empty())
                    .ok_or(ArgsError::Invalid)?
            };
            let string = || {
                value
                    .to_str()
                    .filter(|value| !value.is_empty())
                    .ok_or(ArgsError::Invalid)
            };
            match option {
                "--mode" => {
                    result.mode = Some(match string()? {
                        "restricted" => Mode::Restricted,
                        "normal" => Mode::Normal,
                        _ => return Err(ArgsError::Invalid),
                    })
                }
                "--project" => result.project = Some(PathBuf::from(value)),
                "--provider" => result.provider = Some(string()?.to_owned()),
                "--model" => result.model = Some(string()?.to_owned()),
                "--effort" => result.effort = Some(string()?.to_owned()),
                "--max-turns" => result.max_turns = Some(number(string()?)?),
                "--max-check-fix" => result.max_check_fix = Some(number(string()?)?),
                "--max-run-tokens" => result.max_run_tokens = Some(number(string()?)?),
                "--json" => result.json = true,
                "--output-last-message" => result.output_last_message = Some(PathBuf::from(value)),
                "--sandbox" => {
                    result.sandbox = Some(match string()? {
                        "read-only" => Sandbox::ReadOnly,
                        "workspace-write" => Sandbox::WorkspaceWrite,
                        "danger-full-access" => Sandbox::DangerFullAccess,
                        _ => return Err(ArgsError::Invalid),
                    })
                }
                "--approval" => {
                    result.approval = Some(match string()? {
                        "untrusted" => Approval::Untrusted,
                        "on-request" => Approval::OnRequest,
                        "never" => Approval::Never,
                        _ => return Err(ArgsError::Invalid),
                    })
                }
                "--allow" => {
                    let effects: Vec<String> = string()?.split(',').map(str::to_owned).collect();
                    if effects.iter().any(String::is_empty) {
                        return Err(ArgsError::Invalid);
                    }
                    deferred.allow = Some(effects);
                }
                "--project-parent" => deferred.project_parent = Some(PathBuf::from(value)),
                "--resume" => deferred.resume = Some(string()?.to_owned()),
                "--continue" => deferred.continue_session = true,
                "--no-sandbox" => result.no_sandbox = true,
                _ => return Err(ArgsError::Invalid),
            }
            if matches!(
                option,
                "--allow" | "--project-parent" | "--resume" | "--continue"
            ) && unavailable.is_none()
            {
                unavailable = Some(option.to_owned());
            }
        }
        if let Some(option) = unavailable {
            return Err(ArgsError::Unavailable {
                option,
                values: Box::new(deferred),
            });
        }
        result.request = request.ok_or(ArgsError::Invalid)?;
        Ok(result)
    }
}

fn number(value: &str) -> Result<u64, ArgsError> {
    if !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(ArgsError::Invalid);
    }
    value.parse().map_err(|_| ArgsError::Invalid)
}
