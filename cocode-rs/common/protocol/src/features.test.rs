use super::*;

#[test]
fn test_with_defaults_includes_default_enabled_features() {
    let features = Features::with_defaults();

    // McpResourceTools is default enabled
    assert!(features.enabled(Feature::McpResourceTools));
    // Ls is default enabled
    assert!(features.enabled(Feature::Ls));
}

#[test]
fn test_with_defaults_excludes_non_default_features() {
    let features = Features::with_defaults();

    // WebFetch is not default enabled
    assert!(!features.enabled(Feature::WebFetch));
    // Collab is not default enabled
    assert!(!features.enabled(Feature::Collab));
    // GhostCommit is not default enabled
    assert!(!features.enabled(Feature::GhostCommit));
}

#[test]
fn test_enable_and_disable() {
    let mut features = Features::default();

    // Enable a feature
    features.enable(Feature::WebFetch);
    assert!(features.enabled(Feature::WebFetch));

    // Disable the feature
    features.disable(Feature::WebFetch);
    assert!(!features.enabled(Feature::WebFetch));
}

#[test]
fn test_apply_map_enables_features() {
    let mut features = Features::default();
    let mut map = BTreeMap::new();
    map.insert("web_fetch".to_string(), true);
    map.insert("collab".to_string(), true);

    features.apply_map(&map);

    assert!(features.enabled(Feature::WebFetch));
    assert!(features.enabled(Feature::Collab));
}

#[test]
fn test_apply_map_disables_features() {
    let mut features = Features::with_defaults();
    let mut map = BTreeMap::new();
    // Disable a default-enabled feature
    map.insert("ls".to_string(), false);

    features.apply_map(&map);

    assert!(!features.enabled(Feature::Ls));
}

#[test]
fn test_apply_map_ignores_unknown_keys() {
    let mut features = Features::with_defaults();
    let original = features.clone();
    let mut map = BTreeMap::new();
    map.insert("unknown_feature_xyz".to_string(), true);

    features.apply_map(&map);

    // Features should remain unchanged
    assert_eq!(features, original);
}

#[test]
fn test_feature_for_key_known_keys() {
    assert_eq!(feature_for_key("web_fetch"), Some(Feature::WebFetch));
    assert_eq!(feature_for_key("collab"), Some(Feature::Collab));
    assert_eq!(feature_for_key("undo"), Some(Feature::GhostCommit));
}

#[test]
fn test_feature_for_key_unknown_keys() {
    assert_eq!(feature_for_key("unknown"), None);
    assert_eq!(feature_for_key(""), None);
    assert_eq!(feature_for_key("WEB_FETCH"), None); // Case sensitive
}

#[test]
fn test_is_known_feature_key() {
    assert!(is_known_feature_key("web_fetch"));
    assert!(!is_known_feature_key("unknown"));
    assert!(!is_known_feature_key(""));
}

#[test]
fn test_feature_key_method() {
    assert_eq!(Feature::WebFetch.key(), "web_fetch");
    assert_eq!(Feature::GhostCommit.key(), "undo");
}

#[test]
fn test_feature_stage_method() {
    assert_eq!(Feature::McpResourceTools.stage(), Stage::Stable);
    assert_eq!(Feature::WebFetch.stage(), Stage::Experimental);
}

#[test]
fn test_feature_default_enabled_method() {
    assert!(Feature::Ls.default_enabled());
    assert!(!Feature::WebFetch.default_enabled());
}

#[test]
fn test_enabled_features_returns_all_enabled() {
    let mut features = Features::default();
    features.enable(Feature::WebFetch);
    features.enable(Feature::Collab);

    let enabled = features.enabled_features();
    assert!(enabled.contains(&Feature::WebFetch));
    assert!(enabled.contains(&Feature::Collab));
    assert_eq!(enabled.len(), 2);
}

#[test]
fn test_all_features_contains_all_variants() {
    let specs: Vec<_> = all_features().collect();
    // Ensure we have a reasonable number of features
    assert!(specs.len() >= 12);

    // Check that some expected features are present
    assert!(specs.iter().any(|s| s.id == Feature::WebFetch));
    assert!(specs.iter().any(|s| s.id == Feature::Ls));
}

#[test]
fn test_stage_beta_methods() {
    let beta_stage = Stage::Beta {
        name: "Test Feature",
        menu_description: "Test description",
        announcement: "Test announcement",
    };

    assert_eq!(beta_stage.beta_menu_name(), Some("Test Feature"));
    assert_eq!(beta_stage.beta_menu_description(), Some("Test description"));
    assert_eq!(beta_stage.beta_announcement(), Some("Test announcement"));

    // Non-beta stages should return None
    assert_eq!(Stage::Stable.beta_menu_name(), None);
    assert_eq!(Stage::Experimental.beta_menu_description(), None);
}
