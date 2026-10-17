//! 起動時の方針の選択と、使えないサンドボックスで続ける条件を確かめる（実装プラン P3-02）。

use std::cell::Cell;
use std::error::Error;
use std::fs;
use std::sync::atomic::AtomicBool;

use super::*;
use crate::provider::UserPartKind;

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Result<Self, Box<dyn Error>> {
        let id = SessionId::generate(SystemTime::now())?;
        let root = std::env::temp_dir().join(format!("benitoiteh-normal-startup-{id}"));
        fs::create_dir(&root)?;
        Ok(Self(fs::canonicalize(root)?))
    }

    fn paths(&self, normal: &str) -> Result<DataPaths, Box<dyn Error>> {
        for name in ["home", "project", "tmp", "config/benitoiteh"] {
            fs::create_dir_all(self.0.join(name))?;
        }
        let paths = DataPaths::from_values(
            Some(&self.0.join("home")),
            Some(&self.0.join("config")),
            Some(&self.0.join("state")),
            Some(&self.0.join("tmp")),
        )?;
        fs::write(
            &paths.config_file,
            format!(
                "default_mode = 'normal'\ndefault_provider = 'mock'\n[providers.mock]\nkind = 'openai-chat'\nmodel = 'mock'\n[normal]\n{normal}\n"
            ),
        )?;
        Ok(paths)
    }

    fn args(&self, options: &[&str]) -> Result<ExecArgs, Box<dyn Error>> {
        let mut args = ExecArgs::parse(options.iter().copied().chain(["request"]))?;
        args.project = Some(self.0.join("project"));
        Ok(args)
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _removed = fs::remove_dir_all(&self.0);
    }
}

// 作成時の関門: 実際の起動の設定解決とループを通す。OS の機能の欠落だけを差し替える
// （P3-02 の明示した指示）。方針の単体の検査では、起動時の渡し忘れを検出できない。
#[test]
fn startup_selects_policy_and_fails_closed_unless_explicitly_disabled() -> Result<(), Box<dyn Error>>
{
    for (normal, options, available, expected, probe_expected, disabled) in [
        ("", vec![], true, Some("workspace"), true, false),
        (
            "sandbox = 'read-only'\nnetwork = true",
            vec![],
            true,
            Some("read-only"),
            true,
            false,
        ),
        (
            "sandbox = 'read-only'\nnetwork = true",
            vec!["--sandbox", "workspace-write"],
            true,
            Some("network"),
            true,
            false,
        ),
        (
            "sandbox = 'danger-full-access'",
            vec![],
            false,
            Some("none"),
            false,
            false,
        ),
        ("", vec![], false, None, true, false),
        ("", vec!["--no-sandbox"], false, Some("none"), true, true),
        (
            "",
            vec!["--no-sandbox", "--approval", "never"],
            true,
            Some("workspace"),
            true,
            false,
        ),
    ] {
        let f = Fixture::new()?;
        // 利用者の approval は exec には適用しない。
        let paths = f.paths(&format!("{normal}\napproval = 'untrusted'"))?;
        let args = f.args(&options)?;
        let probed = Cell::new(false);
        let prepared = prepare(&args, &paths, |helper, scratch| {
            probed.set(true);
            assert!(helper.is_absolute());
            assert!(scratch.starts_with(&paths.sessions_dir));
            assert_eq!(scratch.file_name(), Some(std::ffi::OsStr::new("sandbox")));
            assert_eq!(
                fs::metadata(scratch)
                    .map_err(SandboxError::Io)?
                    .permissions()
                    .mode()
                    & 0o7777,
                0o700
            );
            if available {
                Ok(())
            } else {
                Err(SandboxError::Unavailable)
            }
        });
        assert_eq!(probed.get(), probe_expected);
        let Some(expected) = expected else {
            assert!(matches!(
                prepared,
                Err(StartupError::Sandbox(SandboxError::Unavailable))
            ));
            assert_eq!(fs::read_dir(&paths.temporary_root)?.count(), 0);
            continue;
        };
        let Prepared {
            mut agent,
            session,
            request,
        } = prepared.map_err(|e| io::Error::other(e.to_string()))?;
        let (sender, receiver) = mpsc::sync_channel(64);
        // 依頼と開始通知を記録した後、API を呼ぶ前に取り消す。
        agent.run(request, Arc::new(AtomicBool::new(true)), sender)?;
        let events = receiver.into_iter().collect::<Vec<_>>();
        let start = events.first().ok_or("missing session_start")?;
        assert!(
            matches!(start, crate::agent::AgentEvent::SessionStart { notice: Some(notice), .. }
            if notice.contains(text::SANDBOX_DISABLED) == disabled)
        );
        let environment = agent
            .conversation()
            .items
            .iter()
            .find_map(|item| match item {
                Item::UserMessage(message) => message
                    .parts
                    .iter()
                    .find(|part| part.kind == UserPartKind::Environment)
                    .map(|part| part.text.as_str()),
                Item::AssistantTurn(_) | Item::ToolResult(_) => None,
            })
            .ok_or("missing environment")?;
        match expected {
            "none" => {
                assert!(environment.contains("<writable_roots>[]</writable_roots>"));
                assert!(environment.contains("<network>allowed</network>"));
            }
            "read-only" => {
                assert!(!environment.contains(&format!("\"{}\"", f.0.join("project").display())));
                assert!(environment.contains("<network>denied</network>"));
            }
            _ => {
                assert!(environment.contains(&format!("\"{}\"", f.0.join("project").display())));
                assert!(environment.contains(if expected == "network" {
                    "<network>allowed</network>"
                } else {
                    "<network>denied</network>"
                }));
            }
        }
        assert_eq!(
            fs::metadata(paths.data_dir.join("placeholders"))?
                .permissions()
                .mode()
                & 0o7777,
            0o700
        );
        drop(agent);
        drop(session);
        panic::install(None);
        assert_eq!(fs::read_dir(&paths.temporary_root)?.count(), 0);
    }
    Ok(())
}
