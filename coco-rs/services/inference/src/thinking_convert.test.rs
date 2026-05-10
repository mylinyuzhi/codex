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
fn disable_returns_empty_map_when_no_options() {
    let level = ThinkingLevel::disable();
    let out = to_extra_body(&level, ProviderApi::Anthropic);
    assert!(out.is_empty());
}

#[test]
fn auto_returns_empty_map_when_no_options() {
    let level = ThinkingLevel::auto();
    let out = to_extra_body(&level, ProviderApi::OpenaiCompat);
    assert!(
        out.is_empty(),
        "Auto with empty options means: provider decides; no fields emitted"
    );
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

#[test]
fn disable_effort_passes_options_through() {
    // DeepSeek V4 declares `{"thinking":{"type":"disabled"}}` in
    // `level.options` for its `Disable` level. The convert layer must
    // pass options through even when effort is Disable so the wire
    // toggle reaches the body.
    let mut level = ThinkingLevel::disable();
    level.options.insert(
        "thinking".into(),
        serde_json::json!({"type": "disabled"}),
    );
    let out = to_extra_body(&level, ProviderApi::OpenaiCompat);
    assert_eq!(
        out.get("thinking"),
        Some(&serde_json::json!({"type": "disabled"})),
    );
    // No `reasoningEffort` for Disable — typed-arm emission is gated on
    // `is_explicit_level()`, which Disable / Auto fail.
    assert!(
        !out.contains_key("reasoningEffort"),
        "Disable level must not emit reasoningEffort"
    );
}

#[test]
fn deepseek_medium_emits_thinking_enabled_and_reasoning_effort() {
    // Mirrors the registry surface: Medium effort + options carrying
    // the wire-enabled toggle. Output must contain BOTH the
    // `thinking` toggle (from level.options) AND the `reasoningEffort`
    // (from the OpenaiCompat default arm via `Display`).
    let mut level = ThinkingLevel {
        effort: ReasoningEffort::Medium,
        budget_tokens: None,
        options: std::collections::HashMap::new(),
    };
    level.options.insert(
        "thinking".into(),
        serde_json::json!({"type": "enabled"}),
    );
    let out = to_extra_body(&level, ProviderApi::OpenaiCompat);
    assert_eq!(
        out.get("thinking"),
        Some(&serde_json::json!({"type": "enabled"})),
    );
    assert_eq!(
        out.get("reasoningEffort")
            .and_then(serde_json::Value::as_str),
        Some("medium"),
    );
}

#[test]
fn deepseek_xhigh_emits_thinking_enabled_and_xhigh_reasoning_effort() {
    let mut level = ThinkingLevel {
        effort: ReasoningEffort::XHigh,
        budget_tokens: None,
        options: std::collections::HashMap::new(),
    };
    level.options.insert(
        "thinking".into(),
        serde_json::json!({"type": "enabled"}),
    );
    let out = to_extra_body(&level, ProviderApi::OpenaiCompat);
    assert_eq!(
        out.get("thinking"),
        Some(&serde_json::json!({"type": "enabled"})),
    );
    assert_eq!(
        out.get("reasoningEffort")
            .and_then(serde_json::Value::as_str),
        Some("xhigh"),
    );
}
