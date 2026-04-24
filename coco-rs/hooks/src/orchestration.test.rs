use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;

use coco_types::HookEventType;
use coco_types::HookScope;
use tokio_util::sync::CancellationToken;

use super::*;
use crate::HookDefinition;
use crate::HookHandler;
use crate::HookRegistry;

fn test_ctx() -> OrchestrationContext {
    OrchestrationContext {
        session_id: "test-session-123".to_string(),
        cwd: PathBuf::from("/tmp/test"),
        project_dir: Some(PathBuf::from("/tmp/project")),
        permission_mode: Some("default".to_string()),
        cancel: CancellationToken::new(),
        disable_all_hooks: false,
        allow_managed_hooks_only: false,
        attachment_emitter: coco_types::AttachmentEmitter::noop(),
    }
}

fn make_registry(hooks: Vec<HookDefinition>) -> HookRegistry {
    let mut registry = HookRegistry::new();
    for h in hooks {
        registry.register(h);
    }
    registry
}

// -----------------------------------------------------------------------
// parse_hook_output tests
// -----------------------------------------------------------------------

#[test]
fn test_parse_hook_output_plain_text() {
    let parsed = parse_hook_output("just some text\n");
    match parsed {
        ParsedHookOutput::PlainText(t) => assert_eq!(t, "just some text\n"),
        ParsedHookOutput::Json(_) => panic!("expected PlainText"),
    }
}

#[test]
fn test_parse_hook_output_json_decision() {
    let json_str = r#"{"decision": "block", "reason": "not allowed"}"#;
    let parsed = parse_hook_output(json_str);
    match parsed {
        ParsedHookOutput::Json(j) => {
            assert_eq!(j.decision.as_deref(), Some("block"));
            assert_eq!(j.reason.as_deref(), Some("not allowed"));
        }
        ParsedHookOutput::PlainText(_) => panic!("expected Json"),
    }
}

#[test]
fn test_parse_hook_output_json_continue_false() {
    let json_str = r#"{"continue": false, "stop_reason": "user abort"}"#;
    let parsed = parse_hook_output(json_str);
    match parsed {
        ParsedHookOutput::Json(j) => {
            assert_eq!(j.should_continue, Some(false));
            assert_eq!(j.stop_reason.as_deref(), Some("user abort"));
        }
        ParsedHookOutput::PlainText(_) => panic!("expected Json"),
    }
}

#[test]
fn test_parse_hook_output_invalid_json_falls_back_to_plain() {
    let bad = r#"{ invalid json }"#;
    let parsed = parse_hook_output(bad);
    match parsed {
        ParsedHookOutput::PlainText(t) => assert_eq!(t, bad),
        ParsedHookOutput::Json(_) => panic!("expected PlainText for invalid JSON"),
    }
}

// -----------------------------------------------------------------------
// aggregate_results tests
// -----------------------------------------------------------------------

#[test]
fn test_aggregate_results_empty() {
    let agg = aggregate_results(&[]);
    assert!(!agg.is_blocked());
    assert!(!agg.prevent_continuation);
    assert!(agg.additional_contexts.is_empty());
}

#[test]
fn test_aggregate_results_blocking() {
    let results = vec![SingleHookResult {
        command: "check.sh".to_string(),
        succeeded: false,
        output: "forbidden".to_string(),
        blocked: true,
        outcome: HookOutcome::Blocking,
        status_message: None,
        async_rewake: false,
    }];
    let agg = aggregate_results(&results);
    assert!(agg.is_blocked());
    assert_eq!(
        agg.blocking_error
            .as_ref()
            .map(|e| e.blocking_error.as_str()),
        Some("forbidden")
    );
}

#[test]
fn test_aggregate_results_json_permission_deny() {
    let json_output = r#"{"decision": "block", "reason": "security policy"}"#;
    let results = vec![SingleHookResult {
        command: "policy.sh".to_string(),
        succeeded: true,
        output: json_output.to_string(),
        blocked: false,
        outcome: HookOutcome::Success,
        status_message: None,
        async_rewake: false,
    }];
    let agg = aggregate_results(&results);
    assert!(agg.is_blocked());
    assert_eq!(agg.permission_behavior, Some(PermissionBehavior::Deny));
    assert_eq!(
        agg.hook_permission_decision_reason.as_deref(),
        Some("security policy")
    );
}

