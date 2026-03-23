use super::*;

#[test]
fn test_new_store_is_empty() {
    let store = ApprovalStore::new();
    assert!(!store.is_approved("Bash", "git status"));
}

#[test]
fn test_approve_pattern_and_check() {
    let mut store = ApprovalStore::new();
    store.approve_pattern("Bash", "git status");
    assert!(store.is_approved("Bash", "git status"));
    assert!(!store.is_approved("Bash", "rm -rf /"));
}

#[test]
fn test_approve_session_wide() {
    let mut store = ApprovalStore::new();
    store.approve_session("Edit");
    assert!(store.is_approved("Edit", "any-pattern"));
    assert!(store.is_approved("Edit", ""));
    assert!(!store.is_approved("Bash", "something"));
}

#[test]
fn test_wildcard_pattern_match() {
    let mut store = ApprovalStore::new();
    store.approve_pattern("Bash", "git *");
    assert!(store.is_approved("Bash", "git status"));
    assert!(store.is_approved("Bash", "git push origin main"));
    assert!(!store.is_approved("Bash", "npm test"));
}

#[test]
fn test_star_wildcard_matches_all() {
    let mut store = ApprovalStore::new();
    store.approve_pattern("Bash", "*");
    assert!(store.is_approved("Bash", "anything"));
    assert!(store.is_approved("Bash", "git push"));
}

#[test]
fn test_prefix_wildcard_without_space() {
    let mut store = ApprovalStore::new();
    store.approve_pattern("Bash", "npm*");
    assert!(store.is_approved("Bash", "npm"));
    assert!(store.is_approved("Bash", "npmtest"));
    assert!(store.is_approved("Bash", "npm run build"));
    assert!(!store.is_approved("Bash", "git status"));
}

#[test]
fn test_clear_removes_all() {
    let mut store = ApprovalStore::new();
    store.approve_pattern("Bash", "git *");
    store.approve_session("Edit");
    assert!(store.is_approved("Bash", "git status"));
    assert!(store.is_approved("Edit", "anything"));

    store.clear();
    assert!(!store.is_approved("Bash", "git status"));
    assert!(!store.is_approved("Edit", "anything"));
}

#[test]
fn test_different_tools_isolated() {
    let mut store = ApprovalStore::new();
    store.approve_pattern("Bash", "git *");
    assert!(!store.is_approved("Edit", "git status"));
}

#[test]
fn test_serialization_round_trip() {
    let mut store = ApprovalStore::new();
    store.approve_pattern("Bash", "git *");
    store.approve_session("Read");

    let json = serde_json::to_string(&store).expect("serialize");
    let restored: ApprovalStore = serde_json::from_str(&json).expect("deserialize");

    assert!(restored.is_approved("Bash", "git push"));
    assert!(restored.is_approved("Read", "anything"));
    assert!(!restored.is_approved("Write", "foo"));
}
