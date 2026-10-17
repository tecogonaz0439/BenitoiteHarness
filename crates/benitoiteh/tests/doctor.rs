//! doctor の CLI 契約と、秘密・利用者の設定・ファイルシステムの保護を確かめる（設計書 05-01「doctor」、07-01「安全の性質のテスト」）。

#[path = "support/doctor.rs"]
mod support;

use std::error::Error;
use std::fs;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::process::Output;

use serde_json::Value;
use support::Fixture;

type TestResult = Result<(), Box<dyn Error>>;

fn records(output: &Output) -> Result<Vec<Value>, Box<dyn Error>> {
    assert!(
        output.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(|line| serde_json::from_str(line).map_err(Into::into))
        .collect()
}

fn item<'a>(records: &'a [Value], name: &str) -> Result<&'a Value, Box<dyn Error>> {
    records
        .iter()
        .find(|record| record.get("name").and_then(Value::as_str) == Some(name))
        .ok_or_else(|| format!("Missing item {name}: {records:?}").into())
}

const LOCAL: &str = "default_provider = 'local'\n[providers.local]\nkind = 'openai-chat'\nmodel = 'local-model'\nbase_url = 'http://127.0.0.1:1/v1'\ncontext_window = 4096\n";

// 作成時の関門: CLI の解釈と実際に開けるプロジェクトの契約。内部の parser は重ねて試さない。
#[test]
fn invalid_arguments_and_inaccessible_projects_exit_two() -> TestResult {
    for args in [
        vec!["--unknown"],
        vec!["--json", "--json"],
        vec!["--project"],
        vec!["--project", "--json"],
        vec!["--project", ".", "--project", "."],
        vec!["--project", "missing"],
        vec!["--project", "file"],
    ] {
        let fixture = Fixture::new()?;
        fs::write(fixture.project.join("file"), "")?;
        let output = fixture.command().args(&args).output()?;
        assert_eq!(output.status.code(), Some(2), "{args:?}: {output:?}");
        assert!(output.stdout.is_empty());
        assert_eq!(String::from_utf8(output.stderr)?.lines().count(), 1);
    }
    Ok(())
}

// 作成時の関門: 壊れた設定からキーのコマンドを動かす退行、値を診断へ流す退行を入口で捕まえる。
#[test]
fn invalid_user_config_and_missing_home_leave_dependent_checks_unchecked() -> TestResult {
    for missing_home in [false, true] {
        let fixture = Fixture::new()?;
        fixture.write_config("secret-config-value = [invalid TOML")?;
        let mut command = fixture.command();
        if missing_home {
            command.env_remove("HOME");
        }
        let output = command.arg("--json").output()?;
        assert_eq!(output.status.code(), Some(1));
        let rows = records(&output)?;
        assert_eq!(
            item(&rows, "config.user")?.get("status"),
            Some(&Value::from("unavailable"))
        );
        assert_eq!(
            item(&rows, "provider")?.get("status"),
            Some(&Value::from("unchecked"))
        );
        assert!(!String::from_utf8_lossy(&output.stdout).contains("secret-config-value"));
        if missing_home {
            for name in [
                "writable",
                "sandbox.normal",
                "data.sessions",
                "data.storage",
            ] {
                assert_eq!(
                    item(&rows, name)?.get("status"),
                    Some(&Value::from("unchecked"))
                );
            }
        }
        assert_eq!(fs::read_dir(fixture.root.join("state"))?.count(), 0);
        assert_eq!(fs::read_dir(fixture.root.join("tmp"))?.count(), 0);
        assert_eq!(fs::read_dir(&fixture.project)?.count(), 0);
    }
    Ok(())
}

