use super::*;

#[test]
fn test_thinking_level_new() {
    let level = ThinkingLevel::new(ReasoningEffort::High);
    assert_eq!(level.effort, ReasoningEffort::High);
    assert!(level.budget_tokens.is_none());
    assert!(level.max_output_tokens.is_none());
    assert!(!level.interleaved);
}

#[test]
fn test_thinking_level_with_budget() {
    let level = ThinkingLevel::with_budget(ReasoningEffort::High, 32000);
    assert_eq!(level.effort, ReasoningEffort::High);
    assert_eq!(level.budget_tokens, Some(32000));
}

#[test]
fn test_thinking_level_convenience_constructors() {
    assert_eq!(ThinkingLevel::none().effort, ReasoningEffort::None);
    assert_eq!(ThinkingLevel::low().effort, ReasoningEffort::Low);
    assert_eq!(ThinkingLevel::medium().effort, ReasoningEffort::Medium);
    assert_eq!(ThinkingLevel::high().effort, ReasoningEffort::High);
}

#[test]
fn test_thinking_level_is_enabled() {
    assert!(!ThinkingLevel::none().is_enabled());
    assert!(ThinkingLevel::low().is_enabled());
    assert!(ThinkingLevel::medium().is_enabled());
    assert!(ThinkingLevel::high().is_enabled());
}

#[test]
fn test_thinking_level_default() {
    let level = ThinkingLevel::default();
    assert_eq!(level.effort, ReasoningEffort::None);
    assert!(!level.is_enabled());
}

#[test]
fn test_thinking_level_effort_ordering() {
    // Ordering is done via the effort field (ReasoningEffort implements Ord)
    assert!(ThinkingLevel::none().effort < ThinkingLevel::low().effort);
    assert!(ThinkingLevel::low().effort < ThinkingLevel::medium().effort);
    assert!(ThinkingLevel::medium().effort < ThinkingLevel::high().effort);

    // Same effort, different budgets -> PartialEq compares all fields
    let high_no_budget = ThinkingLevel::high();
    let high_with_budget = ThinkingLevel::with_budget(ReasoningEffort::High, 32000);
    assert_eq!(high_no_budget.effort, high_with_budget.effort);
    assert_ne!(high_no_budget, high_with_budget); // Different structs

    // Effort comparison ignores budget
    let medium_with_huge_budget = ThinkingLevel::with_budget(ReasoningEffort::Medium, 100000);
    assert!(medium_with_huge_budget.effort < high_no_budget.effort);
}

#[test]
fn test_thinking_level_from_str() {
    assert_eq!(
        "none".parse::<ThinkingLevel>().unwrap().effort,
        ReasoningEffort::None
    );
    assert_eq!(
        "low".parse::<ThinkingLevel>().unwrap().effort,
        ReasoningEffort::Low
    );
    assert_eq!(
        "medium".parse::<ThinkingLevel>().unwrap().effort,
        ReasoningEffort::Medium
    );
    assert_eq!(
        "high".parse::<ThinkingLevel>().unwrap().effort,
        ReasoningEffort::High
    );
    assert_eq!(
        "xhigh".parse::<ThinkingLevel>().unwrap().effort,
        ReasoningEffort::XHigh
    );
    assert!("invalid".parse::<ThinkingLevel>().is_err());
}

#[test]
fn test_thinking_level_serde_string() {
    // Deserialize from string
    let level: ThinkingLevel = serde_json::from_str("\"high\"").unwrap();
    assert_eq!(level.effort, ReasoningEffort::High);
    assert!(level.budget_tokens.is_none());

    // Serialize simple level as string
    let json = serde_json::to_string(&ThinkingLevel::high()).unwrap();
    assert_eq!(json, "\"high\"");
}

#[test]
fn test_thinking_level_serde_object() {
    // Deserialize from object
    let json = r#"{
            "effort": "high",
            "budget_tokens": 32000,
            "interleaved": true
        }"#;
    let level: ThinkingLevel = serde_json::from_str(json).unwrap();
    assert_eq!(level.effort, ReasoningEffort::High);
    assert_eq!(level.budget_tokens, Some(32000));
    assert!(level.interleaved);

    // Serialize complex level as object
    let level = ThinkingLevel::with_budget(ReasoningEffort::High, 32000).set_interleaved(true);
    let json = serde_json::to_string(&level).unwrap();
    assert!(json.contains("\"effort\""));
    assert!(json.contains("\"budget_tokens\""));
}

#[test]
fn test_thinking_level_serde_object_defaults() {
    // Minimal object with just effort
    let json = r#"{"effort": "medium"}"#;
    let level: ThinkingLevel = serde_json::from_str(json).unwrap();
    assert_eq!(level.effort, ReasoningEffort::Medium);
    assert!(level.budget_tokens.is_none());
    assert!(!level.interleaved);
}

#[test]
fn test_thinking_level_serde_roundtrip() {
    let level = ThinkingLevel {
        effort: ReasoningEffort::High,
        budget_tokens: Some(32000),
        max_output_tokens: Some(16000),
        interleaved: true,
    };

    let json = serde_json::to_string(&level).unwrap();
    let parsed: ThinkingLevel = serde_json::from_str(&json).unwrap();
    assert_eq!(level, parsed);
}

#[test]
fn test_thinking_level_validate() {
    let level = ThinkingLevel::with_budget(ReasoningEffort::High, 32000);
    assert!(level.validate().is_ok());

    let level = ThinkingLevel {
        budget_tokens: Some(-1),
        ..Default::default()
    };
    assert!(level.validate().is_err());

    let level = ThinkingLevel {
        max_output_tokens: Some(-1),
        ..Default::default()
    };
    assert!(level.validate().is_err());
}

#[test]
fn test_thinking_level_builder_methods() {
    let level = ThinkingLevel::high()
        .set_budget(32000)
        .set_max_output_tokens(16000)
        .set_interleaved(true);

    assert_eq!(level.effort, ReasoningEffort::High);
    assert_eq!(level.budget_tokens, Some(32000));
    assert_eq!(level.max_output_tokens, Some(16000));
    assert!(level.interleaved);
}

#[test]
fn test_thinking_level_display() {
    assert_eq!(ThinkingLevel::none().to_string(), "none");
    assert_eq!(ThinkingLevel::low().to_string(), "low");
    assert_eq!(ThinkingLevel::medium().to_string(), "medium");
    assert_eq!(ThinkingLevel::high().to_string(), "high");
}
