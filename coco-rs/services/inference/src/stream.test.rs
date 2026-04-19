use futures::StreamExt;
use pretty_assertions::assert_eq;
use tokio::sync::mpsc;
use vercel_ai_provider::AssistantContentPart;
use vercel_ai_provider::FinishReason;
use vercel_ai_provider::ReasoningPart;
use vercel_ai_provider::TextPart;
use vercel_ai_provider::ToolCallPart;
use vercel_ai_provider::UnifiedFinishReason;
use vercel_ai_provider::Usage;

use super::StreamEvent;
use super::process_stream;
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

    // Expected: ReasoningDelta, TextDelta, ToolCallStart, ToolCallDelta, ToolCallEnd, Finish.
    // The Start/End bracket variants for text/reasoning are filtered out by
    // `process_stream` (they're not produced as StreamEvent).
    assert_eq!(events.len(), 6);
    assert!(
        matches!(&events[0], StreamEvent::ReasoningDelta { text } if text == "thinking about it"),
        "first event should be ReasoningDelta, got {:?}",
        &events[0]
    );
    assert!(
        matches!(&events[1], StreamEvent::TextDelta { text } if text == "Hello, world."),
        "second event should be TextDelta, got {:?}",
        &events[1]
    );
    assert!(
        matches!(
            &events[2],
            StreamEvent::ToolCallStart { id, tool_name }
                if id == "call_42" && tool_name == "Bash"
        ),
        "third event should be ToolCallStart, got {:?}",
        &events[2]
    );
    assert!(
        matches!(&events[3], StreamEvent::ToolCallDelta { id, delta }
            if id == "call_42" && delta.contains("echo hi")),
        "fourth event should be ToolCallDelta with serialized input, got {:?}",
        &events[3]
    );
    assert!(
        matches!(&events[4], StreamEvent::ToolCallEnd { id } if id == "call_42"),
        "fifth event should be ToolCallEnd, got {:?}",
        &events[4]
    );
    assert!(
        matches!(&events[5], StreamEvent::Finish { .. }),
        "last event should be Finish, got {:?}",
        &events[5]
    );
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
