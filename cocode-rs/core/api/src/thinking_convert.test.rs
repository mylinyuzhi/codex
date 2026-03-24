use super::*;

fn default_model_info() -> ModelInfo {
    ModelInfo::default()
}

// =========================================================================
// effort_to_reasoning_level mapping
// =========================================================================

#[test]
fn test_effort_to_reasoning_level_all_variants() {
    assert_eq!(effort_to_reasoning_level(ReasoningEffort::None), None);
    assert_eq!(
        effort_to_reasoning_level(ReasoningEffort::Minimal),
        Some(ReasoningLevel::Minimal)
    );
    assert_eq!(
        effort_to_reasoning_level(ReasoningEffort::Low),
        Some(ReasoningLevel::Low)
    );
    assert_eq!(
        effort_to_reasoning_level(ReasoningEffort::Medium),
        Some(ReasoningLevel::Medium)
    );
    assert_eq!(
        effort_to_reasoning_level(ReasoningEffort::High),
        Some(ReasoningLevel::High)
    );
    assert_eq!(
        effort_to_reasoning_level(ReasoningEffort::XHigh),
        Some(ReasoningLevel::Xhigh)
    );
}

#[test]
fn test_to_anthropic_options_with_budget() {
    let level = ThinkingLevel::high().set_budget(32000);
    let model_info = default_model_info();
    let opts = to_provider_options(&level, &model_info, ProviderApi::Anthropic);

    assert!(opts.is_some());
    let opts = opts.unwrap();
    let ant = opts
        .0
        .get("anthropic")
        .expect("should have anthropic entry");
    let thinking = ant.get("thinking").expect("should have thinking key");
    assert_eq!(thinking["type"], serde_json::json!("enabled"));
    assert_eq!(thinking["budgetTokens"], serde_json::json!(32000));
}

#[test]
fn test_to_anthropic_options_adaptive_no_budget() {
    // Without budget_tokens, Anthropic should use Adaptive mode (for newer models)
    let level = ThinkingLevel::high();
    let model_info = default_model_info();
    let opts = to_provider_options(&level, &model_info, ProviderApi::Anthropic);

    assert!(opts.is_some());
    let opts = opts.unwrap();
    let ant = opts
        .0
        .get("anthropic")
        .expect("should have anthropic entry");
    let thinking = ant.get("thinking").expect("should have thinking key");
    assert_eq!(thinking["type"], serde_json::json!("adaptive"));
    // Adaptive mode has no budgetTokens field
    assert!(thinking.get("budgetTokens").is_none());
}

#[test]
fn test_to_openai_options_high() {
    let level = ThinkingLevel::high();
    let model_info = default_model_info();
    let opts = to_provider_options(&level, &model_info, ProviderApi::Openai);

    assert!(opts.is_some());
    let opts = opts.unwrap();
    let openai = opts.0.get("openai").expect("should have openai entry");
    assert_eq!(
        openai.get("reasoningEffort"),
        Some(&serde_json::json!("high"))
    );
}

#[test]
fn test_to_openai_options_medium() {
    let level = ThinkingLevel::medium();
    let model_info = default_model_info();
    let opts = to_provider_options(&level, &model_info, ProviderApi::Openai);

    assert!(opts.is_some());
    let opts = opts.unwrap();
    let openai = opts.0.get("openai").expect("should have openai entry");
    assert_eq!(
        openai.get("reasoningEffort"),
        Some(&serde_json::json!("medium"))
    );
}

#[test]
fn test_to_openai_options_low() {
    let level = ThinkingLevel::low();
    let model_info = default_model_info();
    let opts = to_provider_options(&level, &model_info, ProviderApi::Openai);

    assert!(opts.is_some());
    let opts = opts.unwrap();
    let openai = opts.0.get("openai").expect("should have openai entry");
    assert_eq!(
        openai.get("reasoningEffort"),
        Some(&serde_json::json!("low"))
    );
}

#[test]
fn test_to_openai_options_none() {
    let level = ThinkingLevel::none();
    let model_info = default_model_info();
    let opts = to_provider_options(&level, &model_info, ProviderApi::Openai);

    assert!(opts.is_none());
}

#[test]
fn test_to_openai_options_with_reasoning_summary() {
    let level = ThinkingLevel::high();
    let mut model_info = default_model_info();
    model_info.reasoning_summary = Some(ReasoningSummary::Detailed);

    let opts = to_provider_options(&level, &model_info, ProviderApi::Openai);
    assert!(opts.is_some());
    let opts = opts.unwrap();
    let openai = opts.0.get("openai").expect("should have openai entry");
    assert_eq!(
        openai.get("reasoningSummary"),
        Some(&serde_json::json!("detailed"))
    );
}

#[test]
fn test_to_gemini_options_high() {
    let level = ThinkingLevel::high();
    let model_info = default_model_info();
    let opts = to_provider_options(&level, &model_info, ProviderApi::Gemini);

    assert!(opts.is_some());
    let opts = opts.unwrap();
    let gem = opts.0.get("google").expect("should have google entry");
    let thinking_config = gem
        .get("thinkingConfig")
        .expect("should have thinkingConfig key");
    assert_eq!(thinking_config["thinkingLevel"], serde_json::json!("high"));
    // Default includeThoughts is true
    assert_eq!(thinking_config["includeThoughts"], serde_json::json!(true));
}

