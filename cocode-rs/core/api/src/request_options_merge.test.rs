use super::*;

#[test]
fn test_merge_openai_seed() {
    let mut opts = HashMap::new();
    opts.insert("seed".to_string(), serde_json::json!(42));

    let result = merge_into_provider_options(None, &opts, ProviderType::Openai);
    let openai = downcast_options::<OpenAIOptions>(&result).unwrap();
    assert_eq!(openai.seed, Some(42));
}

#[test]
fn test_merge_openai_unknown_goes_to_extra() {
    let mut opts = HashMap::new();
    opts.insert("seed".to_string(), serde_json::json!(42));
    opts.insert("store".to_string(), serde_json::json!(true));

    let result = merge_into_provider_options(None, &opts, ProviderType::Openai);
    let openai = downcast_options::<OpenAIOptions>(&result).unwrap();
    assert_eq!(openai.seed, Some(42));
    assert_eq!(openai.extra.get("store"), Some(&serde_json::json!(true)));
}

#[test]
fn test_merge_preserves_existing_thinking_options() {
    // Simulate thinking_convert already set reasoning_effort
    let existing = OpenAIOptions::new()
        .with_reasoning_effort(hyper_sdk::options::openai::ReasoningEffort::High)
        .boxed();

    let mut opts = HashMap::new();
    opts.insert("seed".to_string(), serde_json::json!(42));

    let result = merge_into_provider_options(Some(existing), &opts, ProviderType::Openai);
    let openai = downcast_options::<OpenAIOptions>(&result).unwrap();

    // Thinking-derived field preserved
    assert_eq!(
        openai.reasoning_effort,
        Some(hyper_sdk::options::openai::ReasoningEffort::High)
    );
    // New field merged
    assert_eq!(openai.seed, Some(42));
}

#[test]
fn test_merge_anthropic_cache_control() {
    let mut opts = HashMap::new();
    opts.insert("cache_control".to_string(), serde_json::json!("ephemeral"));

    let result = merge_into_provider_options(None, &opts, ProviderType::Anthropic);
    let ant = downcast_options::<AnthropicOptions>(&result).unwrap();
    assert_eq!(
        ant.cache_control,
        Some(hyper_sdk::options::anthropic::CacheControl::Ephemeral)
    );
}

#[test]
fn test_merge_gemini_grounding() {
    let mut opts = HashMap::new();
    opts.insert("grounding".to_string(), serde_json::json!(true));

    let result = merge_into_provider_options(None, &opts, ProviderType::Gemini);
    let gem = downcast_options::<GeminiOptions>(&result).unwrap();
    assert_eq!(gem.grounding, Some(true));
}

#[test]
fn test_merge_volcengine_caching() {
    let mut opts = HashMap::new();
    opts.insert("caching_enabled".to_string(), serde_json::json!(true));

    let result = merge_into_provider_options(None, &opts, ProviderType::Volcengine);
    let volc = downcast_options::<VolcengineOptions>(&result).unwrap();
    assert_eq!(volc.caching_enabled, Some(true));
}

#[test]
fn test_merge_zai_options() {
    let mut opts = HashMap::new();
    opts.insert("do_sample".to_string(), serde_json::json!(true));
    opts.insert("custom_key".to_string(), serde_json::json!("value"));

    let result = merge_into_provider_options(None, &opts, ProviderType::Zai);
    let zai = downcast_options::<ZaiOptions>(&result).unwrap();
    assert_eq!(zai.do_sample, Some(true));
    assert_eq!(
        zai.extra.get("custom_key"),
        Some(&serde_json::json!("value"))
    );
}

#[test]
fn test_merge_does_not_overwrite_existing() {
    let existing = OpenAIOptions::new().with_seed(99).boxed();

    let mut opts = HashMap::new();
    opts.insert("seed".to_string(), serde_json::json!(42));

    let result = merge_into_provider_options(Some(existing), &opts, ProviderType::Openai);
    let openai = downcast_options::<OpenAIOptions>(&result).unwrap();
    // Existing value preserved
    assert_eq!(openai.seed, Some(99));
}
