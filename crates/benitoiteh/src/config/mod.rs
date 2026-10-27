//! 利用者設定と信頼しないプロジェクト設定を分けて検証する（設計書 05-01「設定ファイル」「プロジェクトの設定ファイル」）。

pub mod credentials;
pub(crate) mod domain;
mod environment;
pub use environment::child_environment;
pub mod paths;
mod text;
pub mod writable;

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::io::{self, Read};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::cli::args::ExecArgs;
use crate::provider::{AdapterKind, Capabilities, Destination, ReasoningReplay};
use crate::safe_fs::{SafeFsError, ScopeRelPath, ScopeRoot};
use credentials::ApiKeyRef;

/// 子プロセスに渡さない提供者のキーの変数（設計書 04-01「処理系の起動」）。
pub const PROVIDER_KEY_ENV_VARS: &[&str] = &[
    "OPENAI_API_KEY",
    "ANTHROPIC_API_KEY",
    "GEMINI_API_KEY",
    "DEEPSEEK_API_KEY",
];

/// 設定の読み取り・検証の誤り。TOML の入力本文やキーは保持しない。
#[derive(Debug)]
pub enum ConfigError {
    /// 入出力に失敗した場所。
    Io { path: PathBuf, source: io::Error },
    /// safe_fs がプロジェクト内の読み取りを拒否した。
    SafeFs(SafeFsError),
    /// 値を含めない TOML の診断。
    Parse(String),
    /// 設定の値の制約に反した。
    Invalid(&'static str),
    /// 提供者の選択に失敗した。
    Provider(String),
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { path, source } => write!(f, "{}: {source}", path.display()),
            Self::SafeFs(error) => error.fmt(f),
            Self::Parse(message) | Self::Provider(message) => f.write_str(message),
            Self::Invalid(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for ConfigError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::SafeFs(source) => Some(source),
            Self::Parse(_) | Self::Invalid(_) | Self::Provider(_) => None,
        }
    }
}

/// 起動時に固定するモード（設計書 01-02「モードの選び方」）。
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub enum Mode {
    /// Benitoite と範囲内の読み取りだけを許す。
    #[default]
    Restricted,
    /// シェルと一般の編集を持つ。
    Normal,
}

/// 通常モードのサンドボックスの方針。
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub enum Sandbox {
    /// 読み取りのみ。
    ReadOnly,
    /// プロジェクトと一時領域に書ける。
    #[default]
    WorkspaceWrite,
    /// OS のサンドボックスを掛けない。
    DangerFullAccess,
}

impl Sandbox {
    /// 子プロセスの環境と方針の表示に使う名前。
    pub const fn name(self) -> &'static str {
        match self {
            Self::ReadOnly => "read-only",
            Self::WorkspaceWrite => "workspace-write",
            Self::DangerFullAccess => "danger-full-access",
        }
    }
}

/// 通常モードの承認の方針。
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub enum Approval {
    /// コマンドごとに確認する。
    Untrusted,
    /// 要求時に確認する。
    #[default]
    OnRequest,
    /// 確認できないため範囲外を拒否する。
    Never,
}

/// 利用者が機能ごとに上書きした値。省略と false を区別する。
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct CapabilityOverrides {
    pub image_input: Option<bool>,
    pub cache_control: Option<bool>,
    pub strict_tools: Option<bool>,
    pub eager_input_streaming: Option<bool>,
    pub tool_choice_none: Option<bool>,
    pub reasoning_replay: Option<ReasoningReplay>,
    pub parallel_tool_calls: Option<bool>,
    pub reasoning_effort: Option<bool>,
    pub context_window: Option<u64>,
    pub max_output_tokens: Option<u64>,
}

/// API キーの代わりに使う利用者の許可。
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum ProviderAuth {
    /// 保存した ChatGPT の許可を使う。
    Chatgpt,
}

