//! プラグインの検査と読み取り範囲を探索と道具の境界で確かめる（設計書 01-04）。

use super::*;
use benitoiteh::config::UserPluginsConfig;

const SCHEMA: &str = "https://agent-plugins.org/schemas/1.0.0/plugin.schema.json";

impl Fixture {
    fn plugin(
        &self,
        base: &str,
        name: &str,
        fields: serde_json::Value,
    ) -> Result<PathBuf, Box<dyn Error>> {
        let path = format!("{base}/{name}/plugin.json");
        self.write(path, serde_json::to_vec(&fields)?)?
            .parent()
            .map(Path::to_path_buf)
            .ok_or("plugin root".into())
    }

    pub(super) fn manifest(&self, base: &str, name: &str) -> Result<PathBuf, Box<dyn Error>> {
        self.plugin(
            base,
            name,
            json!({"$schema":SCHEMA,"name":name,"version":"1.2.3"}),
        )
    }

    fn with_plugins(
        &self,
        skills: &UserSkillsConfig,
        plugins: &UserPluginsConfig,
    ) -> (skills::UserSkills, Vec<ScopeRoot>) {
        skills::collect(&self.project, &self.home, skills, plugins, &self.exclude)
    }
}

// 関門: 四段階と祖先の優先順位、二つの設定の独立性は、索引を持つ探索の境界で守る。
#[test]
fn discovers_plugins_in_four_stages_and_obeys_independent_switches() -> TestResult {
    let f = Fixture::new()?;
    f.skill("home/.agents/skills", "common", "plain user")?;
    for (base, plugin, name, description) in [
        ("home/.agents/plugins", "a", "common", "loses to user"),
        ("home/.agents/plugins", "a", "from-user", "user plugin"),
        ("home/.agents/plugins", "b", "from-user", "loses to a"),
        (
            "repo/.agents/plugins",
            "c",
            "from-user",
            "loses to user plugin",
        ),
        (
            "repo/.agents/plugins",
            "c",
            "project",
            "loses to plain project",
        ),
        ("repo/.agents/plugins", "c", "root", "root plugin"),
        (
            "repo/sub/.agents/plugins",
            "c",
            "ignored",
            "duplicate plugin",
        ),
        (
            "repo/sub/project/.agents/plugins",
            "d",
            "root",
            "loses to root",
        ),
        (
            "repo/sub/project/.agents/plugins",
            "d",
            "leaf",
            "leaf plugin",
        ),
    ] {
        f.manifest(base, plugin)?;
        f.skill(&format!("{base}/{plugin}/skills"), name, description)?;
    }
    f.skill(
        "repo/sub/project/.agents/skills",
        "project",
        "plain project",
    )?;
    f.manifest(".agents/plugins", "outside")?;
    // 利用者のプラグインと同名のプロジェクトのものも、部品ごと読まない。
    f.manifest("repo/.agents/plugins", "a")?;
    f.skill("repo/.agents/plugins/a/skills", "never", "duplicate")?;
    for (user, project, expected_plugins) in [
        (true, true, vec!["a", "b", "c", "d"]),
        (true, false, vec!["a", "b"]),
        (false, true, vec!["a", "c", "d"]),
        (false, false, vec![]),
    ] {
        let (collected, _) = f.with_plugins(
            &UserSkillsConfig::default(),
            &UserPluginsConfig { user, project },
        );
        assert_eq!(
            collected
                .plugins
                .iter()
                .map(|p| p.name.as_str())
                .collect::<Vec<_>>(),
            expected_plugins
        );
        assert!(
            collected
                .loaded
                .iter()
                .any(|s| s.name == "common" && s.plugin.is_none())
        );
        assert!(
            collected
                .loaded
                .iter()
                .any(|s| s.name == "project" && s.plugin.is_none())
        );
        if user && project {
            assert_eq!(collected.skipped_plugins.len(), 2);
            assert!(
                collected
                    .skipped_plugins
                    .iter()
                    .all(|p| p.reason.contains("takes precedence"))
            );
            assert_eq!(
                collected
                    .loaded
                    .iter()
                    .map(|s| (s.name.as_str(), s.description.as_str()))
                    .collect::<Vec<_>>(),
                [
                    ("common", "plain user"),
                    ("from-user", "user plugin"),
                    ("leaf", "leaf plugin"),
                    ("project", "plain project"),
                    ("root", "root plugin")
                ]
            );
            assert!(
                collected
                    .plugins
                    .iter()
                    .all(|p| p.version.as_deref() == Some("1.2.3") && p.sha256.len() == 64)
            );
        }
    }
    let (collected, _) = f.with_plugins(
        &UserSkillsConfig {
            user: false,
            project: false,
        },
        &UserPluginsConfig::default(),
    );
    assert!(collected.loaded.iter().all(|s| s.plugin.is_some()));
    assert!(collected.loaded.iter().any(|s| s.name == "common"));
    Ok(())
}

