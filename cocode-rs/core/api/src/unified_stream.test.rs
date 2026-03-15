use super::*;
use hyper_sdk::TokenUsage;

fn make_response(text: &str) -> GenerateResponse {
    GenerateResponse {
        id: "resp_1".to_string(),
        content: vec![ContentBlock::text(text)],
        finish_reason: FinishReason::Stop,
        usage: Some(TokenUsage {
            prompt_tokens: 10,
            completion_tokens: 5,
            total_tokens: 15,
            cache_read_tokens: None,
            cache_creation_tokens: None,
            reasoning_tokens: None,
        }),
        model: "test-model".to_string(),
    }
}

#[tokio::test]
async fn test_non_streaming_response() {
    let response = make_response("Hello, world!");
    let mut stream = UnifiedStream::from_response(response);

    let result = stream.next().await;
    assert!(result.is_some());

    let result = result.unwrap().unwrap();
    assert_eq!(result.result_type, QueryResultType::Assistant);
    assert!(!result.content.is_empty());

    // Should be consumed
    let result = stream.next().await;
    assert!(result.is_none());
}

#[tokio::test]
async fn test_collect_non_streaming() {
    let response = make_response("Hello!");
    let stream = UnifiedStream::from_response(response);

    let collected = stream.collect().await.unwrap();
    assert_eq!(collected.text(), "Hello!");
    assert_eq!(collected.finish_reason, FinishReason::Stop);
    assert!(collected.usage.is_some());
}

#[test]
fn test_streaming_query_result_constructors() {
    let assistant = StreamingQueryResult::assistant(vec![ContentBlock::text("test")]);
    assert!(assistant.has_content());

    let event = StreamingQueryResult::event(StreamUpdate::TextDelta {
        index: 0,
        delta: "hi".to_string(),
    });
    assert_eq!(event.result_type, QueryResultType::Event);

    let retry = StreamingQueryResult::retry();
    assert_eq!(retry.result_type, QueryResultType::Retry);

    let error = StreamingQueryResult::error("test error");
    assert_eq!(error.result_type, QueryResultType::Error);
    assert_eq!(error.error, Some("test error".to_string()));

    let done = StreamingQueryResult::done(None, FinishReason::Stop);
    assert_eq!(done.result_type, QueryResultType::Done);
}

#[test]
fn test_tool_call_detection() {
    let result = StreamingQueryResult::assistant(vec![
        ContentBlock::text("Let me help"),
        ContentBlock::tool_use("call_1", "get_weather", serde_json::json!({"city": "NYC"})),
    ]);

    assert!(result.has_tool_calls());
    assert_eq!(result.tool_calls().len(), 1);
}

#[test]
fn test_collected_response_into_message() {
    let collected = CollectedResponse {
        content: vec![ContentBlock::text("Hello, world!")],
        usage: Some(ProtocolUsage {
            input_tokens: 10,
            output_tokens: 5,
            cache_read_tokens: None,
            cache_creation_tokens: None,
            reasoning_tokens: None,
        }),
        finish_reason: FinishReason::Stop,
    };

    let msg = collected.into_message("anthropic", "claude-sonnet-4");

    assert_eq!(msg.role, Role::Assistant);
    assert_eq!(msg.text(), "Hello, world!");
    assert_eq!(msg.source_provider(), Some("anthropic"));
    assert_eq!(msg.source_model(), Some("claude-sonnet-4"));
}

#[test]
fn test_collected_response_into_response() {
    let collected = CollectedResponse {
        content: vec![ContentBlock::text("Response text")],
        usage: Some(ProtocolUsage {
            input_tokens: 100,
            output_tokens: 50,
            cache_read_tokens: Some(20),
            cache_creation_tokens: None,
            reasoning_tokens: None,
        }),
        finish_reason: FinishReason::Stop,
    };

    let response = collected.into_response("resp_123", "gpt-4o");

    assert_eq!(response.id, "resp_123");
    assert_eq!(response.model, "gpt-4o");
    assert_eq!(response.finish_reason, FinishReason::Stop);
    assert_eq!(response.text(), "Response text");

    let usage = response.usage.unwrap();
    assert_eq!(usage.prompt_tokens, 100);
    assert_eq!(usage.completion_tokens, 50);
    assert_eq!(usage.total_tokens, 150);
    assert_eq!(usage.cache_read_tokens, Some(20));
}
