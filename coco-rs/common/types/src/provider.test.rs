use pretty_assertions::assert_eq;

use super::*;

#[test]
fn test_provider_api_serde_roundtrip() {
    let provider = ProviderApi::Anthropic;
    let json = serde_json::to_string(&provider).unwrap();
    assert_eq!(json, "\"anthropic\"");
    let parsed: ProviderApi = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, provider);
}

#[test]
fn test_capability_set() {
    let mut caps = CapabilitySet::new();
    caps.insert(Capability::Vision);
    caps.insert(Capability::ToolCalling);
    assert!(caps.contains(&Capability::Vision));
    assert!(!caps.contains(&Capability::Audio));
}

#[test]
fn test_model_spec_equality_ignores_display_name() {
    let a = ModelSpec {
        provider: "anthropic".into(),
        api: ProviderApi::Anthropic,
        model_id: "claude-opus-4-6".into(),
        display_name: "Claude Opus 4.6".into(),
    };
    let b = ModelSpec {
        provider: "anthropic".into(),
        api: ProviderApi::Anthropic,
        model_id: "claude-opus-4-6".into(),
        display_name: "Different Name".into(),
    };
    assert_eq!(a, b);
}

#[test]
fn test_apply_patch_tool_type_default() {
    let default = ApplyPatchToolType::default();
    assert_eq!(default, ApplyPatchToolType::Freeform);
}
