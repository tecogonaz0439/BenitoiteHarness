//! 設定・認証の参照・保存先とサンドボックスを通信せずに診断する（設計書 05-01「doctor」）。

mod probe;
mod processor;
mod text;

use std::collections::BTreeSet;
use std::ffi::OsString;
use std::fs;
use std::io::{self, Write};
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

use serde::Serialize;

use super::{CliError, exec::StartupError};
use crate::config::credentials::{ApiKeyRef, CredentialError};
use crate::config::paths::DataPaths;
use crate::config::writable::validate_writable_with_executable;
use crate::config::{ProjectConfig, ProviderConfig, UserConfig};
use crate::safe_fs::ScopeRoot;
use crate::{sandbox, session, ui};
use probe::Probe;

#[derive(Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
enum Status {
    Ok,
    Unavailable,
    Unchecked,
    Warning,
}

impl Status {
    fn name(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::Unavailable => "unavailable",
            Self::Unchecked => "unchecked",
            Self::Warning => "warning",
        }
    }
}

#[derive(Serialize)]
struct Item {
    #[serde(rename = "type")]
    kind: &'static str,
    name: String,
    status: Status,
    detail: String,
    #[serde(rename = "default", skip_serializing_if = "Option::is_none")]
    is_default: Option<bool>,
}

