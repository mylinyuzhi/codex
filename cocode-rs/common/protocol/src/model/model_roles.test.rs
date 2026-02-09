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
    roles.main = Some(ModelSpec::new("anthropic", "claude-opus-4"));
    roles.fast = Some(ModelSpec::new("anthropic", "claude-haiku"));

    // Specific role returns specific model
    let fast = roles.get(ModelRole::Fast).unwrap();
    assert_eq!(fast.model, "claude-haiku");
}

#[test]
fn test_model_roles_get_fallback() {
    let mut roles = ModelRoles::default();
    roles.main = Some(ModelSpec::new("anthropic", "claude-opus-4"));

    // Unset role falls back to main
    let vision = roles.get(ModelRole::Vision).unwrap();
    assert_eq!(vision.model, "claude-opus-4");
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

    assert_eq!(roles.fast.as_ref().unwrap().model, "gpt-4o-mini");
}

#[test]
fn test_model_roles_merge() {
    let mut base = ModelRoles::default();
    base.main = Some(ModelSpec::new("anthropic", "claude-opus-4"));
    base.fast = Some(ModelSpec::new("anthropic", "claude-haiku"));

    let mut other = ModelRoles::default();
    other.fast = Some(ModelSpec::new("openai", "gpt-4o-mini"));
    other.vision = Some(ModelSpec::new("openai", "gpt-4o"));

    base.merge(&other);

    // main unchanged
    assert_eq!(base.main.as_ref().unwrap().model, "claude-opus-4");
    // fast overridden
    assert_eq!(base.fast.as_ref().unwrap().model, "gpt-4o-mini");
    // vision added
    assert_eq!(base.vision.as_ref().unwrap().model, "gpt-4o");
}

#[test]
fn test_serde_roundtrip() {
    let mut roles = ModelRoles::default();
    roles.main = Some(ModelSpec::new("anthropic", "claude-opus-4"));
    roles.fast = Some(ModelSpec::new("anthropic", "claude-haiku"));
    roles.vision = Some(ModelSpec::new("openai", "gpt-4o"));

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

    assert_eq!(roles.main.as_ref().unwrap().provider, "anthropic");
    assert_eq!(roles.main.as_ref().unwrap().model, "claude-opus-4");
    assert_eq!(roles.fast.as_ref().unwrap().model, "claude-haiku");
    assert_eq!(roles.vision.as_ref().unwrap().provider, "openai");
}

#[test]
fn test_serde_partial() {
    let json = r#"{"main": "anthropic/claude-opus-4"}"#;
    let roles: ModelRoles = serde_json::from_str(json).unwrap();

    assert!(roles.main.is_some());
    assert!(roles.fast.is_none());
    assert!(roles.vision.is_none());
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

    assert_eq!(roles.compact.as_ref().unwrap().model, "claude-haiku");
}

#[test]
fn test_model_roles_get_compact_fallback() {
    let mut roles = ModelRoles::default();
    roles.main = Some(ModelSpec::new("anthropic", "claude-opus-4"));

    // Compact falls back to main
    let compact = roles.get(ModelRole::Compact).unwrap();
    assert_eq!(compact.model, "claude-opus-4");
}

#[test]
fn test_model_roles_merge_compact() {
    let mut base = ModelRoles::default();
    base.compact = Some(ModelSpec::new("anthropic", "claude-haiku"));

    let mut other = ModelRoles::default();
    other.compact = Some(ModelSpec::new("openai", "gpt-4o-mini"));

    base.merge(&other);

    assert_eq!(base.compact.as_ref().unwrap().model, "gpt-4o-mini");
}
