//! Gemini の署名を次の道具の要求へ渡すループの契約（実装プラン P1-12、設計書 02-01）。

use super::*;

#[test]
fn gemini_tool_result_and_signatures_reach_the_next_request() -> TestResult {
    let f = Fixture::new()?;
    // 2026-10-18 の応答の形に合わせ、推論の印は偽の値とする。
    let raw = json!({"role":"assistant","content":null,
        "extra_content":{"google":{"thought_signature":"fake-message-signature"}},
        "tool_calls":[{"id":"gemini-call","type":"function",
            "function":{"name":"read","arguments":"{\"path\":\"a.bnt\"}"},
            "extra_content":{"google":{"thought_signature":"fake-call-signature"}}}]});
    let mut server = MockProvider::start(vec![
        ResponseScript::sse(chat(&raw, "stop", true)),
        final_text(),
    ])?;
    let (mut agent, directory) = f.agent(f.config(&server, AdapterKind::OpenAiChat))?;
    let (report, events) = complete(&mut agent)?;
    assert_eq!(report.outcome, Outcome::Completed);
    assert_eq!(report.summary.turns, 2);
    assert_eq!(report.summary.tool_calls.get("read"), Some(&1));
    assert!(!events.iter().any(|e| matches!(e, AgentEvent::Retry { .. })));
    let requests = server.requests()?;
    assert_eq!(requests.len(), 2);
    let body = &requests
        .get(1)
        .ok_or(io::Error::from(io::ErrorKind::InvalidData))?
        .body;
    assert_eq!(body.pointer("/messages/2"), Some(&raw));
    assert_eq!(body.pointer("/messages/3/role"), Some(&json!("tool")));
    assert_eq!(
        body.pointer("/messages/3/tool_call_id"),
        Some(&json!("gemini-call"))
    );
    assert!(
        body.pointer("/messages/3/content")
            .and_then(Value::as_str)
            .is_some_and(|s| s.contains("before"))
    );
    assert_eq!(replay(&directory)?, *agent.conversation());
    server.shutdown()?;
    Ok(())
}
