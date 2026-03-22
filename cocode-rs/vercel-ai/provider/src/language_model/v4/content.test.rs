use super::*;

#[test]
fn test_content_text() {
    let content = LanguageModelV4Content::text("Hello");
    assert!(content.is_text());
    assert!(!content.is_reasoning());
    assert!(!content.is_tool_call());

    let text = content.as_text().unwrap();
    assert_eq!(text.text, "Hello");
}

#[test]
fn test_content_reasoning() {
    let content = LanguageModelV4Content::reasoning("Thinking...");
    assert!(!content.is_text());
    assert!(content.is_reasoning());
}

#[test]
fn test_content_text_serialization() {
    let content = LanguageModelV4Content::text("Test");
    let json = serde_json::to_string(&content).unwrap();
    assert!(json.contains(r#""type":"text"#));
    assert!(json.contains(r#""text":"Test"#));
}

#[test]
fn test_content_tool_call() {
    let call = LanguageModelV4ToolCall::new("call-1", "tool", "{}");
    let content = LanguageModelV4Content::ToolCall(call);
    assert!(content.is_tool_call());

    let tool_call = content.as_tool_call().unwrap();
    assert_eq!(tool_call.tool_call_id, "call-1");
}

#[test]
fn test_content_tool_result() {
    let result = LanguageModelV4ToolResult::new("call-1", "tool", serde_json::json!({}));
    let content = LanguageModelV4Content::ToolResult(result);
    assert!(content.is_tool_result());
}

#[test]
fn test_content_deserialization() {
    let json = r#"{"type":"text","text":"Hello"}"#;
    let content: LanguageModelV4Content = serde_json::from_str(json).unwrap();
    assert!(content.is_text());
}
