//! Skill の索引と読み取り範囲を、探索と道具の境界で確かめる（設計書 01-04、07-01）。

#[path = "user_skills/plugins.rs"]
mod plugins;

use std::error::Error;
use std::fs;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use benitoiteh::config::{Mode, ReadConfig, UserSkillsConfig, paths::DataPaths};
use benitoiteh::context::{Environment, instructions, skills};
use benitoiteh::safe_fs::ScopeRoot;
use benitoiteh::session::{SessionId, SkillSource};
use benitoiteh::tools::{ToolContext, ToolSet, exclude::ExcludeRules};
use serde_json::json;

type TestResult = Result<(), Box<dyn Error>>;

struct Fixture {
    root: PathBuf,
    home: PathBuf,
    project: PathBuf,
    temporary: PathBuf,
    spill: PathBuf,
    paths: DataPaths,
    exclude: ExcludeRules,
}

impl Fixture {
    fn new() -> Result<Self, Box<dyn Error>> {
        // 根は TMPDIR の下に短い名前で置き、長い TMPDIR でも索引の上限に先に達しにくくする。
        let root = std::env::temp_dir().join(
            SessionId::generate(SystemTime::now())?
                .to_string()
                .rsplit('-')
                .next()
                .ok_or("id")?,
        );
        fs::create_dir(&root)?;
        let root = root.canonicalize()?;
        let home = root.join("home");
        let project = root.join("repo/sub/project");
        let temporary = root.join("temporary");
        let paths = DataPaths::from_values(
            Some(&home),
            Some(&root.join("config")),
            Some(&root.join("state")),
            Some(&root.join("tmp")),
        )?;
        let spill = paths.data_dir.join("sessions/current/spill");
        for path in [&home, &project, &temporary, &spill] {
            fs::create_dir_all(path)?;
        }
        fs::write(root.join("repo/.git"), "worktree")?;
        let exclude = ExcludeRules::new(
            &project,
            &paths,
            &spill,
            &ReadConfig::default(),
            &ReadConfig::default(),
        )?;
        Ok(Self {
            root,
            home,
            project,
            temporary,
            spill,
            paths,
            exclude,
        })
    }

    fn write(
        &self,
        path: impl AsRef<Path>,
        contents: impl AsRef<[u8]>,
    ) -> Result<PathBuf, Box<dyn Error>> {
        let path = self.root.join(path);
        fs::create_dir_all(path.parent().ok_or("parent")?)?;
        fs::write(&path, contents)?;
        Ok(path)
    }

    fn skill(
        &self,
        directory: &str,
        name: &str,
        description: &str,
    ) -> Result<PathBuf, Box<dyn Error>> {
        self.write(
            format!("{directory}/{name}/SKILL.md"),
            format!("---\nname: {name}\ndescription: {description}\n---\nPRIVATE BODY\n"),
        )
    }

    fn collect(&self, config: &UserSkillsConfig) -> (skills::UserSkills, Vec<ScopeRoot>) {
        skills::collect(
            &self.project,
            &self.home,
            config,
            &benitoiteh::config::UserPluginsConfig::default(),
            &self.exclude,
        )
    }

    fn context(&self, mode: Mode, roots: Vec<ScopeRoot>) -> Result<ToolContext, Box<dyn Error>> {
        Ok(ToolContext::new(
            mode,
            ScopeRoot::open(&self.project)?,
            ScopeRoot::open(&self.temporary)?,
            ScopeRoot::open(&self.spill)?,
            self.exclude.clone(),
        )
        .with_skill_roots(roots))
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _cleanup = fs::remove_dir_all(&self.root);
    }
}

