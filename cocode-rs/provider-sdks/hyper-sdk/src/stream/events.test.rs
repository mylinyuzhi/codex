use super::*;

#[test]
fn test_stream_event_constructors() {
    let delta = StreamEvent::text_delta(0, "Hello");
    assert!(delta.is_delta());
    assert_eq!(delta.as_text_delta(), Some("Hello"));

    let done = StreamEvent::response_done("resp_1", FinishReason::Stop);
    assert!(done.is_done());
}

#[test]
fn test_stream_event_serde() {
    let event = StreamEvent::text_delta(0, "world");
    let json = serde_json::to_string(&event).unwrap();
    assert!(json.contains("\"type\":\"text_delta\""));

    let parsed: StreamEvent = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.as_text_delta(), Some("world"));
}

#[test]
fn test_tool_call_events() {
    let start = StreamEvent::tool_call_start(0, "call_1", "get_weather");
    assert!(!start.is_delta());

    let done = StreamEvent::tool_call_done(
        0,
        ToolCall::new("call_1", "get_weather", serde_json::json!({})),
    );
    assert!(!done.is_done());
}

#[test]
fn test_ignored_event() {
    let ignored = StreamEvent::Ignored;
    assert!(!ignored.is_delta());
    assert!(!ignored.is_done());
    assert!(ignored.as_text_delta().is_none());
}
