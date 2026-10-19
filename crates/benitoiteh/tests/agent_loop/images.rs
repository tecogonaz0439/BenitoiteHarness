//! 画像の送り直しの HTTP 本文、記録、再開の契約（設計書 02-01「送る画像の上限」）。

use super::*;
use benitoiteh::session::ResumeState;

const REMOVED: &str =
    "(This image was removed to keep the request small. Read the file again if you need it.)";

fn image_result(agent: &Agent) -> Result<&benitoiteh::provider::ToolResult, Box<dyn Error>> {
    agent
        .conversation()
        .items
        .iter()
        .find_map(|item| match item {
            Item::ToolResult(result) if result.text.contains("image/gif") => Some(result),
            Item::UserMessage(_) | Item::AssistantTurn(_) | Item::ToolResult(_) => None,
        })
        .ok_or_else(|| "missing image result".into())
}

fn invalid(status: u16) -> Result<ResponseScript, io::Error> {
    ResponseScript::json(
        status,
        &json!({"type":"error","error":{"type":"request_too_large","message":"invalid image"}}),
    )
}

// 作成時の関門: 画像があるときだけ起きる課金済み要求の再送を、HTTP 200 の
// 途中の誤りで捕まえる。画像のない既存の途中エラーのテストでは検出できない。
#[test]
fn stream_errors_with_images_do_not_resend_or_record_removal() -> TestResult {
    for (kind, compacting) in [
        (AdapterKind::OpenAiChat, true),
        (AdapterKind::AnthropicMessages, true),
        (AdapterKind::OpenAiChat, false),
        (AdapterKind::AnthropicMessages, false),
    ] {
        let f = Fixture::new()?;
        fs::write(f.project.join("image.gif"), b"GIF89a\x01\0\x01\0")?;
        let body = match kind {
            AdapterKind::OpenAiChat => concat!(
                "data: {\"choices\":[{\"delta\":{\"content\":\"partial\"},\"finish_reason\":null}]}\n\n",
                "data: {\"error\":{\"code\":\"invalid_request\"}}\n\n"
            ),
            AdapterKind::AnthropicMessages => concat!(
                "event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"usage\":{\"input_tokens\":10,\"output_tokens\":0}}}\n\n",
                "event: error\ndata: {\"type\":\"error\",\"error\":{\"type\":\"invalid_request_error\",\"message\":\"bad\"}}\n\n"
            ),
            AdapterKind::OpenAiResponses => return Err("unexpected adapter".into()),
        };
        let mut scripts = Vec::new();
        if compacting {
            scripts.push(compaction::response(kind, &"old".repeat(24_000), None, 10));
        }
        scripts.push(compaction::response(
            kind,
            "",
            Some(("read", json!({"path":"image.gif"}))),
            10,
        ));
        if compacting {
            scripts.push(compaction::response(kind, "done", None, 10));
        }
        scripts.push(ResponseScript::sse(body));
        scripts.push(compaction::response(kind, "unexpected retry", None, 10));
        let mut server = MockProvider::start(scripts)?;
        let mut config = f.config(&server, kind);
        config.capabilities.image_input = true;
        config.capabilities.tool_choice_none = compacting;
        config.capabilities.context_window = Some(60_000);
        let (mut agent, directory) = f.agent(config)?;
        let events = if compacting {
            assert_eq!(complete(&mut agent)?.0.outcome, Outcome::Completed);
            assert_eq!(complete(&mut agent)?.0.outcome, Outcome::Completed);
            let (sender, receiver) = mpsc::sync_channel(64);
            let report = agent.compact(None, Arc::new(AtomicBool::new(false)), sender)?;
            assert_eq!(report.outcome, benitoiteh::agent::CompactOutcome::Failed);
            receiver.into_iter().collect::<Vec<_>>()
        } else {
            let (report, events) = complete(&mut agent)?;
            assert_eq!(report.outcome, Outcome::Failed);
            assert!(
                events
                    .iter()
                    .any(|event| matches!(event, AgentEvent::Error { kind, message }
                if kind == "provider" && message == "Invalid provider request"))
            );
            events
        };
        let requests = server.requests()?;
        assert_eq!(requests.len(), if compacting { 4 } else { 2 });
        assert!(
            requests
                .last()
                .ok_or("missing image request")?
                .body
                .to_string()
                .contains("base64")
        );
        assert!(
            !events
                .iter()
                .any(|event| matches!(event, AgentEvent::ImagesRemoved { .. }))
        );
        assert!(
            !read_records(&directory)?
                .iter()
                .any(|record| matches!(record, Record::ImagesRemoved { .. }))
        );
        assert_eq!(image_result(&agent)?.images.len(), 1);
        assert!(!image_result(&agent)?.text.contains(REMOVED));
        server.shutdown()?;
    }
    Ok(())
}