#[test]
fn test_aggregate_results_json_permission_allow() {
    let json_output = r#"{"decision": "approve", "reason": "pre-approved"}"#;
    let results = vec![SingleHookResult {
        command: "approve.sh".to_string(),
        succeeded: true,
        output: json_output.to_string(),
        blocked: false,
        outcome: HookOutcome::Success,
        status_message: None,
        async_rewake: false,
    }];
    let agg = aggregate_results(&results);
    assert!(!agg.is_blocked());
    assert_eq!(agg.permission_behavior, Some(PermissionBehavior::Allow));
}

#[test]
fn test_aggregate_results_additional_context() {
    let results = vec![
        SingleHookResult {
            command: "ctx1.sh".to_string(),
            succeeded: true,
            output: "context alpha".to_string(),
            blocked: false,
            outcome: HookOutcome::Success,
            status_message: None,
            async_rewake: false,
        },
        SingleHookResult {
            command: "ctx2.sh".to_string(),
            succeeded: true,
            output: "context beta".to_string(),
            blocked: false,
            outcome: HookOutcome::Success,
            status_message: None,
            async_rewake: false,
        },
    ];
    let agg = aggregate_results(&results);
    assert_eq!(agg.additional_contexts.len(), 2);
    assert_eq!(agg.additional_contexts[0], "context alpha");
    assert_eq!(agg.additional_contexts[1], "context beta");
}

// -----------------------------------------------------------------------
// build_hook_env tests
// -----------------------------------------------------------------------

#[test]
fn test_build_hook_env_basic() {
    let env = build_hook_env("sess-1", "/home/user", Some("Bash"), "PreToolUse", None);
    assert_eq!(env.get("HOOK_EVENT").unwrap(), "PreToolUse");
    assert_eq!(env.get("HOOK_SESSION_ID").unwrap(), "sess-1");
    assert_eq!(env.get("HOOK_TOOL_NAME").unwrap(), "Bash");
    assert!(!env.contains_key("CLAUDE_PROJECT_DIR"));
}

#[test]
fn test_build_hook_env_with_project_dir() {
    let env = build_hook_env("sess-2", "/tmp", None, "SessionStart", Some("/proj/root"));
    assert!(!env.contains_key("HOOK_TOOL_NAME"));
    assert_eq!(env.get("CLAUDE_PROJECT_DIR").unwrap(), "/proj/root");
}

// -----------------------------------------------------------------------
// execute_hooks_parallel tests (integration with actual shell execution)
// -----------------------------------------------------------------------

#[tokio::test]
async fn test_parallel_execution_multiple_hooks() {
    let registry = make_registry(vec![
        HookDefinition {
            event: HookEventType::PreToolUse,
            matcher: None,
            handler: HookHandler::Command {
                command: "echo hook-a".to_string(),
                timeout_ms: Some(5000),
                shell: None,
            },
            priority: 0,
            scope: HookScope::default(),
            if_condition: None,
            once: false,
            is_async: false,
            async_rewake: false,
            shell: None,
            status_message: None,
        },
        HookDefinition {
            event: HookEventType::PreToolUse,
            matcher: None,
            handler: HookHandler::Command {
                command: "echo hook-b".to_string(),
                timeout_ms: Some(5000),
                shell: None,
            },
            priority: 1,
            scope: HookScope::default(),
            if_condition: None,
            once: false,
            is_async: false,
            async_rewake: false,
            shell: None,
            status_message: None,
        },
    ]);

    let cancel = CancellationToken::new();
    let results = execute_hooks_parallel(
        &registry,
        HookEventType::PreToolUse,
        Some("Bash"),
        "{}",
        &HashMap::new(),
        &cancel,
        Duration::from_secs(10),
        /*event_tx*/ None,
        &coco_types::AttachmentEmitter::noop(),
    )
    .await;

    assert_eq!(results.len(), 2);
    for r in &results {
        assert!(r.succeeded);
        assert!(!r.blocked);
        assert_eq!(r.outcome, HookOutcome::Success);
    }

    // Both should produce output (order may vary since they run in parallel).
    let outputs: Vec<&str> = results.iter().map(|r| r.output.trim()).collect();
    assert!(outputs.contains(&"hook-a"));
    assert!(outputs.contains(&"hook-b"));
}