// 関門: 不正なマニフェストが一部の Skill だけを読み込ませる退行を、表の各入力で防ぐ。
#[test]
fn validates_manifest_fields_before_loading_any_component() -> TestResult {
    let f = Fixture::new()?;
    let base = "home/.agents/plugins";
    f.skill("home/.agents/plugins/p/skills", "sample", "valid component")?;
    let valid = json!({"$schema":SCHEMA,"name":"p"});
    let mut cases = vec![(json!([]), "JSON object"), (json!(null), "JSON object")];
    for schema in [
        None,
        Some(json!(null)),
        Some(json!(1)),
        Some(json!(
            "https://agent-plugins.org/schemas/2.0.0/plugin.schema.json"
        )),
    ] {
        let mut value = valid.clone();
        let fields = value.as_object_mut().ok_or("fields")?;
        if let Some(schema) = schema {
            fields.insert("$schema".into(), schema);
        } else {
            fields.remove("$schema");
        }
        cases.push((value, "other Agent Plugins versions are not supported"));
    }
    for name in [
        json!(null),
        json!(1),
        json!(""),
        json!("a".repeat(65)),
        json!("A"),
        json!("あ"),
        json!("a_b"),
        json!("-p"),
        json!("p."),
        json!("a--b"),
        json!("a..b"),
    ] {
        let mut value = valid.clone();
        value
            .as_object_mut()
            .ok_or("fields")?
            .insert("name".into(), name);
        cases.push((value, "Plugin name must contain"));
    }
    cases.push((json!({"$schema":SCHEMA}), "Plugin name must contain"));
    cases.push((json!({"$schema":SCHEMA,"name":"different"}), "match both"));
    for field in [
        "version",
        "description",
        "homepage",
        "repository",
        "license",
        "author",
        "keywords",
    ] {
        let mut value = valid.clone();
        value
            .as_object_mut()
            .ok_or("fields")?
            .insert(field.into(), json!(42));
        cases.push((value, "invalid type"));
    }
    for author in [
        json!({"extra":"x"}),
        json!({"name":null}),
        json!({"email":3}),
        json!({"url":[]}),
    ] {
        cases.push((
            json!({"$schema":SCHEMA,"name":"p","author":author}),
            "field author has an invalid type",
        ));
    }
    cases.push((
        json!({"$schema":SCHEMA,"name":"p","keywords":["good",false]}),
        "field keywords has an invalid type",
    ));
    for (input, reason) in cases {
        f.plugin(base, "p", input.clone())?;
        let (collected, roots) = f.collect(&UserSkillsConfig::default());
        assert!(
            collected.plugins.is_empty() && collected.loaded.is_empty() && roots.is_empty(),
            "{input}"
        );
        assert_eq!(collected.skipped_plugins.len(), 1, "{input}");
        assert!(
            collected
                .skipped_plugins
                .first()
                .is_some_and(|p| p.reason.contains(reason)),
            "{input}: {:?}",
            collected.skipped_plugins
        );
    }
    for name in ["p", "1", "a.b-c", "a.-b", &"a".repeat(64)] {
        fs::remove_dir_all(f.home.join(".agents/plugins"))?;
        f.plugin(base, name, json!({"$schema":SCHEMA,"name":name,"author":{"name":"n","email":"e","url":"u"},"keywords":["k"],"version":"1","description":"","homepage":"","repository":"","license":"","extensions":{"unknown":null}}))?;
        let collected = f.collect(&UserSkillsConfig::default()).0;
        assert_eq!(
            collected.plugins.len(),
            1,
            "{name}: {:?}",
            collected.skipped_plugins
        );
        assert!(collected.plugin_notices.is_empty());
    }
    Ok(())
}

