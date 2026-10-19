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
use benitoiteh::config::writable::{
    WritableError, WritableRule, validate_writable, validate_writable_with_executable,
};
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
        ("[limits]\nmax_check_fix=0", Some("max_check_fix")),
        ("[normal]\nnetwrok = true", Some("netwrok")),
        ("[skills]\nusers = false", Some("users")),
        ("[skills]\nuser = 'false'", None),
        (
            "[providers.p]\nkind='openai-chat'\nmodel='m'\ncapabilities={strcit_tools=true}",
            Some("strcit_tools"),
        ),
        ("[providers.p]\nkind='not-an-adapter'\nmodel='m'", None),
        (
            "[providers.p]\nkind='anthropic-messages'\nmodel='m'\neffort=''",
            None,
        ),
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
        ("[restricted]\nscratch_auto_effects=['Process.Run']", None),
        ("[restricted]\nscratch_auto_effects=['Http.Connect']", None),
        ("[restricted]\nscratch_auto_effects=['Console.Read']", None),
        ("[restricted]\nscratch_auto_effects=['Http.Listen']", None),
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
        [providers.chat_effort]
        kind = "openai-chat"
        base_url = "https://compatible.example/v1"
        model = "m"
        capabilities = { reasoning_effort = true }
    "#,
    )?;
    let official = provider(&config, "official")?;
    assert_eq!(official.base_url(), "https://api.anthropic.com");
    assert_eq!(official.effort, None);
    assert!(!official.resolved_capabilities().cache_control);
    assert!(official.resolved_capabilities().tool_choice_none);
    assert!(!official.resolved_capabilities().reasoning_effort);
    let compatible = provider(&config, "compatible")?.resolved_capabilities();
    assert!(!compatible.cache_control);
    assert!(!compatible.tool_choice_none);
    assert!(!compatible.parallel_tool_calls);
    assert!(!compatible.reasoning_effort);
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
    assert!(!responses.resolved_capabilities().reasoning_effort);
    assert!(
        !provider(&config, "chat")?
            .resolved_capabilities()
            .reasoning_effort
    );
    assert!(
        provider(&config, "chat_effort")?
            .resolved_capabilities()
            .reasoning_effort
    );
    assert!(official.resolved_capabilities().image_input);
    assert!(responses.resolved_capabilities().image_input);
    assert!(!compatible.image_input);
    assert!(
        !provider(&config, "chat")?
            .resolved_capabilities()
            .image_input
    );
    for (kind, base_url, value) in [
        ("anthropic-messages", "https://api.anthropic.com", false),
        ("openai-responses", "https://api.openai.com/v1", false),
        ("openai-chat", "https://compatible.example/v1", true),
        ("anthropic-messages", "https://compatible.example", true),
        ("openai-responses", "https://compatible.example/v1", false),
    ] {
        let parsed = UserConfig::parse(&format!(
            r#"
            [providers.images]
            kind = "{kind}"
            base_url = "{base_url}"
            model = "m"
            capabilities = {{ image_input = {value} }}
        "#
        ))?;
        assert_eq!(
            provider(&parsed, "images")?
                .resolved_capabilities()
                .image_input,
            value
        );
    }
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
    assert!(defaults.restricted.after_write_check);
    assert!(defaults.read.line_numbers);
    assert!(defaults.skills.user && defaults.skills.project);
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
    let zero = user.resolve_exec(
        &ExecArgs::parse(["--max-check-fix", "0", "request"])?,
        &project,
        &fixture.0,
    );
    assert!(zero.is_err_and(|error| error.to_string().contains("must be at least 1")));
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
    // 省略を high で埋める退行と、明示の設定を種類の既定で上書きする退行を捕まえる。
    // 既存の --effort high の検査と合わせて、設定の公開 API で優先順を確かめる（設計書 05-01「オプション」）。
    for (kind, configured, expected) in [
        ("anthropic-messages", "", "low"),
        ("openai-responses", "", "high"),
        ("openai-chat", "", "high"),
        ("anthropic-messages", "effort='medium'", "medium"),
        ("openai-responses", "effort='low'", "low"),
    ] {
        let config = UserConfig::parse(&format!(
            "default_provider='p'\n[providers.p]\nkind='{kind}'\nmodel='m'\n{configured}"
        ))?;
        let resolved = config.resolve_exec(
            &ExecArgs::parse(["request"])?,
            &ProjectConfig::default(),
            &fixture.0,
        )?;
        assert_eq!(resolved.effort, expected, "{kind}: {configured}");
    }
    // 対話は確認できるので、三つの承認の設定とオプションを受ける。
    for (approval, options, accepted) in [
        ("on-request", vec![], true),
        ("untrusted", vec![], true),
        ("never", vec![], true),
        ("on-request", vec!["--approval", "never"], true),
    ] {
        let config = UserConfig::parse(&format!(
            "default_mode='normal'\ndefault_provider='p'\n[providers.p]\nkind='openai-chat'\nmodel='model'\n[normal]\napproval='{approval}'"
        ))?;
        let args = ExecArgs::parse_interactive(options)?;
        let resolved = config.resolve_exec(&args, &ProjectConfig::default(), &fixture.0);
        assert_eq!(resolved.is_ok(), accepted);
    }
    Ok(())
}