// 作成時の関門: 実際の要求の画像と HTTP の拒否を使い、成功だけを確定する契約を守る。
// 既存の通信再試行のテストでは、画像の除去・記録順・次の番への反映は確かめられない。
#[test]
fn image_rejection_retries_once_and_commits_removal_only_on_success() -> TestResult {
    for kind in [
        AdapterKind::OpenAiChat,
        AdapterKind::AnthropicMessages,
        AdapterKind::OpenAiResponses,
    ] {
        for (status, succeeds) in [(400, true), (413, true), (400, false)] {
            let f = Fixture::new()?;
            fs::write(f.project.join("image.gif"), b"GIF89a\x01\0\x01\0")?;
            let mut scripts = vec![
                compaction::response(kind, "", Some(("read", json!({"path":"image.gif"}))), 10),
                invalid(status)?,
            ];
            if succeeds {
                scripts.push(compaction::response(
                    kind,
                    "",
                    Some(("read", json!({"path":"a.bnt"}))),
                    10,
                ));
                scripts.push(compaction::response(kind, "done", None, 10));
            } else {
                scripts.push(invalid(400)?);
            }
            let mut server = MockProvider::start(scripts)?;
            let mut config = f.config(&server, kind);
            config.capabilities.image_input = true;
            let (mut agent, directory) = f.agent(config)?;
            let (report, events) = complete(&mut agent)?;
            assert_eq!(
                report.outcome,
                if succeeds {
                    Outcome::Completed
                } else {
                    Outcome::Failed
                }
            );
            let requests = server.requests()?;
            assert_eq!(requests.len(), if succeeds { 4 } else { 3 });
            assert!(
                requests
                    .get(1)
                    .ok_or("missing image request")?
                    .body
                    .to_string()
                    .contains("base64")
            );
            for request in requests.iter().skip(2) {
                assert!(!request.body.to_string().contains("base64"));
                assert!(request.body.to_string().contains(REMOVED));
            }
            let result = image_result(&agent)?;
            let records = read_records(&directory)?;
            let removals = records
                .iter()
                .filter_map(|record| {
                    if let Record::ImagesRemoved { results, .. } = record {
                        Some(results)
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>();
            assert_eq!(removals.len(), usize::from(succeeds));
            assert_eq!(
                events
                    .iter()
                    .filter(|event| matches!(event, AgentEvent::ImagesRemoved { .. }))
                    .count(),
                usize::from(succeeds)
            );
            if succeeds {
                assert_eq!(removals.first().copied(), Some(&vec![result.id.clone()]));
                assert!(result.images.is_empty());
                assert!(result.text.ends_with(REMOVED));
                let position = records
                    .iter()
                    .position(|r| matches!(r, Record::ImagesRemoved { .. }))
                    .ok_or("missing removal")?;
                assert!(matches!(
                    records.get(position.saturating_add(1)),
                    Some(Record::Assistant { .. })
                ));
                let value = serde_json::to_value(records.get(position).ok_or("missing record")?)?;
                assert_eq!(
                    value,
                    json!({"type":"images_removed","node": 4,"results":[result.id]})
                );
            } else {
                assert_eq!(result.images.len(), 1);
                assert!(!result.text.contains(REMOVED));
                assert!(
                    events
                        .iter()
                        .any(|event| matches!(event, AgentEvent::Error { kind, message }
                    if kind == "provider" && message == "Invalid provider request"))
                );
            }
            assert_eq!(replay(&directory)?, *agent.conversation());
            let state = ResumeState::read(&directory)?;
            assert_eq!(state.conversation, *agent.conversation());
            server.shutdown()?;
        }
    }
    Ok(())
}

#[test]
fn invalid_requests_without_sent_images_and_other_errors_do_not_remove_images() -> TestResult {
    for (has_image, status, code) in [
        (false, 400, "bad_request"),
        (true, 401, "invalid_api_key"),
        (true, 429, "insufficient_quota"),
    ] {
        let f = Fixture::new()?;
        fs::write(f.project.join("image.gif"), b"GIF89a\x01\0\x01\0")?;
        let mut scripts = Vec::new();
        if has_image {
            scripts.push(ResponseScript::sse(calls(
                &[("read", json!({"path":"image.gif"}))],
                "tool_calls",
            )));
        }
        scripts.push(ResponseScript::json(
            status,
            &json!({"error":{"code":code,"message":"rejected"}}),
        )?);
        let mut server = MockProvider::start(scripts)?;
        let mut config = f.config(&server, AdapterKind::OpenAiChat);
        config.capabilities.image_input = true;
        let (mut agent, directory) = f.agent(config)?;
        let (report, events) = complete(&mut agent)?;
        assert_eq!(report.outcome, Outcome::Failed);
        assert_eq!(server.requests()?.len(), if has_image { 2 } else { 1 });
        assert!(
            !events
                .iter()
                .any(|event| matches!(event, AgentEvent::ImagesRemoved { .. }))
        );
        assert!(
            !read_records(&directory)?
                .iter()
                .any(|record| matches!(record, Record::ImagesRemoved { .. }))
        );
        if has_image {
            assert_eq!(image_result(&agent)?.images.len(), 1);
        }
        server.shutdown()?;
    }
    Ok(())
}

#[test]
fn cancelling_image_free_retry_preserves_images_and_record() -> TestResult {
    let f = Fixture::new()?;
    fs::write(f.project.join("image.gif"), b"GIF89a\x01\0\x01\0")?;
    let prefix = chat(&json!({"content":"partial retry"}), "stop", false);
    let mut retry = ResponseScript::sse([prefix.clone(), b"data: [DONE]\n\n".to_vec()].concat());
    retry.delivery = Delivery::Chunks {
        size: prefix.len(),
        delay: Duration::from_millis(100),
    };
    let mut server = MockProvider::start(vec![
        ResponseScript::sse(calls(
            &[("read", json!({"path":"image.gif"}))],
            "tool_calls",
        )),
        invalid(400)?,
        retry,
    ])?;
    let mut config = f.config(&server, AdapterKind::OpenAiChat);
    config.capabilities.image_input = true;
    let (mut agent, directory) = f.agent(config)?;
    let cancel = Arc::new(AtomicBool::new(false));
    let (report, events) = run(&mut agent, Arc::clone(&cancel), |event| {
        if matches!(event, AgentEvent::TextDelta { text, .. } if text.contains("partial retry")) {
            cancel.store(true, Ordering::Release);
        }
    })?;
    assert_eq!(report.outcome, Outcome::Cancelled);
    assert_eq!(server.requests()?.len(), 3);
    assert_eq!(image_result(&agent)?.images.len(), 1);
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, AgentEvent::ImagesRemoved { .. }))
    );
    assert_eq!(replay(&directory)?, *agent.conversation());
    assert!(
        !read_records(&directory)?
            .iter()
            .any(|record| matches!(record, Record::ImagesRemoved { .. }))
    );
    server.shutdown()?;
    Ok(())
}

