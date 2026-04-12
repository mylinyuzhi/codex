use pretty_assertions::assert_eq;

use super::*;

// ---------------------------------------------------------------------------
// SdkRequest serialization
// ---------------------------------------------------------------------------

#[test]
fn test_sdk_request_initialize_serialization() {
    let req = SdkRequest::Initialize {
        system_prompt: Some("You are helpful.".to_string()),
        append_system_prompt: None,
        json_schema: None,
        prompt_suggestions: false,
    };
    let json = serde_json::to_string(&req).unwrap();
    assert!(json.contains("\"subtype\":\"initialize\""));
    assert!(json.contains("\"system_prompt\":\"You are helpful.\""));

    let decoded: SdkRequest = serde_json::from_str(&json).unwrap();
    match decoded {
        SdkRequest::Initialize { system_prompt, .. } => {
            assert_eq!(system_prompt.as_deref(), Some("You are helpful."));
        }
        _ => panic!("expected Initialize"),
    }
}

#[test]
fn test_sdk_request_interrupt_serialization() {
    let req = SdkRequest::Interrupt;
    let json = serde_json::to_string(&req).unwrap();
    assert!(json.contains("\"subtype\":\"interrupt\""));

    let decoded: SdkRequest = serde_json::from_str(&json).unwrap();
    assert!(matches!(decoded, SdkRequest::Interrupt));
}

#[test]
fn test_sdk_request_can_use_tool_serialization() {
    let req = SdkRequest::CanUseTool {
        tool_name: "Bash".to_string(),
        tool_use_id: "tu-1".to_string(),
        input: serde_json::json!({"command": "ls -la"}),
        title: Some("Run command".to_string()),
        display_name: None,
        description: None,
        permission_suggestions: vec![],
        blocked_path: None,
        decision_reason: None,
        agent_id: None,
    };
    let json = serde_json::to_string(&req).unwrap();
    assert!(json.contains("\"subtype\":\"can_use_tool\""));
    assert!(json.contains("\"tool_name\":\"Bash\""));
    assert!(json.contains("\"tool_use_id\":\"tu-1\""));

    let decoded: SdkRequest = serde_json::from_str(&json).unwrap();
    match decoded {
        SdkRequest::CanUseTool {
            tool_name,
            tool_use_id,
            input,
            title,
            ..
        } => {
            assert_eq!(tool_name, "Bash");
            assert_eq!(tool_use_id, "tu-1");
            assert_eq!(input["command"], "ls -la");
            assert_eq!(title.as_deref(), Some("Run command"));
        }
        _ => panic!("expected CanUseTool"),
    }
}

#[test]
fn test_sdk_request_set_model_serialization() {
    let req = SdkRequest::SetModel {
        model: Some("claude-opus-4".to_string()),
    };
    let json = serde_json::to_string(&req).unwrap();
    assert!(json.contains("\"subtype\":\"set_model\""));
    assert!(json.contains("\"model\":\"claude-opus-4\""));
}

#[test]
fn test_sdk_request_mcp_status_serialization() {
    let req = SdkRequest::McpStatus;
    let json = serde_json::to_string(&req).unwrap();
    assert!(json.contains("\"subtype\":\"mcp_status\""));

    let decoded: SdkRequest = serde_json::from_str(&json).unwrap();
    assert!(matches!(decoded, SdkRequest::McpStatus));
}

#[test]
fn test_sdk_request_rewind_files_serialization() {
    let req = SdkRequest::RewindFiles {
        user_message_id: "msg-1".to_string(),
        dry_run: true,
    };
    let json = serde_json::to_string(&req).unwrap();
    assert!(json.contains("\"subtype\":\"rewind_files\""));
    assert!(json.contains("\"dry_run\":true"));
}

#[test]
fn test_sdk_request_elicitation_serialization() {
    let req = SdkRequest::Elicitation {
        mcp_server_name: "test-server".to_string(),
        message: "Please enter API key".to_string(),
        mode: Some(ElicitationMode::Form),
        url: None,
        elicitation_id: Some("elic-1".to_string()),
        requested_schema: None,
    };
    let json = serde_json::to_string(&req).unwrap();
    assert!(json.contains("\"subtype\":\"elicitation\""));
    assert!(json.contains("\"mode\":\"form\""));
}

// ---------------------------------------------------------------------------
// SdkResponse serialization
// ---------------------------------------------------------------------------

#[test]
fn test_sdk_response_success_serialization() {
    let resp = SdkResponse::Success {
        request_id: "req-1".to_string(),
        response: serde_json::json!({"models": []}),
    };
    let json = serde_json::to_string(&resp).unwrap();
    assert!(json.contains("\"subtype\":\"success\""));
    assert!(json.contains("\"request_id\":\"req-1\""));
}

#[test]
fn test_sdk_response_error_serialization() {
    let resp = SdkResponse::Error {
        request_id: "req-1".to_string(),
        error: "not found".to_string(),
    };
    let json = serde_json::to_string(&resp).unwrap();
    assert!(json.contains("\"subtype\":\"error\""));
    assert!(json.contains("\"error\":\"not found\""));
}

// ---------------------------------------------------------------------------
// Thinking config
// ---------------------------------------------------------------------------

