use std::time::Duration;

use coco_types::TokenUsage;
use pretty_assertions::assert_eq;
use tokio::sync::mpsc;

use super::*;
use crate::stream::StreamEvent;

fn make_usage() -> TokenUsage {
    TokenUsage {
        input_tokens: 100,
        output_tokens: 50,
        cache_read_input_tokens: 0,
        cache_creation_input_tokens: 0,
    }
}

#[tokio::test]
async fn test_collect_text_only_response() {
    let (tx, rx) = mpsc::channel(16);

    tx.send(StreamEvent::TextDelta {
        text: "Hello ".to_string(),
    })
    .await
    .unwrap();
    tx.send(StreamEvent::TextDelta {
        text: "world!".to_string(),
    })
    .await
    .unwrap();
    tx.send(StreamEvent::Finish {
        usage: make_usage(),
        stop_reason: "end_turn".to_string(),
    })
    .await
    .unwrap();
    drop(tx);

    let streaming = StreamingInference::new(rx);
    let response = streaming.collect_full_response().await.unwrap();

    assert_eq!(response.text, "Hello world!");
    assert_eq!(response.stop_reason, "end_turn");
    assert_eq!(response.usage.input_tokens, 100);
    assert_eq!(response.usage.output_tokens, 50);
    assert!(response.tool_calls.is_empty());
    assert!(response.ttft_ms.is_some());
}

#[tokio::test]
async fn test_collect_reasoning_response() {
    let (tx, rx) = mpsc::channel(16);

    tx.send(StreamEvent::ReasoningDelta {
        text: "Let me think...".to_string(),
    })
    .await
    .unwrap();
    tx.send(StreamEvent::TextDelta {
        text: "The answer is 42.".to_string(),
    })
    .await
    .unwrap();
    tx.send(StreamEvent::Finish {
        usage: make_usage(),
        stop_reason: "end_turn".to_string(),
    })
    .await
    .unwrap();
    drop(tx);

    let streaming = StreamingInference::new(rx);
    let response = streaming.collect_full_response().await.unwrap();

    assert_eq!(response.reasoning, "Let me think...");
    assert_eq!(response.text, "The answer is 42.");
}

#[tokio::test]
async fn test_collect_tool_call_response() {
    let (tx, rx) = mpsc::channel(16);

    tx.send(StreamEvent::ToolCallStart {
        id: "call_1".to_string(),
        tool_name: "read_file".to_string(),
    })
    .await
    .unwrap();
    tx.send(StreamEvent::ToolCallDelta {
        id: "call_1".to_string(),
        delta: r#"{"path":"#.to_string(),
    })
    .await
    .unwrap();
    tx.send(StreamEvent::ToolCallDelta {
        id: "call_1".to_string(),
        delta: r#""test.rs"}"#.to_string(),
    })
    .await
    .unwrap();
    tx.send(StreamEvent::ToolCallEnd {
        id: "call_1".to_string(),
    })
    .await
    .unwrap();
    tx.send(StreamEvent::Finish {
        usage: make_usage(),
        stop_reason: "tool_calls".to_string(),
    })
    .await
    .unwrap();
    drop(tx);

    let streaming = StreamingInference::new(rx);
    let response = streaming.collect_full_response().await.unwrap();

    assert_eq!(response.tool_calls.len(), 1);
    assert_eq!(response.tool_calls[0].tool_name, "read_file");
    assert_eq!(response.tool_calls[0].input_json, r#"{"path":"test.rs"}"#);
    assert!(response.tool_calls[0].complete);
    assert_eq!(response.stop_reason, "tool_calls");
}

#[tokio::test]
async fn test_collect_incomplete_tool_calls_filtered() {
    let (tx, rx) = mpsc::channel(16);

    // Start a tool call but never complete it
    tx.send(StreamEvent::ToolCallStart {
        id: "call_incomplete".to_string(),
        tool_name: "some_tool".to_string(),
    })
    .await
    .unwrap();
    tx.send(StreamEvent::ToolCallDelta {
        id: "call_incomplete".to_string(),
        delta: r#"{"partial"#.to_string(),
    })
    .await
    .unwrap();

    // Complete another tool call
    tx.send(StreamEvent::ToolCallStart {
        id: "call_complete".to_string(),
        tool_name: "done_tool".to_string(),
    })
    .await
    .unwrap();
    tx.send(StreamEvent::ToolCallEnd {
        id: "call_complete".to_string(),
    })
    .await
    .unwrap();

    tx.send(StreamEvent::Finish {
        usage: make_usage(),
        stop_reason: "tool_calls".to_string(),
    })
    .await
    .unwrap();
    drop(tx);

    let streaming = StreamingInference::new(rx);
    let response = streaming.collect_full_response().await.unwrap();

    // Only the complete tool call should be included
    assert_eq!(response.tool_calls.len(), 1);
    assert_eq!(response.tool_calls[0].tool_name, "done_tool");
}

#[tokio::test]
async fn test_error_event_stops_collection() {
    let (tx, rx) = mpsc::channel(16);

    tx.send(StreamEvent::TextDelta {
        text: "partial".to_string(),
    })
    .await
    .unwrap();
    tx.send(StreamEvent::Error {
        message: "connection reset".to_string(),
    })
    .await
    .unwrap();
    drop(tx);

    let streaming = StreamingInference::new(rx);
    let result = streaming.collect_full_response().await;

    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.to_string().contains("connection reset"));
}

