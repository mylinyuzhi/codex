//! Unit tests for config loading.

use super::*;

#[test]
fn test_env_loading_does_not_panic() {
    ensure_env_loaded();
}

#[test]
fn test_load_returns_none_for_unconfigured() {
    let result = load_provider_config("totally_fake_provider_xyz");
    assert!(result.is_none());
}

#[test]
fn test_parse_capabilities_basic() {
    let caps = parse_capabilities("text,streaming,tools");
    assert!(caps.contains("text"));
    assert!(caps.contains("streaming"));
    assert!(caps.contains("tools"));
    assert_eq!(caps.len(), 3);
}

#[test]
fn test_parse_capabilities_none() {
    assert!(parse_capabilities("none").is_empty());
    assert!(parse_capabilities("").is_empty());
}

#[test]
fn test_parse_capabilities_whitespace() {
    let caps = parse_capabilities(" text , streaming ");
    assert!(caps.contains("text"));
    assert!(caps.contains("streaming"));
}

#[test]
fn test_provider_type_mapping() {
    assert_eq!(provider_type_for("openai"), ProviderType::Openai);
    assert_eq!(provider_type_for("openai_chat"), ProviderType::Openai);
    assert_eq!(provider_type_for("anthropic"), ProviderType::Anthropic);
    assert_eq!(provider_type_for("gemini"), ProviderType::Gemini);
    assert_eq!(provider_type_for("volcengine"), ProviderType::Volcengine);
    assert_eq!(provider_type_for("zai"), ProviderType::Zai);
    assert_eq!(
        provider_type_for("openai_compat"),
        ProviderType::OpenaiCompat
    );
    assert_eq!(provider_type_for("custom"), ProviderType::OpenaiCompat);
}

#[test]
fn test_config_provider_name_mapping() {
    assert_eq!(config_provider_name("openai_chat"), "openai");
    assert_eq!(config_provider_name("openai"), "openai");
    assert_eq!(config_provider_name("anthropic"), "anthropic");
}

#[test]
fn test_json_config_parsing() {
    let json = r#"{
        "name": "test",
        "type": "openai",
        "base_url": "https://api.example.com",
        "api_key": "sk-test"
    }"#;
    let caps = ALL_CAPABILITIES.iter().map(|s| (*s).to_string()).collect();
    let cfg = load_from_json("test_provider", json, caps).unwrap();
    assert_eq!(cfg.provider, "test_provider");
    assert_eq!(cfg.provider_info.base_url, "https://api.example.com");
    assert_eq!(cfg.provider_info.api_key, "sk-test");
    assert!(!cfg.enabled); // no model slug
}