// 作成時の関門: 本物のコマンドと環境で、stdin の漏れ・二行目の混入・秘密を含む診断を捕まえる。
#[test]
fn credential_references_resolve_real_environment_and_commands_without_disclosing_keys()
-> TestResult {
    use benitoiteh::config::credentials::{ApiKey, CredentialError};

    fn resolve_script_key(reference: &ApiKeyRef) -> Result<ApiKey, CredentialError> {
        let mut retries = 50_u32;
        loop {
            match reference.resolve() {
                Err(CredentialError::Spawn(error))
                    if error.kind() == io::ErrorKind::ExecutableFileBusy && retries > 0 =>
                {
                    // 並行する fork が書き込みの記述子を一時的に持つ競合だけを待つ。
                    // 回数を限り、ほかの起動失敗やキーの解決の誤りはそのまま返す。
                    retries = retries.saturating_sub(1);
                    std::thread::sleep(std::time::Duration::from_millis(10));
                }
                result => return result,
            }
        }
    }

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
    let key = resolve_script_key(&reference)?;
    assert!(key.expose_secret() == "sample-secret");
    assert!(!format!("{key:?}").contains("sample-secret"));
    let script = fixture.script(
        "failure",
        "printf 'sample-secret\\n'\nprintf 'lookup failed for sample-secret\\n' >&2\nexit 9\n",
    )?;
    let result = resolve_script_key(&ApiKeyRef::Command(vec![
        script.to_string_lossy().into_owned(),
    ]));
    let error = result
        .err()
        .ok_or_else(|| io::Error::from(io::ErrorKind::InvalidData))?;
    for diagnostic in [error.to_string(), format!("{error:?}")] {
        assert!(!diagnostic.contains("sample-secret"));
        assert!(diagnostic.contains("lookup failed"));
        assert!(diagnostic.contains('9'));
    }
    let script = fixture.script("empty", "printf '\\nsecret-on-second-line\\n'\n")?;
    assert!(matches!(
        resolve_script_key(&ApiKeyRef::Command(vec![
            script.to_string_lossy().into_owned(),
        ])),
        Err(CredentialError::Output)
    ));
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

// 作成時の関門: 自身の書き換えとキーの漏れを、設定の持ち主の境界で確かめる（P3-02）。
#[test]
fn writable_locations_reject_the_real_executable_in_any_root() -> TestResult {
    let fixture = Fixture::new()?;
    let paths = fixture.paths()?;
    let project = fixture.dir("project")?;
    let temporary = fixture.dir("tmp/session")?;
    let additional = fixture.dir("additional")?;
    let outside = fixture.dir("outside")?;
    for root in [&project, &temporary, &additional, &outside] {
        let executable = root.join("benitoiteh");
        fs::write(&executable, "executable")?;
        let alias = fixture.0.join("executable-alias");
        symlink(&executable, &alias)?;
        let result = validate_writable_with_executable(
            &paths,
            &project,
            &temporary,
            std::slice::from_ref(&additional),
            &alias,
        );
        if root == &outside {
            assert!(result.is_ok(), "{result:?}");
        } else {
            assert!(
                matches!(result, Err(WritableError::Rejected { rule: WritableRule::Executable, ref path, ref other })
                if path == root && other == &executable),
                "{result:?}"
            );
        }
        fs::remove_file(alias)?;
    }
    Ok(())
}

#[test]
fn child_process_environment_removes_all_provider_keys_and_replaces_inherited_markers() -> TestResult
{
    use benitoiteh::config::child_environment;
    use std::collections::BTreeMap;
    use std::ffi::OsString;
    use std::process::Command;
    let f = Fixture::new()?;
    let temporary = f.dir("tmp")?;
    let user = UserConfig::parse(
        "[providers.p]\nkind='openai-chat'\nmodel='m'\napi_key={env='CUSTOM_PROVIDER_KEY'}",
    )?;
    // 固定の一覧は実装の定数から写さず、仕様のキーを全て渡す。
    let inherited: BTreeMap<OsString, OsString> = [
        ("OPENAI_API_KEY", "secret-openai"),
        ("ANTHROPIC_API_KEY", "secret-anthropic"),
        ("GEMINI_API_KEY", "secret-gemini"),
        ("DEEPSEEK_API_KEY", "secret-deepseek"),
        ("CUSTOM_PROVIDER_KEY", "secret-custom"),
        ("BENITOITEH_SANDBOX", "outer"),
        ("BENITOITEH_SANDBOX_NETWORK_DISABLED", "outer"),
        ("TMPDIR", "/outer/tmp"),
        ("TERM", "xterm"),
        ("PAGER", "less"),
        ("GIT_PAGER", "less"),
        ("KEEP_ME", "inherited"),
        ("BENITOITEH_ASKPASS_SOCKET", "/stale/socket"),
        ("BENITOITEH_ASKPASS_TOKEN", "stale-token"),
    ]
    .into_iter()
    .map(|(k, v)| (k.into(), v.into()))
    .collect();
    for sandbox in [
        Some(("workspace-write", false)),
        Some(("workspace-write", true)),
        Some(("read-only", false)),
        None,
    ] {
        let env = child_environment(
            inherited.clone(),
            &user.referenced_env_vars(),
            &temporary,
            sandbox,
        );
        // 実際の子に渡し、外での起動でも秘密を除外する安全の契約を守る。
        let output = Command::new("/usr/bin/env")
            .env_clear()
            .envs(env)
            .current_dir(&f.0)
            .output()?;
        assert!(output.status.success());
        let output = String::from_utf8(output.stdout)?;
        assert!(!output.contains("secret-"), "{output}");
        assert!(!output.contains("outer"), "{output}");
        assert!(!output.contains("BENITOITEH_ASKPASS_"), "{output}");
        for expected in [
            format!("TMPDIR={}", temporary.display()),
            "NO_COLOR=1".into(),
            "TERM=dumb".into(),
            "PAGER=cat".into(),
            "GIT_PAGER=cat".into(),
            "KEEP_ME=inherited".into(),
        ] {
            assert!(output.lines().any(|line| line == expected), "{output}");
        }
        assert_eq!(
            output
                .lines()
                .any(|line| line.starts_with("BENITOITEH_SANDBOX=")),
            sandbox.is_some()
        );
        if let Some((name, _)) = sandbox {
            assert!(
                output
                    .lines()
                    .any(|line| line == format!("BENITOITEH_SANDBOX={name}"))
            );
        }
        assert_eq!(
            output
                .lines()
                .any(|line| line == "BENITOITEH_SANDBOX_NETWORK_DISABLED=1"),
            sandbox.is_some_and(|(_, ip)| !ip)
        );
    }
    Ok(())
}

// 関門: 利用者だけが切り替えられ、省略時に true が保たれる契約。
// 除外規則の結合も公開の設定 API で確認し、型の共有による信頼境界の退行を捕まえる。
#[test]
fn evaluation_switches_are_user_only_and_preserve_read_exclusions() -> TestResult {
    use benitoiteh::safe_fs::ScopeRoot;
    let f = Fixture::new()?;
    let project_dir = f.dir("project")?;
    fs::create_dir(project_dir.join(".benitoiteh"))?;
    for input in ["", "[read]\nexclude=[]\n[restricted]\nallowed_programs=[]"] {
        let config = UserConfig::parse(input)?;
        assert!(config.restricted.after_write_check);
        assert!(config.read.line_numbers);
    }
    let user = UserConfig::parse(
        "default_provider='p'\n[providers.p]\nkind='openai-chat'\nmodel='m'\n[restricted]\nafter_write_check=false\n[read]\nline_numbers=false\nexclude=['user.*']\n[skills]\nuser=false\nproject=false",
    )?;
    assert!(!user.restricted.after_write_check);
    assert!(!user.read.line_numbers);
    let root = ScopeRoot::open(&project_dir)?;
    let path = project_dir.join(".benitoiteh/config.toml");
    fs::write(&path, "[read]\nexclude=['project.*']")?;
    let loaded = ProjectConfig::load(&root);
    assert!(loaded.error.is_none());
    let resolved =
        user.resolve_exec(&ExecArgs::parse(["request"])?, &loaded.config, &project_dir)?;
    assert!(!resolved.read_line_numbers);
    assert!(!resolved.skills.user && !resolved.skills.project);
    assert_eq!(resolved.read.exclude, ["user.*", "project.*"]);
    for (input, field) in [
        ("[read]\nline_numbers=false", "line_numbers"),
        ("[restricted]\nafter_write_check=false", "restricted"),
        ("[skills]\nuser=false", "skills"),
    ] {
        fs::write(&path, input)?;
        let loaded = ProjectConfig::load(&root);
        assert!(loaded.read);
        let error = loaded.error.ok_or("accepted project evaluation switch")?;
        assert_eq!(
            error.to_string(),
            format!("unknown configuration field `{field}`")
        );
    }
    for input in [
        "[read]\nline_numbers=0",
        "[restricted]\nafter_write_check='false'",
    ] {
        assert!(UserConfig::parse(input).is_err());
    }
    Ok(())
}

// 関門: 秘密の仲介を変えられるのは利用者だけ。未知の欄とプロジェクト側の設定を拒む。
#[test]
fn askpass_is_enabled_by_default_and_only_user_config_can_disable_it() -> TestResult {
    assert!(UserConfig::default().secrets.askpass);
    assert!(
        !UserConfig::parse("[secrets]\naskpass = false")?
            .secrets
            .askpass
    );
    assert!(UserConfig::parse("[secrets]\nunknown = true").is_err());
    let fixture = Fixture::new()?;
    let project = fixture.dir("project")?;
    fs::create_dir(project.join(".benitoiteh"))?;
    fs::write(
        project.join(".benitoiteh/config.toml"),
        "[secrets]\naskpass = false",
    )?;
    let loaded = ProjectConfig::load(&benitoiteh::safe_fs::ScopeRoot::open(&project)?);
    assert!(loaded.read && loaded.error.is_some());
    Ok(())
}

// 関門: 認証と送り先の組み合わせを設定の境界で断り、プロジェクトから認証を選べなくする。
#[test]
fn chatgpt_authentication_is_only_allowed_for_fixed_responses_destination() -> TestResult {
    use benitoiteh::config::ProviderAuth;
    let input = "default_provider='p'\n[providers.p]\nkind='openai-responses'\nmodel='arbitrary-model'\nauth='chatgpt'\nmax_output_tokens=123\n";
    let config = UserConfig::parse(input)?;
    let resolved = config.resolve_exec(
        &ExecArgs::parse(["request"])?,
        &ProjectConfig::default(),
        Path::new("/project"),
    )?;
    assert_eq!(resolved.auth, Some(ProviderAuth::Chatgpt));
    assert_eq!(resolved.destination.base_url, "https://api.openai.com/v1");
    assert_eq!(resolved.capabilities.max_output_tokens, Some(123));
    for invalid in [
        input.replace("openai-responses", "openai-chat"),
        input.replace("openai-responses", "anthropic-messages"),
        format!("{input}base_url='http://127.0.0.1:1/secret-value'\n"),
        format!("{input}api_key={{env='secret-value'}}\n"),
        input.replace("chatgpt", "secret-value"),
    ] {
        let error = UserConfig::parse(&invalid)
            .err()
            .ok_or("accepted invalid authentication")?;
        assert!(!error.to_string().contains("secret-value"));
    }
    let f = Fixture::new()?;
    let project = f.dir("project/.benitoiteh")?;
    fs::write(project.join("config.toml"), "auth='chatgpt'")?;
    let root = benitoiteh::safe_fs::ScopeRoot::open(f.0.join("project"))?;
    let loaded = ProjectConfig::load(&root);
    assert!(loaded.error.is_some());
    Ok(())
}