/// 一つの提供者の接続先とモデル。
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderConfig {
    pub kind: AdapterKind,
    pub base_url: Option<String>,
    pub model: String,
    pub effort: Option<String>,
    pub api_key: Option<ApiKeyRef>,
    pub auth: Option<ProviderAuth>,
    #[serde(default)]
    pub capabilities: CapabilityOverrides,
    pub context_window: Option<u64>,
    pub max_output_tokens: Option<u64>,
}

impl ProviderConfig {
    /// 省略されたときはアダプタの本家の URL を使う。
    pub fn base_url(&self) -> &str {
        self.base_url.as_deref().unwrap_or(self.official_base_url())
    }

    fn official_base_url(&self) -> &'static str {
        match self.kind {
            AdapterKind::AnthropicMessages => "https://api.anthropic.com",
            AdapterKind::OpenAiChat | AdapterKind::OpenAiResponses => "https://api.openai.com/v1",
        }
    }

    /// 本家の URL と一致するときだけ本家の機能を採り、明示された値で上書きする。
    pub fn resolved_capabilities(&self) -> Capabilities {
        let mut result = if self.base_url() == self.official_base_url() {
            self.kind
                .official_capabilities()
                .unwrap_or_else(|| self.kind.default_capabilities())
        } else {
            self.kind.default_capabilities()
        };
        let overrides = &self.capabilities;
        if let Some(value) = overrides.image_input {
            result.image_input = value;
        }
        if let Some(value) = overrides.cache_control {
            result.cache_control = value;
        }
        if let Some(value) = overrides.strict_tools {
            result.strict_tools = value;
        }
        if let Some(value) = overrides.eager_input_streaming {
            result.eager_input_streaming = value;
        }
        if let Some(value) = overrides.tool_choice_none {
            result.tool_choice_none = value;
        }
        if let Some(value) = overrides.reasoning_replay {
            result.reasoning_replay = value;
        }
        if let Some(value) = overrides.parallel_tool_calls {
            result.parallel_tool_calls = value;
        }
        if let Some(value) = overrides.reasoning_effort {
            result.reasoning_effort = value;
        }
        result.context_window = self
            .context_window
            .or(overrides.context_window)
            .or(result.context_window);
        result.max_output_tokens = self
            .max_output_tokens
            .or(overrides.max_output_tokens)
            .or(result.max_output_tokens);
        result
    }
}

/// 処理系の場所。
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct BenitoiteConfig {
    pub path: Option<PathBuf>,
}

/// 制限モードで利用者だけが広げられる範囲。
#[derive(Clone, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct RestrictedConfig {
    pub allowed_programs: Vec<PathBuf>,
    /// 既定の効果の集合に追加する効果。
    pub scratch_auto_effects: Vec<String>,
    /// 評価で比較する、書いた後の自動検査。
    pub after_write_check: bool,
}

impl Default for RestrictedConfig {
    fn default() -> Self {
        Self {
            allowed_programs: Vec::new(),
            scratch_auto_effects: Vec::new(),
            after_write_check: true,
        }
    }
}

/// 通常モードの方針。
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct NormalConfig {
    pub sandbox: Sandbox,
    pub approval: Approval,
    pub network: bool,
    pub writable_roots: Vec<PathBuf>,
}

/// 一連の処理の上限。
#[derive(Clone, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Limits {
    pub max_turns: u64,
    pub max_check_fix: u64,
    pub max_run_tokens: Option<u64>,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            max_turns: 200,
            max_check_fix: 10,
            max_run_tokens: None,
        }
    }
}

/// 許可した取得先。
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct WebFetchConfig {
    pub allowed_domains: Vec<String>,
}

/// 読み取りの除外規則。
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ReadConfig {
    pub exclude: Vec<String>,
}

/// 利用者だけが読み取りの表示を切り替えられる設定（設計書 05-01）。
#[derive(Clone, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct UserReadConfig {
    pub exclude: Vec<String>,
    pub line_numbers: bool,
}

impl Default for UserReadConfig {
    fn default() -> Self {
        Self {
            exclude: Vec::new(),
            line_numbers: true,
        }
    }
}

/// 利用者だけが Skill の探索元を切り替えられる設定（設計書 01-04）。
#[derive(Clone, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct UserSkillsConfig {
    pub user: bool,
    pub project: bool,
}

