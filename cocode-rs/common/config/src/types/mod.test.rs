use super::*;

#[test]
fn test_model_info_config_merge() {
    let mut base = ModelInfo {
        display_name: Some("Base Model".to_string()),
        context_window: Some(4096),
        max_output_tokens: Some(1024),
        capabilities: Some(vec![Capability::TextGeneration]),
        ..Default::default()
    };

    let override_cfg = ModelInfo {
        context_window: Some(8192),
        capabilities: Some(vec![
            Capability::TextGeneration,
            Capability::ParallelToolCalls,
        ]),
        ..Default::default()
    };

    base.merge_from(&override_cfg);

    assert_eq!(base.display_name, Some("Base Model".to_string())); // Not overridden
    assert_eq!(base.context_window, Some(8192)); // Overridden
    assert_eq!(base.max_output_tokens, Some(1024)); // Not overridden
    assert!(base.has_capability(Capability::ParallelToolCalls)); // New value
}

#[test]
fn test_provider_type_serde() {
    let pt = ProviderType::Anthropic;
    let json = serde_json::to_string(&pt).expect("serialize");
    assert_eq!(json, "\"anthropic\"");

    let parsed: ProviderType = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(parsed, ProviderType::Anthropic);
}

#[test]
fn test_models_file_from_vec() {
    // External config files use Vec format (array of models).
    // This tests the real config file format and add_models() method.
    let json = r#"[
        {
            "slug": "gpt-4o",
            "display_name": "GPT-4o",
            "context_window": 128000,
            "max_output_tokens": 16384,
            "capabilities": ["text_generation", "streaming", "vision"]
        }
    ]"#;

    let models: Vec<ModelInfo> = serde_json::from_str(json).expect("deserialize");
    let mut file = ModelsFile::default();
    file.add_models(models, "test.json").expect("add models");

    assert!(file.models.contains_key("gpt-4o"));
    let model = file.models.get("gpt-4o").expect("model exists");
    assert_eq!(model.display_name, Some("GPT-4o".to_string()));
    assert_eq!(model.context_window, Some(128000));
}

#[test]
fn test_providers_file_from_vec() {
    // External config files use Vec format (array of providers).
    // This tests the real config file format and add_providers() method.
    let json = r#"[
        {
            "name": "openai",
            "type": "openai",
            "env_key": "OPENAI_API_KEY",
            "base_url": "https://api.openai.com/v1",
            "models": []
        }
    ]"#;

    let providers: Vec<ProviderConfig> = serde_json::from_str(json).expect("deserialize");
    let mut file = ProvidersFile::default();
    file.add_providers(providers, "test.json")
        .expect("add providers");

    let provider = file.providers.get("openai").expect("provider exists");
    assert_eq!(provider.name, "openai");
    assert_eq!(provider.provider_type, ProviderType::Openai);
}

#[test]
fn test_provider_model_entry_serde() {
    let json = r#"{
        "slug": "deepseek-r1",
        "model_id": "ep-20250101-xxxxx",
        "timeout_secs": 300,
        "max_output_tokens": 16384,
        "default_thinking_level": {"effort": "high", "budget_tokens": 32000}
    }"#;

    let entry: ProviderModelEntry = serde_json::from_str(json).expect("deserialize");
    assert_eq!(entry.slug(), "deepseek-r1");
    assert_eq!(entry.model_alias, Some("ep-20250101-xxxxx".to_string()));
    assert_eq!(entry.model_info.timeout_secs, Some(300));
    assert_eq!(entry.model_info.max_output_tokens, Some(16384));
    let level = entry.model_info.default_thinking_level.unwrap();
    assert_eq!(level.budget_tokens, Some(32000));
}

#[test]
fn test_provider_model_entry_api_model_name() {
    let entry1 = ProviderModelEntry::new("gpt-5");
    assert_eq!(entry1.api_model_name(), "gpt-5");

    let entry2 = ProviderModelEntry::with_alias("deepseek-r1", "ep-xxxxx");
    assert_eq!(entry2.api_model_name(), "ep-xxxxx");
}

#[test]
fn test_provider_model_entry_empty_alias_falls_back() {
    let entry = ProviderModelEntry {
        model_info: ModelInfo {
            slug: "test-model".to_string(),
            ..Default::default()
        },
        model_alias: Some("".to_string()),
        model_options: HashMap::new(),
    };
    // Empty alias should fall back to slug
    assert_eq!(entry.api_model_name(), "test-model");
}

#[test]
fn test_wire_api_serde() {
    let api1 = WireApi::Responses;
    let json1 = serde_json::to_string(&api1).unwrap();
    assert_eq!(json1, "\"responses\"");

    let api2 = WireApi::Chat;
    let json2 = serde_json::to_string(&api2).unwrap();
    assert_eq!(json2, "\"chat\"");
}

#[test]
fn test_provider_config_with_models() {
    let json = r#"{
        "name": "Custom OpenAI",
        "type": "openai",
        "base_url": "https://api.openai.com/v1",
        "env_key": "OPENAI_API_KEY",
        "streaming": true,
        "wire_api": "chat",
        "models": [
            {"slug": "gpt-5"},
            {"slug": "gpt-4o", "timeout_secs": 120}
        ]
    }"#;

    let config: ProviderConfig = serde_json::from_str(json).expect("deserialize");
    assert_eq!(config.name, "Custom OpenAI");
    assert!(config.streaming);
    assert_eq!(config.wire_api, WireApi::Chat);
    assert_eq!(config.models.len(), 2);

    // Check model lookup
    let gpt5 = config.find_model("gpt-5").expect("gpt-5 exists");
    assert_eq!(gpt5.slug(), "gpt-5");

    let gpt4o = config.find_model("gpt-4o").expect("gpt-4o exists");
    assert_eq!(gpt4o.model_info.timeout_secs, Some(120));
}
