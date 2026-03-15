use super::*;

#[test]
fn test_continue_serde() {
    let result = HookResult::Continue;
    let json = serde_json::to_string(&result).expect("serialize");
    let parsed: HookResult = serde_json::from_str(&json).expect("deserialize");
    assert!(matches!(parsed, HookResult::Continue));
}

#[test]
fn test_reject_serde() {
    let result = HookResult::Reject {
        reason: "not allowed".to_string(),
    };
    let json = serde_json::to_string(&result).expect("serialize");
    assert!(json.contains("not allowed"));
    let parsed: HookResult = serde_json::from_str(&json).expect("deserialize");
    if let HookResult::Reject { reason } = parsed {
        assert_eq!(reason, "not allowed");
    } else {
        panic!("Expected Reject");
    }
}

#[test]
fn test_modify_input_serde() {
    let result = HookResult::ModifyInput {
        new_input: serde_json::json!({"modified": true}),
    };
    let json = serde_json::to_string(&result).expect("serialize");
    let parsed: HookResult = serde_json::from_str(&json).expect("deserialize");
    if let HookResult::ModifyInput { new_input } = parsed {
        assert_eq!(new_input["modified"], true);
    } else {
        panic!("Expected ModifyInput");
    }
}

#[test]
fn test_continue_with_context_serde() {
    let result = HookResult::ContinueWithContext {
        additional_context: Some("Extra context from hook".to_string()),
    };
    let json = serde_json::to_string(&result).expect("serialize");
    assert!(json.contains("continue_with_context"));
    assert!(json.contains("Extra context from hook"));

    let parsed: HookResult = serde_json::from_str(&json).expect("deserialize");
    if let HookResult::ContinueWithContext { additional_context } = parsed {
        assert_eq!(
            additional_context,
            Some("Extra context from hook".to_string())
        );
    } else {
        panic!("Expected ContinueWithContext");
    }
}

#[test]
fn test_continue_with_context_none() {
    let result = HookResult::ContinueWithContext {
        additional_context: None,
    };
    let json = serde_json::to_string(&result).expect("serialize");
    let parsed: HookResult = serde_json::from_str(&json).expect("deserialize");
    if let HookResult::ContinueWithContext { additional_context } = parsed {
        assert!(additional_context.is_none());
    } else {
        panic!("Expected ContinueWithContext");
    }
}

#[test]
fn test_hook_outcome() {
    let outcome = HookOutcome {
        hook_name: "lint-check".to_string(),
        result: HookResult::Continue,
        duration_ms: 42,
    };
    let json = serde_json::to_string(&outcome).expect("serialize");
    let parsed: HookOutcome = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(parsed.hook_name, "lint-check");
    assert_eq!(parsed.duration_ms, 42);
    assert!(matches!(parsed.result, HookResult::Continue));
}

#[test]
fn test_async_result_serde() {
    let result = HookResult::Async {
        task_id: "async-123".to_string(),
        hook_name: "test-hook".to_string(),
    };
    let json = serde_json::to_string(&result).expect("serialize");
    assert!(json.contains("async"));
    assert!(json.contains("async-123"));
    assert!(json.contains("test-hook"));

    let parsed: HookResult = serde_json::from_str(&json).expect("deserialize");
    if let HookResult::Async { task_id, hook_name } = parsed {
        assert_eq!(task_id, "async-123");
        assert_eq!(hook_name, "test-hook");
    } else {
        panic!("Expected Async");
    }
}