impl Item {
    fn new(name: &str, status: Status, detail: impl Into<String>) -> Self {
        Self {
            kind: "item",
            name: ui::terminal_text(name, false),
            status,
            detail: ui::terminal_text(&detail.into(), false),
            is_default: None,
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
enum NormalSandbox {
    Enabled,
    DisabledByConfig,
    Unavailable,
}

#[derive(Serialize)]
struct Summary {
    #[serde(rename = "type")]
    kind: &'static str,
    restricted_sandbox: bool,
    #[serde(skip)]
    processor_validated: bool,
    normal_sandbox: NormalSandbox,
    exit_status: u8,
}

impl Summary {
    fn restricted_detail(&self, processor_checked: bool) -> &'static str {
        if !self.restricted_sandbox {
            text::RESTRICTED_UNAVAILABLE
        } else if self.processor_validated {
            text::RESTRICTED_OK
        } else if !processor_checked {
            text::RESTRICTED_UNCHECKED_OK
        } else {
            text::RESTRICTED_FALLBACK_OK
        }
    }
}

/// 診断を一度行い、項目と要約を出力して終了状態を返す。
pub(super) fn run(args: impl Iterator<Item = OsString>) -> Result<u8, CliError> {
    let (project, json) = arguments(args)?;
    let project_from_option = project.is_some();
    let project = project
        .map(Ok)
        .unwrap_or_else(std::env::current_dir)
        .map_err(|error| CliError::Startup(StartupError::Io(error)))?;
    let root = ScopeRoot::open(&project)
        .map_err(|error| CliError::Startup(StartupError::SafeFs(error)))?;
    let mut items = Vec::new();
    let paths = DataPaths::from_env();
    let config = match &paths {
        Ok(paths) => match UserConfig::load(&paths.config_file) {
            Ok(config) => {
                items.push(Item::new(
                    "config.user",
                    Status::Ok,
                    text::at(
                        &paths.config_file,
                        if paths.config_file.exists() {
                            text::LOADED
                        } else {
                            text::DEFAULTS
                        },
                    ),
                ));
                Some(config)
            }
            Err(error) => {
                items.push(Item::new(
                    "config.user",
                    Status::Unavailable,
                    error.to_string(),
                ));
                None
            }
        },
        Err(error) => {
            items.push(Item::new(
                "config.user",
                Status::Unavailable,
                error.to_string(),
            ));
            None
        }
    };
    let project_config = ProjectConfig::load(&root);
    items.push(match project_config.error {
        Some(error) => Item::new("config.project", Status::Warning, text::ignored(&error)),
        None => Item::new(
            "config.project",
            Status::Ok,
            if project_config.read {
                text::LOADED
            } else {
                text::DEFAULTS
            },
        ),
    });
    providers(&mut items, config.as_ref(), paths.as_ref().ok());
    let probe = paths
        .as_ref()
        .map_err(ToString::to_string)
        .and_then(|paths| Probe::new(paths, root.real_path()).map_err(|error| error.to_string()));
    let processor = processor::prepare(
        &mut items,
        paths.as_ref().ok(),
        root.real_path(),
        config.as_ref(),
        probe.as_ref().map_err(String::as_str),
    );
    let restricted = match &probe {
        Ok(probe) => probe.check(&mut items, processor.as_ref().ok()),
        Err(reason) => {
            #[cfg(target_os = "macos")]
            let names = ["sandbox.sandbox_exec"];
            #[cfg(target_os = "linux")]
            let names = [
                "sandbox.bubblewrap",
                "sandbox.user_namespaces",
                "sandbox.landlock",
                "sandbox.seccomp",
                "sandbox.capabilities",
            ];
            for name in names {
                items.push(Item::new(name, Status::Unchecked, reason.clone()));
            }
            items.push(Item::new(
                "sandbox.restricted_rule",
                if paths.is_ok() {
                    Status::Unavailable
                } else {
                    Status::Unchecked
                },
                reason.clone(),
            ));
            #[cfg(target_os = "linux")]
            items.push(Item::new(
                "sandbox.linux_scope",
                Status::Warning,
                text::LINUX_SCOPE,
            ));
            false
        }
    };
    let processor_validated = processor.is_ok();
    processor::finish(&mut items, processor, restricted);
    let normal = match (&paths, &probe) {
        (Ok(paths), Ok(probe)) => normal(
            &mut items,
            paths,
            root.real_path(),
            project_from_option,
            probe,
            config.as_ref().unwrap_or(&UserConfig::default()),
            restricted,
        ),
        (Ok(_) | Err(_), Ok(_) | Err(_)) => {
            let reason = probe
                .as_ref()
                .err()
                .map_or(text::CONFIG_REQUIRED, String::as_str);
            for name in ["writable", "sandbox.normal"] {
                items.push(Item::new(name, Status::Unchecked, reason));
            }
            NormalSandbox::Unavailable
        }
    };
    data(&mut items, paths.as_ref().ok());
    // 出力の失敗の経路でも、一時領域を出力より前に片付ける。
    drop(probe);
    let summary = Summary {
        kind: "summary",
        restricted_sandbox: restricted,
        processor_validated,
        normal_sandbox: normal,
        exit_status: exit_status(items.iter().map(|item| item.status)),
    };
    output(&items, &summary, json).map_err(CliError::Output)?;
    Ok(summary.exit_status)
}

fn arguments(args: impl Iterator<Item = OsString>) -> Result<(Option<PathBuf>, bool), CliError> {
    let mut args = args;
    let mut project = None;
    let mut json = false;
    let mut seen = BTreeSet::new();
    while let Some(arg) = args.next() {
        if !seen.insert(arg.clone()) {
            return Err(CliError::Usage);
        }
        if arg == "--project" {
            let value = args.next().ok_or(CliError::Usage)?;
            if value.to_string_lossy().starts_with("--") {
                return Err(CliError::Usage);
            }
            project = Some(PathBuf::from(value));
        } else if arg == "--json" {
            json = true;
        } else {
            return Err(CliError::Usage);
        }
    }
    Ok((project, json))
}

fn exit_status(statuses: impl IntoIterator<Item = Status>) -> u8 {
    u8::from(
        statuses
            .into_iter()
            .any(|status| status == Status::Unavailable),
    )
}

fn providers(items: &mut Vec<Item>, config: Option<&UserConfig>, paths: Option<&DataPaths>) {
    let Some(config) = config else {
        items.push(Item::new(
            "provider",
            Status::Unchecked,
            text::CONFIG_REQUIRED,
        ));
        return;
    };
    if config.providers.is_empty() {
        items.push(Item::new(
            "provider",
            Status::Unavailable,
            text::NO_PROVIDERS,
        ));
        return;
    }
    if !config
        .default_provider
        .as_ref()
        .is_some_and(|name| config.providers.contains_key(name))
    {
        items.push(Item::new(
            "provider",
            Status::Unavailable,
            text::INVALID_DEFAULT,
        ));
    }
    for (name, provider) in &config.providers {
        let default = config.default_provider.as_ref() == Some(name);
        let kind = match provider.kind {
            crate::provider::AdapterKind::AnthropicMessages => "anthropic-messages",
            crate::provider::AdapterKind::OpenAiChat => "openai-chat",
            crate::provider::AdapterKind::OpenAiResponses => "openai-responses",
        };
        let mut detail = text::provider(
            kind,
            &safe_url(provider.base_url()),
            &provider.model,
            default,
        );
        let (mut status, key) = if provider.auth.is_some() {
            chatgpt_credential(paths, default)
        } else {
            let (status, key) = credential(provider);
            (
                if status == Status::Unavailable && !default {
                    Status::Warning
                } else {
                    status
                },
                key,
            )
        };
        detail.push_str(&format!("; {key}"));
        if provider.resolved_capabilities().context_window.is_none() {
            if status == Status::Ok {
                status = Status::Warning;
            }
            detail.push_str(&format!("; {}", text::NO_CONTEXT));
        }
        let mut item = Item::new(&format!("provider.{name}"), status, detail);
        item.is_default = Some(default);
        items.push(item);
    }
}

fn chatgpt_credential(paths: Option<&DataPaths>, default: bool) -> (Status, String) {
    let Some(paths) = paths else {
        return (Status::Unchecked, text::CONFIG_REQUIRED.into());
    };
    let missing = || {
        (
            if default {
                Status::Unavailable
            } else {
                Status::Warning
            },
            text::CHATGPT_MISSING.into(),
        )
    };
    // doctor は保存を新設せず、更新の通信も行わない（設計書 05-01「doctor」）。
    match fs::symlink_metadata(&paths.auth_dir) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => return missing(),
        Err(_) => return (Status::Unavailable, text::CHATGPT_STORAGE.into()),
        Ok(_) => {}
    }
    let saved = crate::auth::Store::open(&paths.auth_dir).and_then(|store| store.load());
    match saved {
        Ok(Some(saved)) => {
            let Some(tokens) = saved.tokens else {
                return missing();
            };
            let email = saved.account.and_then(|account| account.email);
            (
                Status::Ok,
                text::chatgpt(
                    email.as_deref(),
                    tokens.expires_at,
                    tokens
                        .scopes
                        .iter()
                        .any(|scope| scope == "chatgpt.tokens.use.direct"),
                ),
            )
        }
        Ok(None) => missing(),
        Err(_) => (Status::Unavailable, text::CHATGPT_STORAGE.into()),
    }
}

fn credential(provider: &ProviderConfig) -> (Status, String) {
    let Some(reference) = &provider.api_key else {
        return (Status::Ok, text::NO_KEY.to_owned());
    };
    // CredentialError の Display は stderr を含む。ここでは種類だけに落とす。
    // 得たキーも長さも保持しない（設計書 02-01「認証の情報」）。
    let reason = match reference.resolve() {
        Ok(key) => {
            drop(key);
            return (Status::Ok, text::KEY_OK.to_owned());
        }
        Err(CredentialError::Environment) => match reference {
            ApiKeyRef::Env(name) => text::environment(name),
            ApiKeyRef::Command(_) => text::KEY_INVALID.to_owned(),
        },
        Err(CredentialError::Spawn(error)) => text::spawn(error.kind()),
        Err(CredentialError::Command { status, .. }) => text::command(status),
        Err(CredentialError::Output) => text::KEY_OUTPUT.to_owned(),
        Err(CredentialError::InvalidReference) => text::KEY_INVALID.to_owned(),
    };
    (Status::Unavailable, reason)
}

fn safe_url(url: &str) -> String {
    let Ok(uri) = url.parse::<crate::http::Uri>() else {
        return text::INVALID_URL.to_owned();
    };
    let (Some(scheme), Some(host), Some(authority)) = (uri.scheme(), uri.host(), uri.authority())
    else {
        return text::INVALID_URL.to_owned();
    };
    // Uri は user:1234?@host を host=user・port=1234 と問い合わせに分けて受理する。
    // authority の外にも @ がある入力は、壊れた利用者情報と区別せず伏せる。
    // 解析済みの要素だけで表示を作り、秘密の解釈の食い違いを避ける（実装プラン P4-05）。
    let at_signs = |value: &str| value.bytes().filter(|byte| *byte == b'@').count();
    if at_signs(url) != at_signs(authority.as_str()) {
        return text::INVALID_URL.to_owned();
    }
    let port = uri
        .port()
        .map(|port| format!(":{port}"))
        .unwrap_or_default();
    format!("{scheme}://{host}{port}{}", uri.path())
}

fn normal(
    items: &mut Vec<Item>,
    paths: &DataPaths,
    project: &Path,
    project_from_option: bool,
    probe: &Probe,
    config: &UserConfig,
    restricted: bool,
) -> NormalSandbox {
    let locations = match validate_writable_with_executable(
        paths,
        project,
        &probe.write,
        &config.normal.writable_roots,
        &probe.helper,
    ) {
        Ok(locations) => {
            items.push(Item::new("writable", Status::Ok, text::WRITABLE_OK));
            locations
        }
        Err(error) => {
            items.push(Item::new(
                "writable",
                Status::Unavailable,
                text::writable_error(project, project_from_option, &error),
            ));
            items.push(Item::new(
                "sandbox.normal",
                Status::Unchecked,
                text::WRITABLE_REQUIRED,
            ));
            return NormalSandbox::Unavailable;
        }
    };
    match sandbox::normal_policy(
        config.normal.sandbox,
        &locations,
        config.normal.network,
        &super::denied_candidates(paths),
    ) {
        Ok(None) => {
            items.push(Item::new("sandbox.normal", Status::Warning, text::DISABLED));
            NormalSandbox::DisabledByConfig
        }
        Ok(Some(_)) if restricted => {
            items.push(Item::new("sandbox.normal", Status::Ok, text::NORMAL_OK));
            NormalSandbox::Enabled
        }
        Ok(Some(_)) => {
            items.push(Item::new(
                "sandbox.normal",
                Status::Unavailable,
                text::NO_SANDBOX,
            ));
            NormalSandbox::Unavailable
        }
        Err(error) => {
            items.push(Item::new(
                "sandbox.normal",
                Status::Unavailable,
                format!("{error} {}", text::NO_SANDBOX),
            ));
            NormalSandbox::Unavailable
        }
    }
}

fn directory(items: &mut Vec<Item>, name: &str, path: &Path) -> (bool, bool) {
    let (status, detail, exists) = match fs::symlink_metadata(path) {
        Ok(metadata)
            if metadata.is_dir()
                && metadata.mode() & 0o7777 == 0o700
                && metadata.uid() == rustix::process::geteuid().as_raw() =>
        {
            (Status::Ok, text::DIRECTORY_OK.to_owned(), true)
        }
        Ok(_) => (Status::Unavailable, text::DIRECTORY_UNSAFE.to_owned(), true),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            (Status::Ok, text::DIRECTORY_MISSING.to_owned(), false)
        }
        Err(error) => (Status::Unavailable, error.to_string(), true),
    };
    items.push(Item::new(name, status, text::at(path, &detail)));
    (exists, status == Status::Ok)
}

