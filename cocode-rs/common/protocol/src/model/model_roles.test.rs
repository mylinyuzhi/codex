use super::*;

#[test]
fn test_model_role_as_str() {
    assert_eq!(ModelRole::Main.as_str(), "main");
    assert_eq!(ModelRole::Fast.as_str(), "fast");
    assert_eq!(ModelRole::Vision.as_str(), "vision");
    assert_eq!(ModelRole::Compact.as_str(), "compact");
}

#[test]
fn test_model_roles_default() {
    let roles = ModelRoles::default();
    assert!(roles.is_empty());
    assert!(roles.main().is_none());
}

#[test]
fn test_model_roles_with_main() {
    let spec = ModelSpec::new("anthropic", "claude-opus-4");
    let roles = ModelRoles::with_main(spec.clone());

    assert_eq!(roles.main(), Some(&spec));
    assert!(!roles.is_empty());
}

#[test]
fn test_model_roles_get_specific() {
    let mut roles = ModelRoles::default();
    roles.set(
        ModelRole::Main,
        ModelSpec::new("anthropic", "claude-opus-4"),
    );
    roles.set(ModelRole::Fast, ModelSpec::new("anthropic", "claude-haiku"));

    // Specific role returns specific model
    let fast = roles.get(ModelRole::Fast).unwrap();
    assert_eq!(fast.slug, "claude-haiku");
}

#[test]
fn test_model_roles_get_fallback() {
    let roles = ModelRoles::with_main(ModelSpec::new("anthropic", "claude-opus-4"));

    // Unset role falls back to main
    let vision = roles.get(ModelRole::Vision).unwrap();
    assert_eq!(vision.slug, "claude-opus-4");
}

#[test]
fn test_model_roles_get_none() {
    let roles = ModelRoles::default();

    // No main set, returns None
    assert!(roles.get(ModelRole::Fast).is_none());
    assert!(roles.get(ModelRole::Main).is_none());
}

#[test]
fn test_model_roles_set() {
    let mut roles = ModelRoles::default();
    roles.set(ModelRole::Fast, ModelSpec::new("openai", "gpt-4o-mini"));

    assert_eq!(
        roles.get_direct(ModelRole::Fast).unwrap().slug,
        "gpt-4o-mini"
    );
}

#[test]
fn test_model_roles_merge() {
    let mut base = ModelRoles::default();
    base.set(
        ModelRole::Main,
        ModelSpec::new("anthropic", "claude-opus-4"),
    );
    base.set(ModelRole::Fast, ModelSpec::new("anthropic", "claude-haiku"));

    let mut other = ModelRoles::default();
    other.set(ModelRole::Fast, ModelSpec::new("openai", "gpt-4o-mini"));
    other.set(ModelRole::Vision, ModelSpec::new("openai", "gpt-4o"));

    base.merge(&other);

    // main unchanged
    assert_eq!(base.main().unwrap().slug, "claude-opus-4");
    // fast overridden
    assert_eq!(
        base.get_direct(ModelRole::Fast).unwrap().slug,
        "gpt-4o-mini"
    );
    // vision added
    assert_eq!(base.get_direct(ModelRole::Vision).unwrap().slug, "gpt-4o");
}

#[test]
fn test_serde_roundtrip() {
    let mut roles = ModelRoles::default();
    roles.set(
        ModelRole::Main,
        ModelSpec::new("anthropic", "claude-opus-4"),
    );
    roles.set(ModelRole::Fast, ModelSpec::new("anthropic", "claude-haiku"));
    roles.set(ModelRole::Vision, ModelSpec::new("openai", "gpt-4o"));

    let json = serde_json::to_string(&roles).unwrap();
    let parsed: ModelRoles = serde_json::from_str(&json).unwrap();

    assert_eq!(parsed, roles);
}

#[test]
fn test_serde_from_json() {
    let json = r#"{
        "main": "anthropic/claude-opus-4",
        "fast": "anthropic/claude-haiku",
        "vision": "openai/gpt-4o"
    }"#;

    let roles: ModelRoles = serde_json::from_str(json).unwrap();

    assert_eq!(roles.main().unwrap().provider, "anthropic");
    assert_eq!(roles.main().unwrap().slug, "claude-opus-4");
    assert_eq!(
        roles.get_direct(ModelRole::Fast).unwrap().slug,
        "claude-haiku"
    );
    assert_eq!(
        roles.get_direct(ModelRole::Vision).unwrap().provider,
        "openai"
    );
}

#[test]
fn test_serde_partial() {
    let json = r#"{"main": "anthropic/claude-opus-4"}"#;
    let roles: ModelRoles = serde_json::from_str(json).unwrap();

    assert!(roles.main().is_some());
    assert!(roles.get_direct(ModelRole::Fast).is_none());
    assert!(roles.get_direct(ModelRole::Vision).is_none());
}

#[test]
fn test_serde_empty() {
    let json = "{}";
    let roles: ModelRoles = serde_json::from_str(json).unwrap();
    assert!(roles.is_empty());
}

#[test]
fn test_model_role_all() {
    let all = ModelRole::all();
    assert_eq!(all.len(), 7);
    assert!(all.contains(&ModelRole::Main));
    assert!(all.contains(&ModelRole::Explore));
    assert!(all.contains(&ModelRole::Compact));
}

#[test]
fn test_model_roles_set_compact() {
    let mut roles = ModelRoles::default();
    roles.set(
        ModelRole::Compact,
        ModelSpec::new("anthropic", "claude-haiku"),
    );

    assert_eq!(
        roles.get_direct(ModelRole::Compact).unwrap().slug,
        "claude-haiku"
    );
}

#[test]
fn test_model_roles_get_compact_fallback() {
    let roles = ModelRoles::with_main(ModelSpec::new("anthropic", "claude-opus-4"));

    // Compact falls back to main
    let compact = roles.get(ModelRole::Compact).unwrap();
    assert_eq!(compact.slug, "claude-opus-4");
}

#[test]
fn test_model_roles_merge_compact() {
    let mut base = ModelRoles::default();
    base.set(
        ModelRole::Compact,
        ModelSpec::new("anthropic", "claude-haiku"),
    );

    let mut other = ModelRoles::default();
    other.set(ModelRole::Compact, ModelSpec::new("openai", "gpt-4o-mini"));

    base.merge(&other);

    assert_eq!(
        base.get_direct(ModelRole::Compact).unwrap().slug,
        "gpt-4o-mini"
    );
}
