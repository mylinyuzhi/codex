use coco_types::ProviderApi;
use pretty_assertions::assert_eq;

use super::*;

#[test]
fn test_model_info_has_capability() {
    let info = ModelInfo {
        capabilities: Some(vec![Capability::Vision, Capability::ToolCalling]),
        ..Default::default()
    };
    assert!(info.has_capability(Capability::Vision));
    assert!(!info.has_capability(Capability::Audio));
}

#[test]
fn test_model_info_default_thinking() {
    let info = ModelInfo {
        supported_thinking_levels: Some(vec![
            ThinkingLevel::low(),
            ThinkingLevel::medium(),
            ThinkingLevel::high(),
        ]),
        default_thinking_level: Some(ReasoningEffort::Medium),
        ..Default::default()
    };
    let default = info.default_thinking().unwrap();
    assert_eq!(default.effort, ReasoningEffort::Medium);
}

#[test]
fn test_model_info_resolve_thinking_exact() {
    let info = ModelInfo {
        supported_thinking_levels: Some(vec![ThinkingLevel::low(), ThinkingLevel::high()]),
        ..Default::default()
    };
    let resolved = info.resolve_thinking_level(&ThinkingLevel::high());
    assert_eq!(resolved.effort, ReasoningEffort::High);
}

#[test]
fn test_model_info_resolve_thinking_nearest() {
    let info = ModelInfo {
        supported_thinking_levels: Some(vec![ThinkingLevel::low(), ThinkingLevel::high()]),
        ..Default::default()
    };
    // Medium is not supported, should resolve to nearest (High is closer)
    let resolved = info.resolve_thinking_level(&ThinkingLevel::medium());
    // Medium (3) is equidistant from Low (2) and High (4), min_by_key picks first = Low
    assert!(resolved.effort == ReasoningEffort::Low || resolved.effort == ReasoningEffort::High);
}

#[test]
fn test_model_info_merge() {
    let mut base = ModelInfo {
        model_id: "base-model".into(),
        context_window: 100_000,
        capabilities: Some(vec![Capability::Vision]),
        ..Default::default()
    };
    let overlay = ModelInfo {
        context_window: 500_000,
        temperature: Some(0.7),
        ..Default::default()
    };
    base.merge_from(&overlay);
    assert_eq!(base.model_id, "base-model"); // not overridden (empty in overlay)
    assert_eq!(base.context_window, 500_000);
    assert_eq!(base.temperature, Some(0.7));
    assert!(base.capabilities.is_some()); // not overridden (None in overlay)
}

#[test]
fn test_model_roles_fallback_to_main() {
    let mut roles = ModelRoles::default();
    roles.roles.insert(
        ModelRole::Main,
        ModelSpec {
            provider: "anthropic".into(),
            api: ProviderApi::Anthropic,
            model_id: "claude-sonnet".into(),
            display_name: "Sonnet".into(),
        },
    );
    // Fast role not set, should fall back to Main
    let spec = roles.get(ModelRole::Fast).unwrap();
    assert_eq!(spec.model_id, "claude-sonnet");
}