#[tokio::test]
async fn test_parallel_execution_cancellation() {
    let registry = make_registry(vec![HookDefinition {
        event: HookEventType::SessionEnd,
        matcher: None,
        handler: HookHandler::Command {
            command: "sleep 60".to_string(),
            timeout_ms: Some(60_000),
            shell: None,
        },
        priority: 0,
        scope: HookScope::default(),
        if_condition: None,
        once: false,
        is_async: false,
        async_rewake: false,
        shell: None,
        status_message: None,
    }]);

    let cancel = CancellationToken::new();
    // Cancel immediately.
    cancel.cancel();

    let results = execute_hooks_parallel(
        &registry,
        HookEventType::SessionEnd,
        None,
        "{}",
        &HashMap::new(),
        &cancel,
        Duration::from_secs(1),
        /*event_tx*/ None,
        &coco_types::AttachmentEmitter::noop(),
    )
    .await;

    assert_eq!(results.len(), 1);
    assert!(!results[0].succeeded);
    assert_eq!(results[0].outcome, HookOutcome::Cancelled);
}

#[tokio::test]
async fn test_parallel_execution_timeout() {
    let registry = make_registry(vec![HookDefinition {
        event: HookEventType::SessionEnd,
        matcher: None,
        handler: HookHandler::Command {
            command: "sleep 60".to_string(),
            timeout_ms: None,
            shell: None,
        },
        priority: 0,
        scope: HookScope::default(),
        if_condition: None,
        once: false,
        is_async: false,
        async_rewake: false,
        shell: None,
        status_message: None,
    }]);

    let cancel = CancellationToken::new();
    let results = execute_hooks_parallel(
        &registry,
        HookEventType::SessionEnd,
        None,
        "{}",
        &HashMap::new(),
        &cancel,
        Duration::from_millis(100),
        /*event_tx*/ None,
        &coco_types::AttachmentEmitter::noop(),
    )
    .await;

    assert_eq!(results.len(), 1);
    assert!(!results[0].succeeded);
    assert!(results[0].output.contains("timed out"));
}

#[tokio::test]
async fn test_parallel_execution_exit_code_2_is_blocking() {
    let registry = make_registry(vec![HookDefinition {
        event: HookEventType::PreToolUse,
        matcher: None,
        handler: HookHandler::Command {
            command: "echo 'blocked' >&2; exit 2".to_string(),
            timeout_ms: Some(5000),
            shell: None,
        },
        priority: 0,
        scope: HookScope::default(),
        if_condition: None,
        once: false,
        is_async: false,
        async_rewake: false,
        shell: None,
        status_message: None,
    }]);

    let cancel = CancellationToken::new();
    let results = execute_hooks_parallel(
        &registry,
        HookEventType::PreToolUse,
        Some("Write"),
        "{}",
        &HashMap::new(),
        &cancel,
        Duration::from_secs(5),
        /*event_tx*/ None,
        &coco_types::AttachmentEmitter::noop(),
    )
    .await;

    assert_eq!(results.len(), 1);
    assert!(!results[0].succeeded);
    assert!(results[0].blocked);
    assert!(results[0].output.contains("blocked"));
}

// -----------------------------------------------------------------------
// Event-specific orchestration tests
// -----------------------------------------------------------------------

#[tokio::test]
async fn test_execute_pre_tool_use_with_prompt_hook() {
    let registry = make_registry(vec![HookDefinition {
        event: HookEventType::PreToolUse,
        matcher: Some("Bash".to_string()),
        handler: HookHandler::Prompt {
            prompt: "Check for dangerous commands".to_string(),
        },
        priority: 0,
        scope: HookScope::default(),
        if_condition: None,
        once: false,
        is_async: false,
        async_rewake: false,
        shell: None,
        status_message: None,
    }]);

    let ctx = test_ctx();
    let result = execute_pre_tool_use(
        &registry,
        &ctx,
        "Bash",
        "tool-use-1",
        &serde_json::json!({"command": "rm -rf /"}),
        /*event_tx*/ None,
    )
    .await
    .expect("should succeed");

    assert!(!result.is_blocked());
    assert_eq!(result.additional_contexts.len(), 1);
    assert_eq!(
        result.additional_contexts[0],
        "Check for dangerous commands"
    );
}

