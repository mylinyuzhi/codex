use super::*;

fn make_response(text: &str) -> LanguageModelGenerateResult {
    LanguageModelGenerateResult::new(
        vec![AssistantContentPart::text(text)],
        Usage::new(10, 5),
        FinishReason::stop(),
    )
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
    assert_eq!(collected.finish_reason, FinishReason::stop());
    assert!(collected.usage.is_some());
}

#[test]
fn test_streaming_query_result_constructors() {
    let assistant = StreamingQueryResult::assistant(vec![AssistantContentPart::text("test")]);
    assert!(assistant.has_content());

    let event = StreamingQueryResult::event(LanguageModelStreamPart::TextDelta {
        id: "0".to_string(),
        delta: "hi".to_string(),
        provider_metadata: None,
    });
    assert_eq!(event.result_type, QueryResultType::Event);

    let retry = StreamingQueryResult::retry();
    assert_eq!(retry.result_type, QueryResultType::Retry);

    let error = StreamingQueryResult::error("test error");
    assert_eq!(error.result_type, QueryResultType::Error);
    assert_eq!(error.error, Some("test error".to_string()));

    let done = StreamingQueryResult::done(None, FinishReason::stop());
    assert_eq!(done.result_type, QueryResultType::Done);
}

#[test]
fn test_tool_call_detection() {
    let result = StreamingQueryResult::assistant(vec![
        AssistantContentPart::text("Let me help"),
        AssistantContentPart::tool_call(
            "call_1",
            "get_weather",
            serde_json::json!({"city": "NYC"}),
        ),
    ]);

    assert!(result.has_tool_calls());
    assert_eq!(result.tool_calls().len(), 1);
}

#[test]
fn test_collected_response_into_assistant_message() {
    let collected = CollectedResponse {
        content: vec![AssistantContentPart::text("Hello, world!")],
        usage: Some(ProtocolUsage {
            input_tokens: 10,
            output_tokens: 5,
            cache_read_tokens: None,
            cache_creation_tokens: None,
            reasoning_tokens: None,
        }),
        finish_reason: FinishReason::stop(),
    };

    let msg = collected.into_assistant_message();

    match &msg {
        crate::LanguageModelMessage::Assistant { content, .. } => {
            assert_eq!(content.len(), 1);
            match &content[0] {
                AssistantContentPart::Text(tp) => assert_eq!(tp.text, "Hello, world!"),
                _ => panic!("Expected text part"),
            }
        }
        _ => panic!("Expected assistant message"),
    }
}

#[test]
fn test_check_for_completed_content_file() {
    let file_part = vercel_ai_provider::LanguageModelV4StreamPart::File(
        vercel_ai_provider::language_model::v4::stream::File {
            data: "base64data==".to_string(),
            media_type: "image/png".to_string(),
            provider_metadata: None,
        },
    );
    let snapshot = StreamSnapshot::default();
    let result = UnifiedStream::check_for_completed_content(&file_part, &snapshot);
    assert!(result.is_some());
    let result = result.unwrap();
    assert_eq!(result.result_type, QueryResultType::Assistant);
    assert!(matches!(&result.content[0], AssistantContentPart::File(_)));
}

#[test]
fn test_check_for_completed_content_source() {
    use vercel_ai_provider::content::SourcePart;

    let source = SourcePart::url("src-1", "https://example.com");
    let source_part = vercel_ai_provider::LanguageModelV4StreamPart::Source(source);
    let snapshot = StreamSnapshot::default();
    let result = UnifiedStream::check_for_completed_content(&source_part, &snapshot);
    assert!(result.is_some());
    let result = result.unwrap();
    assert_eq!(result.result_type, QueryResultType::Assistant);
    assert!(matches!(
        &result.content[0],
        AssistantContentPart::Source(_)
    ));
}

#[test]
fn test_collected_response_into_generate_result() {
    let collected = CollectedResponse {
        content: vec![AssistantContentPart::text("Response text")],
        usage: Some(ProtocolUsage {
            input_tokens: 100,
            output_tokens: 50,
            cache_read_tokens: Some(20),
            cache_creation_tokens: Some(10),
            reasoning_tokens: Some(15),
        }),
        finish_reason: FinishReason::stop(),
    };

    let response = collected.into_generate_result();

    assert_eq!(response.finish_reason, FinishReason::stop());
    assert_eq!(response.content.len(), 1);

    let usage = &response.usage;
    assert_eq!(usage.total_input_tokens(), 100);
    assert_eq!(usage.total_output_tokens(), 50);
    assert_eq!(usage.input_tokens.cache_read, Some(20));
    assert_eq!(usage.input_tokens.cache_write, Some(10));
    assert_eq!(usage.output_tokens.reasoning, Some(15));
}

// =========================================================================
// P20: Stream error event propagation
// =========================================================================

#[test]
fn test_check_for_completed_content_error() {
    use vercel_ai_provider::language_model::v4::stream::StreamError;

    let error_part = LanguageModelStreamPart::Error {
        error: StreamError {
            message: "Overloaded, please retry".to_string(),
            code: Some("overloaded".to_string()),
            is_retryable: true,
        },
    };
    let snapshot = StreamSnapshot::default();
    let result = UnifiedStream::check_for_completed_content(&error_part, &snapshot);
    assert!(result.is_some());
    let result = result.unwrap();
    assert_eq!(result.result_type, QueryResultType::Error);
    assert_eq!(result.error, Some("Overloaded, please retry".to_string()));
}

