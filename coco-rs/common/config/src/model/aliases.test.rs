use pretty_assertions::assert_eq;

use super::*;

#[test]
fn test_resolve_anthropic_aliases() {
    let sonnet = resolve_alias(ModelAlias::Sonnet, ProviderApi::Anthropic);
    assert!(sonnet.contains("sonnet"));

    let opus = resolve_alias(ModelAlias::Opus, ProviderApi::Anthropic);
    assert!(opus.contains("opus"));

    let haiku = resolve_alias(ModelAlias::Haiku, ProviderApi::Anthropic);
    assert!(haiku.contains("haiku"));
}

#[test]
fn test_best_resolves_to_opus() {
    let best = resolve_alias(ModelAlias::Best, ProviderApi::Anthropic);
    let opus = resolve_alias(ModelAlias::Opus, ProviderApi::Anthropic);
    assert_eq!(best, opus);
}

#[test]
fn test_parse_user_model_alias() {
    let model = parse_user_model("sonnet");
    assert!(model.contains("sonnet"));
}

#[test]
fn test_parse_user_model_direct() {
    let model = parse_user_model("my-custom-model-v2");
    assert_eq!(model, "my-custom-model-v2");
}
