use cocode_app_server_protocol::AskForApprovalParams;
use cocode_app_server_protocol::KeepAliveParams;
use cocode_app_server_protocol::RequestId;
use cocode_app_server_protocol::ServerNotification;
use cocode_app_server_protocol::ServerRequest;
use cocode_protocol::ToolName;
use serde_json::json;

use super::JsonRpcRequestEnvelope;

#[test]
fn test_notification_wire_format() {
    let notif = ServerNotification::KeepAlive(KeepAliveParams { timestamp: 1234 });
    let json = serde_json::to_value(&notif).unwrap();

    let obj = json.as_object().unwrap();
    assert_eq!(obj.len(), 2, "expected exactly method + params");
    assert_eq!(json["method"], "keepAlive");
    assert_eq!(json["params"]["timestamp"], 1234);
}

#[test]
fn test_request_envelope_wire_format() {
    let req = ServerRequest::AskForApproval(AskForApprovalParams {
        request_id: "req_1".to_string(),
        tool_name: ToolName::Bash.as_str().to_string(),
        input: json!({"command": "ls"}),
        description: None,
        permission_suggestions: None,
        blocked_path: None,
        decision_reason: None,
    });

    let envelope = JsonRpcRequestEnvelope {
        id: RequestId::Integer(42),
        inner: &req,
    };
    let json = serde_json::to_value(&envelope).unwrap();

    // Exactly id + method + params at top level
    let obj = json.as_object().unwrap();
    assert_eq!(obj.len(), 3, "expected exactly id + method + params");
    assert_eq!(json["id"], 42);
    assert_eq!(json["method"], "approval/askForApproval");
    assert!(json["params"]["request_id"].is_string());
    assert_eq!(json["params"]["tool_name"], ToolName::Bash.as_str());
}
