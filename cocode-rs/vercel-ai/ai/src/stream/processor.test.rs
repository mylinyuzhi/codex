use futures::stream;
use serde_json::json;
use vercel_ai_provider::errors::AISdkError;
use vercel_ai_provider::language_model::v4::finish_reason::FinishReason;
use vercel_ai_provider::language_model::v4::stream::LanguageModelV4StreamPart;
use vercel_ai_provider::language_model::v4::usage::Usage;
use vercel_ai_provider::tool::ToolCall;

use super::*;

/// Helper: create a StreamProcessor from a vec of parts.
fn processor_from_parts(parts: Vec<LanguageModelV4StreamPart>) -> StreamProcessor {
    let stream: Vec<Result<LanguageModelV4StreamPart, AISdkError>> =
        parts.into_iter().map(Ok).collect();
    StreamProcessor::from_stream(Box::pin(stream::iter(stream)))
}

#[tokio::test]
async fn test_text_accumulation() {
    let parts = vec![
        LanguageModelV4StreamPart::TextStart {
            id: "t1".into(),
            provider_metadata: None,
        },
        LanguageModelV4StreamPart::TextDelta {
            id: "t1".into(),
            delta: "Hello ".into(),
            provider_metadata: None,
        },
        LanguageModelV4StreamPart::TextDelta {
            id: "t1".into(),
            delta: "world!".into(),
            provider_metadata: None,
        },
        LanguageModelV4StreamPart::TextEnd {
            id: "t1".into(),
            provider_metadata: None,
        },
        LanguageModelV4StreamPart::Finish {
            usage: Usage::new(10, 5),
            finish_reason: FinishReason::stop(),
            provider_metadata: None,
        },
    ];

    let snapshot = processor_from_parts(parts).collect().await.unwrap();

    assert_eq!(snapshot.text, "Hello world!");
    assert!(snapshot.is_complete);
    assert!(snapshot.finish_reason.unwrap().is_stop());
    assert_eq!(snapshot.usage.unwrap().total_input_tokens(), 10);
}

#[tokio::test]
async fn test_into_text() {
    let parts = vec![
        LanguageModelV4StreamPart::TextStart {
            id: "t1".into(),
            provider_metadata: None,
        },
        LanguageModelV4StreamPart::TextDelta {
            id: "t1".into(),
            delta: "Hi!".into(),
            provider_metadata: None,
        },
        LanguageModelV4StreamPart::TextEnd {
            id: "t1".into(),
            provider_metadata: None,
        },
        LanguageModelV4StreamPart::Finish {
            usage: Usage::new(1, 1),
            finish_reason: FinishReason::stop(),
            provider_metadata: None,
        },
    ];

    let text = processor_from_parts(parts).into_text().await.unwrap();
    assert_eq!(text, "Hi!");
}

#[tokio::test]
async fn test_reasoning_accumulation() {
    let parts = vec![
        LanguageModelV4StreamPart::ReasoningStart {
            id: "r1".into(),
            provider_metadata: None,
        },
        LanguageModelV4StreamPart::ReasoningDelta {
            id: "r1".into(),
            delta: "Let me think...".into(),
            provider_metadata: None,
        },
        LanguageModelV4StreamPart::ReasoningEnd {
            id: "r1".into(),
            provider_metadata: None,
        },
        LanguageModelV4StreamPart::TextStart {
            id: "t1".into(),
            provider_metadata: None,
        },
        LanguageModelV4StreamPart::TextDelta {
            id: "t1".into(),
            delta: "Answer".into(),
            provider_metadata: None,
        },
        LanguageModelV4StreamPart::TextEnd {
            id: "t1".into(),
            provider_metadata: None,
        },
        LanguageModelV4StreamPart::Finish {
            usage: Usage::new(10, 20),
            finish_reason: FinishReason::stop(),
            provider_metadata: None,
        },
    ];

    let snapshot = processor_from_parts(parts).collect().await.unwrap();

    assert!(snapshot.has_reasoning());
    let reasoning = snapshot.reasoning.as_ref().unwrap();
    assert_eq!(reasoning.content, "Let me think...");
    assert!(reasoning.is_complete);
    assert_eq!(snapshot.text, "Answer");
}

