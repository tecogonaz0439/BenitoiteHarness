//! 公開の道具と設定の境界で承認前の拒否を確かめる（設計書 01-03、05-01、07-01）。

use benitoiteh::config::{Mode, ProjectConfig, ReadConfig, UserConfig, paths::DataPaths};
use benitoiteh::safe_fs::ScopeRoot;
use benitoiteh::tools::{ToolContext, ToolSet, exclude::ExcludeRules};
use serde_json::json;
use std::error::Error;
use std::fs;
use std::path::PathBuf;

type TestResult = Result<(), Box<dyn Error>>;
struct Fixture {
    root: PathBuf,
}
impl Fixture {
    fn new() -> Result<Self, Box<dyn Error>> {
        let mut random = [0_u8; 16];
        getrandom::fill(&mut random)?;
        let root = std::env::temp_dir().join(format!(
            "benitoiteh-web-public-{:032x}",
            u128::from_ne_bytes(random)
        ));
        fs::create_dir(&root)?;
        let root = fs::canonicalize(root)?;
        for dir in ["project", "project/.benitoiteh", "tmp", "spill"] {
            fs::create_dir(root.join(dir))?;
        }
        Ok(Self { root })
    }
    fn context(&self, mode: Mode) -> Result<ToolContext, Box<dyn Error>> {
        let paths = DataPaths::from_values(Some(&self.root), None, None, Some(&self.root))?;
        Ok(ToolContext::new(
            mode,
            ScopeRoot::open(self.root.join("project"))?,
            ScopeRoot::open(self.root.join("tmp"))?,
            ScopeRoot::open(self.root.join("spill"))?,
            ExcludeRules::new(
                &self.root.join("project"),
                &paths,
                &self.root.join("spill"),
                &ReadConfig::default(),
                &ReadConfig::default(),
            )?,
        ))
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _removed = fs::remove_dir_all(&self.root);
    }
}

// 作成時の関門: プロジェクトの設定が許可を広げる退行と、無効な一覧を黙って受ける退行を設定の境界で捕まえる。
#[test]
fn domains_are_validated_normalized_and_cannot_be_set_by_the_project() -> TestResult {
    for domain in [
        "*.x",
        "https://x",
        "x:443",
        "1.2.3.4",
        "2130706433",
        "0x7f000001",
        "a..com",
        "日本.jp",
        "",
        "x.",
        "localhost",
        "machine.",
    ] {
        let input = format!(
            "[web_fetch]\nallowed_domains = [{}]\n",
            serde_json::to_string(domain)?
        );
        assert!(UserConfig::parse(&input).is_err(), "{domain}");
    }
    let config =
        UserConfig::parse("[web_fetch]\nallowed_domains=['EXAMPLE.COM.', 'xn--wgv71a.jp']")?;
    assert_eq!(
        config.web_fetch.allowed_domains,
        ["example.com", "xn--wgv71a.jp"]
    );
    let f = Fixture::new()?;
    fs::write(
        f.root.join("project/.benitoiteh/config.toml"),
        "model='ignored'\n[web_fetch]\nallowed_domains=['mock.invalid']\n",
    )?;
    let loaded = ProjectConfig::load(&ScopeRoot::open(f.root.join("project"))?);
    assert!(loaded.read && loaded.error.is_some());
    assert!(loaded.config.model.is_none());
    assert!(
        loaded
            .error
            .is_some_and(|e| e.to_string().contains("web_fetch"))
    );
    Ok(())
}

#[test]
fn web_fetch_is_serial_in_both_modes_and_untrusted_json_cannot_approve_it() -> TestResult {
    let f = Fixture::new()?;
    for mode in [Mode::Normal, Mode::Restricted] {
        let tools = ToolSet::new(mode);
        let definition = tools.definitions().last().ok_or("missing definition")?;
        assert_eq!(definition.id.name(), "web_fetch");
        assert!(!definition.parallel);
        let context = f.context(mode)?;
        for arguments in [
            json!({}),
            json!({"url":1}),
            json!({"url":"https://example.invalid/", "approved":true}),
            json!({"url":"https://example.invalid/", "domain":"localhost"}),
        ] {
            let output = tools.execute(&context, "web_fetch", arguments);
            assert!(output.is_error && output.approval.is_none());
            assert!(output.text.contains("Expected arguments"));
        }
        let output = tools.execute(
            &context,
            "web_fetch",
            json!({"url":"https://EXAMPLE.INVALID.:443/path#fragment"}),
        );
        let request = output.approval.ok_or("missing approval")?;
        assert_eq!(
            request.facts.url.as_deref(),
            Some("https://example.invalid/path")
        );
        assert_eq!(request.facts.domain.as_deref(), Some("example.invalid"));
        // 新しい欄のない以前の記録も読める。
        let old: benitoiteh::approval::ApprovalFacts = serde_json::from_value(
            json!({"command":null,"cwd":null,"path":null,"sandbox_policy":null}),
        )?;
        assert!(old.url.is_none() && old.domain.is_none());
    }
    Ok(())
}