impl Default for UserSkillsConfig {
    fn default() -> Self {
        Self {
            user: true,
            project: true,
        }
    }
}

/// 利用者だけがプラグインの探索元を切り替えられる設定（設計書 01-04）。
#[derive(Clone, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct UserPluginsConfig {
    pub user: bool,
    pub project: bool,
}

impl Default for UserPluginsConfig {
    fn default() -> Self {
        Self {
            user: true,
            project: true,
        }
    }
}

/// セッションを残す期間。
#[derive(Clone, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SessionsConfig {
    pub retention_days: u64,
}
impl Default for SessionsConfig {
    fn default() -> Self {
        Self { retention_days: 30 }
    }
}

/// 利用者だけが選べる秘密の仲介の設定（設計書 03-04「段 2」）。
#[derive(Clone, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SecretsConfig {
    pub askpass: bool,
}
impl Default for SecretsConfig {
    fn default() -> Self {
        Self { askpass: true }
    }
}

/// 利用者だけが選べるループの振る舞い（設計書 01-01「完了の前の確かめ」）。
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct AgentBehaviorConfig {
    pub completion_check: bool,
}

/// 圧縮の予備と残す量。利用者だけが変更できる（設計書 01-04「圧縮の値を比べる設定」）。
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct CompactionConfig {
    /// 出力の上限へ足す予備。加算は飽和させる。
    pub reserve_tokens: u64,
    /// 残す会話の目安。文脈の上限の 4 分の 1 までに抑える。
    pub keep_tokens: u64,
}

impl Default for CompactionConfig {
    fn default() -> Self {
        Self {
            reserve_tokens: 16_384,
            keep_tokens: 20_000,
        }
    }
}

/// 利用者の設定。信頼しないプロジェクトの設定とは別の型にする。
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct UserConfig {
    pub default_mode: Mode,
    pub default_provider: Option<String>,
    pub providers: BTreeMap<String, ProviderConfig>,
    pub benitoite: BenitoiteConfig,
    pub restricted: RestrictedConfig,
    pub normal: NormalConfig,
    pub limits: Limits,
    pub agent: AgentBehaviorConfig,
    pub compaction: CompactionConfig,
    pub web_fetch: WebFetchConfig,
    pub read: UserReadConfig,
    pub skills: UserSkillsConfig,
    pub plugins: UserPluginsConfig,
    pub sessions: SessionsConfig,
    pub secrets: SecretsConfig,
}