#[test]
fn removed_images_stay_removed_on_agent_resume_even_without_stored_object() -> TestResult {
    let f = Fixture::new()?;
    fs::write(f.project.join("image.gif"), b"GIF89a\x01\0\x01\0")?;
    let mut server = MockProvider::start(vec![
        ResponseScript::sse(calls(
            &[("read", json!({"path":"image.gif"}))],
            "tool_calls",
        )),
        invalid(400)?,
        final_text(),
        final_text(),
    ])?;
    let mut config = f.config(&server, AdapterKind::OpenAiChat);
    config.capabilities.image_input = true;
    let (mut agent, directory) = f.agent(config.clone())?;
    assert_eq!(complete(&mut agent)?.0.outcome, Outcome::Completed);
    let state = ResumeState::read(&directory)?;
    drop(agent);
    for entry in fs::read_dir(directory.join("objects"))? {
        fs::remove_file(entry?.path())?;
    }
    let id: SessionId = directory
        .file_name()
        .and_then(|s| s.to_str())
        .ok_or("missing id")?
        .parse()?;
    let session = Session::open(&f.paths, &id)?;
    let context = ToolContext::new(
        config.mode,
        ScopeRoot::open(&f.project)?,
        ScopeRoot::open(session.temporary_dir())?,
        ScopeRoot::open(directory.join("spill"))?,
        ExcludeRules::new(
            &f.project,
            &f.paths,
            &directory.join("spill"),
            &ReadConfig::default(),
            &ReadConfig::default(),
        )?,
    )
    .with_session(session.clone());
    let environment = Environment::capture(&f.project, config.mode, &[], false, SystemTime::now())?;
    let mut agent = Agent::resume(
        config,
        "synthetic-agent-key".into(),
        context,
        environment,
        session,
        state,
    )?;
    assert!(image_result(&agent)?.images.is_empty());
    assert!(image_result(&agent)?.text.ends_with(REMOVED));
    assert_eq!(complete(&mut agent)?.0.outcome, Outcome::Completed);
    assert!(
        !server
            .requests()?
            .last()
            .ok_or("missing resumed request")?
            .body
            .to_string()
            .contains("base64")
    );
    assert_eq!(replay(&directory)?, *agent.conversation());
    server.shutdown()?;
    Ok(())
}