// 関門: 未知の欄と使わない部品を拒否してしまう、または MCP を開いて待つ退行を防ぐ。
#[test]
fn reports_bounded_unknown_fields_extensions_and_mcp_without_reading_them() -> TestResult {
    let f = Fixture::new()?;
    let mut fields = json!({"$schema":SCHEMA,"name":"p","extensions":false});
    for n in (0..13).rev() {
        fields
            .as_object_mut()
            .ok_or("fields")?
            .insert(format!("unknown{n:02}"), json!(false));
    }
    let root = f.plugin("home/.agents/plugins", "p", fields)?;
    assert!(
        std::process::Command::new("mkfifo")
            .arg(root.join("mcp.json"))
            .status()?
            .success()
    );
    f.skill("home/.agents/plugins/p/skills", "sample", "valid")?;
    let collected = f.collect(&UserSkillsConfig::default()).0;
    assert_eq!(collected.plugins.len(), 1);
    assert_eq!(collected.loaded.len(), 1);
    // P5-24 で期待を変えた: 一つのプラグインの通知を一行にまとめ、リポジトリが名前を選べる
    // 知らない欄の文を最後に置く（設計書 01-04 の「長さと数の上限」）。
    assert_eq!(collected.plugin_notices.len(), 1);
    let notice = collected.plugin_notices.first().ok_or("notice")?;
    assert_eq!(notice.path, root);
    let fixed = "Ignored plugin.json extensions because it is not an object. mcp.json is present; MCP servers from this plugin are not used. ";
    let unknown = notice
        .reason
        .strip_prefix(fixed)
        .ok_or("fixed notices first")?;
    assert!(unknown.starts_with("Ignored unknown plugin.json fields: unknown00, unknown01,"));
    assert!(unknown.ends_with("unknown09 (and 3 more)."));
    assert!(!unknown.contains("unknown10"));
    Ok(())
}

