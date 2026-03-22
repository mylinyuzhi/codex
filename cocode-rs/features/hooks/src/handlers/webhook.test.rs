use super::*;
use crate::event::HookEventType;
use cocode_protocol::ToolName;
use std::path::PathBuf;

fn make_ctx() -> HookContext {
    HookContext::new(
        HookEventType::PreToolUse,
        "test-session".to_string(),
        PathBuf::from("/tmp"),
    )
    .with_tool_name(ToolName::Write.as_str())
}

#[test]
fn test_parse_webhook_response_hook_result() {
    let json = r#"{"action":"continue"}"#;
    let (result, suppress) = parse_webhook_response("http://test", json);
    assert!(matches!(result, HookResult::Continue));
    assert!(!suppress);

    let json = r#"{"action":"reject","reason":"blocked by policy"}"#;
    let (result, suppress) = parse_webhook_response("http://test", json);
    if let HookResult::Reject { reason } = result {
        assert_eq!(reason, "blocked by policy");
    } else {
        panic!("Expected Reject");
    }
    assert!(!suppress);
}

#[test]
fn test_parse_webhook_response_hook_output() {
    let json = r#"{"continue_execution":true}"#;
    let (result, _) = parse_webhook_response("http://test", json);
    assert!(matches!(result, HookResult::Continue));

    let json = r#"{"continue_execution":false,"stop_reason":"denied"}"#;
    let (result, _) = parse_webhook_response("http://test", json);
    if let HookResult::Reject { reason } = result {
        assert_eq!(reason, "denied");
    } else {
        panic!("Expected Reject");
    }

    let json = r#"{"continue_execution":true,"updated_input":{"modified":true}}"#;
    let (result, _) = parse_webhook_response("http://test", json);
    if let HookResult::ModifyInput { new_input } = result {
        assert_eq!(new_input["modified"], true);
    } else {
        panic!("Expected ModifyInput");
    }
}

#[test]
fn test_parse_webhook_response_invalid() {
    let (result, _) = parse_webhook_response("http://test", "not json");
    assert!(matches!(result, HookResult::Continue));

    let (result, _) = parse_webhook_response("http://test", r#"{"unknown":"format"}"#);
    assert!(matches!(result, HookResult::Continue));
}

#[tokio::test]
async fn test_execute_nonexistent_url() {
    let ctx = make_ctx();
    // Use a non-routable IP to ensure quick failure
    let (result, _) =
        WebhookHandler::execute_with_timeout("http://192.0.2.1:9999/hook", &ctx, 1).await;
    assert!(matches!(result, HookResult::Continue));
}

#[tokio::test]
async fn test_execute_invalid_url() {
    let ctx = make_ctx();
    let (result, _) = WebhookHandler::execute("not-a-valid-url", &ctx).await;
    assert!(matches!(result, HookResult::Continue));
}

// --- suppress_output propagation tests ---

#[test]
fn test_parse_webhook_response_suppress_output_propagated() {
    let json = r#"{"continue_execution":true,"suppressOutput":true}"#;
    let (result, suppress) = parse_webhook_response("http://test", json);
    assert!(matches!(result, HookResult::Continue));
    assert!(
        suppress,
        "suppress_output should be propagated from webhook HookOutput"
    );
}

#[test]
fn test_parse_webhook_response_suppress_output_false_by_default() {
    let json = r#"{"continue_execution":true}"#;
    let (_, suppress) = parse_webhook_response("http://test", json);
    assert!(!suppress);
}