#[tokio::test]
async fn test_execute_post_tool_use_failure_with_prompt_hook() {
    let registry = make_registry(vec![HookDefinition {
        event: HookEventType::PostToolUseFailure,
        matcher: Some("Bash".to_string()),
        handler: HookHandler::Prompt {
            prompt: "Recover from failed command".to_string(),
        },
        priority: 0,
        scope: HookScope::default(),
        if_condition: None,
        once: false,
        is_async: false,
        async_rewake: false,
        shell: None,
        status_message: None,
    }]);

    let ctx = test_ctx();
    let result = execute_post_tool_use_failure(
        &registry,
        &ctx,
        "Bash",
        &serde_json::json!({"command": "false"}),
        "exit code 1",
        Some("execution_error"),
        /*event_tx*/ None,
    )
    .await
    .expect("should succeed");

    assert!(!result.is_blocked());
    assert_eq!(result.additional_contexts.len(), 1);
    assert_eq!(result.additional_contexts[0], "Recover from failed command");
}

#[tokio::test]
async fn test_execute_session_start() {
    let registry = make_registry(vec![HookDefinition {
        event: HookEventType::SessionStart,
        matcher: None,
        handler: HookHandler::Command {
            command: "echo initialized".to_string(),
            timeout_ms: Some(5000),
            shell: None,
        },
        priority: 0,
        scope: HookScope::default(),
        if_condition: None,
        once: false,
        is_async: false,
        async_rewake: false,
        shell: None,
        status_message: None,
    }]);

    let ctx = test_ctx();
    let result = execute_session_start(&registry, &ctx, "startup", None, None)
        .await
        .expect("should succeed");

    assert!(!result.is_blocked());
    // The "initialized" output appears as additional context (plain text).
    assert!(
        result
            .additional_contexts
            .iter()
            .any(|c| c == "initialized"),
        "expected 'initialized' in contexts: {:?}",
        result.additional_contexts
    );
}

#[tokio::test]
async fn test_execute_stop_failure() {
    let registry = make_registry(vec![HookDefinition {
        event: HookEventType::StopFailure,
        matcher: None,
        handler: HookHandler::Command {
            command: "echo stop-failure-handled".to_string(),
            timeout_ms: Some(5000),
            shell: None,
        },
        priority: 0,
        scope: HookScope::default(),
        if_condition: None,
        once: false,
        is_async: false,
        async_rewake: false,
        shell: None,
        status_message: None,
    }]);

    let ctx = test_ctx();
    let results = execute_stop_failure(
        &registry,
        &ctx,
        "context_overflow",
        Some("token limit exceeded"),
        None,
    )
    .await
    .expect("should succeed");

    assert_eq!(results.len(), 1);
    assert!(results[0].succeeded);
    assert_eq!(results[0].output.trim(), "stop-failure-handled");
}

// -----------------------------------------------------------------------
// HTTP hook filtering for SessionStart/Setup
// -----------------------------------------------------------------------

#[tokio::test]
async fn test_http_hooks_filtered_for_session_start() {
    let registry = make_registry(vec![
        HookDefinition {
            event: HookEventType::SessionStart,
            matcher: None,
            handler: HookHandler::Http {
                url: "https://example.com/hook".to_string(),
                method: None,
                headers: None,
                timeout_ms: Some(5000),
            },
            priority: 0,
            scope: HookScope::default(),
            if_condition: None,
            once: false,
            is_async: false,
            async_rewake: false,
            shell: None,
            status_message: None,
        },
        HookDefinition {
            event: HookEventType::SessionStart,
            matcher: None,
            handler: HookHandler::Command {
                command: "echo allowed".to_string(),
                timeout_ms: Some(5000),
                shell: None,
            },
            priority: 1,
            scope: HookScope::default(),
            if_condition: None,
            once: false,
            is_async: false,
            async_rewake: false,
            shell: None,
            status_message: None,
        },
    ]);

    let cancel = CancellationToken::new();
    let results = execute_hooks_parallel(
        &registry,
        HookEventType::SessionStart,
        None,
        "{}",
        &HashMap::new(),
        &cancel,
        Duration::from_secs(5),
        /*event_tx*/ None,
        &coco_types::AttachmentEmitter::noop(),
    )
    .await;

    // HTTP hook should be filtered out, only command hook remains
    assert_eq!(results.len(), 1);
    assert!(results[0].succeeded);
    assert_eq!(results[0].output.trim(), "allowed");
}

