use pretty_assertions::assert_eq;

use super::*;

#[test]
fn test_builtin_providers_count() {
    let providers = builtin_providers();
    assert_eq!(providers.len(), 5);
}

#[test]
fn test_find_anthropic_provider() {
    let provider = find_builtin_provider(ProviderApi::Anthropic).unwrap();
    assert_eq!(provider.name, "anthropic");
    assert_eq!(provider.env_key, "ANTHROPIC_API_KEY");
}

#[test]
fn test_find_openai_provider() {
    let provider = find_builtin_provider(ProviderApi::Openai).unwrap();
    assert_eq!(provider.name, "openai");
    assert_eq!(provider.env_key, "OPENAI_API_KEY");
}
