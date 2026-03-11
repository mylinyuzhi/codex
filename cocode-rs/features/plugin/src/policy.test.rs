use super::*;

#[test]
fn test_default_policy_is_permissive() {
    let policy = PluginPolicy::default();
    assert!(policy.is_permissive());
    assert_eq!(policy.check_marketplace("any"), PolicyDecision::Allow);
    assert_eq!(policy.check_plugin("any"), PolicyDecision::Allow);
}

#[test]
fn test_disable_installation() {
    let policy = PluginPolicy {
        disable_installation: true,
        ..Default::default()
    };
    assert!(matches!(
        policy.check_marketplace("any"),
        PolicyDecision::Deny(_)
    ));
    assert!(matches!(
        policy.check_plugin("any"),
        PolicyDecision::Deny(_)
    ));
}

#[test]
fn test_blocklist() {
    let policy = PluginPolicy {
        blocked_plugins: vec!["bad-plugin".to_string()],
        blocked_marketplaces: vec!["evil-market".to_string()],
        ..Default::default()
    };
    assert!(matches!(
        policy.check_plugin("bad-plugin"),
        PolicyDecision::Deny(_)
    ));
    assert_eq!(policy.check_plugin("good-plugin"), PolicyDecision::Allow);
    assert!(matches!(
        policy.check_marketplace("evil-market"),
        PolicyDecision::Deny(_)
    ));
    assert_eq!(
        policy.check_marketplace("good-market"),
        PolicyDecision::Allow
    );
}

#[test]
fn test_allowlist() {
    let policy = PluginPolicy {
        allowed_plugins: vec!["approved-*".to_string()],
        ..Default::default()
    };
    assert_eq!(
        policy.check_plugin("approved-plugin"),
        PolicyDecision::Allow
    );
    assert!(matches!(
        policy.check_plugin("unapproved"),
        PolicyDecision::Deny(_)
    ));
}

#[test]
fn test_pattern_matching() {
    assert!(matches_pattern("*", "anything"));
    assert!(matches_pattern("prefix-*", "prefix-foo"));
    assert!(!matches_pattern("prefix-*", "other-foo"));
    assert!(matches_pattern("*-suffix", "foo-suffix"));
    assert!(!matches_pattern("*-suffix", "foo-other"));
    assert!(matches_pattern("exact", "exact"));
    assert!(!matches_pattern("exact", "other"));
}

#[test]
fn test_blocklist_priority_over_allowlist() {
    let policy = PluginPolicy {
        allowed_plugins: vec!["my-*".to_string()],
        blocked_plugins: vec!["my-bad".to_string()],
        ..Default::default()
    };
    // "my-bad" is in allowlist pattern but explicitly blocked
    assert!(matches!(
        policy.check_plugin("my-bad"),
        PolicyDecision::Deny(_)
    ));
    assert_eq!(policy.check_plugin("my-good"), PolicyDecision::Allow);
}

#[test]
fn test_load_nonexistent_returns_default() {
    let policy = PluginPolicy::load(Path::new("/nonexistent/policy.json"));
    assert!(policy.is_permissive());
}