#[test]
fn test_thinking_config_serialization() {
    let configs = [
        (ThinkingConfig::Adaptive, "\"type\":\"adaptive\""),
        (
            ThinkingConfig::Enabled {
                budget_tokens: Some(8000),
            },
            "\"type\":\"enabled\"",
        ),
        (ThinkingConfig::Disabled, "\"type\":\"disabled\""),
    ];

    for (config, expected_fragment) in configs {
        let json = serde_json::to_string(&config).unwrap();
        assert!(json.contains(expected_fragment), "json = {json}");

        let decoded: ThinkingConfig = serde_json::from_str(&json).unwrap();
        match (&config, &decoded) {
            (ThinkingConfig::Adaptive, ThinkingConfig::Adaptive) => {}
            (ThinkingConfig::Disabled, ThinkingConfig::Disabled) => {}
            (
                ThinkingConfig::Enabled { budget_tokens: a },
                ThinkingConfig::Enabled { budget_tokens: b },
            ) => assert_eq!(a, b),
            _ => panic!("mismatch"),
        }
    }
}

// ---------------------------------------------------------------------------
// Model usage
// ---------------------------------------------------------------------------

#[test]
fn test_model_usage_default_and_serialization() {
    let usage = ModelUsage::default();
    assert_eq!(usage.input_tokens, 0);
    assert_eq!(usage.output_tokens, 0);
    assert_eq!(usage.cost_usd, 0.0);

    let json = serde_json::to_string(&usage).unwrap();
    let decoded: ModelUsage = serde_json::from_str(&json).unwrap();
    assert_eq!(decoded.input_tokens, 0);
    assert_eq!(decoded.cost_usd, 0.0);
}

// ---------------------------------------------------------------------------
// Permission types
// ---------------------------------------------------------------------------

#[test]
fn test_sdk_permission_response_serialization() {
    let resp = SdkPermissionResponse::Allow;
    let json = serde_json::to_string(&resp).unwrap();
    assert!(json.contains("\"behavior\":\"allow\""));

    let resp2 = SdkPermissionResponse::Deny;
    let json2 = serde_json::to_string(&resp2).unwrap();
    assert!(json2.contains("\"behavior\":\"deny\""));

    let resp3 = SdkPermissionResponse::AllowAlways {
        scope: Some(PermissionScope::Session),
        updates: vec![],
    };
    let json3 = serde_json::to_string(&resp3).unwrap();
    assert!(json3.contains("\"behavior\":\"allow_always\""));
}

// ---------------------------------------------------------------------------
// Elicitation response
// ---------------------------------------------------------------------------

#[test]
fn test_elicitation_response_serialization() {
    let resp = ElicitationResponse {
        action: ElicitationAction::Accept,
        content: Some(HashMap::from([(
            "api_key".to_string(),
            serde_json::json!("sk-123"),
        )])),
    };
    let json = serde_json::to_string(&resp).unwrap();
    assert!(json.contains("\"action\":\"accept\""));
    assert!(json.contains("\"api_key\""));

    let resp2 = ElicitationResponse {
        action: ElicitationAction::Decline,
        content: None,
    };
    let json2 = serde_json::to_string(&resp2).unwrap();
    assert!(json2.contains("\"action\":\"decline\""));
    assert!(!json2.contains("content"));
}

// ---------------------------------------------------------------------------
// Hook events
// ---------------------------------------------------------------------------

#[test]
fn test_hook_event_serialization() {
    let event = HookEvent::PreToolUse;
    let json = serde_json::to_string(&event).unwrap();
    assert_eq!(json, "\"PreToolUse\"");

    let decoded: HookEvent = serde_json::from_str(&json).unwrap();
    assert_eq!(decoded, HookEvent::PreToolUse);
}

// ---------------------------------------------------------------------------
// Initialize response
// ---------------------------------------------------------------------------

#[test]
fn test_initialize_response_serialization() {
    let resp = InitializeResponse {
        commands: vec![SlashCommand {
            name: "commit".to_string(),
            description: "Create a git commit".to_string(),
            source: Some("built-in".to_string()),
        }],
        agents: vec![AgentInfo {
            name: "review".to_string(),
            description: Some("Code review agent".to_string()),
        }],
        output_style: "text".to_string(),
        available_output_styles: vec!["text".to_string(), "json".to_string()],
        models: vec![ModelInfo {
            id: "claude-opus-4".to_string(),
            name: "Claude Opus 4".to_string(),
            provider: Some("anthropic".to_string()),
        }],
        account: AccountInfo {
            account_type: Some("pro".to_string()),
            email: Some("user@example.com".to_string()),
        },
        pid: Some(12345),
    };

    let json = serde_json::to_string(&resp).unwrap();
    assert!(json.contains("\"output_style\":\"text\""));
    assert!(json.contains("\"name\":\"commit\""));

    let decoded: InitializeResponse = serde_json::from_str(&json).unwrap();
    assert_eq!(decoded.commands.len(), 1);
    assert_eq!(decoded.agents.len(), 1);
    assert_eq!(decoded.models.len(), 1);
    assert_eq!(decoded.pid, Some(12345));
}
