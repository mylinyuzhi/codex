use super::*;

#[test]
fn test_stub_returns_continue() {
    let result = AgentHandler::execute(5);
    assert!(matches!(result, HookResult::Continue));
}

#[test]
fn test_stub_with_different_turns() {
    let result = AgentHandler::execute(10);
    assert!(matches!(result, HookResult::Continue));
}

#[test]
fn test_agent_verification_response_ok() {
    let response = r#"{"ok": true}"#;
    let result = AgentHandler::parse_verification_response(response);
    assert!(matches!(result, HookResult::Continue));
}

#[test]
fn test_agent_verification_response_reject() {
    let response = r#"{"ok": false, "reason": "Dangerous operation"}"#;
    let result = AgentHandler::parse_verification_response(response);
    if let HookResult::Reject { reason } = result {
        assert_eq!(reason, "Dangerous operation");
    } else {
        panic!("Expected Reject");
    }
}

#[test]
fn test_agent_verification_response_reject_no_reason() {
    let response = r#"{"ok": false}"#;
    let result = AgentHandler::parse_verification_response(response);
    if let HookResult::Reject { reason } = result {
        assert_eq!(reason, "Verification rejected by agent");
    } else {
        panic!("Expected Reject");
    }
}

#[test]
fn test_agent_verification_response_at_end() {
    // Agent might output analysis followed by JSON
    let response = "After analyzing the files, I found no issues.\n{\"ok\": true}";
    let result = AgentHandler::parse_verification_response(response);
    assert!(matches!(result, HookResult::Continue));
}

#[test]
fn test_agent_verification_response_invalid() {
    let response = "I could not determine a verdict";
    let result = AgentHandler::parse_verification_response(response);
    // Should fail-open
    assert!(matches!(result, HookResult::Continue));
}

#[test]
fn test_agent_verification_config_default() {
    let config = AgentVerificationConfig::default();
    assert_eq!(config.max_turns, 50);
    assert!(!config.system_prompt.is_empty());
    assert_eq!(
        config.allowed_tools,
        vec!["Read".to_string(), "Grep".to_string(), "Glob".to_string()]
    );
}

#[test]
fn test_agent_verification_response_serde() {
    let resp = AgentVerificationResponse {
        ok: true,
        reason: None,
    };
    let json = serde_json::to_string(&resp).expect("serialize");
    assert!(json.contains("\"ok\":true"));
    assert!(!json.contains("reason")); // Skipped when None

    let resp2 = AgentVerificationResponse {
        ok: false,
        reason: Some("Test".to_string()),
    };
    let json2 = serde_json::to_string(&resp2).expect("serialize");
    let parsed: AgentVerificationResponse = serde_json::from_str(&json2).expect("parse");
    assert!(!parsed.ok);
    assert_eq!(parsed.reason, Some("Test".to_string()));
}
