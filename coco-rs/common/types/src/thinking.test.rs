use pretty_assertions::assert_eq;

use super::*;

#[test]
fn test_thinking_level_none() {
    let level = ThinkingLevel::none();
    assert_eq!(level.effort, ReasoningEffort::None);
    assert!(!level.is_enabled());
}

#[test]
fn test_thinking_level_high() {
    let level = ThinkingLevel::high();
    assert_eq!(level.effort, ReasoningEffort::High);
    assert!(level.is_enabled());
    assert!(level.budget_tokens.is_none());
}

#[test]
fn test_thinking_level_with_budget() {
    let level = ThinkingLevel::with_budget(ReasoningEffort::Medium, 32000);
    assert_eq!(level.effort, ReasoningEffort::Medium);
    assert_eq!(level.budget_tokens, Some(32000));
}

#[test]
fn test_reasoning_effort_ordering() {
    assert!(ReasoningEffort::None < ReasoningEffort::Low);
    assert!(ReasoningEffort::Low < ReasoningEffort::Medium);
    assert!(ReasoningEffort::Medium < ReasoningEffort::High);
    assert!(ReasoningEffort::High < ReasoningEffort::XHigh);
}

#[test]
fn test_thinking_level_from_str() {
    let level: ThinkingLevel = "high".parse().unwrap();
    assert_eq!(level.effort, ReasoningEffort::High);

    let level: ThinkingLevel = "none".parse().unwrap();
    assert!(!level.is_enabled());
}

#[test]
fn test_reasoning_effort_from_str_aliases() {
    assert_eq!(
        "max".parse::<ReasoningEffort>().unwrap(),
        ReasoningEffort::XHigh
    );
    assert_eq!(
        "xhigh".parse::<ReasoningEffort>().unwrap(),
        ReasoningEffort::XHigh
    );
    assert_eq!(
        "x_high".parse::<ReasoningEffort>().unwrap(),
        ReasoningEffort::XHigh
    );
}

#[test]
fn test_thinking_level_serde_roundtrip() {
    let level = ThinkingLevel::with_budget(ReasoningEffort::High, 16000);
    let json = serde_json::to_string(&level).unwrap();
    let parsed: ThinkingLevel = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, level);
}
