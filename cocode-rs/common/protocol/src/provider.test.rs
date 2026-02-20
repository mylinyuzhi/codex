use super::*;

#[test]
fn test_provider_type_default() {
    assert_eq!(ProviderType::default(), ProviderType::Openai);
}

#[test]
fn test_provider_type_display() {
    assert_eq!(ProviderType::Openai.to_string(), "openai");
    assert_eq!(ProviderType::Anthropic.to_string(), "anthropic");
    assert_eq!(ProviderType::Gemini.to_string(), "gemini");
    assert_eq!(ProviderType::Volcengine.to_string(), "volcengine");
    assert_eq!(ProviderType::Zai.to_string(), "zai");
    assert_eq!(ProviderType::OpenaiCompat.to_string(), "openai_compat");
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
fn test_wire_api_default() {
    assert_eq!(WireApi::default(), WireApi::Responses);
}

#[test]
fn test_wire_api_display() {
    assert_eq!(WireApi::Responses.to_string(), "responses");
    assert_eq!(WireApi::Chat.to_string(), "chat");
}

#[test]
fn test_wire_api_serde() {
    let api = WireApi::Chat;
    let json = serde_json::to_string(&api).expect("serialize");
    assert_eq!(json, "\"chat\"");

    let parsed: WireApi = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(parsed, WireApi::Chat);
}

#[test]
fn test_provider_info_new() {
    let info = ProviderInfo::new("OpenAI", ProviderType::Openai, "https://api.openai.com/v1");
    assert_eq!(info.name, "OpenAI");
    assert_eq!(info.provider_type, ProviderType::Openai);
    assert_eq!(info.base_url, "https://api.openai.com/v1");
    assert_eq!(info.timeout_secs, 600);
    assert!(info.streaming);
    assert_eq!(info.wire_api, WireApi::Responses);
    assert!(info.models.is_empty());
    assert!(!info.has_api_key());
}

#[test]
fn test_provider_info_builders() {
    let model = ModelInfo {
        slug: "gpt-4".to_string(),
        timeout_secs: Some(120),
        ..Default::default()
    };

    let info = ProviderInfo::new("Test", ProviderType::Openai, "https://api.test.com")
        .with_api_key("sk-test-key")
        .with_timeout(300)
        .with_streaming(false)
        .with_wire_api(WireApi::Chat)
        .with_model("gpt-4", model);

    assert_eq!(info.api_key, "sk-test-key");
    assert_eq!(info.timeout_secs, 300);
    assert!(!info.streaming);
    assert_eq!(info.wire_api, WireApi::Chat);
    assert!(info.has_api_key());
    assert!(info.get_model("gpt-4").is_some());
}

#[test]
fn test_provider_info_model_methods() {
    let model1 = ModelInfo {
        slug: "model-1".to_string(),
        timeout_secs: Some(120),
        ..Default::default()
    };
    let model2 = ModelInfo {
        slug: "model-2".to_string(),
        ..Default::default()
    };

    let info = ProviderInfo::new("Test", ProviderType::Openai, "https://api.test.com")
        .with_timeout(600)
        .with_model("model-1", model1)
        .with_model("model-2", model2);

    // get_model
    assert!(info.get_model("model-1").is_some());
    assert!(info.get_model("model-2").is_some());
    assert!(info.get_model("nonexistent").is_none());

    // model_slugs
    let slugs = info.model_slugs();
    assert_eq!(slugs.len(), 2);
    assert!(slugs.contains(&"model-1"));
    assert!(slugs.contains(&"model-2"));

    // effective_timeout
    assert_eq!(info.effective_timeout("model-1"), 120); // Model-specific
    assert_eq!(info.effective_timeout("model-2"), 600); // Provider default
    assert_eq!(info.effective_timeout("nonexistent"), 600); // Provider default
}

#[test]
fn test_provider_info_serde() {
    let info = ProviderInfo::new("Test", ProviderType::Anthropic, "https://api.anthropic.com")
        .with_api_key("test-key")
        .with_streaming(true)
        .with_wire_api(WireApi::Chat);

    let json = serde_json::to_string(&info).expect("serialize");
    assert!(json.contains("\"name\":\"Test\""));
    assert!(json.contains("\"type\":\"anthropic\""));
    assert!(json.contains("\"base_url\":\"https://api.anthropic.com\""));
    assert!(json.contains("\"streaming\":true"));
    assert!(json.contains("\"wire_api\":\"chat\""));

    let parsed: ProviderInfo = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(parsed, info);
}

#[test]
fn test_provider_info_serde_defaults() {
    let json = r#"{
            "name": "Test",
            "type": "openai",
            "base_url": "https://api.test.com"
        }"#;
    let info: ProviderInfo = serde_json::from_str(json).expect("deserialize");
    assert_eq!(info.timeout_secs, 600);
    assert!(info.streaming);
    assert_eq!(info.wire_api, WireApi::Responses);
    assert!(info.models.is_empty());
}