fn data(items: &mut Vec<Item>, paths: Option<&DataPaths>) {
    let Some(paths) = paths else {
        for name in [
            "data.sessions",
            #[cfg(target_os = "linux")]
            "data.placeholders",
            "data.history",
            "data.storage",
        ] {
            items.push(Item::new(name, Status::Unchecked, text::CONFIG_REQUIRED));
        }
        return;
    };
    let (exists, safe) = directory(items, "data.sessions", &paths.sessions_dir);
    #[cfg(target_os = "linux")]
    directory(
        items,
        "data.placeholders",
        &paths.data_dir.join("placeholders"),
    );
    directory(items, "data.history", &paths.data_dir.join("history"));
    let (status, detail) = if !safe {
        (Status::Unchecked, text::STORAGE_REQUIRED.to_owned())
    } else {
        let usage = if exists {
            session::storage_usage(paths, None).map(|usage| usage.total)
        } else {
            Ok(0)
        };
        match usage {
            Ok(total) => (
                if total > session::TOTAL_LIMIT {
                    Status::Warning
                } else {
                    Status::Ok
                },
                text::storage(total, session::TOTAL_LIMIT),
            ),
            Err(error) => (Status::Unavailable, error.to_string()),
        }
    };
    items.push(Item::new("data.storage", status, detail));
}