#[test]
fn test_check_for_completed_content_error_no_code() {
    use vercel_ai_provider::language_model::v4::stream::StreamError;

    let error_part = LanguageModelStreamPart::Error {
        error: StreamError {
            message: "Connection reset mid-stream".to_string(),
            code: None,
            is_retryable: false,
        },
    };
    let snapshot = StreamSnapshot::default();
    let result = UnifiedStream::check_for_completed_content(&error_part, &snapshot);
    assert!(result.is_some());
    let result = result.unwrap();
    assert_eq!(result.result_type, QueryResultType::Error);
    assert_eq!(
        result.error,
        Some("Connection reset mid-stream".to_string())
    );
}

// =========================================================================
// P24: ReasoningFile stream event propagation
// =========================================================================

#[test]
fn test_check_for_completed_content_reasoning_file() {
    use vercel_ai_provider::language_model::v4::stream::ReasoningFile;

    let reasoning_file_part = LanguageModelStreamPart::ReasoningFile(ReasoningFile {
        data: "cmVhc29uaW5nZmlsZQ==".to_string(),
        media_type: "image/png".to_string(),
        provider_metadata: None,
    });
    let snapshot = StreamSnapshot::default();
    let result = UnifiedStream::check_for_completed_content(&reasoning_file_part, &snapshot);
    assert!(result.is_some());
    let result = result.unwrap();
    assert_eq!(result.result_type, QueryResultType::Assistant);
    assert!(matches!(
        &result.content[0],
        AssistantContentPart::ReasoningFile(_)
    ));
}

// =========================================================================
// Custom stream part forwarding
// =========================================================================

#[test]
fn test_check_for_completed_content_custom() {
    let custom_part = LanguageModelStreamPart::Custom {
        kind: "openai-compaction".to_string(),
        provider_metadata: None,
    };
    let snapshot = StreamSnapshot::default();
    let result = UnifiedStream::check_for_completed_content(&custom_part, &snapshot);
    assert!(result.is_some());
    let result = result.unwrap();
    assert_eq!(result.result_type, QueryResultType::Assistant);
    assert!(matches!(
        &result.content[0],
        AssistantContentPart::Custom(cp) if cp.kind == "openai-compaction"
    ));
}

#[test]
fn test_check_for_completed_content_custom_with_metadata() {
    use std::collections::HashMap;

    let mut inner = HashMap::new();
    inner.insert(
        "summary".to_string(),
        serde_json::json!("compacted 3 messages"),
    );
    let mut metadata = HashMap::new();
    metadata.insert(
        "openai".to_string(),
        serde_json::Value::Object(inner.into_iter().collect()),
    );
    let provider_metadata = vercel_ai_provider::ProviderMetadata(metadata);

    let custom_part = LanguageModelStreamPart::Custom {
        kind: "openai-compaction".to_string(),
        provider_metadata: Some(provider_metadata),
    };
    let snapshot = StreamSnapshot::default();
    let result = UnifiedStream::check_for_completed_content(&custom_part, &snapshot);
    assert!(result.is_some());
    let result = result.unwrap();
    assert_eq!(result.result_type, QueryResultType::Assistant);
    match &result.content[0] {
        AssistantContentPart::Custom(cp) => {
            assert_eq!(cp.kind, "openai-compaction");
            assert!(cp.provider_options.is_none());
            assert!(cp.provider_metadata.is_some());
        }
        _ => panic!("Expected Custom content part"),
    }
}

// =========================================================================
// P29: StreamError structured fields preservation
// =========================================================================

#[test]
fn test_stream_error_preserves_retryable_and_code() {
    use vercel_ai_provider::language_model::v4::stream::StreamError;

    let error_part = LanguageModelStreamPart::Error {
        error: StreamError {
            message: "Model overloaded".to_string(),
            code: Some("overloaded".to_string()),
            is_retryable: true,
        },
    };
    let snapshot = StreamSnapshot::default();
    let result = UnifiedStream::check_for_completed_content(&error_part, &snapshot);
    assert!(result.is_some());
    let result = result.unwrap();
    assert_eq!(result.result_type, QueryResultType::Error);
    assert_eq!(result.is_retryable, Some(true));
    assert_eq!(result.error_code, Some("overloaded".to_string()));
}

#[test]
fn test_stream_error_non_retryable_fields() {
    use vercel_ai_provider::language_model::v4::stream::StreamError;

    let error_part = LanguageModelStreamPart::Error {
        error: StreamError {
            message: "Fatal error".to_string(),
            code: None,
            is_retryable: false,
        },
    };
    let snapshot = StreamSnapshot::default();
    let result = UnifiedStream::check_for_completed_content(&error_part, &snapshot);
    assert!(result.is_some());
    let result = result.unwrap();
    assert_eq!(result.is_retryable, Some(false));
    assert_eq!(result.error_code, None);
}