#[tokio::test]
async fn test_http_hooks_filtered_for_setup() {
    let registry = make_registry(vec![HookDefinition {
        event: HookEventType::Setup,
        matcher: None,
        handler: HookHandler::Http {
            url: "https://example.com/setup".to_string(),
            method: None,
            headers: None,
            timeout_ms: Some(5000),
        },
        priority: 0,
        scope: HookScope::default(),
        if_condition: None,
        once: false,
        is_async: false,
        async_rewake: false,
        shell: None,
        status_message: None,
    }]);

    let cancel = CancellationToken::new();
    let results = execute_hooks_parallel(
        &registry,
        HookEventType::Setup,
        None,
        "{}",
        &HashMap::new(),
        &cancel,
        Duration::from_secs(5),
        /*event_tx*/ None,
        &coco_types::AttachmentEmitter::noop(),
    )
    .await;

    // All hooks were HTTP, so all filtered out
    assert_eq!(results.len(), 0);
}

#[tokio::test]
async fn test_http_hooks_allowed_for_other_events() {
    // HTTP hooks should NOT be filtered for non-SessionStart/Setup events
    let registry = make_registry(vec![HookDefinition {
        event: HookEventType::PreToolUse,
        matcher: None,
        handler: HookHandler::Http {
            url: "https://example.com/pre".to_string(),
            method: None,
            headers: None,
            timeout_ms: Some(5000),
        },
        priority: 0,
        scope: HookScope::default(),
        if_condition: None,
        once: false,
        is_async: false,
        async_rewake: false,
        shell: None,
        status_message: None,
    }]);

    let cancel = CancellationToken::new();
    let results = execute_hooks_parallel(
        &registry,
        HookEventType::PreToolUse,
        Some("Bash"),
        "{}",
        &HashMap::new(),
        &cancel,
        Duration::from_secs(5),
        /*event_tx*/ None,
        &coco_types::AttachmentEmitter::noop(),
    )
    .await;

    // HTTP hook should NOT be filtered for PreToolUse
    assert_eq!(results.len(), 1);
}

// -----------------------------------------------------------------------
// Formatting helpers
// -----------------------------------------------------------------------

// -----------------------------------------------------------------------
// hookSpecificOutput parsing tests
// -----------------------------------------------------------------------

#[test]
fn test_parse_hook_specific_output_pre_tool_use() {
    let json_str = r#"{"hookSpecificOutput": {"hookEventName": "PreToolUse", "permissionDecision": "allow", "additionalContext": "safe command"}}"#;
    let parsed = parse_hook_output(json_str);
    match parsed {
        ParsedHookOutput::Json(j) => {
            assert!(j.hook_specific_output.is_some());
            match j.hook_specific_output.as_ref().unwrap() {
                HookSpecificOutput::PreToolUse {
                    permission_decision,
                    additional_context,
                    ..
                } => {
                    assert_eq!(permission_decision.as_deref(), Some("allow"));
                    assert_eq!(additional_context.as_deref(), Some("safe command"));
                }
                other => panic!("expected PreToolUse, got {other:?}"),
            }
        }
        ParsedHookOutput::PlainText(_) => panic!("expected Json"),
    }
}

#[test]
fn test_parse_hook_specific_output_permission_request() {
    let json_str = r#"{"hookSpecificOutput": {"hookEventName": "PermissionRequest", "decision": {"behavior": "allow", "updatedInput": {"command": "ls"}}}}"#;
    let parsed = parse_hook_output(json_str);
    match parsed {
        ParsedHookOutput::Json(j) => match j.hook_specific_output.as_ref().unwrap() {
            HookSpecificOutput::PermissionRequest { decision } => {
                match decision.as_ref().unwrap() {
                    PermissionRequestDecision::Allow { updated_input } => {
                        assert!(updated_input.is_some());
                    }
                    PermissionRequestDecision::Deny { .. } => panic!("expected Allow"),
                }
            }
            other => panic!("expected PermissionRequest, got {other:?}"),
        },
        ParsedHookOutput::PlainText(_) => panic!("expected Json"),
    }
}

#[test]
fn test_parse_hook_specific_output_elicitation_decline() {
    let json_str =
        r#"{"hookSpecificOutput": {"hookEventName": "Elicitation", "action": "decline"}}"#;
    let parsed = parse_hook_output(json_str);
    match parsed {
        ParsedHookOutput::Json(j) => match j.hook_specific_output.as_ref().unwrap() {
            HookSpecificOutput::Elicitation { action, .. } => {
                assert_eq!(action.as_deref(), Some("decline"));
            }
            other => panic!("expected Elicitation, got {other:?}"),
        },
        ParsedHookOutput::PlainText(_) => panic!("expected Json"),
    }
}

