use super::*;
use hyper_sdk::options::downcast_options;

fn default_model_info() -> ModelInfo {
    ModelInfo::default()
}

#[test]
fn test_to_anthropic_options_with_budget() {
    let level = ThinkingLevel::high().set_budget(32000);
    let model_info = default_model_info();
    let opts = to_provider_options(&level, &model_info, ProviderType::Anthropic);

    assert!(opts.is_some());
    let opts = opts.unwrap();
    let ant_opts = downcast_options::<AnthropicOptions>(&opts).unwrap();
    assert_eq!(ant_opts.thinking_budget_tokens, Some(32000));
}

#[test]
fn test_to_anthropic_options_no_budget() {
    // Anthropic requires budget_tokens, so effort alone returns None
    let level = ThinkingLevel::high();
    let model_info = default_model_info();
    let opts = to_provider_options(&level, &model_info, ProviderType::Anthropic);

    assert!(opts.is_none());
}

#[test]
fn test_to_openai_options_high() {
    let level = ThinkingLevel::high();
    let model_info = default_model_info();
    let opts = to_provider_options(&level, &model_info, ProviderType::Openai);

    assert!(opts.is_some());
    let opts = opts.unwrap();
    let openai_opts = downcast_options::<OpenAIOptions>(&opts).unwrap();
    assert_eq!(
        openai_opts.reasoning_effort,
        Some(hyper_sdk::options::openai::ReasoningEffort::High)
    );
    // Should always include encrypted content
    assert_eq!(openai_opts.include_encrypted_content, Some(true));
}

#[test]
fn test_to_openai_options_medium() {
    let level = ThinkingLevel::medium();
    let model_info = default_model_info();
    let opts = to_provider_options(&level, &model_info, ProviderType::Openai);

    assert!(opts.is_some());
    let opts = opts.unwrap();
    let openai_opts = downcast_options::<OpenAIOptions>(&opts).unwrap();
    assert_eq!(
        openai_opts.reasoning_effort,
        Some(hyper_sdk::options::openai::ReasoningEffort::Medium)
    );
}

#[test]
fn test_to_openai_options_low() {
    let level = ThinkingLevel::low();
    let model_info = default_model_info();
    let opts = to_provider_options(&level, &model_info, ProviderType::Openai);

    assert!(opts.is_some());
    let opts = opts.unwrap();
    let openai_opts = downcast_options::<OpenAIOptions>(&opts).unwrap();
    assert_eq!(
        openai_opts.reasoning_effort,
        Some(hyper_sdk::options::openai::ReasoningEffort::Low)
    );
}

#[test]
fn test_to_openai_options_none() {
    let level = ThinkingLevel::none();
    let model_info = default_model_info();
    let opts = to_provider_options(&level, &model_info, ProviderType::Openai);

    assert!(opts.is_none());
}

#[test]
fn test_to_openai_options_with_reasoning_summary() {
    let level = ThinkingLevel::high();
    let mut model_info = default_model_info();
    model_info.reasoning_summary = Some(ReasoningSummary::Detailed);

    let opts = to_provider_options(&level, &model_info, ProviderType::Openai);
    assert!(opts.is_some());
    let opts = opts.unwrap();
    let openai_opts = downcast_options::<OpenAIOptions>(&opts).unwrap();
    assert_eq!(
        openai_opts.reasoning_summary,
        Some(hyper_sdk::options::openai::ReasoningSummary::Detailed)
    );
}

#[test]
fn test_to_gemini_options_high() {
    let level = ThinkingLevel::high();
    let model_info = default_model_info();
    let opts = to_provider_options(&level, &model_info, ProviderType::Gemini);

    assert!(opts.is_some());
    let opts = opts.unwrap();
    let gem_opts = downcast_options::<GeminiOptions>(&opts).unwrap();
    assert_eq!(
        gem_opts.thinking_level,
        Some(hyper_sdk::options::gemini::ThinkingLevel::High)
    );
    // Default include_thoughts is true
    assert_eq!(gem_opts.include_thoughts, Some(true));
}

#[test]
fn test_to_gemini_options_none() {
    let level = ThinkingLevel::none();
    let model_info = default_model_info();
    let opts = to_provider_options(&level, &model_info, ProviderType::Gemini);

    assert!(opts.is_none());
}

