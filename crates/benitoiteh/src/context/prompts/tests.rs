//! 雛形の読み取りと引数の埋め込みを、本物のファイルで確かめる（設計書 05-02「利用者の定義するコマンド」）。

use super::*;
use crate::config::ReadConfig;
use crate::config::paths::DataPaths;
use crate::context::text;
use std::error::Error;
use std::fs;
use std::os::unix::fs::symlink;

type TestResult = Result<(), Box<dyn Error>>;

struct Fixture(PathBuf);
impl Drop for Fixture {
    fn drop(&mut self) {
        let _removed = fs::remove_dir_all(&self.0);
    }
}

fn fixture() -> Result<(Fixture, ExcludeRules), Box<dyn Error>> {
    let id = crate::session::SessionId::generate(std::time::SystemTime::now())?;
    let directory = std::env::temp_dir().join(format!("benitoiteh-prompts-{id}"));
    fs::create_dir(&directory)?;
    let root = Fixture(fs::canonicalize(directory)?);
    for name in [
        "home",
        "config/benitoiteh/prompts",
        "repo/.git",
        "repo/app/.agents/prompts",
        "repo/.agents/prompts",
        "outside",
        "spill",
    ] {
        fs::create_dir_all(root.0.join(name))?;
    }
    let paths = DataPaths::from_values(
        Some(&root.0.join("home")),
        Some(&root.0.join("config")),
        Some(&root.0.join("state")),
        Some(&root.0.join("tmp")),
    )?;
    let exclude = ExcludeRules::new(
        &root.0.join("repo/app"),
        &paths,
        &root.0.join("spill"),
        &ReadConfig {
            exclude: vec!["**/hidden.md".into()],
        },
        &ReadConfig::default(),
    )?;
    Ok((root, exclude))
}

// 作成時の関門: 利用者の雛形と、信頼したときだけプロジェクトの雛形を読む。同じ名前は利用者のものを
// 使い、組み込みの名前、誤った名前、リンク、固いリンク、読まないファイル、大きすぎるもの、
// UTF-8 でないものは読まずに理由を示す。
#[test]
fn templates_load_user_first_and_project_only_when_included() -> TestResult {
    let (root, exclude) = fixture()?;
    let user = root.0.join("config/benitoiteh/prompts");
    let app = root.0.join("repo/app");
    let project = app.join(".agents/prompts");
    fs::write(
        user.join("review.md"),
        "---\ndescription: Review the change\n---\nReview $ARGUMENTS.\n",
    )?;
    fs::write(user.join("Bad.md"), "x")?;
    fs::write(user.join("help.md"), "x")?;
    fs::write(user.join("notes.txt"), "not a template")?;
    fs::write(project.join("review.md"), "Project review.")?;
    fs::write(project.join("plan.md"), "Plan $1 then $2.")?;
    fs::write(project.join("hidden.md"), "excluded")?;
    fs::write(project.join("large.md"), vec![b'a'; 64 * 1024 + 1])?;
    fs::write(project.join("binary.md"), [0xff_u8, 0xfe])?;
    fs::write(root.0.join("outside/secret.md"), "secret")?;
    symlink(root.0.join("outside/secret.md"), project.join("linked.md"))?;
    fs::hard_link(root.0.join("outside/secret.md"), project.join("hard.md"))?;
    fs::write(
        root.0.join("repo/.agents/prompts/root.md"),
        "From the root.",
    )?;
    let home = root.0.join("home");

    assert!(project_has_prompts(&app, &home));
    let without = collect(&user, &app, &home, false, &exclude);
    assert_eq!(
        without.iter().map(|t| t.name.as_str()).collect::<Vec<_>>(),
        ["review"]
    );

    let loaded = collect(&user, &app, &home, true, &exclude);
    assert_eq!(
        loaded.iter().map(|t| t.name.as_str()).collect::<Vec<_>>(),
        ["plan", "review", "root"]
    );
    let review = loaded.get("review").ok_or("review")?;
    assert_eq!(review.source, TemplateSource::User);
    assert_eq!(review.description.as_deref(), Some("Review the change"));
    assert_eq!(review.body, "Review $ARGUMENTS.\n");
    assert_eq!(
        loaded.get("root").map(|t| t.source),
        Some(TemplateSource::Project)
    );
    let skipped = |name: &str, reason: &str| {
        loaded
            .notices
            .iter()
            .any(|notice| notice.contains(name) && notice.contains(reason))
    };
    assert!(skipped("Bad.md", text::PROMPT_INVALID_NAME));
    assert!(skipped("help.md", text::PROMPT_RESERVED));
    assert!(skipped(
        "app/.agents/prompts/review.md",
        text::PROMPT_DUPLICATE
    ));
    assert!(skipped("hidden.md", "hidden.md"));
    assert!(skipped("large.md", text::PROMPT_FILE_LIMIT));
    assert!(skipped("binary.md", text::PROMPT_NOT_UTF8));
    assert!(skipped("linked.md", text::PROMPT_NOT_REGULAR));
    assert!(skipped("hard.md", text::PROMPT_HARD_LINKS));
    assert!(
        !loaded
            .notices
            .iter()
            .any(|notice| notice.contains("notes.txt"))
    );
    assert!(loaded.iter().all(|t| !t.body.contains("secret")));
    Ok(())
}