// 作成時の関門: 提供者の選択、機能の上書き、すべてのキー参照の解決は doctor 固有の契約。
#[test]
fn provider_diagnostics_resolve_overrides_and_distinguish_the_default_key() -> TestResult {
    for (config, expected) in [
        (LOCAL.to_owned(), vec![("provider.local", "ok")]),
        (
            LOCAL.replace("context_window = 4096", ""),
            vec![("provider.local", "warning")],
        ),
        (
            LOCAL.replace(
                "context_window = 4096",
                "capabilities = { context_window = 8192 }",
            ),
            vec![("provider.local", "ok")],
        ),
        (
            format!(
                "{LOCAL}api_key = {{ env = 'BENITOITEH_DOCTOR_TEST_KEY' }}\n[providers.other]\nkind = 'openai-chat'\nmodel = 'other-model'\ncontext_window = 8192\napi_key = {{ env = 'BENITOITEH_DOCTOR_TEST_KEY' }}"
            ),
            vec![
                ("provider.local", "unavailable"),
                ("provider.other", "warning"),
            ],
        ),
        (
            LOCAL.replace("default_provider = 'local'", "default_provider = 'missing'"),
            vec![("provider", "unavailable")],
        ),
        (
            LOCAL.replace("default_provider = 'local'", ""),
            vec![("provider", "unavailable")],
        ),
        (String::new(), vec![("provider", "unavailable")]),
    ] {
        let fixture = Fixture::new()?;
        fixture.write_config(&config)?;
        let output = fixture.command().arg("--json").output()?;
        let rows = records(&output)?;
        for (name, status) in expected {
            assert_eq!(
                item(&rows, name)?.get("status"),
                Some(&Value::from(status)),
                "{rows:?}"
            );
            if status == "unavailable" {
                assert_eq!(output.status.code(), Some(1));
            }
        }
    }
    Ok(())
}

// 作成時の関門: CredentialError の Display、参照の Debug、URL の表示による漏洩は別経路。
// 本物のコマンドから異なる秘密を返し、text と JSON の両方の出力で確かめる。
#[test]
fn credentials_and_url_secrets_never_reach_either_output_stream() -> TestResult {
    for json in [false, true] {
        for (url, expected_url) in [
            (
                "https://url-user-secret:url-password-secret@example.invalid:8443/api/v1?query-secret=yes#fragment-secret",
                "https://example.invalid:8443/api/v1",
            ),
            (
                "https://user:1234?@host/",
                "Invalid base URL (value omitted).",
            ),
            (
                "https://user:1234#@host/",
                "Invalid base URL (value omitted).",
            ),
            (
                "https://u:p@user:1234?@host/",
                "Invalid base URL (value omitted).",
            ),
            ("https://u:p@host:8443/v1?key=x#f", "https://host:8443/v1"),
            ("https://u:p@[::1]:8443/v1?key=x#f", "https://[::1]:8443/v1"),
            ("https://u:p@host/v1?key=x#f", "https://host/v1"),
        ] {
            let fixture = Fixture::new()?;
            let script = fixture.root.join("credential.sh");
            fs::write(
                &script,
                "printf 'command-key-secret\\n'\nprintf 'stderr-secret-marker' >&2\ncase \"$1\" in fail) exit 7;; esac\n",
            )?;
            fixture.write_config(&format!(
                r#"
default_provider = 'env'
[providers.env]
kind = 'openai-chat'
model = 'env-model'
base_url = '{url}'
context_window = 4096
api_key = {{ env = 'BENITOITEH_DOCTOR_TEST_KEY' }}
[providers.command]
kind = 'openai-chat'
model = 'command-model'
context_window = 4096
api_key = {{ command = ['/bin/sh', '{}', 'success', 'argument-secret-marker'] }}
[providers.failure]
kind = 'openai-chat'
model = 'failure-model'
context_window = 4096
api_key = {{ command = ['/bin/sh', '{}', 'fail', 'argument-secret-marker'] }}
[providers.spawn]
kind = 'openai-chat'
model = 'spawn-model'
context_window = 4096
api_key = {{ command = ['/missing/argument-secret-marker', 'argument-secret-marker'] }}
[providers.output]
kind = 'openai-chat'
model = 'output-model'
context_window = 4096
api_key = {{ command = ['/bin/sh', '-c', 'printf ""', 'argument-secret-marker'] }}
"#,
                script.display(),
                script.display()
            ))?;
            let mut command = fixture.command();
            command.env("BENITOITEH_DOCTOR_TEST_KEY", "environment-key-secret");
            if json {
                command.arg("--json");
            }
            let output = command.output()?;
            for bytes in [&output.stdout, &output.stderr] {
                let output = String::from_utf8_lossy(bytes);
                for secret in [
                    "command-key-secret",
                    "stderr-secret-marker",
                    "argument-secret-marker",
                    "environment-key-secret",
                    "url-user-secret",
                    "url-password-secret",
                    "query-secret",
                    "fragment-secret",
                    ":1234",
                    "u:p",
                    "key=x",
                    "#f",
                ] {
                    assert!(!output.contains(secret), "Leaked {secret}: {output}");
                }
            }
            let stdout = String::from_utf8(output.stdout.clone())?;
            assert!(stdout.contains(expected_url), "{url}: {stdout}");
            if json {
                let rows = records(&output)?;
                for name in ["provider.env", "provider.command"] {
                    assert_eq!(item(&rows, name)?.get("status"), Some(&Value::from("ok")));
                }
                for name in ["provider.failure", "provider.spawn", "provider.output"] {
                    assert_eq!(
                        item(&rows, name)?.get("status"),
                        Some(&Value::from("warning"))
                    );
                }
                assert!(
                    item(&rows, "provider.failure")?
                        .get("detail")
                        .and_then(Value::as_str)
                        .is_some_and(|detail| detail.contains("status 7"))
                );
            }
        }
    }
    Ok(())
}

