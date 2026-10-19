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
        assert_eq!(recorded.len(), 2);
        assert_eq!(
            recorded.first().and_then(|skill| skill.get("source")),
            Some(&json!("project"))
        );
        assert_eq!(
            recorded.last().and_then(|skill| skill.get("source")),
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
        let start = records
            .iter()
            .find(|record| record.get("type") == Some(&json!("session_start")))
            .ok_or("start")?;
        assert_eq!(start.get("skills"), Some(&json!(recorded)));
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
            "<skills>\n<skill name=\"project-tool\" path=\"{}\">Project tool</skill>\n<skill name=\"user-tool\" path=\"{}\">User tool</skill>\n</skills>",
            project.display(),
            user.display()
        );
        assert!(environment.contains(&expected), "{environment}");
        assert!(!environment.contains("Do not send this body"));
        eprintln!("P5-09 example index:\n{expected}\nP5-09 example stderr:\n{stderr}");
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
            Some(1)
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
        assert!(last_environment.contains("Updated user tool</skill>\n</skills>"));
        assert!(!last_environment.contains("project-tool"));
        // 新しい欄を持たない開始通知も、実際の JSON Lines の読み手で読む。
        let mut legacy = current;
        for record in &mut legacy {
            if let Some(fields) = record.as_object_mut() {
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
