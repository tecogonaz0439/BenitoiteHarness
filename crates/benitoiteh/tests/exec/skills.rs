//! Skill の起動時の接続と再開の再探索を入口から確かめる（設計書 01-04、01-05）。

use super::*;

// 関門: 収集・索引・開始通知・ヘッダの転記漏れと、再開時に一覧を再探索しない退行を守る。
#[test]
fn exec_sends_displays_records_and_rereads_user_and_project_skills() -> TestResult {
    for json_output in [false, true] {
        let mut server = MockProvider::start(vec![
            ResponseScript::sse(Protocol::Chat.response(None, "Done.", false)),
            ResponseScript::sse(Protocol::Chat.response(None, "Resumed.", false)),
        ])?;
        let f = Fixture::new(Protocol::Chat, &server)?;
        let user = f.root.join("home/.agents/skills/user-tool/SKILL.md");
        let project = f.root.join("project/.agents/skills/project-tool/SKILL.md");
        let reserved = f.root.join("project/.agents/skills/benitoite/SKILL.md");
        for (path, name, description) in [
            (&user, "user-tool", "User tool"),
            (&project, "project-tool", "Project tool"),
            (&reserved, "benitoite", "Reserved"),
        ] {
            fs::create_dir_all(path.parent().ok_or("skill directory")?)?;
            fs::write(
                path,
                format!(
                    "---\nname: {name}\ndescription: {description}\n---\nDo not send this body at startup.\n"
                ),
            )?;
        }
        let plugin = f.root.join("home/.agents/plugins/p");
        let plugin_skill = plugin.join("skills/z-plugin/SKILL.md");
        fs::create_dir_all(plugin_skill.parent().ok_or("plugin skill")?)?;
        fs::write(
            plugin.join("plugin.json"),
            r#"{"$schema":"https://agent-plugins.org/schemas/1.0.0/plugin.schema.json","name":"p","version":"1.2.3","future":true}"#,
        )?;
        fs::write(plugin.join("mcp.json"), "Not JSON; must not be read.")?;
        fs::write(
            &plugin_skill,
            "---\nname: z-plugin\ndescription: Plugin tool\n---\nPlugin private body\n",
        )?;
        fs::create_dir_all(f.root.join("home/.agents/plugins/bad"))?;
        fs::write(f.root.join("home/.agents/plugins/bad/plugin.json"), "[]")?;
        let mut command = f.command("Use the skills.");
        command.args(["--mode", "normal", "--sandbox", "danger-full-access"]);
        if json_output {
            command.arg("--json");
        }
        let output = Running::start(&mut command)?.finish()?;
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let (directory, records) = f.records()?;
        let header = records.first().ok_or("header")?.clone();
        let recorded = header
            .get("skills")
            .and_then(Value::as_array)
            .ok_or("header skills")?;
        assert_eq!(recorded.len(), 3);
        assert_eq!(
            recorded.first().and_then(|skill| skill.get("source")),
            Some(&json!("project"))
        );
        assert_eq!(
            recorded
                .iter()
                .find(|skill| skill.get("name") == Some(&json!("user-tool")))
                .and_then(|skill| skill.get("source")),
            Some(&json!("user"))
        );
        assert!(recorded.iter().all(|skill| {
            skill
                .get("sha256")
                .and_then(Value::as_str)
                .is_some_and(|hash| {
                    hash.len() == 64
                        && hash
                            .bytes()
                            .all(|ch| ch.is_ascii_digit() || (b'a'..=b'f').contains(&ch))
                })
        }));
        let expected_plugin = json!([{
            "name":"p", "version":"1.2.3", "path":plugin,
            "sha256":"da8490b39970e273c14aed80202753aed6913321db280d658947e0167136eabb", "source":"user"
        }]);
        assert_eq!(header.get("plugins"), Some(&expected_plugin));
        assert_eq!(
            recorded.last().and_then(|skill| skill.get("plugin")),
            Some(&json!("p"))
        );
        assert!(
            recorded
                .iter()
                .filter(|skill| skill.get("name") != Some(&json!("z-plugin")))
                .all(|skill| skill.get("plugin").is_none())
        );
        let start = records
            .iter()
            .find(|record| record.get("type") == Some(&json!("session_start")))
            .ok_or("start")?;
        assert_eq!(start.get("skills"), Some(&json!(recorded)));
        assert_eq!(start.get("plugins"), Some(&expected_plugin));
        assert_eq!(
            start
                .get("plugin_notices")
                .and_then(Value::as_array)
                .map(Vec::len),
            // P5-24 で期待を変えた: 一つのプラグインの通知（知らない欄と mcp.json）を一行にまとめる。
            Some(1)
        );
        assert_eq!(
            start
                .get("skipped_plugins")
                .and_then(Value::as_array)
                .map(Vec::len),
            Some(1)
        );
        assert_eq!(
            start.pointer("/skipped_skills/0/path"),
            Some(&json!(reserved))
        );
        if json_output {
            assert_eq!(lines(&output.stdout)?.first(), Some(start));
        }
        let stderr = std::str::from_utf8(&output.stderr)?;
        for (name, path) in [("user-tool", &user), ("project-tool", &project)] {
            assert!(
                stderr
                    .lines()
                    .any(|line| line == format!("Skill loaded: {name} ({})", path.display()))
            );
        }
        assert!(stderr.contains(&format!("Skill skipped: {}:", reserved.display())));
        let plugin_line = format!(
            "Plugin loaded: p (user; sha256 da8490b39970e273c14aed80202753aed6913321db280d658947e0167136eabb; version 1.2.3; {})",
            plugin.display()
        );
        assert!(stderr.lines().any(|line| line == plugin_line));
        assert!(stderr.contains(&format!(
            "Skill loaded: z-plugin (plugin p; {})",
            plugin_skill.display()
        )));
        assert!(stderr.contains("Plugin skipped:") && stderr.contains("Plugin notice:"));
        assert!(stderr.find("Plugin loaded:") < stderr.find("Skill loaded:"));
        let requests = server.requests()?;
        let request = &requests.first().ok_or("request")?.body;
        let environment = request
            .get("messages")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|message| message.get("content").and_then(Value::as_array))
            .flatten()
            .filter_map(|part| part.get("text").and_then(Value::as_str))
            .find(|text| text.contains("<environment>"))
            .ok_or("environment")?;
        let expected = format!(
            "<skills>\n<skill name=\"project-tool\" path=\"{}\">Project tool</skill>\n<skill name=\"user-tool\" path=\"{}\">User tool</skill>\n<skill name=\"z-plugin\" path=\"{}\">Plugin tool</skill>\n</skills>",
            project.display(),
            user.display(),
            plugin_skill.display()
        );
        assert!(environment.contains(&expected), "{environment}");
        assert!(
            !environment.contains("Do not send this body")
                && !environment.contains("Plugin private body")
        );
        eprintln!("P5-23 example index:\n{expected}\nP5-23 example stderr:\n{stderr}");
        fs::write(
            &user,
            "---\nname: user-tool\ndescription: Updated user tool\n---\n",
        )?;
        fs::remove_file(&project)?;
        let output = Running::start(
            f.command("Continue.").args([
                "--resume",
                directory
                    .file_name()
                    .and_then(|name| name.to_str())
                    .ok_or("id")?,
                "--sandbox",
                "danger-full-access",
            ]),
        )?
        .finish()?;
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let (_, current) = f.records()?;
        assert_eq!(current.first(), Some(&header));
        let resumed = current
            .iter()
            .find(|record| record.get("resumed") == Some(&json!(true)))
            .ok_or("resumed start")?;
        assert_eq!(
            resumed
                .get("skills")
                .and_then(Value::as_array)
                .map(Vec::len),
            Some(2)
        );
        let last_environment = current
            .iter()
            .rev()
            .filter_map(|record| record.pointer("/message/parts").and_then(Value::as_array))
            .flatten()
            .find(|part| part.get("kind") == Some(&json!("environment")))
            .and_then(|part| part.get("text"))
            .and_then(Value::as_str)
            .ok_or("updated environment")?;
        assert!(last_environment.contains("<skills>\n<skill name=\"user-tool\""));
        assert!(last_environment.contains("Updated user tool</skill>"));
        assert!(last_environment.contains("Plugin tool</skill>\n</skills>"));
        assert!(!last_environment.contains("project-tool"));
        // 新しい欄を持たない開始通知も、実際の JSON Lines の読み手で読む。
        let mut legacy = current;
        for record in &mut legacy {
            if let Some(fields) = record.as_object_mut() {
                fields.remove("plugins");
                fields.remove("skipped_plugins");
                fields.remove("plugin_notices");
                fields.remove("skills");
                fields.remove("skipped_skills");
            }
        }
        fs::write(
            directory.join("session.jsonl"),
            legacy
                .iter()
                .map(serde_json::to_string)
                .collect::<Result<Vec<_>, _>>()?
                .join("\n")
                + "\n",
        )?;
        let restored = benitoiteh::session::read_records(&directory)?;
        assert!(restored.iter().any(|record| matches!(record, benitoiteh::session::Record::SessionStart { skills, skipped_skills, .. } if skills.is_empty() && skipped_skills.is_empty())));
        server.shutdown()?;
    }
    Ok(())
}

