//! 再開で渡されうる会話を初期状態にし、本物の要求の組み立てと圧縮を確かめる（実装プラン P4-01）。

// CLI の非公開のテストでも読む模擬提供者を再利用し、本番の公開範囲を広げない。
#[allow(dead_code, clippy::duplicate_mod)]
#[path = "../../../tests/support/mock_provider.rs"]
mod mock_provider;

use std::error::Error;
use std::fs;
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::{Duration, SystemTime};

use crate::agent::AgentConfig;
use crate::config::{Limits, Mode, ReadConfig, paths::DataPaths};
use crate::context::Environment;
use crate::http::HttpTimeouts;
use crate::provider::{AdapterKind, Capabilities, Destination, ToolResult};
use crate::safe_fs::ScopeRoot;
use crate::session::{Session, SessionId, replay};
use crate::tools::{ToolContext, exclude::ExcludeRules};
use mock_provider::{MockProvider, ResponseScript};
use serde_json::json;

use super::*;

struct Fixture {
    root: PathBuf,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _removed = fs::remove_dir_all(&self.root);
    }
}

// 作成時の関門: 外部の入口にはまだ再開がないため、会話だけを初期状態として与える。
// 本番の差し込み口は増やさず、圧縮・HTTP・記録の境界は本物を通す。
#[test]
fn unbuildable_continuation_falls_back_and_summarizes_first_request_and_steering()
-> Result<(), Box<dyn Error>> {
    let root = std::env::temp_dir().join(format!(
        "benitoiteh-compaction-{}",
        SessionId::generate(SystemTime::now())?
    ));
    fs::create_dir(&root)?;
    let fixture = Fixture {
        root: fs::canonicalize(root)?,
    };
    let project = fixture.root.join("project");
    let home = fixture.root.join("home");
    let temporary = fixture.root.join("tmp");
    for path in [&project, &home, &temporary] {
        fs::create_dir(path)?;
    }
    let paths = DataPaths::from_values(
        Some(&home),
        Some(&fixture.root.join("config")),
        Some(&fixture.root.join("state")),
        Some(&temporary),
    )?;
    let session = Session::create(&paths, SessionId::generate(SystemTime::now())?)?;
    let spill = session.directory().join("spill");
    let context = ToolContext::new(
        Mode::Normal,
        ScopeRoot::open(&project)?,
        ScopeRoot::open(session.temporary_dir())?,
        ScopeRoot::open(&spill)?,
        ExcludeRules::new(
            &project,
            &paths,
            &spill,
            &ReadConfig::default(),
            &ReadConfig::default(),
        )?,
    )
    .with_session(session.clone());
    let environment = Environment::capture(
        &project,
        Mode::Normal,
        &[project.clone(), session.temporary_dir().to_owned()],
        false,
        SystemTime::now(),
    )?;
    let environment_text = environment.render();
    let mut server = MockProvider::start(vec![ResponseScript::sse(format!(
        "data: {}\n\ndata: {}\n\ndata: [DONE]\n\n",
        json!({"choices":[{"delta":{"content":"repaired summary"},"finish_reason":null}]}),
        json!({"choices":[{"delta":{},"finish_reason":"stop"}]}),
    ))])?;
    let config = AgentConfig {
        agent: Default::default(),
        project_parent: None,
        allow_effects: None,
        mode: Mode::Normal,
        destination: Destination {
            provider: "mock".into(),
            base_url: server.base_url(),
            model: "mock-model".into(),
        },
        kind: AdapterKind::OpenAiChat,
        capabilities: Capabilities {
            tool_choice_none: true,
            ..Capabilities::default()
        },
        effort: "high".into(),
        limits: Limits::default(),
        http_timeouts: HttpTimeouts::default(),
        retry_initial: Duration::from_millis(2),
    };
    let mut agent = Agent::new(config, String::new(), context, environment, session.clone())?;
    agent.conversation = Conversation {
        items: vec![
            user_with_parts(vec![
                (UserPartKind::Environment, environment_text.clone()),
                (
                    UserPartKind::Summary,
                    "<compacted_summary>\nprevious goal\n</compacted_summary>".into(),
                ),
                (UserPartKind::Request, "initial request".into()),
                (UserPartKind::Steering, "steering note".into()),
            ]),
            Item::ToolResult(ToolResult {
                id: "orphan".into(),
                provider_id: None,
                text: "orphan result".into(),
                is_error: false,
                images: Vec::new(),
            }),
        ],
    };
    let (sender, receiver) = mpsc::sync_channel(4);
    let report = agent.compact_with_options(
        CompactOptions {
            keep_recent: false,
            ..CompactOptions::for_reason(CompactionReason::Manual)
        },
        Arc::new(AtomicBool::new(false)),
        sender,
    )?;
    assert_eq!(report.outcome, CompactOutcome::Compacted);
    assert_eq!((report.summarized_items, report.kept_items), (4, 0));
    assert_eq!(receiver.into_iter().count(), 1);
    let requests = server.requests()?;
    assert_eq!(requests.len(), 1);
    let body = &requests.first().ok_or("missing fallback")?.body;
    assert!(body.get("tools").is_none());
    let text = body
        .pointer("/messages/1/content/0/text")
        .and_then(serde_json::Value::as_str)
        .ok_or("missing transcript")?;
    assert!(
        text.starts_with(
            "<conversation>\n<compacted_summary>\nprevious goal\n</compacted_summary>"
        )
    );
    assert!(text.contains("[User]\ninitial request\n\n[User]\nsteering note\n"));
    assert!(!text.contains("<environment>"));
    let Some(Item::UserMessage(first)) = agent.conversation().items.first() else {
        return Err("missing user".into());
    };
    assert_eq!(
        first.parts.first().map(|part| &part.text),
        Some(&environment_text)
    );
    assert_eq!(
        first
            .parts
            .iter()
            .filter(|part| part.kind == UserPartKind::Environment)
            .count(),
        1
    );
    assert_eq!(replay(session.directory())?, *agent.conversation());
    server.shutdown()?;
    Ok(())
}