// 関門: マニフェストのファイルと根の信頼境界は、JSON の検査だけでは守れない。
#[test]
fn rejects_unsafe_plugin_roots_and_manifest_files() -> TestResult {
    for (kind, reason) in [
        ("outside-root", "outside the search scope"),
        ("alias", "match both"),
        ("broad", "ancestor"),
        ("outside-file", "outside the search scope"),
        ("hardlink", "hard link"),
        ("fifo", "regular file"),
        ("directory", "regular file"),
        ("secret", "Plugin is excluded by the read rules: "),
        ("secret", "(.env.Skill-SKILL.md)"),
        ("oversize", "256 KiB"),
        ("utf8", "UTF-8"),
        ("json", "JSON object"),
    ] {
        let f = Fixture::new()?;
        let base = f.home.join(".agents/plugins");
        fs::create_dir_all(&base)?;
        let candidate = base.join("p");
        let manifest = format!("{{\"$schema\":\"{SCHEMA}\",\"name\":\"p\"}}");
        match kind {
            "outside-root" => {
                f.write("outside/p/plugin.json", &manifest)?;
                symlink("../../../outside/p", &candidate)?;
            }
            "alias" => {
                f.manifest("home/elsewhere", "real")?;
                symlink("../../elsewhere/real", &candidate)?;
            }
            "broad" => {
                f.write("home/plugin.json", &manifest)?;
                symlink("../..", &candidate)?;
            }
            _ => {
                fs::create_dir(&candidate)?;
                let file = candidate.join("plugin.json");
                match kind {
                    "outside-file" => {
                        let path = f.write("home/manifest.json", &manifest)?;
                        symlink(path, file)?;
                    }
                    "hardlink" => {
                        let path = f.write("home/manifest.json", &manifest)?;
                        fs::hard_link(path, file)?;
                    }
                    "fifo" => assert!(
                        std::process::Command::new("mkfifo")
                            .arg(file)
                            .status()?
                            .success()
                    ),
                    "directory" => fs::create_dir(file)?,
                    "secret" => {
                        // 理由の中のパスの要素名は、Skill の文からの置換で書き換えない。
                        fs::write(candidate.join(".env.Skill-SKILL.md"), &manifest)?;
                        symlink(".env.Skill-SKILL.md", file)?;
                    }
                    "oversize" => fs::write(file, vec![b' '; 256 * 1024 + 1])?,
                    "utf8" => fs::write(file, [0xff])?,
                    "json" => fs::write(file, "{")?,
                    _ => return Err("case".into()),
                }
            }
        }
        let collected = f.collect(&UserSkillsConfig::default()).0;
        assert!(collected.plugins.is_empty(), "{kind}");
        assert_eq!(collected.skipped_plugins.len(), 1, "{kind}");
        assert!(
            collected
                .skipped_plugins
                .first()
                .is_some_and(|p| p.reason.contains(reason)),
            "{kind}: {:?}",
            collected.skipped_plugins
        );
    }
    // 根の中へのリンクと plugin.json の中へのリンクは受け付け、実の根を記録する。
    let f = Fixture::new()?;
    let root = f.manifest("home/elsewhere", "p")?;
    fs::rename(root.join("plugin.json"), root.join("manifest.json"))?;
    symlink("manifest.json", root.join("plugin.json"))?;
    fs::create_dir_all(f.home.join(".agents/plugins"))?;
    symlink("../../elsewhere/p", f.home.join(".agents/plugins/p"))?;
    fs::create_dir(f.home.join(".agents/plugins/no-manifest"))?;
    fs::write(f.home.join(".agents/plugins/file"), "ignored")?;
    let collected = f.collect(&UserSkillsConfig::default()).0;
    assert_eq!(collected.plugins.len(), 1);
    assert_eq!(collected.plugins.first().map(|p| &p.path), Some(&root));
    assert!(collected.skipped_plugins.is_empty());
    Ok(())
}

// 関門: 部品一つの誤りで全体を失わず、道具へ渡す根をプラグインの外へ広げない。
#[test]
fn validates_each_skill_and_exposes_only_accepted_directories_to_tools() -> TestResult {
    let f = Fixture::new()?;
    let root = f.manifest("home/.agents/plugins", "p")?;
    f.skill("home/.agents/plugins/p/skills", "sample", "valid")?;
    f.skill("home/.agents/plugins/p/references", "linked", "inside link")?;
    symlink("../references/linked", root.join("skills/linked"))?;
    f.skill("home/.agents/plugins/p/skills", "benitoite", "reserved")?;
    f.write("home/.agents/plugins/p/skills/invalid/SKILL.md", "invalid")?;
    f.skill("home/elsewhere", "escaped", "outside plugin")?;
    symlink(
        f.home.join("elsewhere/escaped"),
        root.join("skills/escaped"),
    )?;
    f.write(
        "home/.agents/plugins/p/plugin-only.txt",
        "private plugin root",
    )?;
    f.write(
        "home/.agents/plugins/p/skills/sample/reference.txt",
        "reference",
    )?;
    for mode in [Mode::Restricted, Mode::Normal] {
        let (collected, roots) = f.collect(&UserSkillsConfig::default());
        assert_eq!(collected.plugins.len(), 1);
        assert_eq!(collected.skipped.len(), 3, "{:?}", collected.skipped);
        assert_eq!(
            collected
                .loaded
                .iter()
                .map(|s| s.name.as_str())
                .collect::<Vec<_>>(),
            ["linked", "sample"]
        );
        assert!(
            collected
                .recorded()
                .iter()
                .all(|s| s.plugin.as_deref() == Some("p"))
        );
        let context = f.context(mode, roots)?;
        let tools = ToolSet::new(mode);
        let output = tools.execute(
            &context,
            "read",
            json!({"path":root.join("skills/sample/reference.txt")}),
        );
        assert!(
            !output.is_error && output.text.contains("reference"),
            "{output:?}"
        );
        for path in [
            root.join("plugin-only.txt"),
            f.home.join("elsewhere/escaped/SKILL.md"),
        ] {
            if mode == Mode::Restricted {
                assert!(
                    tools
                        .execute(&context, "read", json!({"path":path}))
                        .is_error,
                    "{path:?}"
                );
            }
        }
    }
    Ok(())
}

