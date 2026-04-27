use pretty_assertions::assert_eq;

use super::*;

#[test]
fn test_builtin_providers_count() {
    let providers = builtin_providers().expect("builtin partials must resolve");
    assert_eq!(providers.len(), 5);
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
