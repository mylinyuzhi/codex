use pretty_assertions::assert_eq;
use serde_json::json;

use super::*;

#[test]
fn ask_for_approval_wire_method() {
    let req = ServerRequest::AskForApproval(AskForApprovalParams {
        request_id: "req-1".into(),
        tool_name: "Bash".into(),
        input: json!({ "command": "ls" }),
        tool_use_id: "tu-1".into(),
        description: Some("list files".into()),
        title: None,
        display_name: None,
        blocked_path: None,
        decision_reason: None,
        agent_id: None,
        permission_suggestions: vec![],
    });
    let j = serde_json::to_value(&req).unwrap();
    assert_eq!(j["method"], "approval/askForApproval");
    assert_eq!(j["params"]["tool_name"], "Bash");
    assert_eq!(j["params"]["tool_use_id"], "tu-1");
}

#[test]
fn request_user_input_wire_method() {
    let req = ServerRequest::RequestUserInput(RequestUserInputParams {
        request_id: "req-2".into(),
        prompt: "Choose one:".into(),
        description: None,
        choices: vec!["a".into(), "b".into()],
        default: Some("a".into()),
    });
    let j = serde_json::to_value(&req).unwrap();
    assert_eq!(j["method"], "input/requestUserInput");
    assert_eq!(j["params"]["prompt"], "Choose one:");
    assert_eq!(j["params"]["choices"].as_array().unwrap().len(), 2);
}

#[test]
fn mcp_route_message_wire_method() {
    let req = ServerRequest::McpRouteMessage(McpRouteMessageParams {
        request_id: "req-3".into(),
        server_name: "github".into(),
        message: json!({ "jsonrpc": "2.0", "method": "tools/list" }),
    });
    let j = serde_json::to_value(&req).unwrap();
    assert_eq!(j["method"], "mcp/routeMessage");
    assert_eq!(j["params"]["server_name"], "github");
}

#[test]
fn hook_callback_wire_method() {
    let req = ServerRequest::HookCallback(HookCallbackParams {
        request_id: "req-4".into(),
        callback_id: "cb-1".into(),
        input: json!({ "tool_name": "Bash" }),
        tool_use_id: Some("tu-1".into()),
    });
    let j = serde_json::to_value(&req).unwrap();
    assert_eq!(j["method"], "hook/callback");
    assert_eq!(j["params"]["callback_id"], "cb-1");
}

#[test]
fn cancel_request_wire_method() {
    let req = ServerRequest::CancelRequest(ServerCancelRequestParams {
        request_id: "req-5".into(),
        reason: Some("timeout".into()),
    });
    let j = serde_json::to_value(&req).unwrap();
    assert_eq!(j["method"], "control/cancelRequest");
    assert_eq!(j["params"]["reason"], "timeout");
}

#[test]
fn mcp_status_result_roundtrip() {
    let r = McpStatusResult {
        mcp_servers: vec![McpServerStatus {
            name: "github".into(),
            status: "connected".into(),
            tool_count: 5,
            error: None,
        }],
    };
    let j = serde_json::to_value(&r).unwrap();
    assert_eq!(j["mcp_servers"][0]["name"], "github");
    assert_eq!(j["mcp_servers"][0]["tool_count"], 5);
}

#[test]
fn context_usage_result_roundtrip() {
    let r = ContextUsageResult {
        total_tokens: 50_000,
        max_tokens: 200_000,
        raw_max_tokens: 200_000,
        percentage: 25.0,
        model: "claude-opus".into(),
        categories: vec![ContextUsageCategory {
            name: "system_prompt".into(),
            tokens: 5000,
        }],
        is_auto_compact_enabled: true,
        auto_compact_threshold: Some(180_000),
        message_breakdown: None,
    };
    let j = serde_json::to_value(&r).unwrap();
    assert_eq!(j["total_tokens"], 50_000);
    assert_eq!(j["percentage"], 25.0);
    assert_eq!(j["categories"][0]["name"], "system_prompt");
}
