//! 設定の信頼境界、優先順位、秘密の除外を公開 API で確かめる（設計書 05-01、02-01「認証の情報」）。

use std::error::Error;
use std::fs;
use std::io;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use benitoiteh::cli::args::ExecArgs;
use benitoiteh::config::credentials::ApiKeyRef;
use benitoiteh::config::paths::DataPaths;
use benitoiteh::config::writable::{WritableError, WritableRule, validate_writable};
use benitoiteh::config::{Mode, ProjectConfig, UserConfig};
use benitoiteh::provider::{AdapterKind, ReasoningReplay};

type TestResult = Result<(), Box<dyn Error>>;

struct Fixture(PathBuf);
static FIXTURE_COUNTER: AtomicU64 = AtomicU64::new(0);

impl Fixture {
    fn new() -> Result<Self, Box<dyn Error>> {
        let nonce = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
        let counter = FIXTURE_COUNTER.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "benitoiteh-config-{}-{nonce}-{counter}",
            std::process::id()
        ));
        fs::create_dir(&path)?;
        Ok(Self(fs::canonicalize(path)?))
    }

    fn dir(&self, name: &str) -> Result<PathBuf, io::Error> {
        let path = self.0.join(name);
        fs::create_dir_all(&path)?;
        Ok(path)
    }

    fn paths(&self) -> Result<DataPaths, Box<dyn Error>> {
        let home = self.dir("home")?;
        Ok(DataPaths::from_values(
            Some(&home),
            Some(&self.0.join("config")),
            Some(&self.0.join("state")),
            Some(&self.0.join("tmp")),
        )?)
    }

    fn script(&self, name: &str, body: &str) -> Result<PathBuf, io::Error> {
        let path = self.0.join(name);
        fs::write(&path, format!("#!/bin/sh\n{body}"))?;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700))?;
        Ok(path)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        if let Err(error) = fs::remove_dir_all(&self.0) {
            eprintln!("Could not remove config test directory: {error}");
        }
    }
}

fn provider(
    config: &UserConfig,
    name: &str,
) -> Result<benitoiteh::config::ProviderConfig, io::Error> {
    config
        .providers
        .get(name)
        .cloned()
        .ok_or_else(|| io::Error::from(io::ErrorKind::NotFound))
}

// 作成時の関門: 公式の例を実際に読み、危険な未知の欄や認証の直書きを受理する退行を捕まえる。
// 設定の所有者の公開 API だけを使う。exec の接続前なので、この境界に主のテストを置く（P1-04）。
#[test]
fn user_configuration_reads_the_design_example_and_rejects_invalid_fields() -> TestResult {
    let fixture = Fixture::new()?;
    let design = include_str!("../../../docs/design/05-interface/05-01-cli.md");
    let example = design
        .split_once("```toml\n")
        .and_then(|(_, tail)| tail.split_once("```"))
        .map(|(example, _)| example)
        .ok_or_else(|| io::Error::from(io::ErrorKind::InvalidData))?;
    let path = fixture.0.join("config.toml");
    fs::write(&path, example)?;
    let config = UserConfig::load(&path)?;
    assert_eq!(config.default_mode, Mode::Restricted);
    assert_eq!(config.default_provider.as_deref(), Some("anthropic"));
    assert_eq!(config.providers.len(), 3);
    assert_eq!(
        provider(&config, "anthropic")?.kind,
        AdapterKind::AnthropicMessages
    );
    assert_eq!(
        provider(&config, "local")?
            .resolved_capabilities()
            .context_window,
        Some(131072)
    );
    assert_eq!(
        config.referenced_env_vars().into_iter().collect::<Vec<_>>(),
        ["ANTHROPIC_API_KEY", "DEEPSEEK_API_KEY"]
    );
    assert_eq!(config.web_fetch.allowed_domains, ["docs.rs"]);
    assert_eq!(config.sessions.retention_days, 30);

    for (input, field) in [
        ("unexpected = true", Some("unexpected")),
        ("[normal]\nnetwrok = true", Some("netwrok")),
        (
            "[providers.p]\nkind='openai-chat'\nmodel='m'\ncapabilities={strcit_tools=true}",
            Some("strcit_tools"),
        ),
        ("[providers.p]\nkind='not-an-adapter'\nmodel='m'", None),
        (
            "[providers.p]\nkind='openai-chat'\nmodel='m'\napi_key='secret-literal'",
            None,
        ),
        (
            "[providers.p]\nkind='openai-chat'\nmodel='m'\napi_key={value='secret-literal'}",
            Some("value"),
        ),
        (
            "[providers.p]\nkind='openai-chat'\nmodel='m'\napi_key={env='KEY',command=['secret-literal']}",
            None,
        ),
        (
            "[providers.p]\nkind='openai-chat'\nmodel='m'\napi_key={command=[]}",
            None,
        ),
        ("[restricted]\nscratch_auto_effects=['File.Write']", None),
        ("[restricted]\nallowed_programs=['relative/program']", None),
    ] {
        let error = UserConfig::parse(input)
            .err()
            .ok_or_else(|| io::Error::from(io::ErrorKind::InvalidData))?;
        let diagnostic = format!("{error} {error:?}");
        assert!(!diagnostic.contains("secret-literal"));
        if let Some(field) = field {
            assert!(diagnostic.contains(field), "{diagnostic}");
        }
    }
    Ok(())
}

