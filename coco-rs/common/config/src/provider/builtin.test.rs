use pretty_assertions::assert_eq;

use super::*;

#[test]
fn test_builtin_providers_count() {
    let providers = builtin_providers().expect("builtin partials must resolve");
    assert_eq!(providers.len(), 7);
}

#[test]
fn test_builtin_providers_satisfy_identity_invariant() {
    // Plan §5.1.1: `name == map_key`. For builtins the "map key" is the
    // partial's slot identifier returned by `builtin_provider_partials`.
    // Pair them up and assert each resolved entry's `name` matches.
    let pairs = builtin_provider_partials();
    let resolved = builtin_providers().expect("builtin partials must resolve");
    assert_eq!(pairs.len(), resolved.len(), "builtin pair count mismatch");
    for ((slot_key, _), cfg) in pairs.iter().zip(resolved.iter()) {
        assert_eq!(
            *slot_key, cfg.name,
            "builtin entry {slot_key} name diverged after from_partial"
        );
    }
}

#[test]
fn test_builtin_anthropic_resolves_with_canonical_env_key() {
    let providers = builtin_providers().expect("builtin partials must resolve");
    let provider = providers
        .iter()
        .find(|p| p.api == ProviderApi::Anthropic)
        .expect("anthropic builtin");
    assert_eq!(provider.name, "anthropic");
    assert_eq!(provider.env_key, "ANTHROPIC_API_KEY");
}

#[test]
fn test_builtin_openai_resolves_with_canonical_env_key() {
    let providers = builtin_providers().expect("builtin partials must resolve");
    let provider = providers
        .iter()
        .find(|p| p.api == ProviderApi::Openai)
        .expect("openai builtin");
    assert_eq!(provider.name, "openai");
    assert_eq!(provider.env_key, "OPENAI_API_KEY");
}

#[test]
fn test_builtin_deepseek_providers_resolve() {
    let providers = builtin_providers().expect("builtin partials must resolve");

    let ds_openai = providers
        .iter()
        .find(|p| p.name == "deepseek-openai")
        .expect("deepseek-openai builtin");
    assert_eq!(ds_openai.env_key, "DEEPSEEK_API_KEY");
    assert_eq!(ds_openai.base_url, "https://api.deepseek.com/v1");
    assert_eq!(ds_openai.api, ProviderApi::OpenaiCompat);
    assert!(ds_openai.models.contains_key("deepseek-v4-flash"));
    assert!(ds_openai.models.contains_key("deepseek-v4-pro"));

    let ds_anthropic = providers
        .iter()
        .find(|p| p.name == "deepseek-anthropic")
        .expect("deepseek-anthropic builtin");
    assert_eq!(ds_anthropic.env_key, "DEEPSEEK_API_KEY");
    assert_eq!(
        ds_anthropic.base_url,
        "https://api.deepseek.com/anthropic/v1"
    );
    assert_eq!(ds_anthropic.api, ProviderApi::Anthropic);
    assert!(ds_anthropic.models.contains_key("deepseek-v4-flash"));
    assert!(ds_anthropic.models.contains_key("deepseek-v4-pro"));
}
