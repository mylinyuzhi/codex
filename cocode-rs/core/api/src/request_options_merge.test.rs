use super::*;

#[test]
fn test_merge_openai_all_keys_go_to_provider_entry() {
    let mut opts = HashMap::new();
    opts.insert("seed".to_string(), serde_json::json!(42));
    opts.insert("top_p".to_string(), serde_json::json!(0.9));
    opts.insert("store".to_string(), serde_json::json!(true));

    let result = merge_into_provider_options(None, &opts, ProviderApi::Openai);
    let openai = result.0.get("openai").expect("should have openai entry");

    assert_eq!(openai.get("seed"), Some(&serde_json::json!(42)));
    assert_eq!(openai.get("top_p"), Some(&serde_json::json!(0.9)));
    assert_eq!(openai.get("store"), Some(&serde_json::json!(true)));
}

#[test]
fn test_merge_preserves_existing_thinking_options() {
    // Simulate thinking_convert already set reasoning_effort
    let mut inner = HashMap::new();
    inner.insert("reasoning_effort".to_string(), serde_json::json!("high"));
    let mut existing_map = HashMap::new();
    existing_map.insert("openai".to_string(), inner);
    let existing = ProviderOptions::from_map(existing_map);

    let mut opts = HashMap::new();
    opts.insert("seed".to_string(), serde_json::json!(42));

    let result = merge_into_provider_options(Some(existing), &opts, ProviderApi::Openai);
    let openai = result.0.get("openai").expect("should have openai entry");

    // Thinking-derived option preserved
    assert_eq!(
        openai.get("reasoning_effort"),
        Some(&serde_json::json!("high"))
    );
    // New key merged in
    assert_eq!(openai.get("seed"), Some(&serde_json::json!(42)));
}

#[test]
fn test_merge_anthropic_all_keys_go_to_provider_entry() {
    let mut opts = HashMap::new();
    opts.insert("cache_control".to_string(), serde_json::json!("ephemeral"));
    opts.insert("temperature".to_string(), serde_json::json!(0.5));

    let result = merge_into_provider_options(None, &opts, ProviderApi::Anthropic);
    let ant = result
        .0
        .get("anthropic")
        .expect("should have anthropic entry");

    assert_eq!(
        ant.get("cache_control"),
        Some(&serde_json::json!("ephemeral"))
    );
    assert_eq!(ant.get("temperature"), Some(&serde_json::json!(0.5)));
}

#[test]
fn test_merge_gemini_all_keys_go_to_provider_entry() {
    let mut opts = HashMap::new();
    opts.insert("grounding".to_string(), serde_json::json!(true));
    opts.insert("top_p".to_string(), serde_json::json!(0.95));

    let result = merge_into_provider_options(None, &opts, ProviderApi::Gemini);
    let gem = result.0.get("google").expect("should have google entry");

    assert_eq!(gem.get("grounding"), Some(&serde_json::json!(true)));
    assert_eq!(gem.get("top_p"), Some(&serde_json::json!(0.95)));
}

#[test]
fn test_merge_volcengine_all_keys_go_to_provider_entry() {
    let mut opts = HashMap::new();
    opts.insert("caching_enabled".to_string(), serde_json::json!(true));
    opts.insert("seed".to_string(), serde_json::json!(7));

    let result = merge_into_provider_options(None, &opts, ProviderApi::Volcengine);
    let volc = result
        .0
        .get("volcengine")
        .expect("should have volcengine entry");

    assert_eq!(volc.get("caching_enabled"), Some(&serde_json::json!(true)));
    assert_eq!(volc.get("seed"), Some(&serde_json::json!(7)));
}

#[test]
fn test_merge_zai_all_keys_go_to_provider_entry() {
    let mut opts = HashMap::new();
    opts.insert("do_sample".to_string(), serde_json::json!(true));
    opts.insert("custom_key".to_string(), serde_json::json!("value"));

    let result = merge_into_provider_options(None, &opts, ProviderApi::Zai);
    let zai = result.0.get("zai").expect("should have zai entry");

    assert_eq!(zai.get("do_sample"), Some(&serde_json::json!(true)));
    assert_eq!(zai.get("custom_key"), Some(&serde_json::json!("value")));
}

#[test]
fn test_merge_openai_compat_uses_openai_path() {
    let mut opts = HashMap::new();
    opts.insert("seed".to_string(), serde_json::json!(42));

    let result = merge_into_provider_options(None, &opts, ProviderApi::OpenaiCompat);
    let openai = result.0.get("openai").expect("should have openai entry");
    assert_eq!(openai.get("seed"), Some(&serde_json::json!(42)));
}

#[test]
fn test_merge_empty_request_options() {
    let opts = HashMap::new();
    let result = merge_into_provider_options(None, &opts, ProviderApi::Openai);
    let openai = result.0.get("openai").expect("should have openai entry");
    assert!(openai.is_empty());
}

// =========================================================================
// P16: Provider base options
// =========================================================================

#[test]
fn test_provider_base_options_openai() {
    let opts = provider_base_options(ProviderApi::Openai).expect("OpenAI should have base opts");
    let openai = opts.0.get("openai").expect("should have openai entry");
    assert_eq!(openai.get("store"), Some(&serde_json::json!(false)));
}

#[test]
fn test_provider_base_options_openai_compat() {
    let opts = provider_base_options(ProviderApi::OpenaiCompat)
        .expect("OpenaiCompat should have base opts");
    let openai = opts.0.get("openai").expect("should have openai entry");
    assert_eq!(openai.get("store"), Some(&serde_json::json!(false)));
}

#[test]
fn test_provider_base_options_gemini() {
    let opts = provider_base_options(ProviderApi::Gemini).expect("Gemini should have base opts");
    let google = opts.0.get("google").expect("should have google entry");
    assert_eq!(
        google.get("thinkingConfig"),
        Some(&serde_json::json!({"includeThoughts": true}))
    );
}

#[test]
fn test_provider_base_options_anthropic_none() {
    assert!(provider_base_options(ProviderApi::Anthropic).is_none());
}

#[test]
fn test_provider_base_options_volcengine_none() {
    assert!(provider_base_options(ProviderApi::Volcengine).is_none());
}

#[test]
fn test_provider_base_options_zai_none() {
    assert!(provider_base_options(ProviderApi::Zai).is_none());
}