// 作成時の関門: 互換の URL に本家の機能を付ける、または明示の false を失う退行を捕まえる。
#[test]
fn capabilities_use_official_defaults_only_at_the_exact_official_destination() -> TestResult {
    let config = UserConfig::parse(
        r#"
        [providers.official]
        kind = "anthropic-messages"
        model = "m"
        capabilities = { cache_control = false }
        [providers.compatible]
        kind = "anthropic-messages"
        base_url = "https://compatible.example/anthropic"
        model = "m"
        context_window = 12345
        max_output_tokens = 4096
        capabilities = { strict_tools = true, reasoning_replay = "all-turns" }
        [providers.chat]
        kind = "openai-chat"
        model = "m"
        [providers.responses]
        kind = "openai-responses"
        model = "m"
    "#,
    )?;
    let official = provider(&config, "official")?;
    assert_eq!(official.base_url(), "https://api.anthropic.com");
    assert_eq!(official.effort, "high");
    assert!(!official.resolved_capabilities().cache_control);
    assert!(official.resolved_capabilities().tool_choice_none);
    let compatible = provider(&config, "compatible")?.resolved_capabilities();
    assert!(!compatible.cache_control);
    assert!(!compatible.tool_choice_none);
    assert!(!compatible.parallel_tool_calls);
    assert!(compatible.strict_tools);
    assert_eq!(compatible.reasoning_replay, ReasoningReplay::AllTurns);
    assert_eq!(compatible.context_window, Some(12345));
    assert_eq!(compatible.max_output_tokens, Some(4096));
    assert!(
        provider(&config, "chat")?
            .resolved_capabilities()
            .strict_tools
    );
    let responses = provider(&config, "responses")?;
    assert_eq!(responses.base_url(), "https://api.openai.com/v1");
    assert!(responses.resolved_capabilities().strict_tools);
    Ok(())
}