#[test]
fn test_aggregate_with_hook_specific_output() {
    let json_output = r#"{"hookSpecificOutput": {"hookEventName": "PreToolUse", "permissionDecision": "deny", "permissionDecisionReason": "dangerous", "additionalContext": "blocked cmd"}}"#;
    let results = vec![SingleHookResult {
        command: "check.sh".to_string(),
        succeeded: true,
        output: json_output.to_string(),
        blocked: false,
        outcome: HookOutcome::Success,
        status_message: None,
        async_rewake: false,
    }];
    let agg = aggregate_results(&results);
    assert_eq!(agg.permission_behavior, Some(PermissionBehavior::Deny));
    assert!(agg.is_blocked());
    assert_eq!(
        agg.hook_permission_decision_reason.as_deref(),
        Some("dangerous")
    );
    assert_eq!(agg.additional_contexts, vec!["blocked cmd"]);
}

#[test]
fn test_aggregate_elicitation_decline_blocks() {
    let json_output =
        r#"{"hookSpecificOutput": {"hookEventName": "Elicitation", "action": "decline"}}"#;
    let results = vec![SingleHookResult {
        command: "elicit.sh".to_string(),
        succeeded: true,
        output: json_output.to_string(),
        blocked: false,
        outcome: HookOutcome::Success,
        status_message: None,
        async_rewake: false,
    }];
    let agg = aggregate_results(&results);
    assert!(agg.is_blocked());
    assert!(agg.elicitation_response.is_some());
    assert_eq!(agg.elicitation_response.as_ref().unwrap().action, "decline");
}

// -----------------------------------------------------------------------
// Plugin env vars and CLAUDE_ENV_FILE tests
// -----------------------------------------------------------------------

#[test]
fn test_build_hook_env_with_plugin_context() {
    let mut opts = HashMap::new();
    opts.insert("api_key".to_string(), "secret".to_string());

    let ctx = HookPluginContext {
        plugin_root: Some("/plugins/my-plugin".to_string()),
        plugin_id: Some("my-plugin-id".to_string()),
        plugin_options: opts,
        skill_root: None,
    };

    let env =
        build_hook_env_with_plugin("sess", "/cwd", None, "PreToolUse", None, Some(&ctx), None);
    assert_eq!(env.get("CLAUDE_PLUGIN_ROOT").unwrap(), "/plugins/my-plugin");
    assert!(
        env.get("CLAUDE_PLUGIN_DATA")
            .unwrap()
            .contains("my-plugin-id")
    );
    assert_eq!(env.get("CLAUDE_PLUGIN_OPTION_API_KEY").unwrap(), "secret");
}

#[test]
fn test_build_hook_env_claude_env_file_for_session_start() {
    // CLAUDE_ENV_FILE requires hook_index for uniqueness.
    let env =
        build_hook_env_with_plugin("sess-1", "/cwd", None, "SessionStart", None, None, Some(0));
    assert!(env.contains_key("CLAUDE_ENV_FILE"));
    assert!(env.get("CLAUDE_ENV_FILE").unwrap().contains("sess-1"));
    assert!(env.get("CLAUDE_ENV_FILE").unwrap().contains("-0.sh"));
}

#[test]
fn test_build_hook_env_no_claude_env_file_without_hook_index() {
    // Without hook_index, CLAUDE_ENV_FILE is not set.
    let env = build_hook_env("sess-1", "/cwd", None, "SessionStart", None);
    assert!(!env.contains_key("CLAUDE_ENV_FILE"));
}

#[test]
fn test_build_hook_env_no_claude_env_file_for_pre_tool_use() {
    let env = build_hook_env("sess-1", "/cwd", None, "PreToolUse", None);
    assert!(!env.contains_key("CLAUDE_ENV_FILE"));
}

// -----------------------------------------------------------------------
// Formatting helpers
// -----------------------------------------------------------------------

#[test]
fn test_format_pre_tool_blocking_message() {
    let err = HookBlockingError {
        blocking_error: "write not allowed".to_string(),
        command: "check-write.sh".to_string(),
    };
    let msg = format_pre_tool_blocking_message("PreToolUse:Write", &err);
    assert_eq!(msg, "PreToolUse:Write hook error: write not allowed");
}

#[test]
fn test_format_stop_hook_message() {
    let err = HookBlockingError {
        blocking_error: "tests failed".to_string(),
        command: "run-tests.sh".to_string(),
    };
    let msg = format_stop_hook_message(&err);
    assert_eq!(msg, "Stop hook feedback:\ntests failed");
}