#[test]
fn continuing_compaction_retries_without_committing_and_later_removal_replays_after_compaction()
-> TestResult {
    use benitoiteh::agent::CompactOutcome;
    let f = Fixture::new()?;
    fs::write(f.project.join("image.gif"), b"GIF89a\x01\0\x01\0")?;
    let kind = AdapterKind::OpenAiChat;
    let mut server = MockProvider::start(vec![
        compaction::response(kind, &"old".repeat(24_000), None, 10),
        compaction::response(
            kind,
            "recent",
            Some(("read", json!({"path":"image.gif"}))),
            10,
        ),
        final_text(),
        invalid(400)?,
        compaction::response(kind, "summary", None, 10),
        invalid(400)?,
        final_text(),
    ])?;
    let mut config = f.config(&server, kind);
    config.capabilities.image_input = true;
    config.capabilities.tool_choice_none = true;
    config.capabilities.context_window = Some(60_000);
    let (mut agent, directory) = f.agent(config)?;
    assert_eq!(complete(&mut agent)?.0.outcome, Outcome::Completed);
    assert_eq!(complete(&mut agent)?.0.outcome, Outcome::Completed);
    let (sender, receiver) = mpsc::sync_channel(64);
    let report = agent.compact(None, Arc::new(AtomicBool::new(false)), sender)?;
    let events = receiver.into_iter().collect::<Vec<_>>();
    assert_eq!(report.outcome, CompactOutcome::Compacted);
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, AgentEvent::ImagesRemoved { .. }))
    );
    assert_eq!(image_result(&agent)?.images.len(), 1);
    assert!(
        !read_records(&directory)?
            .iter()
            .any(|record| matches!(record, Record::ImagesRemoved { .. }))
    );
    assert_eq!(complete(&mut agent)?.0.outcome, Outcome::Completed);
    let requests = server.requests()?;
    assert_eq!(requests.len(), 7);
    for (index, has_image) in [(3, true), (4, false), (5, true), (6, false)] {
        let body = &requests.get(index).ok_or("missing request")?.body;
        assert_eq!(body.to_string().contains("base64"), has_image);
    }
    assert_eq!(replay(&directory)?, *agent.conversation());
    assert!(image_result(&agent)?.images.is_empty());
    let records = read_records(&directory)?;
    let compacted = records
        .iter()
        .position(|record| matches!(record, Record::Compaction { .. }))
        .ok_or("missing compaction")?;
    let removed = records
        .iter()
        .position(|record| matches!(record, Record::ImagesRemoved { .. }))
        .ok_or("missing removal")?;
    assert!(compacted < removed);
    server.shutdown()?;
    Ok(())
}

#[test]
fn image_retry_context_overflow_compacts_the_original_conversation() -> TestResult {
    let f = Fixture::new()?;
    fs::write(f.project.join("image.gif"), b"GIF89a\x01\0\x01\0")?;
    let mut server = MockProvider::start(vec![
        ResponseScript::sse(calls(
            &[("read", json!({"path":"image.gif"}))],
            "tool_calls",
        )),
        invalid(400)?,
        ResponseScript::json(
            400,
            &json!({"error":{"code":"context_length_exceeded","message":"context length exceeded"}}),
        )?,
        final_text(),
        final_text(),
    ])?;
    let mut config = f.config(&server, AdapterKind::OpenAiChat);
    config.capabilities.image_input = true;
    config.capabilities.context_window = Some(60_000);
    let (mut agent, directory) = f.agent(config)?;
    let (report, events) = complete(&mut agent)?;
    assert_eq!(report.outcome, Outcome::Completed);
    assert_eq!(report.summary.compactions, 1);
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, AgentEvent::ImagesRemoved { .. }))
    );
    assert!(
        !read_records(&directory)?
            .iter()
            .any(|record| matches!(record, Record::ImagesRemoved { .. }))
    );
    assert_eq!(replay(&directory)?, *agent.conversation());
    assert_eq!(server.requests()?.len(), 5);
    server.shutdown()?;
    Ok(())
}