// 作成時の関門: オプションの優先順位、既定値、不在時の誤りを設定の境界で確かめる。
#[test]
fn exec_resolution_applies_options_project_user_and_defaults_in_order() -> TestResult {
    let fixture = Fixture::new()?;
    let defaults = UserConfig::load(&fixture.0.join("missing"))?;
    assert_eq!(defaults.default_mode, Mode::Restricted);
    assert_eq!(defaults.limits.max_turns, 200);
    assert_eq!(defaults.limits.max_check_fix, 10);
    assert_eq!(defaults.limits.max_run_tokens, None);
    assert!(
        defaults
            .resolve_exec(
                &ExecArgs::parse(["request"])?,
                &ProjectConfig::default(),
                &fixture.0
            )
            .is_err()
    );
    let user = UserConfig::parse(
        "default_mode='normal'\ndefault_provider='p'\n[providers.p]\nkind='openai-chat'\nmodel='user'\neffort='low'\n[providers.other]\nkind='anthropic-messages'\nmodel='other'\n[limits]\nmax_turns=25\nmax_check_fix=3\nmax_run_tokens=500",
    )?;
    let project = ProjectConfig {
        model: Some("project".to_owned()),
        ..ProjectConfig::default()
    };
    let normal = user.resolve_exec(&ExecArgs::parse(["request"])?, &project, &fixture.0)?;
    assert_eq!(normal.mode, Mode::Normal);
    assert_eq!(normal.effort, "low");
    assert_eq!(normal.limits.max_turns, 25);
    assert_eq!(normal.limits.max_run_tokens, Some(500));
    let args = ExecArgs::parse([
        "--mode",
        "restricted",
        "--project",
        "sub",
        "--provider",
        "other",
        "--model",
        "option",
        "--effort",
        "high",
        "--max-turns",
        "8",
        "--max-check-fix",
        "2",
        "--max-run-tokens",
        "100",
        "request",
    ])?;
    let resolved = user.resolve_exec(&args, &project, &fixture.0)?;
    assert_eq!(resolved.mode, Mode::Restricted);
    assert_eq!(resolved.project, fixture.0.join("sub"));
    assert_eq!(resolved.destination.provider, "other");
    assert_eq!(resolved.destination.model, "option");
    assert_eq!(resolved.effort, "high");
    assert_eq!(resolved.limits.max_turns, 8);
    assert_eq!(resolved.limits.max_check_fix, 2);
    assert_eq!(resolved.limits.max_run_tokens, Some(100));
    Ok(())
}

// 作成時の関門: 本物のコマンドと環境で、stdin の漏れ・二行目の混入・秘密を含む診断を捕まえる。
#[test]
fn credential_references_resolve_real_environment_and_commands_without_disclosing_keys()
-> TestResult {
    let fixture = Fixture::new()?;
    let key = ApiKeyRef::Env("PATH".to_owned()).resolve()?;
    assert!(key.expose_secret() == std::env::var("PATH")?);
    assert!(!format!("{key:?}").contains(key.expose_secret()));
    assert!(
        ApiKeyRef::Env(format!("BENITOITEH_MISSING_KEY_{}", std::process::id()))
            .resolve()
            .is_err()
    );
    let script = fixture.script("key", "if read -r line; then exit 7; fi\nprintf '%s\\r\\nignored-line\\n' \"$1\"\nprintf 'diagnostic' >&2\n")?;
    let reference = ApiKeyRef::Command(vec![
        script.to_string_lossy().into_owned(),
        "sample-secret".to_owned(),
    ]);
    assert!(!format!("{reference:?}").contains("sample-secret"));
    let key = reference.resolve()?;
    assert!(key.expose_secret() == "sample-secret");
    assert!(!format!("{key:?}").contains("sample-secret"));
    let script = fixture.script(
        "failure",
        "printf 'sample-secret\\n'\nprintf 'lookup failed for sample-secret\\n' >&2\nexit 9\n",
    )?;
    let result = ApiKeyRef::Command(vec![script.to_string_lossy().into_owned()]).resolve();
    let error = result
        .err()
        .ok_or_else(|| io::Error::from(io::ErrorKind::InvalidData))?;
    for diagnostic in [error.to_string(), format!("{error:?}")] {
        assert!(!diagnostic.contains("sample-secret"));
        assert!(diagnostic.contains("lookup failed"));
        assert!(diagnostic.contains('9'));
    }
    let script = fixture.script("empty", "printf '\\nsecret-on-second-line\\n'\n")?;
    assert!(
        ApiKeyRef::Command(vec![script.to_string_lossy().into_owned()])
            .resolve()
            .is_err()
    );
    Ok(())
}

// 作成時の関門: HOME がなくても進む、XDG を無視する、ID で一時領域を抜ける退行を捕まえる。
#[test]
fn data_paths_use_xdg_values_or_documented_fallbacks_and_require_home() -> TestResult {
    let paths = DataPaths::from_values(Some(Path::new("/home/person")), None, None, None)?;
    assert_eq!(
        paths.config_file,
        Path::new("/home/person/.config/benitoiteh/config.toml")
    );
    assert_eq!(
        paths.instructions_file,
        Path::new("/home/person/.config/benitoiteh/AGENTS.md")
    );
    assert_eq!(
        paths.sessions_dir,
        Path::new("/home/person/.local/state/benitoiteh/sessions")
    );
    assert_eq!(paths.temporary_dir("id")?, Path::new("/tmp/benitoiteh-id"));
    let fixture = Fixture::new()?;
    let paths = fixture.paths()?;
    assert_eq!(
        paths.config_file,
        fixture.0.join("config/benitoiteh/config.toml")
    );
    assert_eq!(
        paths.sessions_dir,
        fixture.0.join("state/benitoiteh/sessions")
    );
    assert_eq!(
        paths.temporary_dir("id")?,
        fixture.0.join("tmp/benitoiteh-id")
    );
    for id in ["", "..", "../outside", "/absolute", "nested/name"] {
        assert!(paths.temporary_dir(id).is_err());
    }
    assert!(
        DataPaths::from_values(
            None,
            Some(Path::new("/config")),
            Some(Path::new("/state")),
            None
        )
        .is_err()
    );
    Ok(())
}

