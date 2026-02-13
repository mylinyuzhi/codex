use super::*;

#[test]
fn test_simple_substitution() {
    let result = PromptHandler::execute("Review: $ARGUMENTS", &serde_json::json!({"file": "a.rs"}));
    if let HookResult::ModifyInput { new_input } = result {
        let text = new_input.as_str().expect("should be a string");
        assert!(text.starts_with("Review: "));
        assert!(text.contains("a.rs"));
    } else {
        panic!("Expected ModifyInput");
    }
}

#[test]
fn test_no_placeholder() {
    let result = PromptHandler::execute("no placeholder here", &serde_json::json!({}));
    if let HookResult::ModifyInput { new_input } = result {
        assert_eq!(new_input.as_str().expect("string"), "no placeholder here");
    } else {
        panic!("Expected ModifyInput");
    }
}

#[test]
fn test_null_arguments() {
    let result = PromptHandler::execute("args=$ARGUMENTS", &Value::Null);
    if let HookResult::ModifyInput { new_input } = result {
        assert_eq!(new_input.as_str().expect("string"), "args=null");
    } else {
        panic!("Expected ModifyInput");
    }
}

#[test]
fn test_multiple_placeholders() {
    let result = PromptHandler::execute(
        "first=$ARGUMENTS second=$ARGUMENTS",
        &serde_json::json!("data"),
    );
    if let HookResult::ModifyInput { new_input } = result {
        let text = new_input.as_str().expect("string");
        // Both occurrences should be replaced
        assert_eq!(text, "first=\"data\" second=\"data\"");
    } else {
        panic!("Expected ModifyInput");
    }
}

#[test]
fn test_llm_verification_response_ok() {
    let response = r#"{"ok": true}"#;
    let result = PromptHandler::parse_verification_response(response);
    assert!(matches!(result, HookResult::Continue));
}

#[test]
fn test_llm_verification_response_reject() {
    let response = r#"{"ok": false, "reason": "Not allowed"}"#;
    let result = PromptHandler::parse_verification_response(response);
    if let HookResult::Reject { reason } = result {
        assert_eq!(reason, "Not allowed");
    } else {
        panic!("Expected Reject");
    }
}

#[test]
fn test_llm_verification_response_reject_no_reason() {
    let response = r#"{"ok": false}"#;
    let result = PromptHandler::parse_verification_response(response);
    if let HookResult::Reject { reason } = result {
        assert_eq!(reason, "Verification rejected by hook");
    } else {
        panic!("Expected Reject");
    }
}

#[test]
fn test_llm_verification_response_with_extra_text() {
    let response = "I've analyzed the request and determined: {\"ok\": true}";
    let result = PromptHandler::parse_verification_response(response);
    assert!(matches!(result, HookResult::Continue));
}

#[test]
fn test_llm_verification_response_invalid() {
    let response = "This is not JSON at all";
    let result = PromptHandler::parse_verification_response(response);
    // Should fail-open with Continue
    assert!(matches!(result, HookResult::Continue));
}

#[test]
fn test_verification_config_default() {
    let config = PromptVerificationConfig::default();
    assert!(!config.system_prompt.is_empty());
    assert!(config.model.is_none());
    assert_eq!(config.max_tokens, 100);
}

#[test]
fn test_llm_verification_response_serde() {
    let resp = LlmVerificationResponse {
        ok: false,
        reason: Some("Test reason".to_string()),
    };
    let json = serde_json::to_string(&resp).expect("serialize");
    let parsed: LlmVerificationResponse = serde_json::from_str(&json).expect("parse");
    assert!(!parsed.ok);
    assert_eq!(parsed.reason, Some("Test reason".to_string()));
}
