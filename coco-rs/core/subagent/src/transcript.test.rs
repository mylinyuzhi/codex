use super::*;

#[test]
fn test_filter_whitespace_only_assistant() {
    let messages = vec![
        serde_json::json!({"role": "user", "content": "hello"}),
        serde_json::json!({"role": "assistant", "content": [
            {"type": "text", "text": "   \n  "}
        ]}),
        serde_json::json!({"role": "user", "content": "world"}),
    ];

    let filtered = filter_transcript(&messages);
    assert_eq!(filtered.len(), 2);
    assert_eq!(filtered[0]["content"].as_str().unwrap(), "hello");
    assert_eq!(filtered[1]["content"].as_str().unwrap(), "world");
}

#[test]
fn test_filter_thinking_only_assistant() {
    let messages = vec![
        serde_json::json!({"role": "user", "content": "hello"}),
        serde_json::json!({"role": "assistant", "content": [
            {"type": "thinking", "text": "Let me think..."}
        ]}),
        serde_json::json!({"role": "user", "content": "world"}),
    ];

    let filtered = filter_transcript(&messages);
    assert_eq!(filtered.len(), 2);
}

#[test]
fn test_filter_keeps_substantive_assistant() {
    let messages = vec![
        serde_json::json!({"role": "user", "content": "hello"}),
        serde_json::json!({"role": "assistant", "content": [
            {"type": "thinking", "text": "Let me think..."},
            {"type": "text", "text": "Here is my answer"}
        ]}),
    ];

    let filtered = filter_transcript(&messages);
    assert_eq!(filtered.len(), 2);
}

#[test]
fn test_strip_unresolved_tool_uses() {
    let messages = vec![
        serde_json::json!({"role": "user", "content": "hello"}),
        serde_json::json!({"role": "assistant", "content": [
            {"type": "tool_use", "id": "tu_1", "name": "Bash", "input": {"command": "ls"}},
            {"type": "tool_use", "id": "tu_2", "name": "Read", "input": {"path": "foo"}}
        ]}),
        serde_json::json!({"role": "user", "content": [
            {"type": "tool_result", "tool_use_id": "tu_1", "content": "result1"}
            // tu_2 is NOT resolved
        ]}),
        serde_json::json!({"role": "assistant", "content": [
            {"type": "tool_use", "id": "tu_3", "name": "Write", "input": {"path": "bar"}}
            // tu_3 is also NOT resolved — this trailing message should be stripped
        ]}),
    ];

    let filtered = filter_transcript(&messages);
    // The last assistant message (with unresolved tu_3) should be removed
    assert_eq!(filtered.len(), 3);
    assert_eq!(
        filtered[2]["role"].as_str().unwrap(),
        "user",
        "last message should be the user message with tool_result"
    );
}

#[test]
fn test_filter_empty_transcript() {
    let messages: Vec<serde_json::Value> = vec![];
    let filtered = filter_transcript(&messages);
    assert!(filtered.is_empty());
}