// 関門: プラグインごとの別枠や、名前順だけで利用者の枠を失う退行を共通の上限で防ぐ。
#[test]
fn shares_count_and_byte_limits_and_caps_plugin_entries() -> TestResult {
    let f = Fixture::new()?;
    for (base, name) in [
        ("home/.agents/plugins", "a"),
        ("home/.agents/plugins", "b"),
        ("repo/.agents/plugins", "c"),
    ] {
        f.manifest(base, name)?;
    }
    for n in 0..20 {
        f.skill("home/.agents/skills", &format!("u{n:02}"), "x")?;
    }
    for n in 0..40 {
        f.skill("home/.agents/plugins/a/skills", &format!("v{n:02}"), "x")?;
    }
    for n in 0..20 {
        f.skill("home/.agents/plugins/b/skills", &format!("w{n:02}"), "x")?;
    }
    for n in 0..20 {
        f.skill("repo/.agents/skills", &format!("x{n:02}"), "x")?;
    }
    f.skill(
        "repo/.agents/plugins/c/skills",
        "a-project",
        "loses despite name",
    )?;
    let (collected, roots) = f.collect(&UserSkillsConfig::default());
    assert_eq!(collected.loaded.len(), 100, "{:?}", collected.skipped);
    assert_eq!(roots.len(), 100);
    assert_eq!(collected.skipped.len(), 1);
    assert!(
        collected
            .skipped
            .first()
            .is_some_and(|s| s.reason.contains("100-skill"))
    );
    drop(roots);
    // プラグイン名の順を Skill 名より先に数え、バイト上限後も次の段へ余りを渡す。
    fs::remove_dir_all(f.home.join(".agents/skills"))?;
    fs::remove_dir_all(f.home.join(".agents/plugins/a/skills"))?;
    fs::remove_dir_all(f.home.join(".agents/plugins/b/skills"))?;
    fs::remove_dir_all(f.root.join("repo/.agents/skills"))?;
    for name in ["z1", "z2", "z3", "z4"] {
        f.skill("home/.agents/plugins/a/skills", name, &"&".repeat(1024))?;
    }
    f.skill(
        "home/.agents/plugins/b/skills",
        "aaa",
        "same stage after cutoff",
    )?;
    let collected = f.collect(&UserSkillsConfig::default()).0;
    assert_eq!(
        collected
            .loaded
            .iter()
            .map(|s| s.name.as_str())
            .collect::<Vec<_>>(),
        ["a-project", "z1", "z2", "z3"]
    );
    assert_eq!(
        collected.loaded.first().and_then(|s| s.plugin.as_deref()),
        Some("c")
    );
    assert!(
        collected
            .skipped
            .iter()
            .all(|s| s.reason.contains("16 KiB"))
    );
    fs::remove_dir_all(f.home.join(".agents/plugins"))?;
    for n in 0..1003 {
        fs::create_dir_all(f.home.join(format!(".agents/plugins/p{n:04}")))?;
    }
    for name in ["p0000", "p0999", "p1000"] {
        f.manifest("home/.agents/plugins", name)?;
    }
    let collected = f.collect(&UserSkillsConfig::default()).0;
    assert_eq!(
        collected
            .plugins
            .iter()
            .filter(|p| p.source == SkillSource::User)
            .map(|p| p.name.as_str())
            .collect::<Vec<_>>(),
        ["p0000", "p0999"]
    );
    assert!(
        collected
            .skipped_plugins
            .iter()
            .any(|p| p.reason.contains("Skipped 3 entries"))
    );
    Ok(())
}