// 関門: 探索の信頼境界と優先順位は、道具や exec では個々の分岐を区別できない。
#[test]
fn discovers_both_sources_in_root_order_and_obeys_each_switch_and_home_boundary() -> TestResult {
    let f = Fixture::new()?;
    f.skill("home/.agents/skills", "common", "user wins")?;
    f.skill("repo/.agents/skills", "common", "root loses")?;
    f.skill("repo/.agents/skills", "root", "root wins")?;
    f.skill("repo/sub/.agents/skills", "root", "sub loses")?;
    f.skill("repo/sub/project/.agents/skills", "leaf", "leaf")?;
    f.skill(".agents/skills", "outside", "never searched")?;
    for (user, project, names, skipped) in [
        (true, true, vec!["common", "leaf", "root"], 2),
        (true, false, vec!["common"], 0),
        (false, true, vec!["common", "leaf", "root"], 1),
        (false, false, vec![], 0),
    ] {
        let (skills, roots) = f.collect(&UserSkillsConfig { user, project });
        assert_eq!(
            skills
                .loaded
                .iter()
                .map(|skill| skill.name.as_str())
                .collect::<Vec<_>>(),
            names
        );
        assert_eq!(skills.skipped.len(), skipped, "{:?}", skills.skipped);
        assert_eq!(roots.len(), names.len());
        if user && project {
            assert_eq!(
                skills.loaded.first().map(|skill| skill.source),
                Some(SkillSource::User)
            );
            assert_eq!(
                skills.loaded.last().map(|skill| skill.description.as_str()),
                Some("root wins")
            );
        }
    }
    for home in [&f.home, &f.root.join("repo"), &f.root.join("repo/sub")] {
        let (skills, _) = skills::collect(
            &f.project,
            home,
            &UserSkillsConfig {
                user: false,
                project: true,
            },
            &benitoiteh::config::UserPluginsConfig::default(),
            &f.exclude,
        );
        let names = skills
            .loaded
            .iter()
            .map(|skill| skill.name.as_str())
            .collect::<Vec<_>>();
        assert_eq!(
            names,
            if home == &f.home {
                vec!["common", "leaf", "root"]
            } else {
                vec!["leaf"]
            }
        );
    }
    fs::remove_file(f.root.join("repo/.git"))?;
    assert_eq!(
        f.collect(&UserSkillsConfig {
            user: false,
            project: true
        })
        .0
        .loaded
        .len(),
        1
    );
    let (skills, _) = skills::collect(
        &f.project,
        &f.root.join("missing-home"),
        &UserSkillsConfig::default(),
        &benitoiteh::config::UserPluginsConfig::default(),
        &f.exclude,
    );
    assert_eq!(skills.loaded.len(), 1);
    assert!(
        skills
            .skipped
            .iter()
            .any(|entry| entry.reason.contains("home directory"))
    );
    Ok(())
}