#[tokio::test]
async fn test_idle_timeout() {
    let (tx, rx) = mpsc::channel(16);

    // Send one event, then let the stream idle
    tx.send(StreamEvent::TextDelta {
        text: "start".to_string(),
    })
    .await
    .unwrap();

    // Keep tx alive but don't send more events
    let _tx = tx;

    let streaming = StreamingInference::new(rx).with_idle_timeout(Duration::from_millis(50));
    let result = streaming.collect_full_response().await;

    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.to_string().contains("idle timeout"));
}

#[tokio::test]
async fn test_next_event_manual_consumption() {
    let (tx, rx) = mpsc::channel(16);

    tx.send(StreamEvent::TextDelta {
        text: "chunk1".to_string(),
    })
    .await
    .unwrap();
    tx.send(StreamEvent::TextDelta {
        text: "chunk2".to_string(),
    })
    .await
    .unwrap();
    tx.send(StreamEvent::Finish {
        usage: make_usage(),
        stop_reason: "end_turn".to_string(),
    })
    .await
    .unwrap();
    drop(tx);

    let mut streaming = StreamingInference::new(rx);

    // Consume events one by one
    let event1 = streaming.next_event().await;
    assert!(matches!(event1, Some(StreamEvent::TextDelta { .. })));
    assert!(streaming.has_received_token());
    assert_eq!(streaming.text(), "chunk1");

    let event2 = streaming.next_event().await;
    assert!(matches!(event2, Some(StreamEvent::TextDelta { .. })));
    assert_eq!(streaming.text(), "chunk1chunk2");

    let event3 = streaming.next_event().await;
    assert!(matches!(event3, Some(StreamEvent::Finish { .. })));
    assert!(streaming.is_finished());

    // After finish, returns None
    let event4 = streaming.next_event().await;
    assert!(event4.is_none());
}

#[test]
fn test_stall_tracker_no_stalls() {
    let mut tracker = StallTracker::new();

    // First event never stalls (no previous)
    assert!(tracker.record_event().is_none());
    assert_eq!(tracker.stall_count(), 0);
    assert_eq!(tracker.total_stall_ms(), 0);
}

#[tokio::test]
async fn test_empty_stream() {
    let (_tx, rx) = mpsc::channel::<StreamEvent>(16);
    drop(_tx); // Close immediately

    let streaming = StreamingInference::new(rx);
    let result = streaming.collect_full_response().await;

    // Empty stream should succeed with defaults
    let response = result.unwrap();
    assert!(response.text.is_empty());
    assert!(response.tool_calls.is_empty());
    assert_eq!(response.stop_reason, "unknown");
}

#[tokio::test]
async fn test_ttft_tracking() {
    let (tx, rx) = mpsc::channel(16);
    let mut streaming = StreamingInference::new(rx);

    // No TTFT before any token
    assert!(streaming.ttft_ms().is_none());
    assert!(!streaming.has_received_token());

    tx.send(StreamEvent::TextDelta {
        text: "first".to_string(),
    })
    .await
    .unwrap();

    streaming.next_event().await;
    assert!(streaming.has_received_token());
    assert!(streaming.ttft_ms().is_some());
    // TTFT should be non-negative
    assert!(streaming.ttft_ms().unwrap() >= 0);

    drop(tx);
}

#[test]
fn test_streaming_error_display() {
    let err = StreamingError::StreamError {
        message: "network failure".to_string(),
    };
    assert_eq!(err.to_string(), "stream error: network failure");

    let err = StreamingError::IdleTimeout;
    assert_eq!(err.to_string(), "stream idle timeout");
}
