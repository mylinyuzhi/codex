use super::*;
use coco_types::ProviderApi;
use coco_types::ReasoningEffort;
use coco_types::ThinkingLevel;
use pretty_assertions::assert_eq;

#[test]
fn anthropic_thinking_uses_camelcase_budget_tokens() {
    let level = ThinkingLevel::with_budget(ReasoningEffort::High, 32_000);
    let out = to_extra_body(&level, ProviderApi::Anthropic);
    let thinking = out.get("thinking").unwrap();
    assert_eq!(
        thinking.get("type").and_then(serde_json::Value::as_str),
        Some("enabled")
    );
    assert_eq!(
        thinking
            .get("budgetTokens")
            .and_then(serde_json::Value::as_i64),
        Some(32_000)
    );
    assert!(
        thinking.get("budget_tokens").is_none(),
        "snake_case key must not appear in extra_body output"
    );
}

#[test]
fn renamed_anthropic_instance_still_emits_anthropic_shape() {
    // Routing key is ProviderApi family, not ProviderConfig.name. A
    // user-renamed instance (e.g., "azure-east" backed by
    // ProviderApi::Anthropic) MUST still get the typed Anthropic wire
    // body, not the OpenAI-compat fallback.
    let level = ThinkingLevel::with_budget(ReasoningEffort::High, 12_000);
    let out = to_extra_body(&level, ProviderApi::Anthropic);
    assert!(out.contains_key("thinking"));
    assert!(!out.contains_key("reasoningEffort"));
}

#[test]
fn openai_responses_uses_reasoning_summary_camelcase() {
    let level = ThinkingLevel::high();
    let out = to_extra_body(&level, ProviderApi::Openai);
    assert_eq!(
        out.get("reasoningSummary")
            .and_then(serde_json::Value::as_str),
        Some("auto")
    );
}

#[test]
fn google_thinking_config_uses_camelcase_keys() {
    let level = ThinkingLevel::with_budget(ReasoningEffort::Medium, 8_000);
    let out = to_extra_body(&level, ProviderApi::Gemini);
    let config = out.get("thinkingConfig").unwrap();
    assert_eq!(
        config
            .get("includeThoughts")
            .and_then(serde_json::Value::as_bool),
        Some(true)
    );
    assert_eq!(
        config
            .get("thinkingBudget")
            .and_then(serde_json::Value::as_i64),
        Some(8_000)
    );
}

#[test]
fn openai_compat_emits_reasoning_effort_camelcase() {
    let level = ThinkingLevel::high();
    let out = to_extra_body(&level, ProviderApi::OpenaiCompat);
    assert_eq!(
        out.get("reasoningEffort")
            .and_then(serde_json::Value::as_str),
        Some("high")
    );
}

#[test]
fn volcengine_uses_reasoning_effort_camelcase() {
    let level = ThinkingLevel::high();
    let out = to_extra_body(&level, ProviderApi::Volcengine);
    assert_eq!(
        out.get("reasoningEffort")
            .and_then(serde_json::Value::as_str),
        Some("high")
    );
}

#[test]
fn none_effort_returns_empty_map() {
    let level = ThinkingLevel::none();
    let out = to_extra_body(&level, ProviderApi::Anthropic);
    assert!(out.is_empty());
}

#[test]
fn level_options_are_passed_through() {
    let mut level = ThinkingLevel::high();
    level
        .options
        .insert("interleaved".into(), serde_json::Value::Bool(true));
    let out = to_extra_body(&level, ProviderApi::Anthropic);
    assert_eq!(
        out.get("interleaved").and_then(serde_json::Value::as_bool),
        Some(true)
    );
}
