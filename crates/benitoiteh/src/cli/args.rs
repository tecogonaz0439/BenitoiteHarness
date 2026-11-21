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

/// 起動の入口の種類（設計書 05-03「起動とセッションの準備の方針の分離」）。
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Entrypoint {
    /// 対話の画面（`benitoiteh`）。
    Tui,
    /// 非対話の単発実行（`benitoiteh exec`・`debug prompt-input`）。
    #[default]
    Exec,
    /// 標準入出力の ACP エージェント（`benitoiteh acp`）。
    Acp,
}

/// 実行時の承認の方針の決め方（設計書 05-01「オプション」、05-03「起動とセッションの準備の方針の分離」）。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RuntimeApprovalPolicy {
    /// 対話の画面: 制限モードは `on-request`（`--approval` 不可）、通常モードは `--approval` または設定値。
    Tui,
    /// 非対話の実行: 常に `never`（`--approval` は `never` のみ、制限モードでは不可）。
    NeverOnly,
    /// ACP: 制限モードは `--approval`（`on-request` または `never`、既定 `on-request`、`untrusted` 不可）、
    /// 通常モードは `--approval` または設定値（既定 `on-request`）。
    Acp,
}

/// 起動とセッションの準備を切り替える五つの独立した方針（設計書 05-03「起動とセッションの準備の方針の分離」）。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PreparationPolicy {
    /// 1. 起動・再開時の確認（プロジェクトの信頼、処理系の変化、モード変更の再開）を画面で尋ねるか。
    pub startup_confirmations: bool,
    /// 2. 実行時の承認の方針の決め方。
    pub runtime_approval: RuntimeApprovalPolicy,
    /// 3. 事前許可のオプション（`--allow`・`--allow-agents-write`・`--project-parent`・`--ephemeral`・`--trust-project`）を受け付けるか。
    pub preapproval_options: bool,
    /// 4. 通常モードの通信の中継（`network = "proxy"`）で未許可ドメインの確認を行うか（`never` でないとき）。
    pub network_proxy_prompts: bool,
    /// 5. 秘密の入力の仲介（`secrets.askpass`）を対話で有効にするか。
    pub askpass: bool,
}

impl Entrypoint {
    /// 入口ごとの準備の方針を返す（設計書 05-03「起動とセッションの準備の方針の分離」）。
    pub const fn policy(self) -> PreparationPolicy {
        match self {
            Self::Tui => PreparationPolicy {
                startup_confirmations: true,
                runtime_approval: RuntimeApprovalPolicy::Tui,
                preapproval_options: false,
                network_proxy_prompts: true,
                askpass: true,
            },
            Self::Exec => PreparationPolicy {
                startup_confirmations: false,
                runtime_approval: RuntimeApprovalPolicy::NeverOnly,
                preapproval_options: true,
                network_proxy_prompts: false,
                askpass: false,
            },
            Self::Acp => PreparationPolicy {
                startup_confirmations: false,
                runtime_approval: RuntimeApprovalPolicy::Acp,
                preapproval_options: true,
                network_proxy_prompts: true,
                askpass: false,
            },
        }
    }
}

/// 起動時に明示された指定。既定の値は config で解決する。
#[derive(Clone, Debug)]
pub struct ExecArgs {
    /// 起動の入口の種類。準備の五つの方針はここから決める（設計書 05-03）。
    pub entrypoint: Entrypoint,
    /// 事前許可でプロジェクトの作成を許す親。起動時に実のパスへ変換する。
    pub project_parent: Option<PathBuf>,
    pub allow: Option<BTreeSet<crate::benitoite::Effect>>,
    /// 事前許可で、.agents に書く要求を確認なしに許す（設計書 03-03「.agents への書き込み」）。
    pub allow_agents_write: bool,
    /// この一回だけプロジェクトを信頼する（設計書 03-01「プロジェクトの信頼」）。
    pub trust_project: bool,
    pub resume: Option<String>,
    pub continue_session: bool,
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
    /// セッションの記録を残さない（設計書 01-05「記録を残さないセッション」）。
    pub ephemeral: bool,
    /// 最後の応答の形を決める JSON Schema のファイル（設計書 05-01「exec の出力」）。
    pub output_schema: Option<PathBuf>,
    /// `debug prompt-input` から使う。最初の要求を作って示し、送らない。
    pub preview: bool,
    pub request: Request,
}

/// 使い方の誤り。呼び出し元は終了状態 2 にする。
#[derive(Debug)]
pub enum ArgsError {
    /// 知らない引数、欠けた値、または不正な値。
    Invalid,
    /// 同じオプションを二度指定した。
    Duplicate(String),
}

impl fmt::Display for ArgsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Invalid => f.write_str(text::INVALID_ARGUMENTS),
            Self::Duplicate(option) => f.write_str(&text::duplicate_option(option)),
        }
    }
}
impl std::error::Error for ArgsError {}

impl ExecArgs {
    /// 入口ごとの準備の方針を返す（設計書 05-03「起動とセッションの準備の方針の分離」）。
    pub const fn policy(&self) -> PreparationPolicy {
        self.entrypoint.policy()
    }

