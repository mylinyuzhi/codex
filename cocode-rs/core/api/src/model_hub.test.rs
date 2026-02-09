use super::*;

#[test]
fn test_hub_error_no_model_configured() {
    let err = HubError::NoModelConfigured {
        identity: "role:main".to_string(),
    };
    assert!(err.is_no_model_configured());
    assert!(err.to_string().contains("No model configured"));
}

#[test]
fn test_hub_error_inherit_without_parent() {
    let err = HubError::InheritWithoutParent;
    assert!(!err.is_no_model_configured());
    assert!(err.to_string().contains("parent_spec"));
}

#[test]
fn test_hub_new() {
    let config = ConfigManager::empty();
    let hub = ModelHub::new(Arc::new(config));
    assert_eq!(hub.provider_cache_size(), 0);
    assert_eq!(hub.model_cache_size(), 0);
}

#[test]
fn test_hub_debug() {
    let config = ConfigManager::empty();
    let hub = ModelHub::new(Arc::new(config));
    let debug_str = format!("{:?}", hub);
    assert!(debug_str.contains("ModelHub"));
    assert!(debug_str.contains("provider_cache_size"));
    assert!(debug_str.contains("model_cache_size"));
}

// ========================================================================
// resolve_identity() function tests (now standalone)
// ========================================================================

#[test]
fn test_resolve_identity_role() {
    let mut selections = RoleSelections::default();
    selections.set(
        ModelRole::Main,
        RoleSelection::with_thinking(
            ModelSpec::new("anthropic", "claude-opus-4"),
            ThinkingLevel::high(),
        ),
    );

    let result = resolve_identity(&ExecutionIdentity::main(), &selections, None);
    assert!(result.is_ok());

    let (spec, selection) = result.unwrap();
    assert_eq!(spec.provider, "anthropic");
    assert_eq!(spec.model, "claude-opus-4");
    assert!(selection.is_some());
    assert!(selection.unwrap().thinking_level.is_some());
}

#[test]
fn test_resolve_identity_spec() {
    let selections = RoleSelections::default();
    let direct_spec = ModelSpec::new("openai", "gpt-5");

    let result = resolve_identity(
        &ExecutionIdentity::Spec(direct_spec.clone()),
        &selections,
        None,
    );

    assert!(result.is_ok());
    let (spec, selection) = result.unwrap();
    assert_eq!(spec, direct_spec);
    assert!(selection.is_none()); // No selection for direct spec
}

#[test]
fn test_resolve_identity_inherit() {
    let selections = RoleSelections::default();
    let parent = ModelSpec::new("anthropic", "claude-opus-4");

    let result = resolve_identity(&ExecutionIdentity::Inherit, &selections, Some(&parent));

    assert!(result.is_ok());
    let (spec, _) = result.unwrap();
    assert_eq!(spec, parent);
}

#[test]
fn test_resolve_identity_inherit_without_parent_returns_error() {
    let selections = RoleSelections::default();

    let result = resolve_identity(&ExecutionIdentity::Inherit, &selections, None);

    assert!(result.is_err());
    matches!(result.unwrap_err(), HubError::InheritWithoutParent);
}

#[test]
fn test_resolve_identity_empty_selections_returns_error() {
    let selections = RoleSelections::default();

    let result = resolve_identity(&ExecutionIdentity::main(), &selections, None);

    assert!(result.is_err());
    assert!(result.unwrap_err().is_no_model_configured());
}

#[test]
fn test_resolve_identity_role_fallback_to_main() {
    let mut selections = RoleSelections::default();
    // Only set Main, not Fast
    selections.set(
        ModelRole::Main,
        RoleSelection::new(ModelSpec::new("anthropic", "claude-opus-4")),
    );

    // Fast should fall back to Main
    let result = resolve_identity(&ExecutionIdentity::fast(), &selections, None);
    assert!(result.is_ok());

    let (spec, _) = result.unwrap();
    assert_eq!(spec.model, "claude-opus-4"); // Got Main's model
}

// ========================================================================
// Hub method tests (using selections as parameter)
// ========================================================================

#[test]
fn test_hub_prepare_main_with_selections_empty_selections_returns_error() {
    let config = ConfigManager::empty();
    let hub = ModelHub::new(Arc::new(config));
    let selections = RoleSelections::default();

    let result = hub.prepare_main_with_selections(&selections, "session-123", 1);
    assert!(result.is_err());
    assert!(result.unwrap_err().is_no_model_configured());
}

#[test]
fn test_hub_get_model_for_role_with_selections() {
    let config = ConfigManager::empty();
    let hub = ModelHub::new(Arc::new(config));

    let mut selections = RoleSelections::default();
    selections.set(
        ModelRole::Main,
        RoleSelection::new(ModelSpec::new("anthropic", "claude-opus-4")),
    );

    // This will fail because we don't have the actual provider configured,
    // but the error should be about provider resolution, not selection lookup
    let result = hub.get_model_for_role_with_selections(ModelRole::Main, &selections);
    assert!(result.is_err());
    // Should be a provider error, not "no model configured"
    assert!(!result.unwrap_err().is_no_model_configured());
}

#[test]
fn test_hub_get_model_for_role_with_selections_empty_returns_error() {
    let config = ConfigManager::empty();
    let hub = ModelHub::new(Arc::new(config));
    let selections = RoleSelections::default();

    let result = hub.get_model_for_role_with_selections(ModelRole::Main, &selections);
    assert!(result.is_err());
    assert!(result.unwrap_err().is_no_model_configured());
}
