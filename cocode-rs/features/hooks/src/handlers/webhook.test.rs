use super::*;
use crate::event::HookEventType;
use std::path::PathBuf;

fn make_ctx() -> HookContext {
    HookContext::new(
        HookEventType::PreToolUse,
        "test-session".to_string(),
        PathBuf::from("/tmp"),
    )
    .with_tool_name("Write")
}

#[test]
fn test_parse_webhook_response_hook_result() {
    let json = r#"{"action":"continue"}"#;
    let result = parse_webhook_response("http://test", json);
    assert!(matches!(result, HookResult::Continue));

    let json = r#"{"action":"reject","reason":"blocked by policy"}"#;
    let result = parse_webhook_response("http://test", json);
    if let HookResult::Reject { reason } = result {
        assert_eq!(reason, "blocked by policy");
    } else {
        panic!("Expected Reject");
    }
}

#[test]
fn test_parse_webhook_response_hook_output() {
    let json = r#"{"continue_execution":true}"#;
    let result = parse_webhook_response("http://test", json);
    assert!(matches!(result, HookResult::Continue));

    let json = r#"{"continue_execution":false,"stop_reason":"denied"}"#;
    let result = parse_webhook_response("http://test", json);
    if let HookResult::Reject { reason } = result {
        assert_eq!(reason, "denied");
    } else {
        panic!("Expected Reject");
    }

    let json = r#"{"continue_execution":true,"updated_input":{"modified":true}}"#;
    let result = parse_webhook_response("http://test", json);
    if let HookResult::ModifyInput { new_input } = result {
        assert_eq!(new_input["modified"], true);
    } else {
        panic!("Expected ModifyInput");
    }
}

#[test]
fn test_parse_webhook_response_invalid() {
    let result = parse_webhook_response("http://test", "not json");
    assert!(matches!(result, HookResult::Continue));

    let result = parse_webhook_response("http://test", r#"{"unknown":"format"}"#);
    assert!(matches!(result, HookResult::Continue));
}

#[tokio::test]
async fn test_execute_nonexistent_url() {
    let ctx = make_ctx();
    // Use a non-routable IP to ensure quick failure
    let result =
        WebhookHandler::execute_with_timeout("http://192.0.2.1:9999/hook", &ctx, 1).await;
    assert!(matches!(result, HookResult::Continue));
}

#[tokio::test]
async fn test_execute_invalid_url() {
    let ctx = make_ctx();
    let result = WebhookHandler::execute("not-a-valid-url", &ctx).await;
    assert!(matches!(result, HookResult::Continue));
}