    /// `exec` の後ろだけを渡して解釈する。パスの値は非 UTF-8 でも保つ。
    pub fn parse(args: impl IntoIterator<Item = impl Into<OsString>>) -> Result<Self, ArgsError> {
        Self::parse_mode(args, Entrypoint::Exec)
    }

    /// `debug prompt-input` の後ろを解釈する。exec と同じ指定を受け付け、出力と記録の指定と
    /// 既存のセッションの指定を拒む（設計書 05-01「debug prompt-input」）。
    pub fn parse_preview(
        args: impl IntoIterator<Item = impl Into<OsString>>,
    ) -> Result<Self, ArgsError> {
        let mut result = Self::parse_mode(args, Entrypoint::Exec)?;
        if result.json
            || result.output_last_message.is_some()
            || result.ephemeral
            || result.resume.is_some()
            || result.continue_session
        {
            return Err(ArgsError::Invalid);
        }
        result.ephemeral = true;
        result.preview = true;
        Ok(result)
    }

    /// 対話の形の引数を解釈し、非対話専用と後続作業の指定を拒む。
    pub fn parse_interactive(
        args: impl IntoIterator<Item = impl Into<OsString>>,
    ) -> Result<Self, ArgsError> {
        Self::parse_mode(args, Entrypoint::Tui)
    }

    /// `acp` の後ろの引数を解釈する（設計書 05-01「オプション」、05-03「コマンドラインと入出力」）。
    pub fn parse_acp(
        args: impl IntoIterator<Item = impl Into<OsString>>,
    ) -> Result<Self, ArgsError> {
        Self::parse_mode(args, Entrypoint::Acp)
    }

    fn parse_mode(
        args: impl IntoIterator<Item = impl Into<OsString>>,
        entrypoint: Entrypoint,
    ) -> Result<Self, ArgsError> {
        let mut result = Self {
            entrypoint,
            allow: None,
            project_parent: None,
            resume: None,
            continue_session: false,
            sandbox: None,
            approval: None,
            no_sandbox: false,
            allow_agents_write: false,
            trust_project: false,
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
            ephemeral: false,
            output_schema: None,
            preview: false,
            request: Request::Stdin,
        };
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
            let flag = matches!(
                option,
                "--json"
                    | "--continue"
                    | "--no-sandbox"
                    | "--allow-agents-write"
                    | "--trust-project"
                    | "--ephemeral"
            );
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
                    | "--output-schema"
                    | "--sandbox"
                    | "--approval"
                    | "--allow"
                    | "--project-parent"
                    | "--resume"
                    | "--continue"
                    | "--no-sandbox"
                    | "--allow-agents-write"
                    | "--trust-project"
                    | "--ephemeral"
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
                "--output-schema" => result.output_schema = Some(PathBuf::from(value)),
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
                    let mut effects = BTreeSet::new();
                    for name in string()?.split(',') {
                        let effect =
                            crate::benitoite::Effect::from_name(name).ok_or(ArgsError::Invalid)?;
                        if !effects.insert(effect) {
                            return Err(ArgsError::Invalid);
                        }
                    }
                    result.allow = Some(effects);
                }
                "--project-parent" => result.project_parent = Some(PathBuf::from(value)),
                "--resume" => result.resume = Some(string()?.to_owned()),
                "--continue" => result.continue_session = true,
                "--no-sandbox" => result.no_sandbox = true,
                "--allow-agents-write" => result.allow_agents_write = true,
                "--trust-project" => result.trust_project = true,
                "--ephemeral" => result.ephemeral = true,
                _ => return Err(ArgsError::Invalid),
            }
        }
        // 記録を残さないセッションは、既存の記録を続けない。
        if (result.resume.is_some() && result.continue_session)
            || (result.ephemeral && (result.resume.is_some() || result.continue_session))
        {
            return Err(ArgsError::Invalid);
        }
        match entrypoint {
            Entrypoint::Tui => {
                if result.project_parent.is_some()
                    || result.allow.is_some()
                    || result.allow_agents_write
                    || result.trust_project
                    || result.json
                    || result.output_last_message.is_some()
                    || result.ephemeral
                    || result.output_schema.is_some()
                    || request == Some(Request::Stdin)
                {
                    return Err(ArgsError::Invalid);
                }
                result.request = request.unwrap_or(Request::Text(String::new()));
            }
            Entrypoint::Exec => {
                result.request = request.ok_or(ArgsError::Invalid)?;
            }
            Entrypoint::Acp => {
                // ACP のメッセージと役割が重なる指定と、制限モードの untrusted は使い方の誤りとする
                // （設計書 05-01「オプション」、05-03「コマンドラインと入出力」）。
                if result.json
                    || result.output_last_message.is_some()
                    || result.output_schema.is_some()
                    || result.resume.is_some()
                    || result.continue_session
                    || request.is_some()
                    || (result.mode == Some(Mode::Restricted)
                        && result.approval == Some(Approval::Untrusted))
                {
                    return Err(ArgsError::Invalid);
                }
                result.request = Request::Text(String::new());
            }
        }
        Ok(result)
    }
}

fn number(value: &str) -> Result<u64, ArgsError> {
    if !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(ArgsError::Invalid);
    }
    value.parse().map_err(|_| ArgsError::Invalid)
}