fn output(items: &[Item], summary: &Summary, json: bool) -> io::Result<()> {
    let mut stdout = io::stdout().lock();
    for item in items {
        if json {
            serde_json::to_writer(&mut stdout, item).map_err(io::Error::other)?;
            writeln!(stdout)?;
        } else {
            writeln!(
                stdout,
                "{}",
                text::item(item.status.name(), &item.name, &item.detail)
            )?;
        }
    }
    if json {
        serde_json::to_writer(&mut stdout, summary).map_err(io::Error::other)?;
        writeln!(stdout)?;
    } else {
        // 探索前の失敗を「見つからなかった」と示さない（実装プラン P4-12）。
        let processor_checked = items
            .iter()
            .any(|item| item.name == "benitoite.discovery" && item.status != Status::Unchecked);
        writeln!(stdout, "{}", summary.restricted_detail(processor_checked))?;
        writeln!(
            stdout,
            "{}",
            match summary.normal_sandbox {
                NormalSandbox::Enabled => text::NORMAL_ENABLED,
                NormalSandbox::DisabledByConfig => text::NORMAL_DISABLED,
                NormalSandbox::Unavailable => text::NORMAL_UNAVAILABLE,
            }
        )?;
    }
    stdout.flush()
}

#[cfg(test)]
mod tests {
    //! 環境の機能に依らない終了状態の契約（設計書 05-01「doctor」）。
    use super::{Item, NormalSandbox, Status, Summary, exit_status, text};

