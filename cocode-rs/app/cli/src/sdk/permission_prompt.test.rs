use cocode_protocol::ApprovalDecision;
use serde_json::json;

use super::McpPermissionRequester;

#[test]
fn test_parse_response_direct_allow() {
    let response = json!({"behavior": "allow"});
    let result = McpPermissionRequester::parse_response(&response);
    assert_eq!(result, Some(ApprovalDecision::Approved));
}

#[test]
fn test_parse_response_direct_deny() {
    let response = json!({"behavior": "deny"});
    let result = McpPermissionRequester::parse_response(&response);
    assert_eq!(result, Some(ApprovalDecision::Denied));
}

#[test]
fn test_parse_response_ask_returns_none() {
    let response = json!({"behavior": "ask"});
    let result = McpPermissionRequester::parse_response(&response);
    assert_eq!(result, None);
}

#[test]
fn test_parse_response_unknown_behavior_returns_none() {
    let response = json!({"behavior": "escalate"});
    let result = McpPermissionRequester::parse_response(&response);
    assert_eq!(result, None);
}

#[test]
fn test_parse_response_error_returns_none() {
    let response = json!({"error": "tool failed"});
    let result = McpPermissionRequester::parse_response(&response);
    assert_eq!(result, None);
}

#[test]
fn test_parse_response_mcp_result_wrapper() {
    let response = json!({
        "result": {
            "behavior": "allow",
            "message": "approved by policy"
        }
    });
    let result = McpPermissionRequester::parse_response(&response);
    assert_eq!(result, Some(ApprovalDecision::Approved));
}

#[test]
fn test_parse_response_mcp_content_text_block() {
    let response = json!({
        "content": [{
            "type": "text",
            "text": "{\"behavior\": \"deny\", \"message\": \"blocked\"}"
        }]
    });
    let result = McpPermissionRequester::parse_response(&response);
    assert_eq!(result, Some(ApprovalDecision::Denied));
}

#[test]
fn test_parse_response_mcp_content_non_json_text() {
    let response = json!({
        "content": [{
            "type": "text",
            "text": "This is just a plain text response"
        }]
    });
    let result = McpPermissionRequester::parse_response(&response);
    assert_eq!(result, None);
}

#[test]
fn test_parse_response_empty_object() {
    let response = json!({});
    let result = McpPermissionRequester::parse_response(&response);
    assert_eq!(result, None);
}

#[test]
fn test_parse_response_null() {
    let response = json!(null);
    let result = McpPermissionRequester::parse_response(&response);
    assert_eq!(result, None);
}

#[test]
fn test_new_splits_server_tool() {
    let fallback = std::sync::Arc::new(cocode_app_server::permission::SdkPermissionBridge::new(
        tokio::sync::mpsc::channel(1).0,
    ));
    let requester = McpPermissionRequester::new("my-server/approval-tool", fallback);
    assert_eq!(requester.server_name, "my-server");
    assert_eq!(requester.tool_name, "approval-tool");
}

#[test]
fn test_new_no_separator() {
    let fallback = std::sync::Arc::new(cocode_app_server::permission::SdkPermissionBridge::new(
        tokio::sync::mpsc::channel(1).0,
    ));
    let requester = McpPermissionRequester::new("approval-tool", fallback);
    assert_eq!(requester.server_name, "approval-tool");
    assert_eq!(requester.tool_name, "approval-tool");
}
