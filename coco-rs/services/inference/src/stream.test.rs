use futures::StreamExt;
use pretty_assertions::assert_eq;
use std::time::Duration;
use tokio::sync::mpsc;
use vercel_ai_provider::AISdkError;
use vercel_ai_provider::AssistantContentPart;
use vercel_ai_provider::FinishReason;
use vercel_ai_provider::LanguageModelV4StreamPart;
use vercel_ai_provider::ReasoningPart;
use vercel_ai_provider::StreamError;
use vercel_ai_provider::TextPart;
use vercel_ai_provider::ToolCallPart;
use vercel_ai_provider::UnifiedFinishReason;
use vercel_ai_provider::Usage;

use super::StreamEvent;
use super::default_process_stream_config;
use super::process_stream;
use super::process_stream_with_config;
use super::synthetic_stream_from_content;

/// End-to-end: feeding `synthetic_stream_from_content`'s output through
/// `process_stream` must yield `StreamEvent`s in the order reasoning → text →
/// tool input (start/delta/end) → Finish. This is the contract the agent
/// loop's per-turn consumer relies on.
#[tokio::test]
async fn synthetic_stream_emits_events_in_content_order() {
    let content = vec![
        AssistantContentPart::Reasoning(ReasoningPart {
            text: "thinking about it".into(),
            provider_metadata: None,
        }),
        AssistantContentPart::Text(TextPart {
            text: "Hello, world.".into(),
            provider_metadata: None,
        }),
        AssistantContentPart::ToolCall(ToolCallPart {
            tool_call_id: "call_42".into(),
            tool_name: "Bash".into(),
            input: serde_json::json!({"command": "echo hi"}),
            provider_executed: None,
            provider_metadata: None,
        }),
    ];

    let stream_result = synthetic_stream_from_content(
        content,
        Usage::new(11, 7),
        FinishReason::new(UnifiedFinishReason::ToolCalls),
    );

    let (tx, mut rx) = mpsc::channel::<StreamEvent>(32);
    tokio::spawn(process_stream(stream_result.stream, tx));

    let mut events = Vec::new();
    while let Some(ev) = rx.recv().await {
        events.push(ev);
    }

    // Expected: ReasoningDelta, ReasoningEnd, TextDelta, ToolCallStart,
    // ToolCallDelta, ToolCallEnd, Finish.
    //
    // `ReasoningEnd` is emitted by `process_stream` as a distinct
    // `StreamEvent::ReasoningEnd` (see `stream.rs:138-140`); only
    // `TextStart`/`TextEnd` and `ReasoningStart` brackets are filtered out.
    assert_eq!(events.len(), 7);
    assert!(
        matches!(&events[0], StreamEvent::ReasoningDelta { text } if text == "thinking about it"),
        "first event should be ReasoningDelta, got {:?}",
        &events[0]
    );
    assert!(
        matches!(&events[1], StreamEvent::ReasoningEnd { .. }),
        "second event should be ReasoningEnd, got {:?}",
        &events[1]
    );
    assert!(
        matches!(&events[2], StreamEvent::TextDelta { text } if text == "Hello, world."),
        "third event should be TextDelta, got {:?}",
        &events[2]
    );
    assert!(
        matches!(
            &events[3],
            StreamEvent::ToolCallStart { id, tool_name }
                if id == "call_42" && tool_name == "Bash"
        ),
        "fourth event should be ToolCallStart, got {:?}",
        &events[3]
    );
    assert!(
        matches!(&events[4], StreamEvent::ToolCallDelta { id, delta }
            if id == "call_42" && delta.contains("echo hi")),
        "fifth event should be ToolCallDelta with serialized input, got {:?}",
        &events[4]
    );
    assert!(
        matches!(&events[5], StreamEvent::ToolCallEnd { id } if id == "call_42"),
        "sixth event should be ToolCallEnd, got {:?}",
        &events[5]
    );
    match &events[6] {
        StreamEvent::Finish { metrics, .. } => {
            assert!(metrics.ttft_ms.is_some());
            assert_eq!(metrics.stall_count, 0);
            assert_eq!(metrics.total_stall_ms, 0);
        }
        other => panic!("last event should be Finish, got {other:?}"),
    }
}

