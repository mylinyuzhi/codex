use futures::StreamExt;
use pretty_assertions::assert_eq;
use std::collections::HashMap;
use std::time::Duration;
use tokio::sync::mpsc;
use vercel_ai_provider::AISdkError;
use vercel_ai_provider::AssistantContentPart;
use vercel_ai_provider::FinishReason;
use vercel_ai_provider::LanguageModelV4StreamPart;
use vercel_ai_provider::LanguageModelV4ToolCall;
use vercel_ai_provider::ProviderMetadata;
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

/// Helper: build a one-key `ProviderMetadata` (e.g. `google.thoughtSignature`).
fn meta(provider: &str, key: &str, value: &str) -> ProviderMetadata {
    let mut outer: HashMap<String, serde_json::Value> = HashMap::new();
    outer.insert(provider.to_string(), serde_json::json!({ key: value }));
    ProviderMetadata::from_map(outer)
}

fn read_meta(pm: &ProviderMetadata, provider: &str, key: &str) -> Option<String> {
    pm.0.get(provider)?
        .get(key)
        .and_then(|v| v.as_str())
        .map(str::to_string)
}

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
    // The synthetic stream also emits `Part::ToolCall(tc)` as the
    // canonical close after `ToolInputEnd` (matching real Anthropic /
    // OpenAI Responses / Google streams), but `stream_event_from_part`
    // doesn't surface a corresponding `StreamEvent` for that variant
    // — the accumulator updates `snapshot.tool_calls` via the
    // `LanguageModelV4StreamPart::ToolCall` path instead. So the
    // observable event count from `process_stream` remains 7.
    //
    // `ReasoningEnd` is emitted by `process_stream` as a distinct
    // `StreamEvent::ReasoningEnd`; only `TextStart`/`TextEnd` and
    // `ReasoningStart` brackets are filtered out.
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
        StreamEvent::Finish {
            metrics, snapshot, ..
        } => {
            assert!(metrics.ttft_ms.is_some());
            assert_eq!(metrics.stall_count, 0);
            assert_eq!(metrics.total_stall_ms, 0);
            // Snapshot must mirror the input content: 3 parts in
            // emission order (Reasoning, Text, ToolCall) with the
            // tool call marked complete (close event was sent).
            assert_eq!(snapshot.parts.len(), 3);
            match &snapshot.parts[2] {
                super::TurnPart::ToolCall(tc) => {
                    assert!(
                        tc.is_complete,
                        "tool call should be complete after ToolCall close"
                    );
                    assert!(tc.is_input_complete);
                    assert_eq!(tc.tool_name, "Bash");
                }
                other => panic!("third part should be ToolCall, got {other:?}"),
            }
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

// ─── AssistantTurnSnapshot accumulator tests ─────────────────────────
//
// The accumulator is private; we exercise it through the public
// `process_stream` path by inspecting `StreamEvent::Finish.snapshot`.

/// Provider metadata on every emitted variant survives into the
/// per-segment snapshot. This is the load-bearing invariant for
/// Gemini `thoughtSignature` / Anthropic `signature` / OpenAI
/// `encrypted_content` round-trip.
#[tokio::test]
async fn snapshot_preserves_provider_metadata_on_every_variant() {
    let reasoning_meta = meta("anthropic", "signature", "S1");
    let tool_meta = meta("google", "thoughtSignature", "T1");
    let text_meta = meta("google", "thoughtSignature", "Tx");

    let content = vec![
        AssistantContentPart::Reasoning(ReasoningPart {
            text: "thinking...".into(),
            provider_metadata: Some(reasoning_meta.clone()),
        }),
        AssistantContentPart::Text(TextPart {
            text: "answer prefix".into(),
            provider_metadata: Some(text_meta.clone()),
        }),
        AssistantContentPart::ToolCall(ToolCallPart {
            tool_call_id: "call_42".into(),
            tool_name: "Bash".into(),
            input: serde_json::json!({"command": "ls"}),
            provider_executed: None,
            provider_metadata: Some(tool_meta.clone()),
        }),
    ];

    let stream_result = synthetic_stream_from_content(
        content,
        Usage::new(10, 5),
        FinishReason::new(UnifiedFinishReason::ToolCalls),
    );
    let (tx, mut rx) = mpsc::channel::<StreamEvent>(64);
    tokio::spawn(process_stream(stream_result.stream, tx));

    let mut snapshot = None;
    while let Some(ev) = rx.recv().await {
        if let StreamEvent::Finish { snapshot: snap, .. } = ev {
            snapshot = Some(snap);
        }
    }
    let snap = snapshot.expect("Finish event should carry a snapshot");

    assert_eq!(snap.parts.len(), 3, "3 parts in emission order");

    match &snap.parts[0] {
        super::TurnPart::Reasoning(r) => {
            assert_eq!(r.text, "thinking...");
            let pm = r.provider_metadata.as_ref().expect("reasoning metadata");
            assert_eq!(read_meta(pm, "anthropic", "signature"), Some("S1".into()));
        }
        other => panic!("expected Reasoning, got {other:?}"),
    }
    match &snap.parts[1] {
        super::TurnPart::Text(t) => {
            assert_eq!(t.text, "answer prefix");
            let pm = t.provider_metadata.as_ref().expect("text metadata");
            assert_eq!(
                read_meta(pm, "google", "thoughtSignature"),
                Some("Tx".into())
            );
        }
        other => panic!("expected Text, got {other:?}"),
    }
    match &snap.parts[2] {
        super::TurnPart::ToolCall(tc) => {
            assert!(tc.is_complete);
            let pm = tc.provider_metadata.as_ref().expect("tool call metadata");
            assert_eq!(
                read_meta(pm, "google", "thoughtSignature"),
                Some("T1".into())
            );
        }
        other => panic!("expected ToolCall, got {other:?}"),
    }
}

/// Multiple reasoning segments per turn preserve their own metadata.
/// Anthropic interleaved thinking and OpenAI Responses multi-item
/// reasoning both produce this shape on the wire.
#[tokio::test]
async fn snapshot_preserves_multiple_reasoning_segments() {
    let parts: Vec<Result<LanguageModelV4StreamPart, AISdkError>> = vec![
        Ok(LanguageModelV4StreamPart::ReasoningStart {
            id: "r1".into(),
            provider_metadata: None,
        }),
        Ok(LanguageModelV4StreamPart::ReasoningDelta {
            id: "r1".into(),
            delta: "first thought".into(),
            provider_metadata: None,
        }),
        Ok(LanguageModelV4StreamPart::ReasoningEnd {
            id: "r1".into(),
            provider_metadata: Some(meta("anthropic", "signature", "S1")),
        }),
        Ok(LanguageModelV4StreamPart::ReasoningStart {
            id: "r2".into(),
            provider_metadata: None,
        }),
        Ok(LanguageModelV4StreamPart::ReasoningDelta {
            id: "r2".into(),
            delta: "second thought".into(),
            provider_metadata: None,
        }),
        Ok(LanguageModelV4StreamPart::ReasoningEnd {
            id: "r2".into(),
            provider_metadata: Some(meta("anthropic", "signature", "S2")),
        }),
        Ok(LanguageModelV4StreamPart::Finish {
            usage: Usage::new(1, 1),
            finish_reason: FinishReason::new(UnifiedFinishReason::Stop),
            provider_metadata: None,
        }),
    ];

    let (tx, mut rx) = mpsc::channel::<StreamEvent>(32);
    tokio::spawn(process_stream(Box::pin(futures::stream::iter(parts)), tx));

    let mut snapshot = None;
    while let Some(ev) = rx.recv().await {
        if let StreamEvent::Finish { snapshot: snap, .. } = ev {
            snapshot = Some(snap);
        }
    }
    let snap = snapshot.expect("Finish snapshot");
    assert_eq!(snap.parts.len(), 2, "two reasoning segments");
    let signatures: Vec<String> = snap
        .parts
        .iter()
        .filter_map(|p| match p {
            super::TurnPart::Reasoning(r) => r
                .provider_metadata
                .as_ref()
                .and_then(|pm| read_meta(pm, "anthropic", "signature")),
            _ => None,
        })
        .collect();
    assert_eq!(signatures, vec!["S1", "S2"]);
}

/// Interleaved emission order is preserved: `[Text(A), ToolCall, Text(B)]`
/// must stay in that order — not collapsed or canonicalized.
#[tokio::test]
async fn snapshot_preserves_text_tool_text_interleaving() {
    let parts: Vec<Result<LanguageModelV4StreamPart, AISdkError>> = vec![
        Ok(LanguageModelV4StreamPart::TextStart {
            id: "t1".into(),
            provider_metadata: None,
        }),
        Ok(LanguageModelV4StreamPart::TextDelta {
            id: "t1".into(),
            delta: "before".into(),
            provider_metadata: None,
        }),
        Ok(LanguageModelV4StreamPart::TextEnd {
            id: "t1".into(),
            provider_metadata: None,
        }),
        Ok(LanguageModelV4StreamPart::ToolInputStart {
            id: "call_1".into(),
            tool_name: "Bash".into(),
            provider_executed: None,
            dynamic: None,
            title: None,
            provider_metadata: None,
        }),
        Ok(LanguageModelV4StreamPart::ToolInputDelta {
            id: "call_1".into(),
            delta: r#"{"command":"ls"}"#.into(),
            provider_metadata: None,
        }),
        Ok(LanguageModelV4StreamPart::ToolInputEnd {
            id: "call_1".into(),
            provider_metadata: None,
        }),
        Ok(LanguageModelV4StreamPart::TextStart {
            id: "t2".into(),
            provider_metadata: None,
        }),
        Ok(LanguageModelV4StreamPart::TextDelta {
            id: "t2".into(),
            delta: "after".into(),
            provider_metadata: None,
        }),
        Ok(LanguageModelV4StreamPart::TextEnd {
            id: "t2".into(),
            provider_metadata: None,
        }),
        Ok(LanguageModelV4StreamPart::Finish {
            usage: Usage::new(1, 1),
            finish_reason: FinishReason::new(UnifiedFinishReason::ToolCalls),
            provider_metadata: None,
        }),
    ];

    let (tx, mut rx) = mpsc::channel::<StreamEvent>(32);
    tokio::spawn(process_stream(Box::pin(futures::stream::iter(parts)), tx));

    let mut snapshot = None;
    while let Some(ev) = rx.recv().await {
        if let StreamEvent::Finish { snapshot: snap, .. } = ev {
            snapshot = Some(snap);
        }
    }
    let snap = snapshot.expect("Finish snapshot");
    assert_eq!(snap.parts.len(), 3);
    assert!(matches!(&snap.parts[0], super::TurnPart::Text(t) if t.text == "before"));
    assert!(matches!(&snap.parts[1], super::TurnPart::ToolCall(_)));
    assert!(matches!(&snap.parts[2], super::TurnPart::Text(t) if t.text == "after"));
}

/// `ToolInputStart` + `ToolInputDelta` + `ToolInputEnd` WITHOUT a
/// terminal `ToolCall(tc)` close — `is_input_complete=true` but
/// `is_complete=false`. Some providers / mocks produce this shape;
/// the snapshot must still surface the tool call.
#[tokio::test]
async fn snapshot_handles_tool_call_without_close_event() {
    let parts: Vec<Result<LanguageModelV4StreamPart, AISdkError>> = vec![
        Ok(LanguageModelV4StreamPart::ToolInputStart {
            id: "call_1".into(),
            tool_name: "Bash".into(),
            provider_executed: None,
            dynamic: None,
            title: None,
            provider_metadata: None,
        }),
        Ok(LanguageModelV4StreamPart::ToolInputDelta {
            id: "call_1".into(),
            delta: r#"{"command":"echo"}"#.into(),
            provider_metadata: None,
        }),
        Ok(LanguageModelV4StreamPart::ToolInputEnd {
            id: "call_1".into(),
            provider_metadata: None,
        }),
        // No ToolCall(tc) close.
        Ok(LanguageModelV4StreamPart::Finish {
            usage: Usage::new(1, 1),
            finish_reason: FinishReason::new(UnifiedFinishReason::ToolCalls),
            provider_metadata: None,
        }),
    ];

    let (tx, mut rx) = mpsc::channel::<StreamEvent>(16);
    tokio::spawn(process_stream(Box::pin(futures::stream::iter(parts)), tx));

    let mut snapshot = None;
    while let Some(ev) = rx.recv().await {
        if let StreamEvent::Finish { snapshot: snap, .. } = ev {
            snapshot = Some(snap);
        }
    }
    let snap = snapshot.expect("Finish snapshot");
    assert_eq!(snap.parts.len(), 1);
    match &snap.parts[0] {
        super::TurnPart::ToolCall(tc) => {
            assert!(tc.is_input_complete);
            assert!(!tc.is_complete);
            assert_eq!(tc.input_json, r#"{"command":"echo"}"#);
        }
        other => panic!("expected ToolCall, got {other:?}"),
    }
}

/// `ToolCall(tc)` close with `provider_metadata = None` MUST NOT
/// overwrite earlier `ToolInputStart`'s `Some(...)`. Without this
/// rule, Gemini's signature on ToolInputStart would be lost any time
/// the close event omitted metadata (rare in practice but a real bug).
#[tokio::test]
async fn snapshot_none_close_does_not_overwrite_some_start() {
    let some_meta = meta("google", "thoughtSignature", "T1");
    let parts: Vec<Result<LanguageModelV4StreamPart, AISdkError>> = vec![
        Ok(LanguageModelV4StreamPart::ToolInputStart {
            id: "call_1".into(),
            tool_name: "Bash".into(),
            provider_executed: None,
            dynamic: None,
            title: None,
            provider_metadata: Some(some_meta.clone()),
        }),
        Ok(LanguageModelV4StreamPart::ToolInputDelta {
            id: "call_1".into(),
            delta: r#"{"command":"ls"}"#.into(),
            provider_metadata: None,
        }),
        Ok(LanguageModelV4StreamPart::ToolInputEnd {
            id: "call_1".into(),
            provider_metadata: None,
        }),
        Ok(LanguageModelV4StreamPart::ToolCall(
            LanguageModelV4ToolCall {
                tool_call_id: "call_1".into(),
                tool_name: "Bash".into(),
                input: r#"{"command":"ls"}"#.into(),
                provider_executed: None,
                dynamic: None,
                provider_metadata: None, // ← critical: None close
            },
        )),
        Ok(LanguageModelV4StreamPart::Finish {
            usage: Usage::new(1, 1),
            finish_reason: FinishReason::new(UnifiedFinishReason::ToolCalls),
            provider_metadata: None,
        }),
    ];

    let (tx, mut rx) = mpsc::channel::<StreamEvent>(16);
    tokio::spawn(process_stream(Box::pin(futures::stream::iter(parts)), tx));

    let mut snapshot = None;
    while let Some(ev) = rx.recv().await {
        if let StreamEvent::Finish { snapshot: snap, .. } = ev {
            snapshot = Some(snap);
        }
    }
    let snap = snapshot.expect("Finish snapshot");
    match &snap.parts[0] {
        super::TurnPart::ToolCall(tc) => {
            assert!(tc.is_complete, "close arrived");
            let pm = tc
                .provider_metadata
                .as_ref()
                .expect("Some(start metadata) must survive None close");
            assert_eq!(
                read_meta(pm, "google", "thoughtSignature"),
                Some("T1".into())
            );
        }
        other => panic!("expected ToolCall, got {other:?}"),
    }
}

/// Duplicate `*Start` events (provider bug) — accumulator must
/// remain idempotent: ignore the second start and keep the first
/// segment. Defensive; real providers don't emit this.
#[tokio::test]
async fn snapshot_idempotent_on_duplicate_text_start() {
    let parts: Vec<Result<LanguageModelV4StreamPart, AISdkError>> = vec![
        Ok(LanguageModelV4StreamPart::TextStart {
            id: "t1".into(),
            provider_metadata: None,
        }),
        Ok(LanguageModelV4StreamPart::TextStart {
            id: "t1".into(),
            provider_metadata: None,
        }),
        Ok(LanguageModelV4StreamPart::TextDelta {
            id: "t1".into(),
            delta: "hello".into(),
            provider_metadata: None,
        }),
        Ok(LanguageModelV4StreamPart::TextEnd {
            id: "t1".into(),
            provider_metadata: None,
        }),
        Ok(LanguageModelV4StreamPart::Finish {
            usage: Usage::new(1, 1),
            finish_reason: FinishReason::new(UnifiedFinishReason::Stop),
            provider_metadata: None,
        }),
    ];

    let (tx, mut rx) = mpsc::channel::<StreamEvent>(16);
    tokio::spawn(process_stream(Box::pin(futures::stream::iter(parts)), tx));

    let mut snapshot = None;
    while let Some(ev) = rx.recv().await {
        if let StreamEvent::Finish { snapshot: snap, .. } = ev {
            snapshot = Some(snap);
        }
    }
    let snap = snapshot.expect("Finish snapshot");
    assert_eq!(snap.parts.len(), 1, "duplicate Start should not add a part");
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
