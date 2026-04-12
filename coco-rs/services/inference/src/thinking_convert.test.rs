use coco_types::ProviderApi;
use coco_types::ReasoningEffort;
use coco_types::ThinkingLevel;
use pretty_assertions::assert_eq;

use super::*;

#[test]
fn test_anthropic_thinking_with_budget() {
    let level = ThinkingLevel::with_budget(ReasoningEffort::High, 32000);
    let opts = convert_thinking_level(&level, ProviderApi::Anthropic);

    let thinking = opts.get("thinking").unwrap().as_object().unwrap();
    assert_eq!(thinking["type"], "enabled");
    assert_eq!(thinking["budget_tokens"], 32000);
}

#[test]
fn test_none_effort_returns_empty() {
    let level = ThinkingLevel::none();
    let opts = convert_thinking_level(&level, ProviderApi::Anthropic);
    assert!(opts.is_empty());
}

#[test]
fn test_openai_reasoning_effort() {
    let level = ThinkingLevel::high();
    let opts = convert_thinking_level(&level, ProviderApi::Openai);
    assert_eq!(opts["reasoning_effort"], "high");
}

#[test]
fn test_options_passthrough() {
    let mut level = ThinkingLevel::high();
    level
        .options
        .insert("interleaved".into(), serde_json::Value::Bool(true));
    let opts = convert_thinking_level(&level, ProviderApi::Anthropic);
    assert_eq!(opts["interleaved"], true);
}
