use super::*;

#[test]
fn test_merge_openai_all_keys_go_to_extra() {
    let mut opts = HashMap::new();
    opts.insert("seed".to_string(), serde_json::json!(42));
    opts.insert("top_p".to_string(), serde_json::json!(0.9));
    opts.insert("store".to_string(), serde_json::json!(true));

    let result = merge_into_provider_options(None, &opts, ProviderType::Openai);
    let openai = downcast_options::<OpenAIOptions>(&result).unwrap();

    // ALL keys end up in extra — SDK's #[serde(flatten)] handles override
    assert_eq!(openai.extra.get("seed"), Some(&serde_json::json!(42)));
    assert_eq!(openai.extra.get("top_p"), Some(&serde_json::json!(0.9)));
    assert_eq!(openai.extra.get("store"), Some(&serde_json::json!(true)));

    // Typed fields are NOT set by this merge
    assert_eq!(openai.seed, None);
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

    // Thinking-derived typed field preserved
    assert_eq!(
        openai.reasoning_effort,
        Some(hyper_sdk::options::openai::ReasoningEffort::High)
    );
    // New key in extra
    assert_eq!(openai.extra.get("seed"), Some(&serde_json::json!(42)));
}

#[test]
fn test_merge_anthropic_all_keys_go_to_extra() {
    let mut opts = HashMap::new();
    opts.insert("cache_control".to_string(), serde_json::json!("ephemeral"));
    opts.insert("temperature".to_string(), serde_json::json!(0.5));

    let result = merge_into_provider_options(None, &opts, ProviderType::Anthropic);
    let ant = downcast_options::<AnthropicOptions>(&result).unwrap();

    // ALL keys in extra
    assert_eq!(
        ant.extra.get("cache_control"),
        Some(&serde_json::json!("ephemeral"))
    );
    assert_eq!(ant.extra.get("temperature"), Some(&serde_json::json!(0.5)));

    // Typed field NOT set
    assert_eq!(ant.cache_control, None);
}

#[test]
fn test_merge_gemini_all_keys_go_to_extra() {
    let mut opts = HashMap::new();
    opts.insert("grounding".to_string(), serde_json::json!(true));
    opts.insert("top_p".to_string(), serde_json::json!(0.95));

    let result = merge_into_provider_options(None, &opts, ProviderType::Gemini);
    let gem = downcast_options::<GeminiOptions>(&result).unwrap();

    assert_eq!(gem.extra.get("grounding"), Some(&serde_json::json!(true)));
    assert_eq!(gem.extra.get("top_p"), Some(&serde_json::json!(0.95)));
    // Typed field NOT set
    assert_eq!(gem.grounding, None);
}

#[test]
fn test_merge_volcengine_all_keys_go_to_extra() {
    let mut opts = HashMap::new();
    opts.insert("caching_enabled".to_string(), serde_json::json!(true));
    opts.insert("seed".to_string(), serde_json::json!(7));

    let result = merge_into_provider_options(None, &opts, ProviderType::Volcengine);
    let volc = downcast_options::<VolcengineOptions>(&result).unwrap();

    assert_eq!(
        volc.extra.get("caching_enabled"),
        Some(&serde_json::json!(true))
    );
    assert_eq!(volc.extra.get("seed"), Some(&serde_json::json!(7)));
    // Typed field NOT set
    assert_eq!(volc.caching_enabled, None);
}

#[test]
fn test_merge_zai_all_keys_go_to_extra() {
    let mut opts = HashMap::new();
    opts.insert("do_sample".to_string(), serde_json::json!(true));
    opts.insert("custom_key".to_string(), serde_json::json!("value"));

    let result = merge_into_provider_options(None, &opts, ProviderType::Zai);
    let zai = downcast_options::<ZaiOptions>(&result).unwrap();

    assert_eq!(zai.extra.get("do_sample"), Some(&serde_json::json!(true)));
    assert_eq!(
        zai.extra.get("custom_key"),
        Some(&serde_json::json!("value"))
    );
    // Typed field NOT set
    assert_eq!(zai.do_sample, None);
}

#[test]
fn test_merge_openai_compat_uses_openai_path() {
    let mut opts = HashMap::new();
    opts.insert("seed".to_string(), serde_json::json!(42));

    let result = merge_into_provider_options(None, &opts, ProviderType::OpenaiCompat);
    let openai = downcast_options::<OpenAIOptions>(&result).unwrap();
    assert_eq!(openai.extra.get("seed"), Some(&serde_json::json!(42)));
}

#[test]
fn test_merge_empty_request_options() {
    let opts = HashMap::new();
    let result = merge_into_provider_options(None, &opts, ProviderType::Openai);
    let openai = downcast_options::<OpenAIOptions>(&result).unwrap();
    assert!(openai.extra.is_empty());
}