// 作成時の関門: プロジェクトの `.agents` をリンクにして範囲の外の雛形を読ませる道筋を塞ぐ。
#[test]
fn linked_project_prompt_directories_are_not_read() -> TestResult {
    let (root, exclude) = fixture()?;
    let app = root.0.join("repo/app");
    fs::remove_dir_all(app.join(".agents"))?;
    fs::create_dir_all(root.0.join("outside/prompts"))?;
    fs::write(root.0.join("outside/prompts/leak.md"), "secret")?;
    symlink(root.0.join("outside"), app.join(".agents"))?;
    let loaded = collect(
        &root.0.join("config/benitoiteh/prompts"),
        &app,
        &root.0.join("home"),
        true,
        &exclude,
    );
    assert!(loaded.get("leak").is_none());
    assert!(
        loaded
            .notices
            .iter()
            .any(|notice| notice.contains(text::PROMPT_LINKED_DIRECTORY)),
        "{:?}",
        loaded.notices
    );
    Ok(())
}

// 作成時の関門: 引数の埋め込みの規則（全体、位置、`$$`、埋める場所がないときの追記）を表で確かめる。
#[test]
fn templates_expand_arguments_and_keep_unplaced_arguments() {
    let template = |body: &str| PromptTemplate {
        name: "t".into(),
        description: None,
        body: body.into(),
        source: TemplateSource::User,
        path: PathBuf::from("/t.md"),
    };
    for (body, arguments, expected) in [
        (
            "Review $ARGUMENTS.",
            "  src/a.rs  b ",
            "Review src/a.rs  b.",
        ),
        ("Move $1 to $2; ignore $3.", "a b", "Move a to b; ignore ."),
        ("Cost $$5 and $x", "", "Cost $5 and $x"),
        (
            "Summarize the diff.\n",
            "focus on tests",
            "Summarize the diff.\n\nfocus on tests",
        ),
        ("Summarize the diff.\n", "", "Summarize the diff.\n"),
        ("Trailing $", "a", "Trailing $\n\na"),
    ] {
        assert_eq!(template(body).expand(arguments), expected, "{body:?}");
    }
}

#[test]
fn names_follow_the_command_rules() {
    for (name, valid) in [
        ("review", true),
        ("fix-tests2", true),
        ("2fix", false),
        ("Review", false),
        ("re_view", false),
        ("", false),
        (&"a".repeat(65), false),
    ] {
        assert_eq!(valid_name(name), valid, "{name}");
    }
}