// 作成時の関門: 通常の起動は記録と保護のためのディレクトリを作る。doctor に流用すると
// プロジェクトと保存先を変えてしまうため、入口からの読み取り専用の契約を守る。
#[test]
fn json_contract_and_project_selection_leave_no_files_behind() -> TestResult {
    let fixture = Fixture::new()?;
    fixture.write_config(LOCAL)?;
    let project = fixture.root.join("selected-project");
    fs::create_dir(&project)?;
    fs::create_dir(project.join(".benitoiteh"))?;
    fs::write(
        project.join(".benitoiteh/config.toml"),
        "model = 'chosen'\nproviders = {}",
    )?;
    let output = fixture
        .command()
        .args([
            "--project",
            project.to_str().ok_or("Non-UTF-8 fixture")?,
            "--json",
        ])
        .output()?;
    let rows = records(&output)?;
    let (summary, items) = rows.split_last().ok_or("Missing summary")?;
    assert_eq!(summary.get("type"), Some(&Value::from("summary")));
    assert!(
        summary
            .get("restricted_sandbox")
            .is_some_and(Value::is_boolean)
    );
    assert!(
        ["enabled", "disabled_by_config", "unavailable"].contains(
            &summary
                .get("normal_sandbox")
                .and_then(Value::as_str)
                .ok_or("Missing normal status")?
        )
    );
    assert_eq!(
        summary.get("exit_status").and_then(Value::as_i64),
        output.status.code().map(i64::from)
    );
    for row in items {
        assert_eq!(row.get("type"), Some(&Value::from("item")));
        assert!(row.get("name").is_some_and(Value::is_string));
        assert!(row.get("detail").is_some_and(Value::is_string));
        assert!(
            ["ok", "unavailable", "unchecked", "warning"].contains(
                &row.get("status")
                    .and_then(Value::as_str)
                    .ok_or("Missing status")?
            )
        );
        if row
            .get("name")
            .and_then(Value::as_str)
            .is_some_and(|name| name.starts_with("provider."))
        {
            assert!(row.get("default").is_some_and(Value::is_boolean));
        }
    }
    assert_eq!(
        item(&rows, "benitoite")?.get("status"),
        Some(&Value::from("unchecked"))
    );
    assert_eq!(
        item(&rows, "config.project")?.get("status"),
        Some(&Value::from("warning"))
    );
    assert_eq!(
        item(&rows, "writable")?.get("status"),
        Some(&Value::from("ok"))
    );
    assert!(
        item(&rows, "data.storage")?
            .get("detail")
            .and_then(Value::as_str)
            .is_some_and(|detail| detail.starts_with("0 / 10737418240 bytes"))
    );
    assert_eq!(fs::read_dir(fixture.root.join("state"))?.count(), 0);
    assert_eq!(fs::read_dir(fixture.root.join("tmp"))?.count(), 0);
    assert_eq!(fs::read_dir(&fixture.project)?.count(), 0);
    assert_eq!(fs::read_dir(&project)?.count(), 1);
    assert_eq!(fs::read_dir(project.join(".benitoiteh"))?.count(), 1);
    assert_eq!(
        fs::read_to_string(project.join(".benitoiteh/config.toml"))?,
        "model = 'chosen'\nproviders = {}"
    );
    Ok(())
}

