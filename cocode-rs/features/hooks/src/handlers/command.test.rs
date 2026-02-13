use super::*;
use crate::event::HookEventType;
use std::path::PathBuf;

fn make_ctx() -> HookContext {
    HookContext::new(
        HookEventType::PreToolUse,
        "test-session".to_string(),
        PathBuf::from("/tmp"),
    )
}

#[tokio::test]
async fn test_execute_echo_command() {
    // Use `echo` which ignores stdin and writes to stdout
    let ctx = make_ctx();
    let result =
        CommandHandler::execute("echo", &[r#"{"action":"continue"}"#.to_string()], &ctx).await;
    // echo output includes a newline, should parse as Continue
    assert!(matches!(result, HookResult::Continue));
}

#[tokio::test]
async fn test_execute_nonexistent_command() {
    let ctx = make_ctx();
    let result =
        CommandHandler::execute("this-command-definitely-does-not-exist-12345", &[], &ctx).await;
    assert!(matches!(result, HookResult::Continue));
}

#[tokio::test]
async fn test_execute_failing_command() {
    let ctx = make_ctx();
    let result = CommandHandler::execute("false", &[], &ctx).await;
    assert!(matches!(result, HookResult::Continue));
}

#[test]
fn test_hook_output_continue() {
    let output = HookOutput {
        continue_execution: true,
        stop_reason: None,
        updated_input: None,
        additional_context: None,
        is_async: false,
        permission_decision: None,
    };
    let result: HookResult = output.into();
    assert!(matches!(result, HookResult::Continue));
}

#[test]
fn test_hook_output_reject() {
    let output = HookOutput {
        continue_execution: false,
        stop_reason: Some("Not allowed".to_string()),
        updated_input: None,
        additional_context: None,
        is_async: false,
        permission_decision: None,
    };
    let result: HookResult = output.into();
    if let HookResult::Reject { reason } = result {
        assert_eq!(reason, "Not allowed");
    } else {
        panic!("Expected Reject");
    }
}

#[test]
fn test_hook_output_reject_default_reason() {
    let output = HookOutput {
        continue_execution: false,
        stop_reason: None,
        updated_input: None,
        additional_context: None,
        is_async: false,
        permission_decision: None,
    };
    let result: HookResult = output.into();
    if let HookResult::Reject { reason } = result {
        assert_eq!(reason, "Hook blocked execution");
    } else {
        panic!("Expected Reject");
    }
}

#[test]
fn test_hook_output_modify_input() {
    let output = HookOutput {
        continue_execution: true,
        stop_reason: None,
        updated_input: Some(serde_json::json!({"modified": true})),
        additional_context: None,
        is_async: false,
        permission_decision: None,
    };
    let result: HookResult = output.into();
    if let HookResult::ModifyInput { new_input } = result {
        assert_eq!(new_input["modified"], true);
    } else {
        panic!("Expected ModifyInput");
    }
}

#[test]
fn test_hook_output_additional_context() {
    let output = HookOutput {
        continue_execution: true,
        stop_reason: None,
        updated_input: None,
        additional_context: Some("Extra info".to_string()),
        is_async: false,
        permission_decision: None,
    };
    let result: HookResult = output.into();
    if let HookResult::ContinueWithContext { additional_context } = result {
        assert_eq!(additional_context, Some("Extra info".to_string()));
    } else {
        panic!("Expected ContinueWithContext");
    }
}

#[test]
fn test_hook_output_async() {
    let output = HookOutput {
        continue_execution: true,
        stop_reason: None,
        updated_input: None,
        additional_context: None,
        is_async: true,
        permission_decision: None,
    };
    let result = output.into_result(Some("test-hook"));
    if let HookResult::Async { task_id, hook_name } = result {
        assert!(task_id.starts_with("async-"));
        assert_eq!(hook_name, "test-hook");
    } else {
        panic!("Expected Async");
    }
}

#[test]
fn test_parse_hook_response_hook_result() {
    let json = r#"{"action":"continue"}"#;
    let result = parse_hook_response(json);
    assert!(matches!(result, HookResult::Continue));

    let json = r#"{"action":"reject","reason":"blocked"}"#;
    let result = parse_hook_response(json);
    if let HookResult::Reject { reason } = result {
        assert_eq!(reason, "blocked");
    } else {
        panic!("Expected Reject");
    }
}

#[test]
fn test_parse_hook_response_hook_output() {
    let json = r#"{"continue_execution":true}"#;
    let result = parse_hook_response(json);
    assert!(matches!(result, HookResult::Continue));

    let json = r#"{"continue_execution":false,"stop_reason":"nope"}"#;
    let result = parse_hook_response(json);
    if let HookResult::Reject { reason } = result {
        assert_eq!(reason, "nope");
    } else {
        panic!("Expected Reject");
    }
}

#[test]
fn test_parse_hook_response_invalid() {
    let result = parse_hook_response("not json at all");
    assert!(matches!(result, HookResult::Continue));

    let result = parse_hook_response(r#"{"unknown":"format"}"#);
    assert!(matches!(result, HookResult::Continue));
}

#[test]
fn test_hook_output_serde() {
    let output = HookOutput {
        continue_execution: true,
        stop_reason: None,
        updated_input: Some(serde_json::json!({"key": "value"})),
        additional_context: Some("context".to_string()),
        is_async: false,
        permission_decision: None,
    };
    let json = serde_json::to_string(&output).expect("serialize");
    let parsed: HookOutput = serde_json::from_str(&json).expect("deserialize");
    assert!(parsed.continue_execution);
    assert!(parsed.updated_input.is_some());
    assert_eq!(parsed.additional_context, Some("context".to_string()));
    assert!(!parsed.is_async);
}

#[test]
fn test_hook_output_serde_async() {
    let json = r#"{"continue_execution":true,"async":true}"#;
    let parsed: HookOutput = serde_json::from_str(json).expect("deserialize");
    assert!(parsed.continue_execution);
    assert!(parsed.is_async);
}

#[test]
fn test_parse_hook_response_async() {
    let json = r#"{"continue_execution":true,"async":true}"#;
    let result = parse_hook_response(json);
    assert!(matches!(result, HookResult::Async { .. }));
}
