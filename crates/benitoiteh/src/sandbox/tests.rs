//! 方針の検証と、パスを文から分離する契約を確かめる（設計書 03-02、実装プラン P0-04「テスト」）。

use std::error::Error;
use std::fs;
use std::os::unix::fs::symlink;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use super::{PolicyConfig, ProgramAccess, ReadAccess, SandboxPolicy};

type TestResult = Result<(), Box<dyn Error>>;

// 関門: 実測した三名の欠落と、sysctl 全体・接頭辞・未確認の名前への許可の拡大を捕まえる。
// OS での sort の動作は sandbox_macos の通常モードの正負の対照が確かめる。
#[test]
fn seatbelt_allows_only_the_three_measured_sysctl_names() -> TestResult {
    let profile = SandboxPolicy::new(PolicyConfig::without_writable())?.seatbelt_profile();
    let mut rules: Vec<_> = profile
        .source
        .lines()
        .filter(|line| !line.trim_start().starts_with(';') && line.contains("sysctl-read"))
        .collect();
    rules.sort_unstable();
    assert_eq!(
        rules,
        [
            "(allow sysctl-read (sysctl-name \"hw.memsize\"))",
            "(allow sysctl-read (sysctl-name \"hw.pagesize\"))",
            "(allow sysctl-read (sysctl-name \"hw.pagesize_compat\"))",
        ],
        "{}",
        profile.source
    );
    Ok(())
}

// 関門: 古い JSON と製品の既定値を変えず、0 や桁あふれを通信の例外にしない。
#[test]
fn loopback_port_defaults_and_json_validation() -> TestResult {
    let config = PolicyConfig::without_writable();
    assert_eq!(config.allow_loopback_port, None);
    let mut json = serde_json::to_value(&config)?;
    json.as_object_mut()
        .ok_or("object")?
        .remove("allow_loopback_port");
    assert_eq!(
        serde_json::from_value::<PolicyConfig>(json.clone())?.allow_loopback_port,
        None
    );
    for invalid in [0, -1, 65536] {
        json.as_object_mut()
            .ok_or("object")?
            .insert("allow_loopback_port".into(), invalid.into());
        assert!(serde_json::from_value::<PolicyConfig>(json.clone()).is_err());
    }
    for port in [1, 65535] {
        json.as_object_mut()
            .ok_or("object")?
            .insert("allow_loopback_port".into(), port.into());
        let parsed = serde_json::from_value::<PolicyConfig>(json.clone())?;
        assert_eq!(
            parsed.allow_loopback_port.map(std::num::NonZeroU16::get),
            Some(port)
        );
        #[cfg(target_os = "linux")]
        assert!(SandboxPolicy::new(parsed).is_err());
        #[cfg(target_os = "macos")]
        {
            let profile = SandboxPolicy::new(parsed)?.seatbelt_profile();
            let rules: Vec<_> = profile
                .source
                .lines()
                .filter(|line| line.contains("network-"))
                .collect();
            assert_eq!(
                rules,
                vec![format!(
                    "(allow network-outbound (remote ip \"localhost:{port}\"))"
                )]
            );
        }
    }
    Ok(())
}

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Result<Self, Box<dyn Error>> {
        let parent = fs::canonicalize(std::env::temp_dir())?;
        let nonce = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
        let path = parent.join(format!("bh-policy-{}-{nonce}", std::process::id()));
        fs::create_dir(&path)?;
        Ok(Self(path))
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        if let Err(error) = fs::remove_dir_all(&self.0) {
            eprintln!("Could not remove {}: {error}", self.0.display());
        }
    }
}