#[tokio::test]
async fn process_stream_reports_provider_stream_errors_with_metrics() {
    let parts: Vec<Result<LanguageModelV4StreamPart, AISdkError>> = vec![
        Ok(LanguageModelV4StreamPart::TextDelta {
            id: "t1".into(),
            delta: "partial".into(),
            provider_metadata: None,
        }),
        Err(AISdkError::new("connection reset")),
    ];

    let (tx, mut rx) = mpsc::channel::<StreamEvent>(16);
    tokio::spawn(process_stream(Box::pin(futures::stream::iter(parts)), tx));

    assert!(matches!(
        rx.recv().await,
        Some(StreamEvent::TextDelta { text }) if text == "partial"
    ));
    match rx.recv().await {
        Some(StreamEvent::Error { message, metrics }) => {
            assert!(message.contains("connection reset"));
            assert!(metrics.ttft_ms.is_some());
        }
        other => panic!("expected stream error, got {other:?}"),
    }
}

#[tokio::test]
async fn process_stream_reports_error_parts_with_metrics() {
    let parts: Vec<Result<LanguageModelV4StreamPart, AISdkError>> = vec![
        Ok(LanguageModelV4StreamPart::ToolInputStart {
            id: "call_1".into(),
            tool_name: "Read".into(),
            provider_executed: None,
            dynamic: None,
            title: None,
            provider_metadata: None,
        }),
        Ok(LanguageModelV4StreamPart::Error {
            error: StreamError::new("provider overloaded"),
        }),
    ];

    let (tx, mut rx) = mpsc::channel::<StreamEvent>(16);
    tokio::spawn(process_stream(Box::pin(futures::stream::iter(parts)), tx));

    assert!(matches!(
        rx.recv().await,
        Some(StreamEvent::ToolCallStart { id, tool_name })
            if id == "call_1" && tool_name == "Read"
    ));
    match rx.recv().await {
        Some(StreamEvent::Error { message, metrics }) => {
            assert_eq!(message, "provider overloaded");
            assert!(metrics.ttft_ms.is_some());
        }
        other => panic!("expected error part, got {other:?}"),
    }
}

#[tokio::test(start_paused = true)]
async fn process_stream_with_config_uses_custom_stall_threshold() {
    let stream = futures::stream::unfold(0usize, |idx| async move {
        if idx == 1 {
            tokio::time::sleep(Duration::from_secs(2)).await;
        }
        let part = match idx {
            0 => LanguageModelV4StreamPart::TextDelta {
                id: "t1".into(),
                delta: "a".into(),
                provider_metadata: None,
            },
            1 => LanguageModelV4StreamPart::TextDelta {
                id: "t1".into(),
                delta: "b".into(),
                provider_metadata: None,
            },
            2 => LanguageModelV4StreamPart::Finish {
                usage: Usage::new(1, 2),
                finish_reason: FinishReason::new(UnifiedFinishReason::Stop),
                provider_metadata: None,
            },
            _ => return None,
        };
        Some((Ok(part), idx + 1))
    });
    let config = default_process_stream_config().with_stall_threshold(Duration::from_secs(1));

    let (tx, mut rx) = mpsc::channel::<StreamEvent>(16);
    tokio::spawn(process_stream_with_config(Box::pin(stream), tx, config));

    let mut finish_metrics = None;
    while let Some(event) = rx.recv().await {
        if let StreamEvent::Finish { metrics, .. } = event {
            finish_metrics = Some(metrics);
            break;
        }
    }

    let metrics = finish_metrics.expect("finish event should include metrics");
    assert_eq!(metrics.stall_count, 1);
    assert_eq!(metrics.total_stall_ms, 2_000);
}

/// Tool input must round-trip through the synthetic stream: delta JSON parsed
/// back into a `Value` equivalent to the original input. Guards against
/// regressions in the delta-chunking strategy (currently single-shot but the
/// contract allows multi-chunk).
#[tokio::test]
async fn tool_input_round_trips_through_synthetic_stream() {
    let original_input = serde_json::json!({
        "command": "ls -la",
        "cwd": "/tmp",
        "timeout_ms": 5000
    });

    let content = vec![AssistantContentPart::ToolCall(ToolCallPart {
        tool_call_id: "rt".into(),
        tool_name: "Bash".into(),
        input: original_input.clone(),
        provider_executed: None,
        provider_metadata: None,
    })];

    let stream_result = synthetic_stream_from_content(
        content,
        Usage::new(5, 3),
        FinishReason::new(UnifiedFinishReason::ToolCalls),
    );

    // Collect all deltas for the tool call.
    let mut json_accumulator = String::new();
    let mut stream = stream_result.stream;
    while let Some(part) = stream.next().await {
        if let Ok(vercel_ai_provider::LanguageModelV4StreamPart::ToolInputDelta { delta, .. }) =
            part
        {
            json_accumulator.push_str(&delta);
        }
    }

    let round_tripped: serde_json::Value =
        serde_json::from_str(&json_accumulator).expect("accumulated JSON should parse");
    assert_eq!(round_tripped, original_input);
}