fn user_with_parts(parts: Vec<(UserPartKind, String)>) -> Item {
    Item::UserMessage(UserMessage {
        parts: parts
            .into_iter()
            .map(|(kind, text)| UserPart { kind, text })
            .collect(),
    })
}

// 作成時の関門: 文の量だけの既存の見積もりは、画像を無視して圧縮が遅れる退行を
// 捕まえられない。寸法による契約と、文字化した圧縮入力から画像を除く契約を守る。
#[test]
fn image_estimates_include_tiles_but_transcript_contains_only_result_text()
-> Result<(), Box<dyn Error>> {
    use crate::provider::{ReasoningReplay, ToolImage};
    let destination = Destination {
        provider: "mock".into(),
        base_url: "https://mock.invalid".into(),
        model: "m".into(),
    };
    for (size, expected) in [(1000, 3888), (2000, 4704)] {
        let item = Item::ToolResult(ToolResult {
            id: "image".into(),
            provider_id: None,
            text: "Read image file image/png".into(),
            is_error: false,
            images: vec![ToolImage {
                media_type: "image/png".into(),
                sha256: "0".repeat(64),
                width: size,
                height: size,
                bytes: 6,
                data: Some(std::sync::Arc::from(b"foobar".as_slice())),
            }],
        });
        assert_eq!(
            conversation::item_bytes(&item, ReasoningReplay::None, &destination),
            25_u64.saturating_add(expected)
        );
        let conversation = Conversation {
            items: vec![
                user_with_parts(vec![(UserPartKind::Request, "read".into())]),
                item,
            ],
        };
        let transcript =
            conversation::Transcript::new(&conversation, conversation.items.len(), None, false);
        let mut body = String::new();
        transcript.append_to(&mut body);
        assert_eq!(
            body,
            "[User]\nread\n\n[Tool result image]\nRead image file image/png\n\n"
        );
        let retained = conversation::rebuild(
            &conversation,
            1,
            "summary",
            &Default::default(),
            ReasoningReplay::None,
        );
        let Some(Item::ToolResult(result)) = retained.items.last() else {
            return Err("missing retained result".into());
        };
        assert_eq!(
            result
                .images
                .first()
                .and_then(|image| image.data.as_deref()),
            Some(b"foobar".as_slice())
        );
    }
    Ok(())
}
