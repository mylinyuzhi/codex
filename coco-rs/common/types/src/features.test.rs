use super::*;
use pretty_assertions::assert_eq;

#[test]
fn defaults_match_registry() {
    let f = Features::with_defaults();
    for spec in all_features() {
        assert_eq!(
            f.enabled(spec.id),
            spec.default_enabled,
            "default mismatch for {:?}",
            spec.id
        );
    }
}

#[test]
fn defaults_for_token_economy_gates() {
    let f = Features::with_defaults();
    assert!(f.enabled(Feature::WebSearch));
    assert!(f.enabled(Feature::WebFetch));
    assert!(f.enabled(Feature::Mcp));
    assert!(f.enabled(Feature::TaskV2));
}

#[test]
fn defaults_for_safety_gate() {
    let f = Features::with_defaults();
    assert!(!f.enabled(Feature::Sandbox), "Sandbox must default off");
}

#[test]
fn defaults_for_experimental_gates() {
    let f = Features::with_defaults();
    for feat in [
        Feature::AutoMemory,
        Feature::Retrieval,
        Feature::AgentTeams,
        Feature::Worktree,
        Feature::Lsp,
        Feature::NotebookEdit,
    ] {
        assert!(!f.enabled(feat), "{feat:?} must default off");
    }
}

#[test]
fn empty_starts_with_no_features() {
    let f = Features::empty();
    for spec in all_features() {
        assert!(!f.enabled(spec.id), "empty must not enable {:?}", spec.id);
    }
}

#[test]
fn enable_and_disable_round_trip() {
    let mut f = Features::empty();
    assert!(!f.enabled(Feature::Retrieval));
    f.enable(Feature::Retrieval);
    assert!(f.enabled(Feature::Retrieval));
    f.disable(Feature::Retrieval);
    assert!(!f.enabled(Feature::Retrieval));
}

#[test]
fn apply_map_overrides_registry_defaults() {
    // Start from `with_defaults` so the flip is meaningful — disabling
    // WebSearch (registry default = on) and enabling AutoMemory
    // (registry default = off) both have to land.
    let mut f = Features::with_defaults();
    let mut m = BTreeMap::new();
    m.insert("auto_memory".to_string(), true);
    m.insert("web_search".to_string(), false);
    m.insert("nonsense_unknown_key".to_string(), true);
    f.apply_map(&m);
    assert!(f.enabled(Feature::AutoMemory));
    assert!(!f.enabled(Feature::WebSearch));
    // Untouched defaults stay put.
    assert!(f.enabled(Feature::WebFetch));
}

#[test]
fn feature_for_key_round_trip() {
    for spec in all_features() {
        assert_eq!(feature_for_key(spec.key), Some(spec.id));
    }
    assert_eq!(feature_for_key("not_a_real_key"), None);
}

#[test]
fn is_known_feature_key_truthy_for_registered() {
    assert!(is_known_feature_key("auto_memory"));
    assert!(is_known_feature_key("web_search"));
    assert!(!is_known_feature_key("missing"));
}

#[test]
fn keys_are_unique() {
    let mut seen = std::collections::HashSet::new();
    for spec in all_features() {
        assert!(seen.insert(spec.key), "duplicate key: {}", spec.key);
    }
}

#[test]
fn ids_are_unique() {
    let mut seen = std::collections::HashSet::new();
    for spec in all_features() {
        assert!(seen.insert(spec.id), "duplicate id: {:?}", spec.id);
    }
}

#[test]
fn enabled_features_is_sorted_and_dedup() {
    let mut f = Features::empty();
    f.enable(Feature::Retrieval);
    f.enable(Feature::AutoMemory);
    f.enable(Feature::AutoMemory);
    let v = f.enabled_features();
    assert_eq!(v.len(), 2);
    assert!(v.windows(2).all(|w| w[0] < w[1]));
}
