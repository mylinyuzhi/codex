use super::*;

#[test]
fn test_role_identity() {
    let identity = ExecutionIdentity::role(ModelRole::Plan);
    assert!(identity.is_role());
    assert!(!identity.is_spec());
    assert!(!identity.requires_parent());
    assert_eq!(identity.as_role(), Some(ModelRole::Plan));
    assert_eq!(identity.to_string(), "role:plan");
}

#[test]
fn test_spec_identity() {
    let spec = ModelSpec::new("anthropic", "claude-haiku");
    let identity = ExecutionIdentity::spec(spec.clone());
    assert!(identity.is_spec());
    assert!(!identity.is_role());
    assert!(!identity.requires_parent());
    assert_eq!(identity.as_spec(), Some(&spec));
    assert_eq!(identity.to_string(), "spec:anthropic/claude-haiku");
}

#[test]
fn test_inherit_identity() {
    let identity = ExecutionIdentity::inherit();
    assert!(!identity.is_role());
    assert!(!identity.is_spec());
    assert!(identity.requires_parent());
    assert_eq!(identity.to_string(), "inherit");
}

#[test]
fn test_convenience_constructors() {
    assert_eq!(
        ExecutionIdentity::main(),
        ExecutionIdentity::Role(ModelRole::Main)
    );
    assert_eq!(
        ExecutionIdentity::fast(),
        ExecutionIdentity::Role(ModelRole::Fast)
    );
    assert_eq!(
        ExecutionIdentity::plan(),
        ExecutionIdentity::Role(ModelRole::Plan)
    );
    assert_eq!(
        ExecutionIdentity::explore(),
        ExecutionIdentity::Role(ModelRole::Explore)
    );
    assert_eq!(
        ExecutionIdentity::compact(),
        ExecutionIdentity::Role(ModelRole::Compact)
    );
}

#[test]
fn test_default() {
    assert_eq!(
        ExecutionIdentity::default(),
        ExecutionIdentity::Role(ModelRole::Main)
    );
}

#[test]
fn test_from_role() {
    let identity: ExecutionIdentity = ModelRole::Plan.into();
    assert_eq!(identity, ExecutionIdentity::Role(ModelRole::Plan));
}

#[test]
fn test_from_spec() {
    let spec = ModelSpec::new("openai", "gpt-5");
    let identity: ExecutionIdentity = spec.clone().into();
    assert_eq!(identity, ExecutionIdentity::Spec(spec));
}

#[test]
fn test_serde_role() {
    let identity = ExecutionIdentity::role(ModelRole::Plan);
    let json = serde_json::to_string(&identity).unwrap();
    assert!(json.contains("Role"));
    let parsed: ExecutionIdentity = serde_json::from_str(&json).unwrap();
    assert_eq!(identity, parsed);
}

#[test]
fn test_serde_spec() {
    let identity = ExecutionIdentity::spec(ModelSpec::new("anthropic", "claude-haiku"));
    let json = serde_json::to_string(&identity).unwrap();
    assert!(json.contains("Spec"));
    let parsed: ExecutionIdentity = serde_json::from_str(&json).unwrap();
    assert_eq!(identity, parsed);
}

#[test]
fn test_serde_inherit() {
    let identity = ExecutionIdentity::inherit();
    let json = serde_json::to_string(&identity).unwrap();
    assert!(json.contains("Inherit"));
    let parsed: ExecutionIdentity = serde_json::from_str(&json).unwrap();
    assert_eq!(identity, parsed);
}
