use super::*;

#[test]
fn test_role_selection_new() {
    let selection = RoleSelection::new(ModelSpec::new("anthropic", "claude-opus-4"));
    assert_eq!(selection.model.provider, "anthropic");
    assert_eq!(selection.model.model, "claude-opus-4");
    assert!(selection.thinking_level.is_none());
    assert!(selection.supported_thinking_levels.is_none());
}

#[test]
fn test_role_selection_with_thinking() {
    let selection = RoleSelection::with_thinking(
        ModelSpec::new("anthropic", "claude-opus-4"),
        ThinkingLevel::high(),
    );
    assert_eq!(selection.model.provider, "anthropic");
    assert!(selection.thinking_level.is_some());
    assert_eq!(
        selection.thinking_level.as_ref().unwrap().effort,
        crate::model::ReasoningEffort::High
    );
}

#[test]
fn test_role_selection_set_thinking_level() {
    let mut selection = RoleSelection::new(ModelSpec::new("openai", "gpt-5"));
    assert!(selection.thinking_level.is_none());

    selection.set_thinking_level(ThinkingLevel::medium());
    assert!(selection.thinking_level.is_some());

    selection.clear_thinking_level();
    assert!(selection.thinking_level.is_none());
}

#[test]
fn test_role_selection_effective_thinking_level() {
    // No override — returns default (None effort)
    let selection = RoleSelection::new(ModelSpec::new("openai", "gpt-5"));
    let effective = selection.effective_thinking_level();
    assert_eq!(effective.effort, crate::model::ReasoningEffort::None);

    // With override — returns the override
    let selection =
        RoleSelection::with_thinking(ModelSpec::new("openai", "gpt-5"), ThinkingLevel::high());
    let effective = selection.effective_thinking_level();
    assert_eq!(effective.effort, crate::model::ReasoningEffort::High);
}

#[test]
fn test_role_selection_supported_thinking_levels() {
    let levels = vec![
        ThinkingLevel::low(),
        ThinkingLevel::medium(),
        ThinkingLevel::high(),
    ];
    let selection = RoleSelection::new(ModelSpec::new("anthropic", "claude-opus-4"))
        .with_supported_thinking_levels(levels);

    assert_eq!(
        selection.supported_thinking_levels.as_ref().unwrap().len(),
        3
    );
}

#[test]
fn test_role_selections_default() {
    let selections = RoleSelections::default();
    assert!(selections.is_empty());
    assert!(selections.get(ModelRole::Main).is_none());
}

#[test]
fn test_role_selections_with_main() {
    let selection = RoleSelection::new(ModelSpec::new("anthropic", "claude-opus-4"));
    let selections = RoleSelections::with_main(selection.clone());

    assert!(!selections.is_empty());
    assert_eq!(selections.get(ModelRole::Main), Some(&selection));
    assert!(selections.get(ModelRole::Fast).is_none());
}

#[test]
fn test_role_selections_set_and_get() {
    let mut selections = RoleSelections::default();

    selections.set(
        ModelRole::Main,
        RoleSelection::new(ModelSpec::new("anthropic", "claude-opus-4")),
    );
    selections.set(
        ModelRole::Fast,
        RoleSelection::new(ModelSpec::new("anthropic", "claude-haiku")),
    );

    assert_eq!(
        selections.get(ModelRole::Main).unwrap().model.model,
        "claude-opus-4"
    );
    assert_eq!(
        selections.get(ModelRole::Fast).unwrap().model.model,
        "claude-haiku"
    );
}

#[test]
fn test_role_selections_get_or_main() {
    let mut selections = RoleSelections::default();

    selections.set(
        ModelRole::Main,
        RoleSelection::new(ModelSpec::new("anthropic", "claude-opus-4")),
    );

    // Fast not set, falls back to main
    let fast = selections.get_or_main(ModelRole::Fast);
    assert!(fast.is_some());
    assert_eq!(fast.unwrap().model.model, "claude-opus-4");

    // Set fast explicitly
    selections.set(
        ModelRole::Fast,
        RoleSelection::new(ModelSpec::new("anthropic", "claude-haiku")),
    );

    let fast = selections.get_or_main(ModelRole::Fast);
    assert_eq!(fast.unwrap().model.model, "claude-haiku");
}

#[test]
fn test_role_selections_set_thinking_level() {
    let mut selections = RoleSelections::default();

    // No selection yet, should return false
    assert!(!selections.set_thinking_level(ModelRole::Main, ThinkingLevel::high()));

    // Add selection
    selections.set(
        ModelRole::Main,
        RoleSelection::new(ModelSpec::new("anthropic", "claude-opus-4")),
    );

    // Now should succeed
    assert!(selections.set_thinking_level(ModelRole::Main, ThinkingLevel::high()));
    assert!(
        selections
            .get(ModelRole::Main)
            .unwrap()
            .thinking_level
            .is_some()
    );
}

#[test]
fn test_role_selections_clear() {
    let mut selections = RoleSelections::default();
    selections.set(
        ModelRole::Main,
        RoleSelection::new(ModelSpec::new("anthropic", "claude-opus-4")),
    );

    assert!(!selections.is_empty());

    selections.clear(ModelRole::Main);
    assert!(selections.is_empty());
}

#[test]
fn test_role_selections_merge() {
    let mut base = RoleSelections::default();
    base.set(
        ModelRole::Main,
        RoleSelection::new(ModelSpec::new("anthropic", "claude-opus-4")),
    );
    base.set(
        ModelRole::Fast,
        RoleSelection::new(ModelSpec::new("anthropic", "claude-haiku")),
    );

    let mut other = RoleSelections::default();
    other.set(
        ModelRole::Fast,
        RoleSelection::new(ModelSpec::new("openai", "gpt-4o-mini")),
    );
    other.set(
        ModelRole::Vision,
        RoleSelection::new(ModelSpec::new("openai", "gpt-4o")),
    );

    base.merge(&other);

    // main unchanged
    assert_eq!(
        base.get(ModelRole::Main).unwrap().model.model,
        "claude-opus-4"
    );
    // fast overridden
    assert_eq!(
        base.get(ModelRole::Fast).unwrap().model.model,
        "gpt-4o-mini"
    );
    // vision added
    assert_eq!(base.get(ModelRole::Vision).unwrap().model.model, "gpt-4o");
}

#[test]
fn test_serde_roundtrip() {
    let mut selections = RoleSelections::default();
    selections.set(
        ModelRole::Main,
        RoleSelection::with_thinking(
            ModelSpec::new("anthropic", "claude-opus-4"),
            ThinkingLevel::high().set_budget(32000),
        ),
    );
    selections.set(
        ModelRole::Fast,
        RoleSelection::new(ModelSpec::new("anthropic", "claude-haiku")),
    );

    let json = serde_json::to_string(&selections).unwrap();
    let parsed: RoleSelections = serde_json::from_str(&json).unwrap();

    assert_eq!(selections, parsed);
}

#[test]
fn test_serde_skip_none_fields() {
    let selection = RoleSelection::new(ModelSpec::new("anthropic", "claude-opus-4"));
    let json = serde_json::to_string(&selection).unwrap();

    // thinking_level and supported_thinking_levels should be skipped when None
    assert!(!json.contains("thinking_level"));
    assert!(!json.contains("supported_thinking_levels"));

    // Empty selections should serialize to {}
    let selections = RoleSelections::default();
    let json = serde_json::to_string(&selections).unwrap();
    assert_eq!(json, "{}");
}
