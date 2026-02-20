use super::*;
use crate::response::TokenUsage;
use futures::stream;
use std::sync::Arc;
use std::sync::Mutex;

fn make_stream(
    events: Vec<StreamEvent>,
) -> Pin<Box<dyn futures::Stream<Item = Result<StreamEvent, HyperError>> + Send>> {
    Box::pin(stream::iter(events.into_iter().map(Ok)))
}

#[tokio::test]
async fn test_processor_collect() {
    let events = vec![
        StreamEvent::response_created("resp_1"),
        StreamEvent::text_delta(0, "Hello "),
        StreamEvent::text_delta(0, "world!"),
        StreamEvent::response_done("resp_1", FinishReason::Stop),
    ];

    let processor = StreamProcessor::new(make_stream(events));
    let response = processor.collect().await.unwrap();

    assert_eq!(response.text(), "Hello world!");
    assert_eq!(response.finish_reason, FinishReason::Stop);
}

#[tokio::test]
async fn test_processor_on_update_receives_accumulated_state() {
    let events = vec![
        StreamEvent::response_created("resp_1"),
        StreamEvent::text_delta(0, "Hello "),
        StreamEvent::text_delta(0, "world!"),
        StreamEvent::response_done("resp_1", FinishReason::Stop),
    ];

    let snapshots: Arc<Mutex<Vec<StreamSnapshot>>> = Arc::new(Mutex::new(Vec::new()));
    let snapshots_clone = snapshots.clone();

    let processor = StreamProcessor::new(make_stream(events));
    processor
        .on_update(|snapshot| {
            let snapshots = snapshots_clone.clone();
            async move {
                snapshots.lock().unwrap().push(snapshot);
                Ok(())
            }
        })
        .await
        .unwrap();

    let snapshots = snapshots.lock().unwrap();
    assert_eq!(snapshots.len(), 4);

    // Verify progressive accumulation
    assert_eq!(snapshots[0].text, ""); // response_created
    assert_eq!(snapshots[1].text, "Hello "); // first delta
    assert_eq!(snapshots[2].text, "Hello world!"); // second delta
    assert_eq!(snapshots[3].text, "Hello world!"); // response_done
    assert!(snapshots[3].is_complete);
}

#[tokio::test]
async fn test_processor_for_each() {
    let events = vec![
        StreamEvent::text_delta(0, "Hi"),
        StreamEvent::ToolCallStart {
            index: 1,
            id: "call_1".to_string(),
            name: "get_weather".to_string(),
        },
        StreamEvent::response_done("resp_1", FinishReason::ToolCalls),
    ];

    let updates: Arc<Mutex<Vec<(String, String)>>> = Arc::new(Mutex::new(Vec::new()));
    let updates_clone = updates.clone();

    let processor = StreamProcessor::new(make_stream(events));
    processor
        .for_each(|update, snapshot| {
            let updates = updates_clone.clone();
            let update_type = format!("{:?}", std::mem::discriminant(&update));
            let text = snapshot.text;
            async move {
                updates.lock().unwrap().push((update_type, text));
                Ok(())
            }
        })
        .await
        .unwrap();

    let updates = updates.lock().unwrap();
    assert_eq!(updates.len(), 3);
}

#[tokio::test]
async fn test_processor_on_text() {
    let events = vec![
        StreamEvent::text_delta(0, "A"),
        StreamEvent::text_delta(0, "B"),
        StreamEvent::text_delta(0, "C"),
        StreamEvent::response_done("resp_1", FinishReason::Stop),
    ];

    let deltas: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let deltas_clone = deltas.clone();

    let processor = StreamProcessor::new(make_stream(events));
    processor
        .on_text(|delta| {
            let deltas = deltas_clone.clone();
            async move {
                deltas.lock().unwrap().push(delta);
                Ok(())
            }
        })
        .await
        .unwrap();

    let deltas = deltas.lock().unwrap();
    assert_eq!(*deltas, vec!["A", "B", "C"]);
}

#[tokio::test]
async fn test_processor_with_thinking() {
    let events = vec![
        StreamEvent::response_created("resp_1"),
        StreamEvent::thinking_delta(0, "Let me "),
        StreamEvent::thinking_delta(0, "think..."),
        StreamEvent::ThinkingDone {
            index: 0,
            content: "Let me think...".to_string(),
            signature: Some("sig123".to_string()),
        },
        StreamEvent::text_delta(1, "The answer is 42."),
        StreamEvent::response_done("resp_1", FinishReason::Stop),
    ];

    let processor = StreamProcessor::new(make_stream(events));
    let response = processor.collect().await.unwrap();

    assert!(response.has_thinking());
    assert_eq!(response.thinking(), Some("Let me think..."));
    assert_eq!(response.text(), "The answer is 42.");
}

