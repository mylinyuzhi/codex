use super::*;

#[test]
fn test_resolve_provider_type() {
    assert_eq!(resolve_provider_type("anthropic"), ProviderType::Anthropic);
    assert_eq!(resolve_provider_type("Anthropic"), ProviderType::Anthropic);
    assert_eq!(resolve_provider_type("openai"), ProviderType::Openai);
    assert_eq!(resolve_provider_type("OpenAI"), ProviderType::Openai);
    assert_eq!(resolve_provider_type("gemini"), ProviderType::Gemini);
    assert_eq!(resolve_provider_type("genai"), ProviderType::Gemini);
    assert_eq!(resolve_provider_type("google"), ProviderType::Gemini);
    assert_eq!(
        resolve_provider_type("volcengine"),
        ProviderType::Volcengine
    );
    assert_eq!(resolve_provider_type("ark"), ProviderType::Volcengine);
    assert_eq!(resolve_provider_type("zai"), ProviderType::Zai);
    assert_eq!(resolve_provider_type("zhipu"), ProviderType::Zai);
    assert_eq!(
        resolve_provider_type("openai_compat"),
        ProviderType::OpenaiCompat
    );
    assert_eq!(
        resolve_provider_type("openai-compat"),
        ProviderType::OpenaiCompat
    );
    // Unknown providers default to OpenaiCompat
    assert_eq!(resolve_provider_type("unknown"), ProviderType::OpenaiCompat);
    assert_eq!(
        resolve_provider_type("custom-provider"),
        ProviderType::OpenaiCompat
    );
}

#[test]
fn test_parse_valid() {
    let spec: ModelSpec = "anthropic/claude-opus-4".parse().unwrap();
    assert_eq!(spec.provider, "anthropic");
    assert_eq!(spec.model, "claude-opus-4");
    assert_eq!(spec.provider_type, ProviderType::Anthropic);
}

#[test]
fn test_parse_with_slashes_in_model() {
    // Model names can contain slashes (e.g., "accounts/fireworks/models/llama-v3")
    let spec: ModelSpec = "fireworks/accounts/fireworks/models/llama-v3"
        .parse()
        .unwrap();
    assert_eq!(spec.provider, "fireworks");
    assert_eq!(spec.model, "accounts/fireworks/models/llama-v3");
    // Unknown provider defaults to OpenaiCompat
    assert_eq!(spec.provider_type, ProviderType::OpenaiCompat);
}

#[test]
fn test_parse_invalid_no_slash() {
    let result: Result<ModelSpec, _> = "claude-opus-4".parse();
    assert!(result.is_err());
    assert!(result.unwrap_err().0.contains("invalid format"));
}

#[test]
fn test_parse_invalid_empty_provider() {
    let result: Result<ModelSpec, _> = "/claude-opus-4".parse();
    assert!(result.is_err());
}

#[test]
fn test_parse_invalid_empty_model() {
    let result: Result<ModelSpec, _> = "anthropic/".parse();
    assert!(result.is_err());
}

#[test]
fn test_new_auto_resolves_provider_type() {
    let spec = ModelSpec::new("openai", "gpt-5");
    assert_eq!(spec.provider, "openai");
    assert_eq!(spec.model, "gpt-5");
    assert_eq!(spec.provider_type, ProviderType::Openai);

    let spec = ModelSpec::new("gemini", "gemini-2.0-flash");
    assert_eq!(spec.provider_type, ProviderType::Gemini);
}

#[test]
fn test_with_type_explicit() {
    // Create with explicit provider type (even if it doesn't match the name)
    let spec = ModelSpec::with_type("my-custom-anthropic", ProviderType::Anthropic, "model-x");
    assert_eq!(spec.provider, "my-custom-anthropic");
    assert_eq!(spec.model, "model-x");
    assert_eq!(spec.provider_type, ProviderType::Anthropic);
}

#[test]
fn test_display() {
    let spec = ModelSpec::new("openai", "gpt-5");
    assert_eq!(spec.to_string(), "openai/gpt-5");
}

#[test]
fn test_serde_roundtrip() {
    let spec = ModelSpec::new("anthropic", "claude-opus-4");
    let json = serde_json::to_string(&spec).unwrap();
    assert_eq!(json, r#""anthropic/claude-opus-4""#);

    let parsed: ModelSpec = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, spec);
}

#[test]
fn test_serde_deserialize_invalid() {
    let result: Result<ModelSpec, _> = serde_json::from_str(r#""invalid""#);
    assert!(result.is_err());
}

#[test]
fn test_equality() {
    let a = ModelSpec::new("anthropic", "claude-opus-4");
    let b = ModelSpec::new("anthropic", "claude-opus-4");
    let c = ModelSpec::new("openai", "gpt-5");
    assert_eq!(a, b);
    assert_ne!(a, c);
}

#[test]
fn test_hash() {
    use std::collections::HashSet;

    let mut set = HashSet::new();
    set.insert(ModelSpec::new("anthropic", "claude-opus-4"));
    set.insert(ModelSpec::new("openai", "gpt-5"));

    assert!(set.contains(&ModelSpec::new("anthropic", "claude-opus-4")));
    assert!(!set.contains(&ModelSpec::new("genai", "gemini-3")));
}