#[test]
fn test_provider_info_equality() {
    let info1 = ProviderInfo::new("Test", ProviderType::Openai, "https://api.test.com")
        .with_api_key("key1");
    let info2 = ProviderInfo::new("Test", ProviderType::Openai, "https://api.test.com")
        .with_api_key("key1");
    let info3 = ProviderInfo::new("Test", ProviderType::Openai, "https://api.test.com")
        .with_api_key("key2");

    assert_eq!(info1, info2);
    assert_ne!(info1, info3);
}

#[test]
fn test_provider_model_new() {
    let model_info = ModelInfo {
        slug: "gpt-4".to_string(),
        timeout_secs: Some(120),
        ..Default::default()
    };
    let pm = ProviderModel::new(model_info);

    assert_eq!(pm.slug(), "gpt-4");
    assert_eq!(pm.api_model_name(), "gpt-4"); // No alias, returns slug
    assert!(pm.model_alias.is_none());
    assert_eq!(pm.info.timeout_secs, Some(120));
}

#[test]
fn test_provider_model_with_alias() {
    let model_info = ModelInfo {
        slug: "deepseek-r1".to_string(),
        ..Default::default()
    };
    let pm = ProviderModel::with_alias(model_info, "ep-20250101-xxxxx");

    assert_eq!(pm.slug(), "deepseek-r1");
    assert_eq!(pm.api_model_name(), "ep-20250101-xxxxx"); // Returns alias
    assert_eq!(pm.model_alias, Some("ep-20250101-xxxxx".to_string()));
}

#[test]
fn test_provider_info_api_model_name() {
    let model1 = ModelInfo {
        slug: "model-1".to_string(),
        ..Default::default()
    };
    let model2 = ModelInfo {
        slug: "model-2".to_string(),
        ..Default::default()
    };

    let info = ProviderInfo::new("Test", ProviderType::Openai, "https://api.test.com")
        .with_model("model-1", model1)
        .with_model_aliased("model-2", model2, "endpoint-xxx");

    // api_model_name returns slug if no alias
    assert_eq!(info.api_model_name("model-1"), Some("model-1"));
    // api_model_name returns alias if set
    assert_eq!(info.api_model_name("model-2"), Some("endpoint-xxx"));
    // api_model_name returns None for unknown model
    assert_eq!(info.api_model_name("unknown"), None);
}

#[test]
fn test_provider_model_empty_alias_falls_back_to_slug() {
    let model_info = ModelInfo {
        slug: "gpt-4".to_string(),
        ..Default::default()
    };
    // Create with empty string alias
    let pm = ProviderModel {
        info: model_info,
        model_alias: Some("".to_string()),
    };
    // Should fall back to slug, not return empty string
    assert_eq!(pm.api_model_name(), "gpt-4");
}

#[test]
fn test_provider_model_serde() {
    let model_info = ModelInfo {
        slug: "test-model".to_string(),
        timeout_secs: Some(120),
        ..Default::default()
    };
    let pm = ProviderModel::with_alias(model_info, "ep-xxx");

    let json = serde_json::to_string(&pm).expect("serialize");
    assert!(json.contains("\"slug\":\"test-model\""));
    assert!(json.contains("\"model_alias\":\"ep-xxx\""));

    let parsed: ProviderModel = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(parsed.slug(), "test-model");
    assert_eq!(parsed.api_model_name(), "ep-xxx");
}