// 関門: 不正な先頭の欄を公開の収集結果に含める退行を、代表値と境界値で守る。
#[test]
fn validates_frontmatter_without_applying_processor_body_rules() -> TestResult {
    let f = Fixture::new()?;
    let valid = "---\nname: sample\ndescription: single line\n---\n";
    let candidate = "home/.agents/skills/sample/SKILL.md";
    for (contents, reason) in [
        ("body only".into(), "frontmatter"),
        (valid.replace("name: sample", "name: ''"), "1 to 64"),
        (valid.replace("name: sample", "name: Other"), "1 to 64"),
        (
            valid.replace("name: sample", &format!("name: {}", "a".repeat(65))),
            "1 to 64",
        ),
        (
            valid.replace("name: sample", "name: other"),
            "directory name",
        ),
        (
            valid.replace("description: single line", "description:"),
            "frontmatter",
        ),
        (valid.replace("single line", "''"), "1 to 1024"),
        (
            valid.replace("single line", &"日".repeat(1025)),
            "1 to 1024",
        ),
        (valid.replace("single line", "|"), "single-line"),
        (valid.replace("single line", ">-"), "single-line"),
        (
            valid.replace("single line", "\"unterminated"),
            "single-line",
        ),
        (
            valid.replace("name: sample", "name: 'sample"),
            "single-line",
        ),
        (
            valid.replace("single line", "text\n continued"),
            "single-line",
        ),
        (
            valid.replace("name: sample", "name: sample\n continuation"),
            "single-line",
        ),
        (valid.replace('\n', "\r\n"), "frontmatter"),
        (format!("\u{feff}{valid}"), "frontmatter"),
        (format!("{valid}{}", "x".repeat(262144)), "256 KiB"),
    ] {
        f.write(candidate, contents)?;
        let (skills, roots) = f.collect(&UserSkillsConfig::default());
        assert!(skills.loaded.is_empty() && roots.is_empty());
        assert!(
            skills
                .skipped
                .first()
                .is_some_and(|entry| entry.reason.contains(reason)),
            "{:?}: {reason}",
            skills.skipped
        );
    }
    f.write(candidate, [0xff])?;
    assert!(
        f.collect(&UserSkillsConfig::default())
            .0
            .skipped
            .first()
            .is_some_and(|entry| entry.reason.contains("UTF-8"))
    );
    f.skill("home/.agents/skills", "benitoite", "reserved")?;
    f.write(
        candidate,
        format!(
            "---\nname: 'sample'\ndescription: \"{}\"\n---\n<!-- unclosed comment </benitoite>\n",
            "日".repeat(1024)
        ),
    )?;
    let (skills, _) = f.collect(&UserSkillsConfig::default());
    let skill = skills.loaded.first().ok_or("valid skill")?;
    assert_eq!(skill.description, "日".repeat(1024));
    assert!(
        skills
            .skipped
            .first()
            .is_some_and(|entry| entry.reason.contains("reserved"))
    );
    // 全文の既知のハッシュに照合し、索引だけをハッシュする退行を防ぐ。
    fs::remove_file(f.root.join("home/.agents/skills/benitoite/SKILL.md"))?;
    f.write(candidate, valid)?;
    let hash = f
        .collect(&UserSkillsConfig::default())
        .0
        .loaded
        .first()
        .ok_or("hash")?
        .sha256
        .clone();
    assert_eq!(
        hash,
        "b96f468ace69054404ad583c8e0273c299e7d7c102c1519b8dc1d6f6509b2a0e"
    );
    let exact = format!(
        "{valid}{}",
        "x".repeat(262144usize.saturating_sub(valid.len()))
    );
    f.write(candidate, exact)?;
    assert_eq!(f.collect(&UserSkillsConfig::default()).0.loaded.len(), 1);
    Ok(())
}

