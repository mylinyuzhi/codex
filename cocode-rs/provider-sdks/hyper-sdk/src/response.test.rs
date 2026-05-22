use super::*;

#[test]
fn test_response_text() {
    let response = GenerateResponse::new("resp_1", "gpt-4o").with_content(vec![
        ContentBlock::text("Hello "),
        ContentBlock::text("world!"),
    ]);

    assert_eq!(response.text(), "Hello world!");
}

#[test]
fn test_response_tool_calls() {
    let response = GenerateResponse::new("resp_1", "gpt-4o")
        .with_content(vec![
            ContentBlock::text("Let me check the weather."),
            ContentBlock::tool_use(
                "call_1",
                "get_weather",
                serde_json::json!({"location": "NYC"}),
            ),
        ])
        .with_finish_reason(FinishReason::ToolCalls);

    assert!(response.has_tool_calls());
    assert!(response.stopped_for_tool_calls());

    let calls = response.tool_calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].name, "get_weather");
}

#[test]
fn test_response_thinking() {
    let response = GenerateResponse::new("resp_1", "claude-3-opus").with_content(vec![
        ContentBlock::thinking("Let me think about this..."),
        ContentBlock::text("The answer is 42."),
    ]);

    assert!(response.has_thinking());
    assert_eq!(response.thinking(), Some("Let me think about this..."));
    assert_eq!(response.text(), "The answer is 42.");
}

#[test]
fn test_token_usage() {
    let usage = TokenUsage::new(100, 50)
        .with_cache_read_tokens(20)
        .with_cache_creation_tokens(15)
        .with_reasoning_tokens(30);

    assert_eq!(usage.prompt_tokens, 100);
    assert_eq!(usage.completion_tokens, 50);
    assert_eq!(usage.total_tokens, 150);
    assert_eq!(usage.cache_read_tokens, Some(20));
    assert_eq!(usage.cache_creation_tokens, Some(15));
    assert_eq!(usage.reasoning_tokens, Some(30));
}