// 作成時の関門: 四つの安全規則を、本物のディレクトリとリンクで検査し、字面だけの判定を捕まえる。
#[test]
fn writable_locations_reject_each_rule_and_resolve_aliases_before_checking() -> TestResult {
    let fixture = Fixture::new()?;
    let paths = fixture.paths()?;
    let project = fixture.dir("project")?;
    fixture.dir("tmp")?;
    let temporary = paths.temporary_dir("id")?;
    let valid = validate_writable(&paths, &project, &temporary, &[])?;
    assert_eq!(valid.project, project);
    assert_eq!(valid.temporary, temporary);
    let file = fixture.0.join("regular-file");
    fs::write(&file, "not a directory")?;
    assert!(matches!(
        validate_writable(&paths, &file, &temporary, &[]),
        Err(WritableError::Resolve { .. })
    ));
    assert!(matches!(
        validate_writable(&paths, &project, &file, &[]),
        Err(WritableError::Resolve { .. })
    ));
    let hidden = fixture.dir("home/.ssh/keys")?;
    let config = fixture.dir("config/benitoiteh/nested")?;
    let data = fixture.dir("state/benitoiteh")?;
    let cases = [
        (PathBuf::from("/"), WritableRule::RootOrHome),
        (paths.home.clone(), WritableRule::RootOrHome),
        (fixture.0.clone(), WritableRule::RootOrHome),
        (hidden, WritableRule::HomePrivate),
        (config, WritableRule::ConfigOrData),
        (fixture.0.join("config"), WritableRule::ConfigOrData),
        (data, WritableRule::ConfigOrData),
    ];
    for (path, expected) in cases {
        let result = validate_writable(&paths, &path, &temporary, &[]);
        assert!(
            matches!(&result, Err(WritableError::Rejected { rule, .. }) if *rule == expected),
            "{path:?}: {result:?}"
        );
        let diagnostic = result
            .err()
            .ok_or_else(|| io::Error::from(io::ErrorKind::InvalidData))?
            .to_string();
        assert!(diagnostic.contains(&path.display().to_string()));
    }
    for temp in [project.clone(), project.join("uncreated")] {
        assert!(matches!(
            validate_writable(&paths, &project, &temp, &[]),
            Err(WritableError::Rejected {
                rule: WritableRule::ProjectTemporary,
                ..
            })
        ));
    }
    let nested_project = fixture.dir("tmp/benitoiteh-id/project")?;
    assert!(matches!(
        validate_writable(&paths, &nested_project, &temporary, &[]),
        Err(WritableError::Rejected {
            rule: WritableRule::ProjectTemporary,
            ..
        })
    ));
    assert!(matches!(
        validate_writable(&paths, &project, &paths.home.join(".cache/future"), &[]),
        Err(WritableError::Rejected {
            rule: WritableRule::HomePrivate,
            ..
        })
    ));
    symlink(&paths.config_dir, fixture.0.join("alias"))?;
    assert!(matches!(
        validate_writable(&paths, &project, &temporary, &[fixture.0.join("alias")]),
        Err(WritableError::Rejected {
            rule: WritableRule::ConfigOrData,
            ..
        })
    ));
    #[cfg(target_os = "macos")]
    {
        let library = fixture.dir("home/Library/LaunchAgents")?;
        assert!(matches!(
            validate_writable(&paths, &library, &temporary, &[]),
            Err(WritableError::Rejected {
                rule: WritableRule::HomePrivate,
                ..
            })
        ));
    }
    Ok(())
}
