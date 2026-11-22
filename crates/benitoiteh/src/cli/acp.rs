//! `benitoiteh acp` サブコマンドの CLI 入口とセッション準備の接続（設計書 05-03）。

use std::io;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::Ordering;

use crate::benitoite::QueryLauncher;
use crate::config::paths::DataPaths;
use crate::config::{Approval, ProjectConfig, ProviderAuth, UserConfig};
use crate::provider::AdapterKind;
use crate::sandbox;
use crate::ui::acp::{
    AcpBackend, AuthMethod, BackendError, InitializeInfo, PreparedSession, text as acp_text,
};

use super::{CliError, args::ExecArgs, exec, panic, signals::Signals};

struct CliAcpBackend {
    args: ExecArgs,
    paths: DataPaths,
}

impl CliAcpBackend {
    fn resolve_initial_user(
        &self,
    ) -> Result<(UserConfig, crate::config::ResolvedExec), BackendError> {
        let user = UserConfig::load(&self.paths.config_file)
            .map_err(|error| BackendError::server(error.to_string(), Vec::new()))?;
        let cwd = std::env::current_dir()
            .map_err(|error| BackendError::server(error.to_string(), Vec::new()))?;
        let resolved = user
            .resolve_exec(&self.args, &ProjectConfig::default(), &cwd)
            .map_err(|error| BackendError::server(error.to_string(), Vec::new()))?;
        Ok((user, resolved))
    }

    fn prepare_args(&self, args: &ExecArgs) -> Result<PreparedSession, BackendError> {
        let prepared = exec::with_noninteractive_prompts(|prompts| {
            exec::prepare(
                args,
                None,
                &self.paths,
                sandbox::check_availability_with_bubblewrap,
                QueryLauncher::Sandboxed,
                prompts,
            )
        })
        .map_err(|failure| BackendError::server(failure.error.to_string(), failure.diagnostics))?;
        let supports_effort = matches!(
            prepared.kind,
            AdapterKind::AnthropicMessages | AdapterKind::OpenAiResponses
        ) || (prepared.kind == AdapterKind::OpenAiChat
            && prepared.capabilities.reasoning_effort);
        let panic_record = prepared.panic_record;
        Ok(PreparedSession {
            agent: prepared.agent,
            session: prepared.session,
            image_input: prepared.capabilities.image_input,
            supports_effort,
            context_window: prepared.capabilities.context_window,
            interactive_approvals: prepared.approval != Approval::Never,
            diagnostics: prepared.diagnostics,
            on_activate: Some(Box::new(move || {
                panic::install(Some(panic_record), None);
            })),
        })
    }
}

impl AcpBackend for CliAcpBackend {
    fn initialize_info(&mut self) -> Result<InitializeInfo, BackendError> {
        let (_, resolved) = self.resolve_initial_user()?;
        let auth_methods = if resolved.auth == Some(ProviderAuth::Chatgpt) {
            vec![AuthMethod {
                id: acp_text::AUTH_CHATGPT_ID,
                name: acp_text::AUTH_CHATGPT_NAME,
                description: acp_text::AUTH_CHATGPT_DESCRIPTION,
            }]
        } else if resolved.api_key.is_some() {
            vec![AuthMethod {
                id: acp_text::AUTH_API_KEY_ID,
                name: acp_text::AUTH_API_KEY_NAME,
                description: acp_text::AUTH_API_KEY_DESCRIPTION,
            }]
        } else {
            Vec::new()
        };
        Ok(InitializeInfo {
            image_input: resolved.capabilities.image_input,
            auth_methods,
        })
    }

    fn authenticate(&mut self, method_id: &str) -> Result<(), BackendError> {
        let (_, resolved) = self.resolve_initial_user()?;
        if resolved.auth == Some(ProviderAuth::Chatgpt) {
            if method_id != acp_text::AUTH_CHATGPT_ID {
                return Err(BackendError::invalid_params(acp_text::UNKNOWN_AUTH_METHOD));
            }
            let _ = exec::chatgpt_tokens(&resolved, &self.paths)
                .map_err(|error| BackendError::server(error.to_string(), Vec::new()))?;
            return Ok(());
        }
        if let Some(api_key) = &resolved.api_key {
            if method_id != acp_text::AUTH_API_KEY_ID {
                return Err(BackendError::invalid_params(acp_text::UNKNOWN_AUTH_METHOD));
            }
            let _ = api_key
                .resolve()
                .map_err(|error| BackendError::server(error.to_string(), Vec::new()))?;
            return Ok(());
        }
        Err(BackendError::invalid_params(acp_text::UNKNOWN_AUTH_METHOD))
    }

    fn new_session(&mut self, cwd: &Path) -> Result<PreparedSession, BackendError> {
        let mut args = self.args.clone();
        args.resume = None;
        args.continue_session = false;
        if args.project.is_none() {
            args.project = Some(cwd.to_path_buf());
        }
        self.prepare_args(&args)
    }

    fn load_session(
        &mut self,
        session_id: &str,
        cwd: &Path,
    ) -> Result<PreparedSession, BackendError> {
        let mut args = self.args.clone();
        args.resume = Some(session_id.to_owned());
        args.continue_session = false;
        // session/load では記録のプロジェクトの実パスと params.cwd の実パスの一致を必ず検査する（設計書 05-03）。
        args.project = Some(cwd.to_path_buf());
        self.prepare_args(&args)
    }

    fn on_session_closed(&mut self) {
        panic::install(None, None);
    }
}

pub(super) fn run(args: ExecArgs) -> Result<u8, CliError> {
    let signals =
        Signals::new().map_err(|error| CliError::Startup(exec::StartupError::Io(error)))?;
    let paths = DataPaths::from_env()
        .map_err(|error| CliError::Startup(exec::StartupError::Config(error)))?;
    // JSON-RPC に入る前に、設定ファイルの誤りや制限モードでの不正な組み合わせを標準エラー出力で拒む。
    let user = UserConfig::load(&paths.config_file)
        .map_err(|error| CliError::Startup(exec::StartupError::Config(error)))?;
    let cwd = std::env::current_dir()
        .map_err(|error| CliError::Startup(exec::StartupError::Io(error)))?;
    let _resolved = user
        .resolve_exec(&args, &ProjectConfig::default(), &cwd)
        .map_err(|error| CliError::Startup(exec::StartupError::Config(error)))?;

    let cancelled = Arc::clone(&signals.cancelled);
    let mut backend = CliAcpBackend { args, paths };
    let result = crate::ui::acp::run(
        io::stdin(),
        io::stdout(),
        io::stderr(),
        &mut backend,
        Arc::clone(&cancelled),
    );
    panic::install(None, None);
    result.map_err(CliError::Output)?;
    Ok(if cancelled.load(Ordering::Acquire) {
        130
    } else {
        0
    })
}
