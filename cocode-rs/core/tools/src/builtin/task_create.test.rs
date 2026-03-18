use super::structured_tasks;
use super::*;

// ── derive_active_form ──────────────────────────────────────────

#[test]
fn test_derive_active_form_common_verbs() {
    assert_eq!(
        structured_tasks::derive_active_form("Fix auth bug"),
        "Fixing auth bug"
    );
    assert_eq!(
        structured_tasks::derive_active_form("Add logging"),
        "Adding logging"
    );
    assert_eq!(
        structured_tasks::derive_active_form("Update config"),
        "Updating config"
    );
    assert_eq!(
        structured_tasks::derive_active_form("Remove dead code"),
        "Removing dead code"
    );
    assert_eq!(
        structured_tasks::derive_active_form("Implement feature"),
        "Implementing feature"
    );
    assert_eq!(
        structured_tasks::derive_active_form("Refactor module"),
        "Refactoring module"
    );
    assert_eq!(
        structured_tasks::derive_active_form("Deploy service"),
        "Deploying service"
    );
    assert_eq!(
        structured_tasks::derive_active_form("Debug crash"),
        "Debugging crash"
    );
}

#[test]
fn test_derive_active_form_case_insensitive() {
    assert_eq!(
        structured_tasks::derive_active_form("fix auth bug"),
        "Fixing auth bug"
    );
    assert_eq!(
        structured_tasks::derive_active_form("add logging"),
        "Adding logging"
    );
}

#[test]
fn test_derive_active_form_unknown_verb() {
    assert_eq!(
        structured_tasks::derive_active_form("Foo bar"),
        "Working on: Foo bar"
    );
}

#[test]
fn test_derive_active_form_empty() {
    assert_eq!(structured_tasks::derive_active_form(""), "Working on task");
    assert_eq!(
        structured_tasks::derive_active_form("  "),
        "Working on task"
    );
}

#[test]
fn test_derive_active_form_verb_only() {
    assert_eq!(structured_tasks::derive_active_form("Fix"), "Fixing");
}

#[test]
fn test_derive_active_form_preserves_subject_case() {
    // Verb is matched case-insensitively but rest of subject preserves case
    assert_eq!(
        structured_tasks::derive_active_form("fix AuthManager Bug"),
        "Fixing AuthManager Bug"
    );
}