// 関門: 信頼しない版・欄の名前・多数のプラグインで記録と画面が膨らむ退行を、収集の境界で防ぐ
// （設計書 01-04「Agent Plugins のプラグイン」の「長さと数の上限」）。
#[test]
fn bounds_versions_unknown_field_names_and_plugins_per_source() -> TestResult {
    let f = Fixture::new()?;
    let base = "home/.agents/plugins";
    for (version, expected) in [
        ("1.2.3+build_7-rc", Some("1.2.3+build_7-rc")),
        (&"9".repeat(64), Some(&*"9".repeat(64))),
        ("", None),
        (&"9".repeat(65), None),
        ("1.0 (sha256 0)", None),
        ("1.0\u{1b}", None),
        ("１", None),
    ] {
        f.plugin(
            base,
            "p",
            json!({"$schema":SCHEMA,"name":"p","version":version}),
        )?;
        let collected = f.collect(&UserSkillsConfig::default()).0;
        assert_eq!(
            collected.plugins.first().map(|p| p.version.as_deref()),
            Some(expected),
            "{version:?}"
        );
        assert_eq!(
            collected
                .plugin_notices
                .iter()
                .any(|n| n.reason.contains("Ignored plugin.json version")),
            expected.is_none(),
            "{version:?}"
        );
    }
    let long = format!("x{}", "y".repeat(199));
    f.plugin(
        base,
        "p",
        json!({"$schema":SCHEMA,"name":"p",long.clone():true}),
    )?;
    let collected = f.collect(&UserSkillsConfig::default()).0;
    let notice = &collected.plugin_notices.first().ok_or("notice")?.reason;
    assert_eq!(
        notice,
        &format!("Ignored unknown plugin.json fields: {}....", &long[..64])
    );
    fs::remove_dir_all(f.home.join(".agents/plugins"))?;
    for n in 0..102 {
        f.manifest(base, &format!("u{n:03}"))?;
    }
    f.manifest("repo/.agents/plugins", "project")?;
    let collected = f.collect(&UserSkillsConfig::default()).0;
    assert_eq!(
        collected
            .plugins
            .iter()
            .filter(|p| p.source == SkillSource::User)
            .count(),
        100
    );
    assert!(
        collected
            .plugins
            .iter()
            .any(|p| p.name == "project" && p.source == SkillSource::Project)
    );
    assert_eq!(
        collected
            .skipped_plugins
            .iter()
            .map(|p| p.reason.as_str())
            .collect::<Vec<_>>(),
        ["Skipped 2 entries beyond the 100-plugin limit for this source."]
    );
    Ok(())
}

// 関門: plugin.json を読む前に断った項目を 100 個の上限に数えず、範囲の外へのリンクを
// 並べて後ろのプラグインを多数開かせる退行を防ぐ（`counted` の扱い、設計書 01-04）。
#[test]
fn entries_rejected_before_reading_the_manifest_count_toward_the_plugin_limit() -> TestResult {
    let f = Fixture::new()?;
    f.write(
        "outside/plugin.json",
        format!("{{\"$schema\":\"{SCHEMA}\",\"name\":\"outside\"}}"),
    )?;
    let base = f.home.join(".agents/plugins");
    fs::create_dir_all(&base)?;
    for n in 0..100 {
        symlink("../../../outside", base.join(format!("a{n:03}")))?;
    }
    f.manifest("home/.agents/plugins", "z")?;
    f.skill("home/.agents/plugins/z/skills", "late", "must not load")?;
    let collected = f.collect(&UserSkillsConfig::default()).0;
    assert!(collected.plugins.is_empty(), "{:?}", collected.plugins);
    assert!(collected.loaded.is_empty());
    let reasons = collected
        .skipped_plugins
        .iter()
        .map(|p| p.reason.as_str())
        .collect::<Vec<_>>();
    assert_eq!(reasons.len(), 101, "{reasons:?}");
    assert!(
        reasons
            .iter()
            .take(100)
            .all(|r| *r == "Plugin path is outside the search scope.")
    );
    assert_eq!(
        reasons.last(),
        Some(&"Skipped 1 entries beyond the 100-plugin limit for this source.")
    );
    Ok(())
}