#[test]
fn test_to_gemini_options_none() {
    let level = ThinkingLevel::none();
    let model_info = default_model_info();
    let opts = to_provider_options(&level, &model_info, ProviderApi::Gemini);

    assert!(opts.is_none());
}

#[test]
fn test_to_gemini_options_include_thoughts_false() {
    let level = ThinkingLevel::high();
    let mut model_info = default_model_info();
    model_info.include_thoughts = Some(false);

    let opts = to_provider_options(&level, &model_info, ProviderApi::Gemini);
    assert!(opts.is_some());
    let opts = opts.unwrap();
    let gem = opts.0.get("google").expect("should have google entry");
    let thinking_config = gem
        .get("thinkingConfig")
        .expect("should have thinkingConfig key");
    assert_eq!(thinking_config["includeThoughts"], serde_json::json!(false));
}

#[test]
fn test_to_volcengine_options_budget() {
    let level = ThinkingLevel::high().set_budget(16000);
    let model_info = default_model_info();
    let opts = to_provider_options(&level, &model_info, ProviderApi::Volcengine);

    assert!(opts.is_some());
    let opts = opts.unwrap();
    let volc = opts
        .0
        .get("volcengine")
        .expect("should have volcengine entry");
    let thinking = volc.get("thinking").expect("should have thinking key");
    assert_eq!(thinking["budgetTokens"], serde_json::json!(16000));
    assert_eq!(
        volc.get("reasoningEffort"),
        Some(&serde_json::json!("high"))
    );
}

#[test]
fn test_to_volcengine_options_effort_only() {
    let level = ThinkingLevel::medium();
    let model_info = default_model_info();
    let opts = to_provider_options(&level, &model_info, ProviderApi::Volcengine);

    assert!(opts.is_some());
    let opts = opts.unwrap();
    let volc = opts
        .0
        .get("volcengine")
        .expect("should have volcengine entry");
    assert!(volc.get("thinking").is_none());
    assert_eq!(
        volc.get("reasoningEffort"),
        Some(&serde_json::json!("medium"))
    );
}

#[test]
fn test_to_zai_options_with_budget() {
    let level = ThinkingLevel::high().set_budget(8192);
    let model_info = default_model_info();
    let opts = to_provider_options(&level, &model_info, ProviderApi::Zai);

    assert!(opts.is_some());
    let opts = opts.unwrap();
    let zai = opts.0.get("zai").expect("should have zai entry");
    let thinking = zai.get("thinking").expect("should have thinking key");
    assert_eq!(thinking["budgetTokens"], serde_json::json!(8192));
}

#[test]
fn test_to_zai_options_no_budget() {
    // Z.AI requires budget_tokens, so effort alone returns None
    let level = ThinkingLevel::high();
    let model_info = default_model_info();
    let opts = to_provider_options(&level, &model_info, ProviderApi::Zai);

    assert!(opts.is_none());
}

#[test]
fn test_xhigh_maps_to_high() {
    let level = ThinkingLevel::xhigh();
    let model_info = default_model_info();

    // OpenAI: XHigh -> High
    let opts = to_provider_options(&level, &model_info, ProviderApi::Openai).unwrap();
    let openai = opts.0.get("openai").expect("should have openai entry");
    assert_eq!(
        openai.get("reasoningEffort"),
        Some(&serde_json::json!("high"))
    );

    // Gemini: XHigh -> High
    let opts = to_provider_options(&level, &model_info, ProviderApi::Gemini).unwrap();
    let gem = opts.0.get("google").expect("should have google entry");
    let thinking_config = gem
        .get("thinkingConfig")
        .expect("should have thinkingConfig key");
    assert_eq!(thinking_config["thinkingLevel"], serde_json::json!("high"));
}

#[test]
fn test_minimal_maps_to_low() {
    let level = ThinkingLevel::new(ReasoningEffort::Minimal);
    let model_info = default_model_info();

    // OpenAI: Minimal -> Low
    let opts = to_provider_options(&level, &model_info, ProviderApi::Openai).unwrap();
    let openai = opts.0.get("openai").expect("should have openai entry");
    assert_eq!(
        openai.get("reasoningEffort"),
        Some(&serde_json::json!("low"))
    );

    // Gemini: Minimal -> Low
    let opts = to_provider_options(&level, &model_info, ProviderApi::Gemini).unwrap();
    let gem = opts.0.get("google").expect("should have google entry");
    let thinking_config = gem
        .get("thinkingConfig")
        .expect("should have thinkingConfig key");
    assert_eq!(thinking_config["thinkingLevel"], serde_json::json!("low"));

    // Volcengine: Minimal is preserved
    let opts = to_provider_options(&level, &model_info, ProviderApi::Volcengine).unwrap();
    let volc = opts
        .0
        .get("volcengine")
        .expect("should have volcengine entry");
    assert_eq!(
        volc.get("reasoningEffort"),
        Some(&serde_json::json!("minimal"))
    );
}