impl UserConfig {
    /// 決まった場所のファイルを読み、なければ既定にする。
    pub fn load(path: &Path) -> Result<Self, ConfigError> {
        match std::fs::read_to_string(path) {
            Ok(input) => Self::parse(&input),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Self::default()),
            Err(source) => Err(ConfigError::Io {
                path: path.to_path_buf(),
                source,
            }),
        }
    }

    /// 知らない欄と、秘密の直書きを拒む。
    pub fn parse(input: &str) -> Result<Self, ConfigError> {
        let mut config: Self = parse(input)?;
        if config.limits.max_check_fix == 0 {
            return Err(ConfigError::Invalid(text::CHECK_FIX_POSITIVE));
        }
        for domain in &mut config.web_fetch.allowed_domains {
            *domain = self::domain::normalize(domain)
                .ok_or(ConfigError::Invalid(text::INVALID_DOMAIN))?;
        }
        if config
            .restricted
            .allowed_programs
            .iter()
            .any(|path| !path.is_absolute())
        {
            return Err(ConfigError::Invalid(text::INVALID_PROGRAM));
        }
        if config
            .benitoite
            .path
            .as_ref()
            .is_some_and(|path| !path.is_absolute())
        {
            return Err(ConfigError::Invalid(text::INVALID_BENITOITE_PATH));
        }
        for effect in &config.restricted.scratch_auto_effects {
            if !matches!(
                effect.as_str(),
                "Console.Write"
                    | "State"
                    | "Process.Exit"
                    | "File.Read"
                    | "Log"
                    | "Clock.Time"
                    | "Random.Generate"
                    | "Process.Environment"
            ) {
                return Err(ConfigError::Invalid(text::INVALID_EFFECT));
            }
        }
        for provider in config.providers.values() {
            if provider.auth.is_some()
                && (provider.kind != AdapterKind::OpenAiResponses
                    || provider.base_url.is_some()
                    || provider.api_key.is_some())
            {
                return Err(ConfigError::Invalid(text::CHATGPT_AUTH));
            }
            if provider.model.is_empty()
                || provider.effort.as_deref() == Some("")
                || provider.base_url().is_empty()
            {
                return Err(ConfigError::Invalid(text::INVALID_VALUE));
            }
            if let Some(reference) = &provider.api_key {
                reference.validate()?;
            }
        }
        Ok(config)
    }

    /// 子プロセスから除く、設定中のキーの環境変数名を重複なしで返す。
    pub fn referenced_env_vars(&self) -> BTreeSet<String> {
        self.providers
            .values()
            .filter_map(|provider| provider.api_key.as_ref()?.env_name().map(str::to_owned))
            .collect()
    }

    /// オプション、プロジェクト、利用者、既定の順で exec の値を確定する。
    pub fn resolve_exec(
        &self,
        args: &ExecArgs,
        project: &ProjectConfig,
        cwd: &Path,
    ) -> Result<ResolvedExec, ConfigError> {
        let mode = args.mode.unwrap_or(self.default_mode);
        if args.max_check_fix.unwrap_or(self.limits.max_check_fix) == 0 {
            return Err(ConfigError::Invalid(text::CHECK_FIX_POSITIVE));
        }
        if !args.interactive
            && args
                .approval
                .is_some_and(|approval| approval != Approval::Never)
        {
            return Err(ConfigError::Invalid(text::EXEC_APPROVAL));
        }
        if mode == Mode::Restricted
            && (args.sandbox.is_some() || args.approval.is_some() || args.no_sandbox)
        {
            return Err(ConfigError::Invalid(text::NORMAL_OPTIONS));
        }
        let name = args
            .provider
            .as_ref()
            .or(self.default_provider.as_ref())
            .ok_or(ConfigError::Invalid(text::NO_PROVIDER))?;
        let provider = self
            .providers
            .get(name)
            .ok_or_else(|| ConfigError::Provider(text::unknown_provider(name)))?;
        let model = args
            .model
            .as_ref()
            .or(project.model.as_ref())
            .unwrap_or(&provider.model)
            .clone();
        let mut exclude = self.read.exclude.clone();
        exclude.extend(project.read.exclude.iter().cloned());
        let requested = args.project.as_deref().unwrap_or(cwd);
        let project_dir = if requested.is_absolute() {
            requested.to_path_buf()
        } else {
            cwd.join(requested)
        };
        Ok(ResolvedExec {
            mode,
            project: project_dir,
            destination: Destination {
                provider: name.clone(),
                base_url: provider.base_url().to_owned(),
                model,
            },
            kind: provider.kind,
            capabilities: provider.resolved_capabilities(),
            effort: args
                .effort
                .as_deref()
                .or(provider.effort.as_deref())
                .unwrap_or_else(|| provider.kind.default_effort())
                .to_owned(),
            api_key: provider.api_key.clone(),
            auth: provider.auth,
            limits: Limits {
                max_turns: args.max_turns.unwrap_or(self.limits.max_turns),
                max_check_fix: args.max_check_fix.unwrap_or(self.limits.max_check_fix),
                max_run_tokens: args.max_run_tokens.or(self.limits.max_run_tokens),
            },
            agent: self.agent.clone(),
            compaction: self.compaction,
            read: ReadConfig { exclude },
            read_line_numbers: self.read.line_numbers,
            skills: self.skills.clone(),
            plugins: self.plugins.clone(),
        })
    }

    /// 再開では記録のモード・送り先を既定にし、今の安全の設定で解決する。
    pub fn resolve_resume(
        &self,
        args: &ExecArgs,
        project: &ProjectConfig,
        cwd: &Path,
        state: &crate::session::ResumeState,
        allow_mode_change: bool,
    ) -> Result<ResolvedExec, ConfigError> {
        let mut args = args.clone();
        let recorded_mode = match state.mode {
            crate::session::RecordedMode::Restricted => Mode::Restricted,
            crate::session::RecordedMode::Normal => Mode::Normal,
        };
        args.mode = Some(args.mode.unwrap_or(recorded_mode));
        if recorded_mode == Mode::Restricted
            && args.mode == Some(Mode::Normal)
            && !allow_mode_change
        {
            return Err(ConfigError::Invalid(text::RESUME_MODE));
        }
        let different_provider = args
            .provider
            .as_ref()
            .is_some_and(|name| *name != state.destination.provider);
        if !different_provider {
            args.provider = Some(state.destination.provider.clone());
            if args.model.is_none() && !state.destination.model.is_empty() {
                args.model = Some(state.destination.model.clone());
            }
        }
        args.project = Some(state.project_dir.clone());
        self.resolve_exec(&args, project, cwd)
    }
}

