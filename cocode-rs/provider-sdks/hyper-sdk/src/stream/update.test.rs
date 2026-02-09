use super::*;

#[test]
fn test_update_classification() {
    let text_delta = StreamUpdate::TextDelta {
        index: 0,
        delta: "hello".to_string(),
    };
    assert!(text_delta.is_text_delta());
    assert!(text_delta.is_delta());
    assert!(!text_delta.is_done());

    let done = StreamUpdate::Done {
        id: "resp_1".to_string(),
        finish_reason: FinishReason::Stop,
        usage: None,
    };
    assert!(done.is_done());
    assert!(!done.is_delta());
}

#[test]
fn test_update_accessors() {
    let text_delta = StreamUpdate::TextDelta {
        index: 0,
        delta: "world".to_string(),
    };
    assert_eq!(text_delta.as_text_delta(), Some("world"));

    let thinking_delta = StreamUpdate::ThinkingDelta {
        index: 0,
        delta: "thinking...".to_string(),
    };
    assert_eq!(thinking_delta.as_thinking_delta(), Some("thinking..."));

    let done = StreamUpdate::Done {
        id: "resp_1".to_string(),
        finish_reason: FinishReason::ToolCalls,
        usage: None,
    };
    assert_eq!(done.finish_reason(), Some(FinishReason::ToolCalls));
}

#[test]
fn test_from_stream_event() {
    use super::super::StreamEvent;

    let event = StreamEvent::text_delta(0, "hello");
    let update: StreamUpdate = event.into();
    assert!(matches!(update, StreamUpdate::TextDelta { delta, .. } if delta == "hello"));
}