#[tokio::test]
async fn test_tool_call_with_streaming_input() {
    let parts = vec![
        LanguageModelV4StreamPart::ToolInputStart {
            id: "tc1".into(),
            tool_name: "read_file".into(),
            provider_executed: None,
            dynamic: None,
            title: None,
            provider_metadata: None,
        },
        LanguageModelV4StreamPart::ToolInputDelta {
            id: "tc1".into(),
            delta: r#"{"path":""#.into(),
            provider_metadata: None,
        },
        LanguageModelV4StreamPart::ToolInputDelta {
            id: "tc1".into(),
            delta: r#"foo.rs"}"#.into(),
            provider_metadata: None,
        },
        LanguageModelV4StreamPart::ToolInputEnd {
            id: "tc1".into(),
            provider_metadata: None,
        },
        LanguageModelV4StreamPart::ToolCall(ToolCall::new(
            "tc1",
            "read_file",
            json!({"path": "foo.rs"}),
        )),
        LanguageModelV4StreamPart::Finish {
            usage: Usage::new(5, 10),
            finish_reason: FinishReason::tool_calls(),
            provider_metadata: None,
        },
    ];

    let snapshot = processor_from_parts(parts).collect().await.unwrap();

    assert!(snapshot.has_tool_calls());
    assert_eq!(snapshot.tool_calls.len(), 1);

    let tc = &snapshot.tool_calls[0];
    assert_eq!(tc.id, "tc1");
    assert_eq!(tc.tool_name, "read_file");
    assert_eq!(tc.input_json, json!({"path": "foo.rs"}).to_string());
    assert!(tc.is_input_complete);
    assert!(tc.is_complete);
    assert!(snapshot.finish_reason.unwrap().is_tool_calls());
}

#[tokio::test]
async fn test_tool_call_without_streaming_input() {
    let parts = vec![
        LanguageModelV4StreamPart::ToolCall(ToolCall::new("tc1", "bash", json!({"cmd": "ls"}))),
        LanguageModelV4StreamPart::Finish {
            usage: Usage::new(5, 3),
            finish_reason: FinishReason::tool_calls(),
            provider_metadata: None,
        },
    ];

    let snapshot = processor_from_parts(parts).collect().await.unwrap();

    assert_eq!(snapshot.tool_calls.len(), 1);
    let tc = &snapshot.tool_calls[0];
    assert_eq!(tc.tool_name, "bash");
    assert!(tc.is_complete);
    assert!(tc.is_input_complete);
}

#[tokio::test]
async fn test_multiple_tool_calls() {
    let parts = vec![
        LanguageModelV4StreamPart::ToolInputStart {
            id: "tc1".into(),
            tool_name: "read".into(),
            provider_executed: None,
            dynamic: None,
            title: None,
            provider_metadata: None,
        },
        LanguageModelV4StreamPart::ToolInputDelta {
            id: "tc1".into(),
            delta: r#"{"a":1}"#.into(),
            provider_metadata: None,
        },
        LanguageModelV4StreamPart::ToolInputEnd {
            id: "tc1".into(),
            provider_metadata: None,
        },
        LanguageModelV4StreamPart::ToolCall(ToolCall::new("tc1", "read", json!({"a": 1}))),
        LanguageModelV4StreamPart::ToolInputStart {
            id: "tc2".into(),
            tool_name: "write".into(),
            provider_executed: None,
            dynamic: None,
            title: None,
            provider_metadata: None,
        },
        LanguageModelV4StreamPart::ToolInputDelta {
            id: "tc2".into(),
            delta: r#"{"b":2}"#.into(),
            provider_metadata: None,
        },
        LanguageModelV4StreamPart::ToolInputEnd {
            id: "tc2".into(),
            provider_metadata: None,
        },
        LanguageModelV4StreamPart::ToolCall(ToolCall::new("tc2", "write", json!({"b": 2}))),
        LanguageModelV4StreamPart::Finish {
            usage: Usage::new(10, 20),
            finish_reason: FinishReason::tool_calls(),
            provider_metadata: None,
        },
    ];

    let snapshot = processor_from_parts(parts).collect().await.unwrap();

    assert_eq!(snapshot.completed_tool_calls().len(), 2);
    assert_eq!(snapshot.pending_tool_calls().len(), 0);
    assert_eq!(snapshot.tool_calls[0].tool_name, "read");
    assert_eq!(snapshot.tool_calls[1].tool_name, "write");
}