/// exec がアダプタとループに渡す、優先順位を解決済みの設定。
#[derive(Clone, Debug)]
pub struct ResolvedExec {
    pub mode: Mode,
    pub project: PathBuf,
    pub destination: Destination,
    pub kind: AdapterKind,
    pub capabilities: Capabilities,
    pub effort: String,
    pub api_key: Option<ApiKeyRef>,
    pub auth: Option<ProviderAuth>,
    pub limits: Limits,
    pub agent: AgentBehaviorConfig,
    pub compaction: CompactionConfig,
    pub read: ReadConfig,
    pub read_line_numbers: bool,
    pub skills: UserSkillsConfig,
    pub plugins: UserPluginsConfig,
}

/// プロジェクトにはモデルの変更と除外の追加だけを許す。
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ProjectConfig {
    pub model: Option<String>,
    pub read: ReadConfig,
}

/// 設定を読んだ事実と、採用した値または診断。
#[derive(Debug)]
pub struct ProjectConfigLoad {
    /// ファイルを開いて読み取りを試みたか。
    pub read: bool,
    /// 誤りなら既定の空の設定。
    pub config: ProjectConfig,
    /// ファイル全体を採用しなかった理由。
    pub error: Option<ConfigError>,
}

impl ProjectConfig {
    /// プロジェクトの根の記述子からだけ読み、誤りなら全体を捨てて続ける。
    pub fn load(root: &ScopeRoot) -> ProjectConfigLoad {
        let mut read = false;
        let result = (|| {
            let path = ScopeRelPath::new(".benitoiteh/config.toml").map_err(ConfigError::SafeFs)?;
            let mut file = match root.open_read(&path) {
                Ok(file) => file,
                Err(SafeFsError::NotFound(_)) => return Ok(Self::default()),
                Err(error) => return Err(ConfigError::SafeFs(error)),
            };
            read = true;
            let mut input = String::new();
            file.read_to_string(&mut input)
                .map_err(|source| ConfigError::Io {
                    path: root.real_path().join(path.as_path()),
                    source,
                })?;
            parse(&input)
        })();
        match result {
            Ok(config) => ProjectConfigLoad {
                read,
                config,
                error: None,
            },
            Err(error) => ProjectConfigLoad {
                read,
                config: Self::default(),
                error: Some(error),
            },
        }
    }
}

fn parse<T: serde::de::DeserializeOwned>(input: &str) -> Result<T, ConfigError> {
    toml::from_str(input).map_err(|error| {
        // toml::de::Error の Display は入力行を含む。キーの直書きの診断でも漏らさない。
        let message = error.message();
        let diagnostic = message
            .strip_prefix("unknown field `")
            .and_then(|rest| rest.split_once('`'))
            .map(|(field, _)| text::unknown_field(field))
            .unwrap_or_else(|| text::INVALID_CONFIG.to_owned());
        ConfigError::Parse(diagnostic)
    })
}