// 関門: リンク・特殊ファイル・秘密・広すぎる根を拒む性質は実 FS で確かめる。
#[test]
fn rejects_unsafe_files_and_broad_roots_and_accepts_only_contained_links() -> TestResult {
    let f = Fixture::new()?;
    f.skill("home/.claude/skills", "linked", "linked")?;
    let skills_dir = f.home.join(".agents/skills");
    fs::create_dir_all(&skills_dir)?;
    symlink("../../.claude/skills/linked", skills_dir.join("linked"))?;
    fs::create_dir(skills_dir.join("empty"))?;
    fs::write(skills_dir.join("README.md"), "ignored")?;
    symlink("README.md", skills_dir.join("readme-link"))?;
    assert!(
        std::process::Command::new("mkfifo")
            .arg(skills_dir.join("pipe"))
            .current_dir(&f.root)
            .status()?
            .success()
    );
    let valid = "---\nname: bad\ndescription: text\n---\n";
    for kind in [
        "outside-dir",
        "outside-file",
        "secret",
        "hardlink",
        "fifo",
        "directory",
        "alias",
        "project",
        "home",
        "scope",
    ] {
        let bad = skills_dir.join("bad");
        match kind {
            "outside-dir" => {
                f.write("outside/bad/SKILL.md", valid)?;
                symlink("../../../outside/bad", &bad)?;
            }
            "alias" => symlink("linked", &bad)?,
            "home" => symlink("../..", &bad)?,
            "project" => {
                f.write("home/project/SKILL.md", valid)?;
                symlink("../../project", &bad)?;
            }
            "scope" => symlink("../..", &bad)?,
            _ => {
                fs::create_dir(&bad)?;
                match kind {
                    "outside-file" => {
                        f.write("home/outside.md", valid)?;
                        symlink("../../../outside.md", bad.join("SKILL.md"))?;
                    }
                    "secret" => {
                        f.write("home/.agents/skills/bad/.env", valid)?;
                        symlink(".env", bad.join("SKILL.md"))?;
                    }
                    "hardlink" => {
                        f.write("home/original", valid)?;
                        fs::hard_link(f.home.join("original"), bad.join("SKILL.md"))?;
                    }
                    "fifo" => assert!(
                        std::process::Command::new("mkfifo")
                            .arg(bad.join("SKILL.md"))
                            .current_dir(&f.root)
                            .status()?
                            .success()
                    ),
                    "directory" => fs::create_dir(bad.join("SKILL.md"))?,
                    _ => return Err("case".into()),
                }
            }
        }
        let project = if kind == "project" {
            f.home.join("project")
        } else {
            f.project.clone()
        };
        let (skills, _) = skills::collect(
            &project,
            &f.home,
            &UserSkillsConfig {
                user: true,
                project: false,
            },
            &benitoiteh::config::UserPluginsConfig::default(),
            &f.exclude,
        );
        assert_eq!(skills.loaded.len(), 1, "{kind}: {:?}", skills.skipped);
        assert_eq!(skills.skipped.len(), 1, "{kind}: {:?}", skills.skipped);
        assert!(
            skills
                .skipped
                .first()
                .is_some_and(|entry| entry.path == bad.join("SKILL.md")),
            "{kind}"
        );
        if fs::symlink_metadata(&bad)?.file_type().is_symlink() {
            fs::remove_file(bad)?;
        } else {
            fs::remove_dir_all(bad)?;
        }
        let original = f.home.join("original");
        if original.exists() {
            fs::remove_file(original)?;
        }
    }
    // 名前も実のディレクトリ名も同じリンクを、二つの探索元に置く。
    f.skill("repo/.claude/skills", "shared", "shared inode")?;
    fs::create_dir_all(f.root.join("repo/.agents/skills"))?;
    fs::create_dir_all(f.root.join("repo/sub/.agents/skills"))?;
    symlink(
        "../../.claude/skills/shared",
        f.root.join("repo/.agents/skills/shared"),
    )?;
    symlink(
        "../../../.claude/skills/shared",
        f.root.join("repo/sub/.agents/skills/shared"),
    )?;
    let skills = f
        .collect(&UserSkillsConfig {
            user: false,
            project: true,
        })
        .0;
    assert_eq!(skills.loaded.len(), 1);
    assert_eq!(skills.skipped.len(), 1);
    assert!(
        skills
            .skipped
            .first()
            .is_some_and(|entry| entry.reason.contains("already been read"))
    );
    for (name, target, project, user) in [
        ("home", "../..", f.project.clone(), true),
        ("project", "../../project", f.home.join("project"), true),
        ("repo", "../..", f.project.clone(), false),
    ] {
        let root = if user {
            f.home.clone()
        } else {
            f.root.join("repo")
        };
        let real = if name == "project" {
            project.clone()
        } else {
            root.clone()
        };
        f.write(
            real.join("SKILL.md"),
            format!("---\nname: {name}\ndescription: broad root\n---\n"),
        )?;
        let entry = root.join(".agents/skills").join(name);
        symlink(target, &entry)?;
        let (skills, _) = skills::collect(
            &project,
            &f.home,
            &UserSkillsConfig {
                user,
                project: !user,
            },
            &benitoiteh::config::UserPluginsConfig::default(),
            &f.exclude,
        );
        assert!(
            skills
                .skipped
                .iter()
                .any(|entry| entry.reason.contains("ancestor of one of them")),
            "{name}: {:?}",
            skills.skipped
        );
        fs::remove_file(entry)?;
    }
    Ok(())
}