    // 作成時の関門: 処理系を確かめていない成功を、通常の成功と区別する。
    // JSON の restricted_sandbox と行の形は変えない。
    #[test]
    fn restricted_summary_distinguishes_the_self_probe_without_changing_json()
    -> Result<(), serde_json::Error> {
        for (restricted, processor_validated, processor_checked, expected) in [
            (true, true, true, text::RESTRICTED_OK),
            (true, false, true, text::RESTRICTED_FALLBACK_OK),
            (true, false, false, text::RESTRICTED_UNCHECKED_OK),
            (false, false, false, text::RESTRICTED_UNAVAILABLE),
        ] {
            let summary = Summary {
                kind: "summary",
                restricted_sandbox: restricted,
                processor_validated,
                normal_sandbox: NormalSandbox::Unavailable,
                exit_status: 1,
            };
            assert_eq!(summary.restricted_detail(processor_checked), expected);
            let json = serde_json::to_value(&summary)?;
            assert_eq!(
                json.get("restricted_sandbox"),
                Some(&serde_json::Value::from(restricted))
            );
            assert_eq!(json.as_object().map(|object| object.len()), Some(4));
            assert!(json.get("processor_validated").is_none());
        }
        Ok(())
    }

    // 作成時の関門: 一行の診断に U+2028・U+2029 や巨大な stderr を混ぜる退行を、
    // 文と JSON に共通する項目の境界で捕まえる。
    #[test]
    fn processor_probe_failure_is_bounded_and_escapes_unicode_line_separators()
    -> Result<(), serde_json::Error> {
        let stderr = format!("a\u{2028}b\u{2029}c{}", "x".repeat(65536));
        let item = Item::new(
            "sandbox.restricted_rule",
            Status::Unavailable,
            text::probe_failed(text::PROCESSOR_POSITIVE_FAILED, &stderr),
        );
        let plain = text::item(item.status.name(), &item.name, &item.detail);
        let json = serde_json::to_string(&item)?;
        for output in [&plain, &json] {
            assert!(!output.contains(['\u{2028}', '\u{2029}']));
            assert!(output.len() < 9000);
        }
        assert!(plain.contains("a\\u{2028}b\\u{2029}c"));
        Ok(())
    }

    // 作成時の関門: 警告を失敗とする退行は、実機の成功試験だけでは拾えない。
    #[test]
    fn only_unavailable_items_fail_the_command() {
        assert_eq!(exit_status([Status::Unchecked, Status::Warning]), 0);
        assert_eq!(exit_status([Status::Ok]), 0);
        assert_eq!(
            exit_status([Status::Warning, Status::Unavailable, Status::Unchecked]),
            1
        );
    }
}