// 作成時の関門: TMPDIR の指定だけで、検査用の場所がプロジェクトや記録の中に
// 作られる退行を捕まえる。後始末で消えた場合も、作成前の拒否であることを確かめる。
#[test]
fn temporary_roots_inside_project_or_data_are_rejected_before_creation() -> TestResult {
    for inside_project in [true, false] {
        let fixture = Fixture::new()?;
        fixture.write_config(LOCAL)?;
        let data = fixture.root.join("state/benitoiteh");
        fs::create_dir(&data)?;
        let temporary = if inside_project {
            &fixture.project
        } else {
            &data
        };
        let output = fixture
            .command()
            .env("TMPDIR", temporary)
            .arg("--json")
            .output()?;
        let rows = records(&output)?;
        assert_eq!(output.status.code(), Some(1));
        let rule = item(&rows, "sandbox.restricted_rule")?;
        assert_eq!(rule.get("status"), Some(&Value::from("unavailable")));
        assert!(
            rule.get("detail").and_then(Value::as_str).is_some_and(
                |detail| detail.contains("must be outside the project and data directories")
            ),
            "{rule}"
        );
        assert_eq!(fs::read_dir(&fixture.project)?.count(), 0);
        assert_eq!(fs::read_dir(data)?.count(), 0);
    }
    Ok(())
}

// 作成時の関門: リンクを辿る、危険な権限を修復してしまう、quota のために記録を作る退行。
#[test]
fn existing_data_directories_are_checked_without_following_links_or_changing_modes() -> TestResult {
    for mode in [0o700, 0o755, 0o1700] {
        let fixture = Fixture::new()?;
        fixture.write_config(LOCAL)?;
        let data = fixture.root.join("state/benitoiteh");
        fs::create_dir(&data)?;
        let sessions = data.join("sessions");
        fs::create_dir(&sessions)?;
        fs::set_permissions(&sessions, fs::Permissions::from_mode(mode))?;
        let history = data.join("history");
        symlink(fixture.root.join("home"), &history)?;
        let output = fixture.command().arg("--json").output()?;
        let rows = records(&output)?;
        assert_eq!(
            item(&rows, "data.sessions")?.get("status"),
            Some(&Value::from(if mode == 0o700 {
                "ok"
            } else {
                "unavailable"
            }))
        );
        assert_eq!(
            item(&rows, "data.history")?.get("status"),
            Some(&Value::from("unavailable"))
        );
        assert_eq!(fs::metadata(&sessions)?.permissions().mode() & 0o7777, mode);
        assert!(fs::symlink_metadata(&history)?.is_symlink());
        assert_eq!(fs::read_dir(&sessions)?.count(), 0);
        assert_eq!(fs::read_dir(fixture.root.join("tmp"))?.count(), 0);
    }
    Ok(())
}

// 作成時の関門: サンドボックスなしの明示設定と不正な書ける場所は、機械の機能に依らない。
#[test]
fn normal_mode_reports_configuration_and_rejects_unsafe_writable_roots() -> TestResult {
    for unsafe_root in [false, true] {
        let fixture = Fixture::new()?;
        fixture.write_config(&format!(
            "{LOCAL}\n[normal]\nsandbox = 'danger-full-access'\nwritable_roots = [{}]",
            if unsafe_root {
                format!("'{}'", fixture.root.join("home").display())
            } else {
                String::new()
            }
        ))?;
        let output = fixture.command().arg("--json").output()?;
        let rows = records(&output)?;
        assert_eq!(
            item(&rows, "sandbox.normal")?.get("status"),
            Some(&Value::from(if unsafe_root {
                "unchecked"
            } else {
                "warning"
            }))
        );
        if unsafe_root {
            assert_eq!(
                item(&rows, "writable")?.get("status"),
                Some(&Value::from("unavailable"))
            );
        } else {
            assert_eq!(
                rows.last().and_then(|row| row.get("normal_sandbox")),
                Some(&Value::from("disabled_by_config"))
            );
        }
    }
    Ok(())
}