#[test]
fn test_to_gemini_options_include_thoughts_false() {
    let level = ThinkingLevel::high();
    let mut model_info = default_model_info();
    model_info.include_thoughts = Some(false);

    let opts = to_provider_options(&level, &model_info, ProviderType::Gemini);
    assert!(opts.is_some());
    let opts = opts.unwrap();
    let gem_opts = downcast_options::<GeminiOptions>(&opts).unwrap();
    assert_eq!(gem_opts.include_thoughts, Some(false));
}

#[test]
fn test_to_volcengine_options_budget() {
    let level = ThinkingLevel::high().set_budget(16000);
    let model_info = default_model_info();
    let opts = to_provider_options(&level, &model_info, ProviderType::Volcengine);

    assert!(opts.is_some());
    let opts = opts.unwrap();
    let volc_opts = downcast_options::<VolcengineOptions>(&opts).unwrap();
    assert_eq!(volc_opts.thinking_budget_tokens, Some(16000));
    assert_eq!(
        volc_opts.reasoning_effort,
        Some(hyper_sdk::options::volcengine::ReasoningEffort::High)
    );
}

#[test]
fn test_to_volcengine_options_effort_only() {
    let level = ThinkingLevel::medium();
    let model_info = default_model_info();
    let opts = to_provider_options(&level, &model_info, ProviderType::Volcengine);

    assert!(opts.is_some());
    let opts = opts.unwrap();
    let volc_opts = downcast_options::<VolcengineOptions>(&opts).unwrap();
    assert!(volc_opts.thinking_budget_tokens.is_none());
    assert_eq!(
        volc_opts.reasoning_effort,
        Some(hyper_sdk::options::volcengine::ReasoningEffort::Medium)
    );
}

#[test]
fn test_to_zai_options_with_budget() {
    let level = ThinkingLevel::high().set_budget(8192);
    let model_info = default_model_info();
    let opts = to_provider_options(&level, &model_info, ProviderType::Zai);

    assert!(opts.is_some());
    let opts = opts.unwrap();
    let zai_opts = downcast_options::<ZaiOptions>(&opts).unwrap();
    assert_eq!(zai_opts.thinking_budget_tokens, Some(8192));
}

#[test]
fn test_to_zai_options_no_budget() {
    // Z.AI requires budget_tokens, so effort alone returns None
    let level = ThinkingLevel::high();
    let model_info = default_model_info();
    let opts = to_provider_options(&level, &model_info, ProviderType::Zai);

    assert!(opts.is_none());
}

#[test]
fn test_xhigh_maps_to_high() {
    let level = ThinkingLevel::xhigh();
    let model_info = default_model_info();

    // OpenAI: XHigh -> High
    let opts = to_provider_options(&level, &model_info, ProviderType::Openai).unwrap();
    let openai_opts = downcast_options::<OpenAIOptions>(&opts).unwrap();
    assert_eq!(
        openai_opts.reasoning_effort,
        Some(hyper_sdk::options::openai::ReasoningEffort::High)
    );

    // Gemini: XHigh -> High
    let opts = to_provider_options(&level, &model_info, ProviderType::Gemini).unwrap();
    let gem_opts = downcast_options::<GeminiOptions>(&opts).unwrap();
    assert_eq!(
        gem_opts.thinking_level,
        Some(hyper_sdk::options::gemini::ThinkingLevel::High)
    );
}

#[test]
fn test_minimal_maps_to_low() {
    let level = ThinkingLevel::new(ReasoningEffort::Minimal);
    let model_info = default_model_info();

    // OpenAI: Minimal -> Low
    let opts = to_provider_options(&level, &model_info, ProviderType::Openai).unwrap();
    let openai_opts = downcast_options::<OpenAIOptions>(&opts).unwrap();
    assert_eq!(
        openai_opts.reasoning_effort,
        Some(hyper_sdk::options::openai::ReasoningEffort::Low)
    );

    // Gemini: Minimal -> Low
    let opts = to_provider_options(&level, &model_info, ProviderType::Gemini).unwrap();
    let gem_opts = downcast_options::<GeminiOptions>(&opts).unwrap();
    assert_eq!(
        gem_opts.thinking_level,
        Some(hyper_sdk::options::gemini::ThinkingLevel::Low)
    );

    // Volcengine: Minimal is preserved
    let opts = to_provider_options(&level, &model_info, ProviderType::Volcengine).unwrap();
    let volc_opts = downcast_options::<VolcengineOptions>(&opts).unwrap();
    assert_eq!(
        volc_opts.reasoning_effort,
        Some(hyper_sdk::options::volcengine::ReasoningEffort::Minimal)
    );
}
