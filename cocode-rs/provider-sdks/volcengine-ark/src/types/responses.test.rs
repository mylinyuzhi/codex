use super::*;

#[test]
fn test_thinking_config() {
    let enabled = ThinkingConfig::enabled(2048);
    let json = serde_json::to_string(&enabled).unwrap();
    assert!(json.contains(r#""type":"enabled""#));
    assert!(json.contains(r#""budget_tokens":2048"#));

    let disabled = ThinkingConfig::disabled();
    let json = serde_json::to_string(&disabled).unwrap();
    assert!(json.contains(r#""type":"disabled""#));

    let auto = ThinkingConfig::auto();
    let json = serde_json::to_string(&auto).unwrap();
    assert!(json.contains(r#""type":"auto""#));
}

#[test]
fn test_thinking_config_checked() {
    assert!(ThinkingConfig::enabled_checked(1024).is_ok());
    assert!(ThinkingConfig::enabled_checked(2048).is_ok());
    assert!(ThinkingConfig::enabled_checked(1023).is_err());
    assert!(ThinkingConfig::enabled_checked(0).is_err());
}

#[test]
fn test_input_message() {
    let msg = InputMessage::user_text("Hello");
    assert_eq!(msg.role, Role::User);
    assert_eq!(msg.content.len(), 1);

    let msg = InputMessage::system("You are helpful");
    assert_eq!(msg.role, Role::System);
}

#[test]
fn test_response_create_params_builder() {
    let params = ResponseCreateParams::new("ep-xxx", vec![InputMessage::user_text("Hello")])
        .instructions("Be helpful")
        .max_output_tokens(1024)
        .temperature(0.7)
        .thinking(ThinkingConfig::enabled(2048));

    assert_eq!(params.model, "ep-xxx");
    assert_eq!(params.instructions, Some("Be helpful".to_string()));
    assert_eq!(params.max_output_tokens, Some(1024));
    assert_eq!(params.temperature, Some(0.7));
    assert!(params.thinking.is_some());
}

#[test]
fn test_temperature_checked() {
    let params = ResponseCreateParams::new("ep-xxx", vec![]);
    assert!(params.clone().temperature_checked(0.5).is_ok());
    assert!(params.clone().temperature_checked(0.0).is_ok());
    assert!(params.clone().temperature_checked(2.0).is_ok());
    assert!(params.clone().temperature_checked(-0.1).is_err());
    assert!(params.clone().temperature_checked(2.1).is_err());
}

#[test]
fn test_store_and_caching() {
    let params = ResponseCreateParams::new("ep-xxx", vec![])
        .store(true)
        .caching(CachingConfig {
            enabled: Some(true),
        });

    assert_eq!(params.store, Some(true));
    assert!(params.caching.is_some());
}

#[test]
fn test_reasoning_output_item() {
    let item = OutputItem::Reasoning {
        id: Some("r-1".to_string()),
        content: "Let me think...".to_string(),
        summary: Some(vec![ReasoningSummary::new("Summary")]),
        status: Some(ReasoningStatus::Completed),
    };
    let json = serde_json::to_string(&item).unwrap();
    assert!(json.contains(r#""type":"reasoning""#));
    assert!(json.contains(r#""content":"Let me think...""#));
    assert!(json.contains(r#""status":"completed""#));
}

#[test]
fn test_reasoning_status() {
    let status = ReasoningStatus::InProgress;
    let json = serde_json::to_string(&status).unwrap();
    assert_eq!(json, r#""in_progress""#);

    let status = ReasoningStatus::Completed;
    let json = serde_json::to_string(&status).unwrap();
    assert_eq!(json, r#""completed""#);

    let status = ReasoningStatus::Incomplete;
    let json = serde_json::to_string(&status).unwrap();
    assert_eq!(json, r#""incomplete""#);
}

#[test]
fn test_reasoning_summary() {
    let summary = ReasoningSummary::new("Test summary");
    assert_eq!(summary.text, "Test summary");
    assert_eq!(summary.summary_type, "summary_text");

    let json = serde_json::to_string(&summary).unwrap();
    assert!(json.contains(r#""text":"Test summary""#));
    assert!(json.contains(r#""type":"summary_text""#));
}

#[test]
fn test_reasoning_effort() {
    let effort = ReasoningEffort::High;
    let json = serde_json::to_string(&effort).unwrap();
    assert_eq!(json, r#""high""#);

    let effort = ReasoningEffort::Minimal;
    let json = serde_json::to_string(&effort).unwrap();
    assert_eq!(json, r#""minimal""#);
}

#[test]
fn test_response_create_params_with_reasoning_effort() {
    let params = ResponseCreateParams::new("ep-xxx", vec![])
        .reasoning_effort(ReasoningEffort::High)
        .thinking(ThinkingConfig::auto());

    assert!(params.reasoning_effort.is_some());
    assert!(params.thinking.is_some());
}

#[test]
fn test_response_status_incomplete() {
    use super::super::ResponseStatus;
    let json = r#""incomplete""#;
    let status: ResponseStatus = serde_json::from_str(json).unwrap();
    assert_eq!(status, ResponseStatus::Incomplete);
}