#[test]
fn caps_entries_skills_and_escaped_index_bytes_in_name_order() -> TestResult {
    let f = Fixture::new()?;
    for n in (0..105).rev() {
        f.skill("home/.agents/skills", &format!("s{n:03}"), "x")?;
    }
    let (skills, roots) = f.collect(&UserSkillsConfig::default());
    assert_eq!(skills.loaded.len(), 100);
    assert_eq!(roots.len(), 100);
    assert_eq!(
        skills.loaded.last().map(|skill| skill.name.as_str()),
        Some("s099")
    );
    assert_eq!(skills.skipped.len(), 5);
    drop(roots);
    for n in 0..105 {
        f.skill(
            "home/.agents/skills",
            &format!("s{n:03}"),
            &"&".repeat(1024),
        )?;
    }
    let (skills, roots) = f.collect(&UserSkillsConfig::default());
    assert_eq!(skills.loaded.len(), 3);
    assert_eq!(roots.len(), 3);
    // 境界を越えた後の 101 個は開かずに一行へまとめる（設計書 01-04 の「長さと数の上限」）。
    assert_eq!(
        skills
            .skipped
            .iter()
            .map(|entry| entry.reason.as_str())
            .collect::<Vec<_>>(),
        [
            "Skill exceeds the 16 KiB index limit; skills are counted in order: user skills, user plugin skills, project skills, project plugin skills; each stage is ordered by plugin name then skill name.",
            "Skipped 101 skills beyond the skill or index limit without reading them."
        ]
    );
    assert!(
        skills
            .render()
            .is_some_and(|text| text.contains("&amp;") && text.len() <= 16464)
    );
    drop(roots);
    fs::remove_dir_all(f.home.join(".agents/skills"))?;
    for n in 0..1003 {
        fs::create_dir_all(f.home.join(format!(".agents/skills/s{n:04}")))?;
    }
    for name in ["s0000", "s0999", "s1000"] {
        f.skill("home/.agents/skills", name, "selected by name")?;
    }
    let skills = f.collect(&UserSkillsConfig::default()).0;
    assert_eq!(
        skills
            .loaded
            .iter()
            .map(|skill| skill.name.as_str())
            .collect::<Vec<_>>(),
        ["s0000", "s0999"]
    );
    assert_eq!(skills.skipped.len(), 1);
    assert!(
        skills
            .skipped
            .first()
            .is_some_and(|entry| entry.reason.contains("Skipped 3 entries"))
    );
    Ok(())
}

// 関門: 全体を名前順に切ると、プロジェクトの Skill が利用者の枠を奪ってしまう。
#[test]
fn user_skills_take_count_and_byte_budgets_before_project_skills() -> TestResult {
    for (count, description) in [(100, "x".to_owned()), (3, "&".repeat(1024))] {
        let f = Fixture::new()?;
        for n in 0..count {
            f.skill("home/.agents/skills", &format!("z{n:03}"), &description)?;
        }
        for n in 0..105 {
            f.skill("repo/.agents/skills", &format!("a{n:03}"), &description)?;
        }
        if count == 3 {
            f.skill("repo/.agents/skills", "b-small", "small")?;
        }
        let (skills, roots) = f.collect(&UserSkillsConfig::default());
        assert_eq!(skills.loaded.len(), count);
        assert_eq!(roots.len(), count);
        assert!(
            skills
                .loaded
                .iter()
                .all(|skill| skill.source == SkillSource::User)
        );
        let mut reasons = skills.skipped.iter().map(|skill| skill.reason.as_str());
        if count == 3 {
            assert!(
                reasons
                    .next()
                    .is_some_and(|reason| reason.contains("16 KiB"))
            );
        }
        assert_eq!(
            reasons.collect::<Vec<_>>(),
            [format!(
                "Skipped {} skills beyond the skill or index limit without reading them.",
                105
            )]
        );
    }
    // 利用者側で一つが上限を超えても、余ったバイト数はプロジェクト側で使える。
    let f = Fixture::new()?;
    for n in 0..4 {
        f.skill(
            "home/.agents/skills",
            &format!("z{n:03}"),
            &"&".repeat(1024),
        )?;
    }
    f.skill("repo/.agents/skills", "a-small", "small")?;
    let (skills, roots) = f.collect(&UserSkillsConfig::default());
    assert_eq!(
        skills
            .loaded
            .iter()
            .map(|skill| skill.name.as_str())
            .collect::<Vec<_>>(),
        ["a-small", "z000", "z001", "z002"]
    );
    assert_eq!(roots.len(), 4);
    assert_eq!(skills.skipped.len(), 1);
    assert!(
        skills
            .skipped
            .first()
            .is_some_and(|skill| skill.path.ends_with("z003/SKILL.md"))
    );
    Ok(())
}