// 作成時の関門: JSON からの生成で検査を迂回する退行を、方針の公開の入口で捕まえる。
#[test]
fn json_round_trip_preserves_rules_and_rejects_invalid_policy() -> TestResult {
    let fixture = Fixture::new()?;
    let writable = fixture.0.join("write");
    let secret = fixture.0.join("secret");
    fs::create_dir(&writable)?;
    fs::create_dir(&secret)?;
    let mut config = PolicyConfig::new(vec![writable]);
    config.denied_read.push(secret);
    config.readable = ReadAccess::Roots(vec![fixture.0.clone()]);
    config.programs = ProgramAccess::Only(vec![fs::canonicalize(std::env::current_exe()?)?]);
    config.allow_ip = true;
    config.protected_names.push(".Cache-2_data".into());
    assert!(config.deny_exec_in_writable);
    for deny_exec in [false, true] {
        config.deny_exec_in_writable = deny_exec;
        let policy = SandboxPolicy::new(config.clone())?;
        let json = serde_json::to_string(&policy)?;
        assert_eq!(serde_json::from_str::<SandboxPolicy>(&json)?, policy);
    }
    let mut omitted = serde_json::to_value(&config)?;
    assert!(
        omitted
            .as_object_mut()
            .ok_or("Policy must be an object")?
            .remove("deny_exec_in_writable")
            .is_some()
    );
    assert!(
        serde_json::from_value::<SandboxPolicy>(omitted)?
            .config()
            .deny_exec_in_writable
    );

    let readonly = SandboxPolicy::new(PolicyConfig::without_writable())?;
    assert_eq!(
        serde_json::from_value::<SandboxPolicy>(serde_json::to_value(&readonly)?)?,
        readonly
    );
    let mut no_fork = serde_json::to_value(&readonly)?;
    no_fork
        .as_object_mut()
        .ok_or("policy object")?
        .remove("allow_fork");
    assert!(serde_json::from_value::<SandboxPolicy>(no_fork).is_err());
    let mut omitted = serde_json::to_value(&config)?;
    omitted
        .as_object_mut()
        .ok_or("policy object")?
        .remove("require_writable");
    assert!(
        serde_json::from_value::<SandboxPolicy>(omitted)?
            .config()
            .require_writable
    );

    let alias = fixture.0.join("alias");
    symlink(&fixture.0, &alias)?;
    let mut invalid_configs = Vec::new();
    for invalid in [PathBuf::from("relative"), alias, fixture.0.join(".")] {
        let mut changed = config.clone();
        changed.writable = vec![invalid];
        invalid_configs.push(changed);
    }
    let mut empty = config.clone();
    empty.writable.clear();
    invalid_configs.push(empty);
    let mut readonly_with_writable = config.clone();
    readonly_with_writable.require_writable = false;
    invalid_configs.push(readonly_with_writable);
    let mut unprotected = config.clone();
    unprotected.protected_names.clear();
    invalid_configs.push(unprotected);
    let mut uppercase_only = config.clone();
    uppercase_only.protected_names = vec![".GIT".into(), ".BENITOITEH".into()];
    invalid_configs.push(uppercase_only);
    let mut denied_writable = config.clone();
    denied_writable.denied_read = vec![fixture.0.clone()];
    invalid_configs.push(denied_writable);
    for name in [
        "../other",
        "space name",
        "quote\")",
        "regex[ab]",
        "日本語",
        ".",
        "..",
        "",
    ] {
        let mut changed = config.clone();
        changed.protected_names.push(name.into());
        invalid_configs.push(changed);
    }
    for invalid in invalid_configs {
        assert!(SandboxPolicy::new(invalid.clone()).is_err(), "{invalid:?}");
        assert!(serde_json::from_str::<SandboxPolicy>(&serde_json::to_string(&invalid)?).is_err());
    }
    Ok(())
}

// 作成時の関門: 方針への文字列の埋め込みに戻す退行を、独立した -D の出力の契約で捕まえる。
// OS の拒否は sandbox_macos に任せる。このテストでは拒否の正しさを主張しない。
#[test]
fn seatbelt_source_does_not_depend_on_path_contents() -> TestResult {
    let fixture = Fixture::new()?;
    let secret = fixture.0.join("secret");
    fs::create_dir(&secret)?;
    let mut profiles = Vec::new();
    for name in ["ordinary", "quote\") (allow default) ; 日本語 [.*]"] {
        let path = fixture.0.join(name);
        fs::create_dir(&path)?;
        let mut config = PolicyConfig::new(vec![path.clone()]);
        config.readable = ReadAccess::Roots(vec![path.clone()]);
        config.denied_read.push(secret.clone());
        config.programs = ProgramAccess::Only(vec![path.clone()]);
        let profile = SandboxPolicy::new(config)?.seatbelt_profile();
        assert!(!profile.source.contains(name));
        for (parameter, value) in &profile.parameters {
            assert!(profile.source.contains(&format!("(param \"{parameter}\")")));
            if parameter.starts_with("PROTECTED_") {
                continue;
            }
            if parameter.starts_with("DENIED_") {
                assert_eq!(value, secret.as_os_str());
            } else {
                assert_eq!(value, path.as_os_str());
            }
        }
        profiles.push(profile.source);
    }
    let mut sources = profiles.iter();
    assert_eq!(sources.next(), sources.next());
    Ok(())
}