// 関門: 範囲の中を指す絶対のパスのリンクを、読むか、範囲の外の理由で示す退行を、
// 二つの段と Skill・プラグインの四つで防ぐ（設計書 01-04「利用者の Agent Skill」）。
#[test]
fn absolute_links_inside_the_scope_are_not_read_and_have_their_own_reason() -> TestResult {
    let skill_reason =
        "Skill path is an absolute symbolic link; use a relative link inside the search scope.";
    let plugin_reason =
        "Plugin path is an absolute symbolic link; use a relative link inside the search scope.";
    for (stage, base) in [("user", "home"), ("project", "repo")] {
        let f = Fixture::new()?;
        f.skill(&format!("{base}/elsewhere"), "abs", "inside the scope")?;
        f.manifest(&format!("{base}/elsewhere"), "absplugin")?;
        for kind in ["skills", "plugins"] {
            fs::create_dir_all(f.root.join(base).join(".agents").join(kind))?;
        }
        let skill_link = f.root.join(base).join(".agents/skills/abs");
        let plugin_link = f.root.join(base).join(".agents/plugins/absplugin");
        symlink(f.root.join(base).join("elsewhere/abs"), &skill_link)?;
        symlink(f.root.join(base).join("elsewhere/absplugin"), &plugin_link)?;
        let collected = f.collect(&UserSkillsConfig::default()).0;
        assert!(
            collected.loaded.is_empty(),
            "{stage}: {:?}",
            collected.loaded
        );
        assert!(
            collected.plugins.is_empty(),
            "{stage}: {:?}",
            collected.plugins
        );
        assert_eq!(
            collected
                .skipped
                .iter()
                .map(|s| (s.path.clone(), s.reason.as_str()))
                .collect::<Vec<_>>(),
            [(skill_link.join("SKILL.md"), skill_reason)],
            "{stage}"
        );
        assert_eq!(
            collected
                .skipped_plugins
                .iter()
                .map(|p| (p.path.clone(), p.reason.as_str()))
                .collect::<Vec<_>>(),
            [(plugin_link, plugin_reason)],
            "{stage}"
        );
    }
    Ok(())
}

// 関門: 候補の上限で開かなかった利用者のプラグインの Skill の名前を覚えず、同名の
// プロジェクトの Skill が読まれて優先順位が変わる退行を防ぐ（設計書 01-04）。
#[test]
fn plugin_skill_candidates_beyond_the_limit_still_shadow_later_stages() -> TestResult {
    let f = Fixture::new()?;
    let a = f.manifest("home/.agents/plugins", "a")?;
    for n in 0..1000 {
        fs::create_dir_all(a.join(format!("skills/e{n:04}")))?;
    }
    f.manifest("home/.agents/plugins", "b")?;
    f.skill("home/.agents/plugins/b/skills", "dup", "beyond the limit")?;
    f.skill("repo/sub/project/.agents/skills", "dup", "project")?;
    let collected = f.collect(&UserSkillsConfig::default()).0;
    assert!(collected.loaded.is_empty(), "{:?}", collected.loaded);
    assert_eq!(
        collected
            .skipped
            .iter()
            .map(|s| (s.path.clone(), s.reason.as_str()))
            .collect::<Vec<_>>(),
        [
            (
                f.project.join(".agents/skills/dup/SKILL.md"),
                "Another skill with this name takes precedence."
            ),
            (
                f.home.join(".agents/plugins/b/skills"),
                "Skipped 1 plugin skill entries beyond the first 1000 for this source without opening them."
            ),
        ]
    );
    Ok(())
}