// 関門: 開かずに打ち切った利用者の Skill の名前を覚えず、後の段の同名に優先順位を
// 覆させる退行を守る（Collector::precut。設計書 01-04 の「長さと数の上限」）。
#[test]
fn a_user_skill_cut_without_opening_still_shadows_a_project_skill() -> TestResult {
    let f = Fixture::new()?;
    let large = "d".repeat(1000);
    for n in 0..20 {
        f.skill("home/.agents/skills", &format!("z{n:03}"), &large)?;
    }
    let project = f.skill("repo/sub/project/.agents/skills", "z019", "small")?;
    let (skills, _roots) = f.collect(&UserSkillsConfig::default());
    let user_cut = f.root.join("home/.agents/skills/z019/SKILL.md");
    assert!(skills.skipped.iter().any(|skill| skill.path == user_cut));
    assert!(skills.skipped.iter().any(|skill| skill.path == project));
    assert!(!skills.loaded.iter().any(|skill| skill.name == "z019"));
    Ok(())
}

// 関門: 残した候補だけでなく、探索の間の持ちすぎで後の候補を失う退行を子で検出する。
#[test]
fn collection_keeps_later_candidates_with_a_256_descriptor_limit() -> TestResult {
    const CHILD: &str = "BENITOITEH_TEST_SKILL_FD_LIMIT";
    if std::env::var_os(CHILD).is_none() {
        let status = std::process::Command::new(std::env::current_exe()?)
            .args([
                "--exact",
                "collection_keeps_later_candidates_with_a_256_descriptor_limit",
                "--nocapture",
            ])
            .env(CHILD, "1")
            .status()?;
        assert!(status.success());
        return Ok(());
    }
    let limit = rustix::process::getrlimit(rustix::process::Resource::Nofile);
    rustix::process::setrlimit(
        rustix::process::Resource::Nofile,
        rustix::process::Rlimit {
            current: Some(256),
            maximum: limit.maximum,
        },
    )?;
    let f = Fixture::new()?;
    for n in 0..600 {
        f.skill("repo/.agents/skills", &format!("z{n:03}"), "root skill")?;
    }
    f.skill("repo/sub/project/.agents/skills", "a-later", "later skill")?;
    for n in 0..100 {
        f.manifest("home/.agents/plugins", &format!("p{n:03}"))?;
    }
    f.skill(
        "home/.agents/plugins/p099/skills",
        "user-plugin",
        "last plugin",
    )?;
    let duplicate = f.skill(
        "repo/sub/project/.agents/skills",
        "z599",
        "must not replace root",
    )?;
    let (skills, roots) = f.collect(&UserSkillsConfig::default());
    assert!(
        skills
            .loaded
            .first()
            .is_some_and(|skill| skill.name == "a-later")
    );
    assert!(roots.len() <= 100);
    assert!(
        skills
            .skipped
            .iter()
            .all(|skill| !skill.reason.contains("Could not open")),
        "{:?}",
        skills.skipped
    );
    assert!(duplicate.exists());
    assert!(!skills.loaded.iter().any(|skill| skill.name == "z599"));
    Ok(())
}

