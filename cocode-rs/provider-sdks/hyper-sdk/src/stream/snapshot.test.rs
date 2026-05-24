use super::*;

#[test]
fn test_snapshot_default() {
    let snapshot = StreamSnapshot::new();
    assert!(!snapshot.has_text());
    assert!(!snapshot.has_thinking());
    assert!(!snapshot.has_tool_calls());
    assert!(!snapshot.is_complete);
}

#[test]
fn test_thinking_snapshot() {
    let mut thinking = ThinkingSnapshot::new();
    thinking.append("Hello ");
    thinking.append("world");
    assert_eq!(thinking.content, "Hello world");
    assert!(!thinking.is_complete);

    thinking.complete(Some("sig123".to_string()));
    assert!(thinking.is_complete);
    assert_eq!(thinking.signature, Some("sig123".to_string()));
}

#[test]
fn test_tool_call_snapshot() {
    let mut tc = ToolCallSnapshot::new("call_1", "get_weather");
    tc.append_arguments("{\"city\":");
    tc.append_arguments("\"NYC\"}");

    assert!(!tc.is_complete);
    assert_eq!(tc.arguments, "{\"city\":\"NYC\"}");

    let args = tc.parsed_arguments().unwrap();
    assert_eq!(args["city"], "NYC");

    tc.complete("{\"city\":\"NYC\"}".to_string());
    assert!(tc.is_complete);
}

#[test]
fn test_snapshot_tool_calls_filtering() {
    let mut snapshot = StreamSnapshot::new();
    snapshot.tool_calls.push(ToolCallSnapshot {
        id: "call_1".to_string(),
        name: "tool_a".to_string(),
        arguments: "{\"a\":1}".to_string(),
        is_complete: true,
    });
    snapshot.tool_calls.push(ToolCallSnapshot {
        id: "call_2".to_string(),
        name: "tool_b".to_string(),
        arguments: "{\"b\":".to_string(),
        is_complete: false,
    });

    assert_eq!(snapshot.completed_tool_calls().len(), 1);
    assert_eq!(snapshot.pending_tool_calls().len(), 1);
    assert_eq!(snapshot.to_tool_calls().len(), 1);
}
