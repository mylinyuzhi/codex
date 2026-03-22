use super::*;
use serde_json::json;

#[test]
fn test_tool_invocation() {
    let invocation = ToolInvocation::pending("call_123", "search", json!({"query": "test"}));

    assert_eq!(invocation.tool_call_id, "call_123");
    assert_eq!(invocation.tool_name, "search");
    assert!(invocation.output.is_none());
    assert!(!invocation.is_error);
}

#[test]
fn test_tool_invocation_with_output() {
    let invocation =
        ToolInvocation::pending("call_1", "test", json!({})).with_output(json!({"status": "ok"}));

    assert!(invocation.output.is_some());
    assert!(!invocation.is_error);
}

#[test]
fn test_tool_invocation_with_error() {
    let invocation =
        ToolInvocation::pending("call_1", "test", json!({})).with_error(json!({"error": "failed"}));

    assert!(invocation.output.is_some());
    assert!(invocation.is_error);
}

#[test]
fn test_tool_call() {
    let call = ToolCall::new("call_123", "search", json!({"query": "hello"}));

    assert_eq!(call.tool_call_id, "call_123");
    assert_eq!(call.tool_name, "search");
    assert!(call.provider_executed.is_none());
    assert!(call.dynamic.is_none());
    assert!(call.provider_metadata.is_none());
}

#[test]
fn test_tool_call_with_provider_executed() {
    let call = ToolCall::new("call_123", "mcp_tool", json!({})).with_provider_executed(true);

    assert_eq!(call.provider_executed, Some(true));
}

#[test]
fn test_tool_call_with_dynamic() {
    let call = ToolCall::new("call_123", "dynamic_tool", json!({})).with_dynamic(true);

    assert_eq!(call.dynamic, Some(true));
}

#[test]
fn test_tool_call_serialization() {
    let call = ToolCall::new("call_123", "search", json!({"query": "test"}))
        .with_provider_executed(true)
        .with_dynamic(true);

    let json = serde_json::to_string(&call).unwrap();
    assert!(json.contains("\"toolCallId\":\"call_123\""));
    assert!(json.contains("\"toolName\":\"search\""));
    assert!(json.contains("\"providerExecuted\":true"));
    assert!(json.contains("\"dynamic\":true"));
}

#[test]
fn test_tool_result() {
    let result = ToolResult::new("call_123", "search", json!({"results": []}));

    assert_eq!(result.tool_call_id, "call_123");
    assert!(!result.is_error);

    let error = ToolResult::error("call_456", "failing_tool", json!({"error": "oops"}));
    assert!(error.is_error);
}