// 関門: Linux の不正な名前を記録へ渡し、起動全体の直列化を失敗させる退行を守る。
#[cfg(target_os = "linux")]
#[test]
fn non_utf8_entries_and_real_paths_do_not_break_skill_records() -> TestResult {
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt;

    let f = Fixture::new()?;
    f.skill("repo/.agents/skills", "good", "valid skill survives")?;
    let directory = f.root.join("repo/.agents/skills");
    let invalid_name = OsString::from_vec(vec![b'b', 0xff]);
    fs::create_dir(directory.join(&invalid_name))?;
    fs::write(
        directory.join(&invalid_name).join("SKILL.md"),
        "---\nname: bad\ndescription: invalid entry\n---\n",
    )?;
    let (skills, roots) = f.collect(&UserSkillsConfig::default());
    assert_eq!(skills.loaded.len(), 1);
    assert!(skills.skipped.is_empty());
    serde_json::to_string(&skills.recorded())?;
    drop(roots);

    let real_directory = f.root.join("repo").join(&invalid_name).join("linked");
    f.write(
        real_directory.join("SKILL.md"),
        "---\nname: linked\ndescription: invalid directory path\n---\n",
    )?;
    symlink(
        Path::new("../..").join(&invalid_name).join("linked"),
        directory.join("linked"),
    )?;
    let document = f.skill("repo/.agents/skills", "document", "invalid document path")?;
    let real_document = document.with_file_name(&invalid_name);
    fs::rename(&document, &real_document)?;
    symlink(&invalid_name, &document)?;
    let (skills, roots) = f.collect(&UserSkillsConfig::default());
    assert_eq!(skills.loaded.len(), 1);
    assert_eq!(roots.len(), 1);
    assert_eq!(skills.skipped.len(), 2);
    assert!(
        skills
            .skipped
            .iter()
            .all(|skill| skill.reason.contains("UTF-8") && skill.path.to_str().is_some())
    );
    serde_json::to_string(&skills.recorded())?;
    serde_json::to_string(&skills.skipped)?;
    drop(roots);

    // 読めなかった側の探索パスも、損失を許す文字列で記録できるようにする。
    let (skills, roots) = skills::collect(
        &f.project,
        &f.root.join(&invalid_name),
        &UserSkillsConfig {
            user: true,
            project: false,
        },
        &benitoiteh::config::UserPluginsConfig::default(),
        &f.exclude,
    );
    drop(roots);
    let skipped = skills.skipped;
    assert_eq!(skipped.len(), 1);
    assert!(
        skipped
            .first()
            .is_some_and(|skill| skill.path.to_str().is_some())
    );
    serde_json::to_string(&skipped)?;
    Ok(())
}

// 関門: 索引を二重に置換する、毎回全文を追加する、切り替えで消す退行を環境の公開口で捕まえる。
#[test]
fn environment_escapes_index_once_and_keeps_it_across_project_changes() -> TestResult {
    let f = Fixture::new()?;
    f.skill("home/.agents/skills", "sample", "'A < B & \"quoted\"' ")?;
    let original = f.home.join(".agents/skills/sample/SKILL.md");
    let real_document = original.with_file_name("text\"<&.md");
    fs::rename(&original, &real_document)?;
    symlink("text\"<&.md", &original)?;
    f.write("repo/sub/project/AGENTS.md", "instructions")?;
    let instructions = instructions::collect(
        Mode::Normal,
        &f.project,
        &f.paths.instructions_file,
        &f.home,
        &f.exclude,
    );
    let empty = Environment::capture(&f.project, Mode::Normal, &[], false, UNIX_EPOCH)?;
    assert!(!empty.render().contains("<skills>"));
    let env = empty
        .clone()
        .with_project_instructions(instructions)
        .with_skills(f.collect(&UserSkillsConfig::default()).0);
    let rendered = env.render();
    let expected = format!(
        "<skills>\n<skill name=\"sample\" path=\"{}\">A &lt; B &amp; &quot;quoted&quot;</skill>\n</skills>",
        format_args!(
            "{}/text&quot;&lt;&amp;.md",
            f.home.join(".agents/skills/sample").display()
        )
    );
    assert!(rendered.contains(&expected), "{rendered}");
    assert!(!rendered.contains("PRIVATE BODY"));
    assert!(rendered.find("</project_instructions>") < rendered.find("<skills>"));
    assert_eq!(env.changes_since_rendered(&[&rendered]), None);
    let next = env.clone().with_project(&f.home, &[])?;
    assert!(next.render().contains(&expected));
    assert!(
        !next
            .changes_since(&env)
            .ok_or("project delta")?
            .contains("<skills>")
    );
    let delta = env
        .changes_since_rendered(&[&empty.render()])
        .ok_or("index delta")?;
    assert!(delta.contains(&expected));
    Ok(())
}

