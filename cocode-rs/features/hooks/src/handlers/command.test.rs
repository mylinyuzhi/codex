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

fn default_output() -> HookOutput {
    HookOutput {
        continue_execution: true,
        stop_reason: None,
        updated_input: None,
        additional_context: None,
        is_async: false,
        permission_decision: None,
        decision: None,
        hook_specific_output: None,
        system_message: None,
        updated_tool_output: None,
        blocking_error: None,
        prevent_continuation: false,
        suppress_output: false,
    }
}

#[tokio::test]
async fn test_execute_echo_command() {
    // Use `echo` which ignores stdin and writes to stdout
    let ctx = make_ctx();
    let (result, _suppress) =
        CommandHandler::execute(r#"echo '{"action":"continue"}'"#, &ctx).await;
    // echo output includes a newline, should parse as Continue
    assert!(matches!(result, HookResult::Continue));
}

#[tokio::test]
async fn test_execute_nonexistent_command() {
    let ctx = make_ctx();
    let (result, _) =
        CommandHandler::execute("this-command-definitely-does-not-exist-12345", &ctx).await;
    assert!(matches!(result, HookResult::Continue));
}

#[tokio::test]
async fn test_execute_failing_command() {
    let ctx = make_ctx();
    let (result, _) = CommandHandler::execute("false", &ctx).await;
    assert!(matches!(result, HookResult::Continue));
}

#[test]
fn test_hook_output_continue() {
    let output = default_output();
    let result: HookResult = output.into();
    assert!(matches!(result, HookResult::Continue));
}

#[test]
fn test_hook_output_reject() {
    let mut output = default_output();
    output.continue_execution = false;
    output.stop_reason = Some("Not allowed".to_string());
    let result: HookResult = output.into();
    if let HookResult::Reject { reason } = result {
        assert_eq!(reason, "Not allowed");
    } else {
        panic!("Expected Reject");
    }
}

#[test]
fn test_hook_output_reject_default_reason() {
    let mut output = default_output();
    output.continue_execution = false;
    let result: HookResult = output.into();
    if let HookResult::Reject { reason } = result {
        assert_eq!(reason, "Hook blocked execution");
    } else {
        panic!("Expected Reject");
    }
}

#[test]
fn test_hook_output_modify_input() {
    let mut output = default_output();
    output.updated_input = Some(serde_json::json!({"modified": true}));
    let result: HookResult = output.into();
    if let HookResult::ModifyInput { new_input } = result {
        assert_eq!(new_input["modified"], true);
    } else {
        panic!("Expected ModifyInput");
    }
}

#[test]
fn test_hook_output_additional_context() {
    let mut output = default_output();
    output.additional_context = Some("Extra info".to_string());
    let result: HookResult = output.into();
    if let HookResult::ContinueWithContext {
        additional_context, ..
    } = result
    {
        assert_eq!(additional_context, Some("Extra info".to_string()));
    } else {
        panic!("Expected ContinueWithContext");
    }
}

#[test]
fn test_hook_output_async() {
    let mut output = default_output();
    output.is_async = true;
    let result = output.into_result(Some("test-hook"));
    if let HookResult::Async { task_id, hook_name } = result {
        assert!(task_id.starts_with("async-"));
        assert_eq!(hook_name, "test-hook");
    } else {
        panic!("Expected Async");
    }
}

#[test]
fn test_hook_output_blocking_error() {
    let mut output = default_output();
    output.blocking_error = Some("critical failure".to_string());
    let result: HookResult = output.into();
    if let HookResult::Reject { reason } = result {
        assert_eq!(reason, "critical failure");
    } else {
        panic!("Expected Reject from blockingError");
    }
}

#[test]
fn test_hook_output_blocking_error_takes_precedence_over_async() {
    let mut output = default_output();
    output.blocking_error = Some("blocked".to_string());
    output.is_async = true;
    let result: HookResult = output.into();
    // blockingError should take precedence over async
    assert!(matches!(result, HookResult::Reject { .. }));
}

#[test]
fn test_hook_output_prevent_continuation() {
    let mut output = default_output();
    output.prevent_continuation = true;
    output.stop_reason = Some("done".to_string());
    let result: HookResult = output.into();
    if let HookResult::PreventContinuation { reason } = result {
        assert_eq!(reason, Some("done".to_string()));
    } else {
        panic!("Expected PreventContinuation, got: {result:?}");
    }
}

#[test]
fn test_hook_output_prevent_continuation_no_reason() {
    let mut output = default_output();
    output.prevent_continuation = true;
    let result: HookResult = output.into();
    if let HookResult::PreventContinuation { reason } = result {
        assert!(reason.is_none());
    } else {
        panic!("Expected PreventContinuation, got: {result:?}");
    }
}

#[test]
fn test_hook_output_suppress_output() {
    let json = r#"{"continue_execution":true,"suppressOutput":true}"#;
    let parsed: HookOutput = serde_json::from_str(json).expect("deserialize");
    assert!(parsed.suppress_output);
}

#[test]
fn test_parse_hook_response_hook_result() {
    let json = r#"{"action":"continue"}"#;
    let (result, suppress) = parse_hook_response(json);
    assert!(matches!(result, HookResult::Continue));
    assert!(!suppress);

    let json = r#"{"action":"reject","reason":"blocked"}"#;
    let (result, suppress) = parse_hook_response(json);
    if let HookResult::Reject { reason } = result {
        assert_eq!(reason, "blocked");
    } else {
        panic!("Expected Reject");
    }
    assert!(!suppress);
}

#[test]
fn test_parse_hook_response_hook_output() {
    let json = r#"{"continue_execution":true}"#;
    let (result, suppress) = parse_hook_response(json);
    assert!(matches!(result, HookResult::Continue));
    assert!(!suppress);

    let json = r#"{"continue_execution":false,"stop_reason":"nope"}"#;
    let (result, _) = parse_hook_response(json);
    if let HookResult::Reject { reason } = result {
        assert_eq!(reason, "nope");
    } else {
        panic!("Expected Reject");
    }
}

#[test]
fn test_parse_hook_response_blocking_error() {
    let json = r#"{"continue_execution":true,"blockingError":"fatal"}"#;
    let (result, _) = parse_hook_response(json);
    if let HookResult::Reject { reason } = result {
        assert_eq!(reason, "fatal");
    } else {
        panic!("Expected Reject from blockingError, got: {result:?}");
    }
}

#[test]
fn test_parse_hook_response_prevent_continuation() {
    let json = r#"{"continue_execution":true,"preventContinuation":true,"stop_reason":"halted"}"#;
    let (result, _) = parse_hook_response(json);
    if let HookResult::PreventContinuation { reason } = result {
        assert_eq!(reason, Some("halted".to_string()));
    } else {
        panic!("Expected PreventContinuation, got: {result:?}");
    }
}

#[test]
fn test_parse_hook_response_invalid() {
    let (result, _) = parse_hook_response("not json at all");
    assert!(matches!(result, HookResult::Continue));

    let (result, _) = parse_hook_response(r#"{"unknown":"format"}"#);
    assert!(matches!(result, HookResult::Continue));
}

#[test]
fn test_hook_output_serde() {
    let mut output = default_output();
    output.updated_input = Some(serde_json::json!({"key": "value"}));
    output.additional_context = Some("context".to_string());
    let json = serde_json::to_string(&output).expect("serialize");
    let parsed: HookOutput = serde_json::from_str(&json).expect("deserialize");
    assert!(parsed.continue_execution);
    assert!(parsed.updated_input.is_some());
    assert_eq!(parsed.additional_context, Some("context".to_string()));
    assert!(!parsed.is_async);
    assert!(!parsed.prevent_continuation);
    assert!(parsed.blocking_error.is_none());
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
    let (result, _) = parse_hook_response(json);
    assert!(matches!(result, HookResult::Async { .. }));
}

#[tokio::test]
async fn test_execute_exit_code_2_blocks() {
    let ctx = make_ctx();
    // `exit 2` on unix should produce exit code 2
    let (result, _) = CommandHandler::execute("echo 'blocked reason' >&2; exit 2", &ctx).await;
    if let HookResult::Reject { reason } = result {
        assert!(reason.contains("blocked reason"));
    } else {
        panic!("Expected Reject for exit code 2, got: {result:?}");
    }
}

#[tokio::test]
async fn test_execute_exit_code_2_default_reason() {
    let ctx = make_ctx();
    // exit 2 with no stderr should give a default reason
    let (result, _) = CommandHandler::execute("exit 2", &ctx).await;
    if let HookResult::Reject { reason } = result {
        assert!(reason.contains("exit code 2"));
    } else {
        panic!("Expected Reject for exit code 2, got: {result:?}");
    }
}

#[tokio::test]
async fn test_execute_exit_code_1_continues() {
    let ctx = make_ctx();
    // exit 1 should not block (only exit code 2 blocks)
    let (result, _) = CommandHandler::execute("exit 1", &ctx).await;
    assert!(matches!(result, HookResult::Continue));
}

#[test]
fn test_hook_output_decision_block() {
    let mut output = default_output();
    output.stop_reason = Some("stopped by hook".to_string());
    output.decision = Some("block".to_string());
    let result: HookResult = output.into();
    if let HookResult::Reject { reason } = result {
        assert_eq!(reason, "stopped by hook");
    } else {
        panic!("Expected Reject for decision=block, got: {result:?}");
    }
}

#[test]
fn test_hook_output_hook_specific_output() {
    let json = r#"{"continue_execution":true,"hookSpecificOutput":{"key":"value"}}"#;
    let parsed: HookOutput = serde_json::from_str(json).expect("deserialize");
    assert!(parsed.hook_specific_output.is_some());
    assert_eq!(parsed.hook_specific_output.unwrap()["key"], "value");
}

// --- suppress_output propagation tests (Issue 2) ---

#[test]
fn test_parse_hook_response_suppress_output_propagated() {
    let json = r#"{"continue_execution":true,"suppressOutput":true}"#;
    let (result, suppress) = parse_hook_response(json);
    assert!(matches!(result, HookResult::Continue));
    assert!(
        suppress,
        "suppress_output should be propagated from HookOutput"
    );
}

#[test]
fn test_parse_hook_response_suppress_output_false_by_default() {
    let json = r#"{"continue_execution":true}"#;
    let (_, suppress) = parse_hook_response(json);
    assert!(!suppress, "suppress_output should default to false");
}

#[test]
fn test_parse_hook_response_hook_result_format_never_suppresses() {
    // HookResult (legacy format) doesn't have suppressOutput
    let json = r#"{"action":"continue"}"#;
    let (_, suppress) = parse_hook_response(json);
    assert!(
        !suppress,
        "HookResult format should never set suppress_output"
    );
}

#[tokio::test]
async fn test_execute_suppress_output_from_command() {
    let ctx = make_ctx();
    let (result, suppress) = CommandHandler::execute(
        r#"echo '{"continue_execution":true,"suppressOutput":true}'"#,
        &ctx,
    )
    .await;
    assert!(matches!(result, HookResult::Continue));
    assert!(
        suppress,
        "suppress_output should propagate through execute()"
    );
}

// --- env var merging tests (Issue 5) ---

#[tokio::test]
async fn test_session_start_env_file_creates_continue_with_context() {
    // SessionStart hooks get COCODE_ENV_FILE set; writing to it produces env vars
    let ctx = HookContext::new(
        HookEventType::SessionStart,
        "test-env-create".to_string(),
        PathBuf::from("/tmp"),
    );
    let (result, _) =
        CommandHandler::execute(r#"echo "MY_VAR=my_value" > "$COCODE_ENV_FILE""#, &ctx).await;
    if let HookResult::ContinueWithContext {
        additional_context,
        env_vars,
    } = result
    {
        assert!(additional_context.is_none());
        assert_eq!(env_vars.get("MY_VAR").unwrap(), "my_value");
    } else {
        panic!("Expected ContinueWithContext with env vars, got: {result:?}");
    }
}

#[tokio::test]
async fn test_session_start_env_file_merges_with_stdout_continue() {
    // Command returns Continue on stdout AND writes env vars to COCODE_ENV_FILE
    // → should merge into ContinueWithContext
    let ctx = HookContext::new(
        HookEventType::SessionStart,
        "test-env-merge-continue".to_string(),
        PathBuf::from("/tmp"),
    );
    let (result, _) = CommandHandler::execute(
        r#"echo "FILE_VAR=from_file" > "$COCODE_ENV_FILE"; echo '{"action":"continue"}'"#,
        &ctx,
    )
    .await;
    if let HookResult::ContinueWithContext { env_vars, .. } = result {
        assert_eq!(env_vars.get("FILE_VAR").unwrap(), "from_file");
    } else {
        panic!("Expected ContinueWithContext, got: {result:?}");
    }
}

#[tokio::test]
async fn test_session_start_env_file_merges_with_stdout_context() {
    // Command returns ContinueWithContext on stdout AND writes env vars to COCODE_ENV_FILE
    // → should merge both env var sources
    let ctx = HookContext::new(
        HookEventType::SessionStart,
        "test-env-merge-ctx".to_string(),
        PathBuf::from("/tmp"),
    );
    let (result, _) = CommandHandler::execute(
        r#"echo "FILE_VAR=from_file" > "$COCODE_ENV_FILE"; echo '{"continue_execution":true,"additional_context":"from stdout"}'"#,
        &ctx,
    )
    .await;
    if let HookResult::ContinueWithContext {
        additional_context,
        env_vars,
    } = result
    {
        assert_eq!(additional_context, Some("from stdout".to_string()));
        assert_eq!(env_vars.get("FILE_VAR").unwrap(), "from_file");
    } else {
        panic!("Expected ContinueWithContext, got: {result:?}");
    }
}

#[tokio::test]
async fn test_session_start_env_file_does_not_override_reject() {
    // Command returns Reject on stdout AND writes env vars to COCODE_ENV_FILE
    // → Reject should be preserved (env vars don't override blocking results)
    let ctx = HookContext::new(
        HookEventType::SessionStart,
        "test-env-reject".to_string(),
        PathBuf::from("/tmp"),
    );
    let (result, _) = CommandHandler::execute(
        r#"echo "FILE_VAR=from_file" > "$COCODE_ENV_FILE"; echo '{"action":"reject","reason":"blocked"}'"#,
        &ctx,
    )
    .await;
    assert!(
        matches!(result, HookResult::Reject { .. }),
        "Reject should not be overridden by env vars, got: {result:?}"
    );
}

#[tokio::test]
async fn test_non_session_start_no_env_file() {
    // PreToolUse hooks don't get COCODE_ENV_FILE
    let ctx = make_ctx();
    let (result, _) = CommandHandler::execute(r#"echo '{"action":"continue"}'"#, &ctx).await;
    assert!(matches!(result, HookResult::Continue));
}

#[tokio::test]
async fn test_session_start_env_file_with_export_prefix() {
    // env file lines with "export " prefix should be parsed correctly
    let ctx = HookContext::new(
        HookEventType::SessionStart,
        "test-env-export".to_string(),
        PathBuf::from("/tmp"),
    );
    let (result, _) = CommandHandler::execute(
        r#"echo 'export MY_KEY="my_val"' > "$COCODE_ENV_FILE""#,
        &ctx,
    )
    .await;
    if let HookResult::ContinueWithContext { env_vars, .. } = result {
        assert_eq!(env_vars.get("MY_KEY").unwrap(), "my_val");
    } else {
        panic!("Expected ContinueWithContext, got: {result:?}");
    }
}