#[tokio::test]
async fn test_next_returns_parts_and_snapshot() {
    let parts = vec![
        LanguageModelV4StreamPart::TextStart {
            id: "t1".into(),
            provider_metadata: None,
        },
        LanguageModelV4StreamPart::TextDelta {
            id: "t1".into(),
            delta: "Hi".into(),
            provider_metadata: None,
        },
        LanguageModelV4StreamPart::TextEnd {
            id: "t1".into(),
            provider_metadata: None,
        },
    ];

    let mut processor = processor_from_parts(parts);

    // First event: TextStart
    let (part, snapshot) = processor.next().await.unwrap().unwrap();
    assert!(matches!(part, LanguageModelV4StreamPart::TextStart { .. }));
    assert_eq!(snapshot.text, "");

    // Second event: TextDelta
    let (part, snapshot) = processor.next().await.unwrap().unwrap();
    assert!(matches!(part, LanguageModelV4StreamPart::TextDelta { .. }));
    assert_eq!(snapshot.text, "Hi");

    // Third event: TextEnd
    let (_, snapshot) = processor.next().await.unwrap().unwrap();
    assert_eq!(snapshot.text, "Hi");

    // Stream ends
    assert!(processor.next().await.is_none());
}

#[tokio::test]
async fn test_stream_error_propagation() {
    let items: Vec<Result<LanguageModelV4StreamPart, AISdkError>> = vec![
        Ok(LanguageModelV4StreamPart::TextStart {
            id: "t1".into(),
            provider_metadata: None,
        }),
        Err(AISdkError::new("connection lost")),
    ];

    let mut processor = StreamProcessor::from_stream(Box::pin(stream::iter(items)));

    // First event succeeds
    assert!(processor.next().await.unwrap().is_ok());

    // Second event is an error
    let err = processor.next().await.unwrap().unwrap_err();
    assert_eq!(err.message, "connection lost");
}

#[tokio::test]
async fn test_empty_stream() {
    let processor = processor_from_parts(vec![]);
    let snapshot = processor.collect().await.unwrap();
    assert_eq!(snapshot.text, "");
    assert!(!snapshot.is_complete);
    assert!(!snapshot.has_tool_calls());
    assert!(!snapshot.has_reasoning());
}

#[tokio::test]
async fn test_warnings_from_stream_start() {
    use vercel_ai_provider::shared::Warning;

    let parts = vec![
        LanguageModelV4StreamPart::StreamStart {
            warnings: vec![Warning::unsupported("thinking")],
        },
        LanguageModelV4StreamPart::TextStart {
            id: "t1".into(),
            provider_metadata: None,
        },
        LanguageModelV4StreamPart::TextDelta {
            id: "t1".into(),
            delta: "ok".into(),
            provider_metadata: None,
        },
        LanguageModelV4StreamPart::TextEnd {
            id: "t1".into(),
            provider_metadata: None,
        },
        LanguageModelV4StreamPart::Finish {
            usage: Usage::new(1, 1),
            finish_reason: FinishReason::stop(),
            provider_metadata: None,
        },
    ];

    let snapshot = processor_from_parts(parts).collect().await.unwrap();
    assert_eq!(snapshot.warnings.len(), 1);
}