// 関門: 読める根の接続漏れと書ける範囲の拡大を、実行器の JSON の入口で確かめる。
#[test]
fn tools_read_and_search_loaded_roots_without_expanding_writes_or_following_replaced_roots()
-> TestResult {
    let f = Fixture::new()?;
    let skill_md = f.skill("home/.agents/skills", "sample", "sample")?;
    let directory = skill_md.parent().ok_or("directory")?;
    f.write(directory.join("note.txt"), "needle\n")?;
    f.write(directory.join(".env"), "SECRET")?;
    f.write(directory.join("main.bnt"), "original")?;
    f.write("home/outside.txt", "outside")?;
    symlink("../../../outside.txt", directory.join("escape.txt"))?;
    for mode in [Mode::Normal, Mode::Restricted] {
        let (_, roots) = f.collect(&UserSkillsConfig::default());
        let context = f.context(mode, roots)?;
        let tools = ToolSet::new(mode);
        for (tool, args, expected) in [
            ("read", json!({"path":directory.join("note.txt")}), "needle"),
            (
                "glob",
                json!({"path":directory,"pattern":"*.txt"}),
                directory.join("note.txt").to_str().ok_or("utf8")?,
            ),
            (
                "grep",
                json!({"path":directory,"pattern":"needle"}),
                directory.join("note.txt").to_str().ok_or("utf8")?,
            ),
            (
                "grep",
                json!({"path":directory.join("note.txt"),"pattern":"needle"}),
                "needle",
            ),
        ] {
            let output = tools.execute(&context, tool, args);
            assert!(
                !output.is_error && output.text.contains(expected),
                "{tool}: {output:?}"
            );
            assert!(!output.text.contains("SECRET"));
        }
        assert!(
            tools
                .execute(&context, "read", json!({"path":directory.join(".env")}))
                .is_error
        );
        if mode == Mode::Restricted {
            for path in [directory.join("escape.txt"), f.home.join("outside.txt")] {
                assert!(
                    tools
                        .execute(&context, "read", json!({"path":path}))
                        .is_error
                );
            }
            for tool in ["glob", "grep"] {
                assert!(
                    tools
                        .execute(&context, tool, json!({"path": &f.home,"pattern":"*"}))
                        .is_error
                );
            }
            assert!(
                !tools
                    .execute(&context, "read", json!({"path":directory.join("main.bnt")}))
                    .is_error
            );
            for (tool, args) in [
                (
                    "write",
                    json!({"path":directory.join("new.bnt"),"content":"changed"}),
                ),
                (
                    "edit",
                    json!({"path":directory.join("main.bnt"),"old_string":"original","new_string":"changed"}),
                ),
            ] {
                assert!(tools.execute(&context, tool, args).is_error);
            }
            assert_eq!(fs::read_to_string(directory.join("main.bnt"))?, "original");
            // 根のパスを別のディレクトリへ付け替えても、記述子の示す場所だけを読む。
            let moved = directory.with_file_name("moved");
            fs::rename(directory, &moved)?;
            fs::create_dir(directory)?;
            fs::write(directory.join("note.txt"), "replacement")?;
            let output =
                tools.execute(&context, "read", json!({"path":directory.join("note.txt")}));
            assert!(!output.text.contains("replacement"), "{output:?}");
            let output = tools.execute(
                &context,
                "grep",
                json!({"path":directory,"pattern":"replacement"}),
            );
            assert!(!output.text.contains("1:replacement"), "{output:?}");
        }
    }
    Ok(())
}