// 関門: 信頼しないリポジトリの長い版と多数のプラグインで、ヘッダが記録の上限を超えて
// 一覧と再開に使えなくなる退行を、入口から防ぐ（設計書 01-04 の「長さと数の上限」）。
#[test]
fn long_versions_in_many_project_plugins_keep_the_session_listable_and_resumable() -> TestResult {
    let mut server = MockProvider::start(vec![
        ResponseScript::sse(Protocol::Chat.response(None, "Done.", false)),
        ResponseScript::sse(Protocol::Chat.response(None, "Resumed.", false)),
    ])?;
    let f = Fixture::new(Protocol::Chat, &server)?;
    let version = "1".repeat(200 * 1024);
    for n in 0..101 {
        let name = format!("p{n:03}");
        let root = f.root.join("project/.agents/plugins").join(&name);
        fs::create_dir_all(&root)?;
        fs::write(
            root.join("plugin.json"),
            serde_json::to_vec(&json!({
                "$schema":"https://agent-plugins.org/schemas/1.0.0/plugin.schema.json",
                "name":name, "version":version,
            }))?,
        )?;
    }
    let output = Running::start(
        f.command("Start.")
            .args(["--sandbox", "danger-full-access"]),
    )?
    .finish()?;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let (directory, records) = f.records()?;
    let header = records.first().ok_or("header")?;
    let plugins = header
        .get("plugins")
        .and_then(Value::as_array)
        .ok_or("plugins")?;
    assert_eq!(plugins.len(), 100);
    assert!(plugins.iter().all(|plugin| plugin.get("version").is_none()));
    let first_line = fs::read(directory.join("session.jsonl"))?
        .split(|byte| *byte == b'\n')
        .next()
        .map(<[u8]>::len)
        .ok_or("line")?;
    assert!(first_line < 64 * 1024, "{first_line}");
    let stderr = std::str::from_utf8(&output.stderr)?;
    assert!(stderr.contains("Skipped 1 entries beyond the 100-plugin limit"));
    assert!(stderr.contains("; no version; "));
    assert!(stderr.len() < 256 * 1024);
    let paths = benitoiteh::config::paths::DataPaths::from_values(
        Some(&f.root.join("home")),
        Some(&f.root.join("config")),
        Some(&f.root.join("state")),
        Some(&f.root.join("tmp")),
    )?;
    let entries = benitoiteh::session::list_sessions(&paths)?;
    assert_eq!(entries.len(), 1);
    assert!(
        entries.iter().all(|entry| entry.error.is_none()),
        "{entries:?}"
    );
    let output = Running::start(
        f.command("Continue.").args([
            "--resume",
            directory
                .file_name()
                .and_then(|name| name.to_str())
                .ok_or("id")?,
            "--sandbox",
            "danger-full-access",
        ]),
    )?
    .finish()?;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    server.shutdown()?;
    Ok(())
}