#[tokio::test]
async fn test_processor_with_tool_calls() {
    let events = vec![
        StreamEvent::ToolCallStart {
            index: 0,
            id: "call_1".to_string(),
            name: "get_weather".to_string(),
        },
        StreamEvent::ToolCallDelta {
            index: 0,
            id: "call_1".to_string(),
            arguments_delta: "{\"city\":".to_string(),
        },
        StreamEvent::ToolCallDelta {
            index: 0,
            id: "call_1".to_string(),
            arguments_delta: "\"NYC\"}".to_string(),
        },
        StreamEvent::ToolCallDone {
            index: 0,
            tool_call: crate::tools::ToolCall::new(
                "call_1",
                "get_weather",
                serde_json::json!({"city": "NYC"}),
            ),
        },
        StreamEvent::response_done("resp_1", FinishReason::ToolCalls),
    ];

    let processor = StreamProcessor::new(make_stream(events));
    let response = processor.collect().await.unwrap();

    let tool_calls = response.tool_calls();
    assert_eq!(tool_calls.len(), 1);
    assert_eq!(tool_calls[0].name, "get_weather");
    assert_eq!(tool_calls[0].arguments["city"], "NYC");
}

#[tokio::test]
async fn test_processor_snapshot_accessible_during_iteration() {
    let events = vec![
        StreamEvent::text_delta(0, "A"),
        StreamEvent::text_delta(0, "B"),
        StreamEvent::response_done("resp_1", FinishReason::Stop),
    ];

    let mut processor = StreamProcessor::new(make_stream(events));

    // Initial state
    assert_eq!(processor.snapshot().text, "");

    // After first event
    let _ = processor.next().await;
    assert_eq!(processor.snapshot().text, "A");

    // After second event
    let _ = processor.next().await;
    assert_eq!(processor.snapshot().text, "AB");
}

#[tokio::test]
async fn test_processor_with_usage() {
    let events = vec![
        StreamEvent::text_delta(0, "Hi"),
        StreamEvent::response_done_full(
            "resp_1",
            "test-model",
            Some(TokenUsage {
                prompt_tokens: 10,
                completion_tokens: 5,
                total_tokens: 15,
                cache_read_tokens: None,
                cache_creation_tokens: None,
                reasoning_tokens: None,
            }),
            FinishReason::Stop,
        ),
    ];

    let processor = StreamProcessor::new(make_stream(events));
    let response = processor.collect().await.unwrap();

    assert!(response.usage.is_some());
    let usage = response.usage.unwrap();
    assert_eq!(usage.prompt_tokens, 10);
    assert_eq!(usage.completion_tokens, 5);
}

#[tokio::test]
async fn test_processor_thinking_preserves_accumulated_deltas() {
    // Test that ThinkingDone does NOT replace accumulated deltas
    // (pure accumulation principle)
    let events = vec![
        StreamEvent::response_created("resp_1"),
        // Deltas accumulate to "Accumulated content"
        StreamEvent::thinking_delta(0, "Accumulated "),
        StreamEvent::thinking_delta(0, "content"),
        // ThinkingDone has DIFFERENT content - should be ignored
        StreamEvent::ThinkingDone {
            index: 0,
            content: "Different final content".to_string(),
            signature: Some("sig_abc".to_string()),
        },
        StreamEvent::text_delta(1, "Response text"),
        StreamEvent::response_done("resp_1", FinishReason::Stop),
    ];

    let processor = StreamProcessor::new(make_stream(events));
    let response = processor.collect().await.unwrap();

    // Verify accumulated deltas are preserved, not replaced
    assert_eq!(response.thinking(), Some("Accumulated content"));

    // Verify signature from ThinkingDone is still applied
    if let Some(ContentBlock::Thinking { signature, .. }) = response.content.first() {
        assert_eq!(*signature, Some("sig_abc".to_string()));
    } else {
        panic!("Expected Thinking block");
    }
}

#[tokio::test]
async fn test_processor_thinking_uses_final_content_when_no_deltas() {
    // Test that ThinkingDone content is used when no deltas were received
    let events = vec![
        StreamEvent::response_created("resp_1"),
        // No thinking deltas - only ThinkingDone
        StreamEvent::ThinkingDone {
            index: 0,
            content: "Final content only".to_string(),
            signature: Some("sig_xyz".to_string()),
        },
        StreamEvent::text_delta(1, "Response"),
        StreamEvent::response_done("resp_1", FinishReason::Stop),
    ];

    let processor = StreamProcessor::new(make_stream(events));
    let response = processor.collect().await.unwrap();

    // Should use ThinkingDone content since no deltas
    assert_eq!(response.thinking(), Some("Final content only"));
}

#[tokio::test]
async fn test_processor_with_custom_timeout() {
    let events = vec![
        StreamEvent::response_created("resp_1"),
        StreamEvent::text_delta(0, "Hello"),
        StreamEvent::response_done("resp_1", FinishReason::Stop),
    ];

    let config = StreamConfig {
        idle_timeout: Duration::from_secs(120),
    };
    let processor = StreamProcessor::with_config(make_stream(events), config);
    assert_eq!(processor.config().idle_timeout, Duration::from_secs(120));
}

#[tokio::test]
async fn test_processor_idle_timeout_builder() {
    let events = vec![StreamEvent::response_created("resp_1")];

    let processor = StreamProcessor::new(make_stream(events)).idle_timeout(Duration::from_secs(30));
    assert_eq!(processor.config().idle_timeout, Duration::from_secs(30));
}