// 関門: 上限を越える Skill を開いて読み、一件ずつ記録を膨らませる退行を防ぐ。
#[test]
fn skips_skills_beyond_the_limits_without_opening_them() -> TestResult {
    let f = Fixture::new()?;
    for n in 0..100 {
        f.skill("home/.agents/skills", &format!("u{n:03}"), "x")?;
    }
    f.manifest("home/.agents/plugins", "few")?;
    f.manifest("home/.agents/plugins", "many")?;
    // 開けば書式の誤りとして示す内容にし、開かずに飛ばしたことを理由で区別する。
    for n in 0..3 {
        f.write(
            format!("home/.agents/plugins/few/skills/f{n}/SKILL.md"),
            "invalid",
        )?;
    }
    for n in 0..21 {
        f.write(
            format!("home/.agents/plugins/many/skills/m{n:02}/SKILL.md"),
            "invalid",
        )?;
    }
    let collected = f.collect(&UserSkillsConfig::default()).0;
    assert_eq!(collected.loaded.len(), 100);
    assert_eq!(collected.skipped.len(), 4, "{:?}", collected.skipped);
    assert!(
        collected
            .skipped
            .iter()
            .take(3)
            .all(|s| s.reason.contains("100-skill") && s.path.ends_with("SKILL.md"))
    );
    assert_eq!(
        collected
            .skipped
            .last()
            .map(|s| (&s.path, s.reason.as_str())),
        Some((
            &f.home.join(".agents/plugins/many/skills"),
            "Skipped 21 skills beyond the skill or index limit without reading them."
        ))
    );
    Ok(())
}

// 関門: プロジェクトのプラグインの根、skills/、SKILL.md のリンクで範囲を広げる退行を防ぐ。
#[test]
fn rejects_project_roots_skill_directories_and_skill_files_outside_their_bounds() -> TestResult {
    let f = Fixture::new()?;
    f.manifest("outside", "escaped")?;
    fs::create_dir_all(f.root.join("repo/.agents/plugins"))?;
    symlink(
        "../../../outside/escaped",
        f.root.join("repo/.agents/plugins/escaped"),
    )?;
    let linked = f.manifest("repo/.agents/plugins", "linked")?;
    f.skill("repo/elsewhere/skills", "away", "outside plugin root")?;
    symlink(f.root.join("repo/elsewhere/skills"), linked.join("skills"))?;
    let file = f.manifest("repo/.agents/plugins", "file")?;
    f.write(
        "repo/.agents/plugins/file/copy.md",
        "---\nname: copy\ndescription: outside skill directory\n---\n",
    )?;
    fs::create_dir_all(file.join("skills/copy"))?;
    symlink("../../copy.md", file.join("skills/copy/SKILL.md"))?;
    let collected = f.collect(&UserSkillsConfig::default()).0;
    assert_eq!(
        collected
            .plugins
            .iter()
            .map(|p| p.name.as_str())
            .collect::<Vec<_>>(),
        ["file", "linked"]
    );
    assert!(collected.loaded.is_empty(), "{:?}", collected.loaded);
    assert_eq!(
        collected
            .skipped_plugins
            .iter()
            .map(|p| (p.path.clone(), p.reason.as_str()))
            .collect::<Vec<_>>(),
        [(
            f.root.join("repo/.agents/plugins/escaped"),
            "Plugin path is outside the search scope."
        )]
    );
    let mut skipped = collected
        .skipped
        .iter()
        .map(|s| (s.path.clone(), s.reason.as_str()))
        .collect::<Vec<_>>();
    skipped.sort();
    assert_eq!(
        skipped,
        [
            (
                file.join("skills/copy/SKILL.md"),
                "Skill path is outside the search scope."
            ),
            (
                linked.join("skills"),
                "Skill path is outside the search scope."
            ),
        ]
    );
    Ok(())
}