// 関門: 規則に合わない候補と同名で負けた候補が上限を進めず、起動のたびに多数を開いて
// 一件ずつ示し、開始の記録が分けて置かれて一覧に使えなくなる退行を、入口から防ぐ
// （設計書 01-04 の「長さと数の上限」）。
#[test]
fn many_bad_or_shadowed_plugin_skill_candidates_keep_output_and_records_small() -> TestResult {
    for shadowed in [false, true] {
        let mut server = MockProvider::start(vec![ResponseScript::sse(
            Protocol::Chat.response(None, "Done.", false),
        )])?;
        let f = Fixture::new(Protocol::Chat, &server)?;
        for n in 0..100 {
            let name = format!("p{n:03}");
            let root = f.root.join("project/.agents/plugins").join(&name);
            let skills = root.join("skills");
            fs::create_dir_all(&skills)?;
            fs::write(
                root.join("plugin.json"),
                serde_json::to_vec(&json!({
                    "$schema":"https://agent-plugins.org/schemas/1.0.0/plugin.schema.json",
                    "name":name,
                }))?,
            )?;
            // 合わせて 1,100 個で、段の 1,000 個の上限を少し超える。
            for k in 0..11 {
                let skill = format!("s{k:02}");
                if n == 99 && k == 10 {
                    // 上限の後ろの、名前の重ならない有効な Skill。開けば読まれる。
                    let unique = skills.join("t-unique");
                    fs::create_dir_all(&unique)?;
                    fs::write(
                        unique.join("SKILL.md"),
                        "---\nname: t-unique\ndescription: Beyond the limit\n---\n",
                    )?;
                } else if shadowed {
                    fs::create_dir_all(skills.join(&skill))?;
                    fs::write(
                        skills.join(&skill).join("SKILL.md"),
                        format!("---\nname: {skill}\ndescription: Shadowed\n---\n"),
                    )?;
                } else {
                    std::os::unix::fs::symlink("/nonexistent-outside", skills.join(&skill))?;
                }
            }
        }
        let output = Running::start(
            f.command("First request.")
                .args(["--sandbox", "danger-full-access"]),
        )?
        .finish()?;
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(output.stderr.len() <= 256 * 1024, "{}", output.stderr.len());
        let (directory, records) = f.records()?;
        let contents = fs::read(directory.join("session.jsonl"))?;
        assert!(
            contents
                .split(|byte| *byte == b'\n')
                .all(|line| line.len() <= 256 * 1024)
        );
        let start = records
            .iter()
            .find(|record| record.get("type") == Some(&json!("session_start")))
            .ok_or("start")?;
        let skipped = start
            .get("skipped_skills")
            .and_then(Value::as_array)
            .ok_or("skipped skills")?;
        // 一件ずつの 100 件と、数をまとめた行（候補の上限と、示さなかった数）。
        assert!(skipped.len() <= 102, "{}", skipped.len());
        // 候補の上限: 1,000 個目は p090 の s09 で、超えた 100 個を開かずに数える。
        let limit = skipped
            .iter()
            .find(|item| {
                item.get("reason").and_then(Value::as_str) == Some(
                    "Skipped 100 plugin skill entries beyond the first 1000 for this source without opening them.",
                )
            })
            .ok_or("candidate limit line")?;
        assert!(
            limit
                .get("path")
                .and_then(Value::as_str)
                .is_some_and(|path| path.ends_with("project/.agents/plugins/p090/skills")),
            "{limit}"
        );
        assert!(
            !start
                .get("skills")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .any(|skill| skill.get("name") == Some(&json!("t-unique")))
        );
        let paths = benitoiteh::config::paths::DataPaths::from_values(
            Some(&f.root.join("home")),
            Some(&f.root.join("config")),
            Some(&f.root.join("state")),
            Some(&f.root.join("tmp")),
        )?;
        let entries = benitoiteh::session::list_sessions(&paths)?;
        assert_eq!(
            entries
                .first()
                .and_then(|entry| entry.first_request.as_deref()),
            Some("First request.")
        );
        if shadowed {
            let loaded = start.get("skills").and_then(Value::as_array).map(Vec::len);
            assert_eq!(loaded, Some(11));
        }
        server.shutdown()?;
    }
    Ok(())
}

// 関門: 長いパスのプラグインの通知と飛ばした項目が件数の枠の中で並び、画面と開始の記録を
// 膨らませる退行を、入口から防ぐ（設計書 01-04 の「長さと数の上限」の 64 KiB の枠）。
#[test]
fn long_plugin_paths_keep_listed_notices_and_skips_within_64_kib_per_stage() -> TestResult {
    let mut server = MockProvider::start(vec![ResponseScript::sse(
        Protocol::Chat.response(None, "Done.", false),
    )])?;
    let f = Fixture::new(Protocol::Chat, &server)?;
    let mut fields = serde_json::Map::new();
    fields.insert(
        "$schema".into(),
        json!("https://agent-plugins.org/schemas/1.0.0/plugin.schema.json"),
    );
    fields.insert("version".into(), json!("9".repeat(65)));
    for n in 0..10 {
        fields.insert(format!("{n}{}", "u".repeat(63)), json!(true));
    }
    for (stage, letter) in [("home", 'u'), ("project", 'p')] {
        // macOS の PATH_MAX（1023 バイト）に収まるよう、プラグインの根の実のパスを 920 バイトに
        // 揃え、要素を 200 バイト以下に分ける。
        let base = f.root.join(stage).join("long");
        let mut left = 920usize
            .checked_sub(base.as_os_str().len() + "/p000".len())
            .ok_or("temporary root too long")?;
        let mut long = base;
        while left > 0 {
            let mut part = left.min(200);
            if left - part == 1 {
                part -= 1;
            }
            long.push("d".repeat(part - 1));
            left -= part;
        }
        let links = f.root.join(stage).join(".agents/plugins");
        fs::create_dir_all(&links)?;
        for n in 0..100 {
            let name = format!("{letter}{n:03}");
            if n == 99 {
                // 枠を越えた後ろの、短いパスの小さな項目。越えた後に詰めて示せば出てしまう。
                fs::create_dir_all(links.join(&name))?;
                fs::write(links.join(&name).join("plugin.json"), "{")?;
                continue;
            }
            let root = long.join(&name);
            assert_eq!(root.as_os_str().len(), 920);
            fs::create_dir_all(root.join("skills/bad"))?;
            fields.insert("name".into(), json!(name));
            fs::write(root.join("plugin.json"), serde_json::to_vec(&fields)?)?;
            fs::write(root.join("mcp.json"), "{}")?;
            fs::write(root.join("skills/bad/SKILL.md"), "invalid")?;
            // 範囲の中へのリンクは相対のパスで置く（絶対のパスのリンクは読まない）。
            let target = std::path::Path::new("../..").join(root.strip_prefix(f.root.join(stage))?);
            std::os::unix::fs::symlink(target, links.join(&name))?;
        }
    }
    // 標準エラー出力はパイプの容量を超えるので、終わりを待たずに読み続ける。
    let output = f
        .command("First request.")
        .args(["--sandbox", "danger-full-access"])
        .output()?;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.len() <= 512 * 1024, "{}", output.stderr.len());
    let (directory, records) = f.records()?;
    let contents = fs::read(directory.join("session.jsonl"))?;
    assert!(
        contents
            .split(|byte| *byte == b'\n')
            .all(|line| line.len() <= 512 * 1024)
    );
    // 開始の記録は分けて置かれず、示した項目を本体に持つ。
    let start = records
        .iter()
        .find(|record| record.get("type") == Some(&json!("session_start")))
        .ok_or("start")?;
    let plugins = start
        .get("plugins")
        .and_then(Value::as_array)
        .ok_or_else(|| format!("plugins: {start}"))?;
    assert_eq!(plugins.len(), 198, "{:?}", start.get("skipped_plugins"));
    let items = ["plugin_notices", "skipped_plugins", "skipped_skills"]
        .into_iter()
        .filter_map(|key| start.get(key).and_then(Value::as_array))
        .flatten()
        .filter_map(|item| Some((item.get("path")?.as_str()?, item.get("reason")?.as_str()?)))
        .collect::<Vec<_>>();
    let notice = items
        .iter()
        .find(|(_, reason)| reason.starts_with("Ignored plugin.json version"))
        .ok_or("merged notice")?;
    let unknown = notice
        .1
        .split_once(" mcp.json is present; MCP servers from this plugin are not used. ")
        .map(|(_, unknown)| unknown)
        .ok_or("mcp notice")?;
    assert!(unknown.starts_with("Ignored unknown plugin.json fields: 0uuu"));
    assert!(notice.1.len() > 800, "{}", notice.1.len());
    for stage in ["home", "project"] {
        let prefix = f.root.join(stage);
        let prefix = prefix.to_str().ok_or("prefix")?;
        let (hidden, listed): (Vec<_>, Vec<_>) = items
            .iter()
            .filter(|(path, _)| path.starts_with(prefix))
            .partition(|(_, reason)| reason.contains("for this source are not listed"));
        let bytes = listed
            .iter()
            .map(|(path, reason)| path.len() + reason.len())
            .sum::<usize>();
        assert!(bytes <= 64 * 1024, "{stage}: {bytes}");
        // 下限: 枠を越えたのは、一件の最大の大きさより少ない残りに達してからである。
        let largest = listed
            .iter()
            .map(|(path, reason)| path.len() + reason.len())
            .max()
            .ok_or("listed items")?;
        assert!(bytes > 64 * 1024 - largest, "{stage}: {bytes} {largest}");
        assert!(listed.len() < 100, "{stage}: {}", listed.len());
        // 越えた後ろの小さな項目は、収まる大きさでも示さない。
        let small = f.root.join(stage).join(".agents/plugins");
        let small = small.to_str().ok_or("small")?;
        assert!(
            !listed
                .iter()
                .any(|(path, _)| path.starts_with(small) && path.ends_with("099")),
            "{stage}"
        );
        let [(_, hidden)] = hidden.as_slice() else {
            return Err(format!("{stage}: {hidden:?}").into());
        };
        assert!(
            hidden.contains(" plugin notices ") && !hidden.contains(" and 0 plugin notices "),
            "{hidden}"
        );
        eprintln!("P5-24 example hidden line ({stage}): {hidden}");
    }
    eprintln!("P5-24 example merged notice: {} {}", notice.0, notice.1);
    server.shutdown()?;
    Ok(())
}
