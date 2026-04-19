use super::ValidationIssue;
use super::validate;
use crate::Keybinding;

fn binding(key: &str, action: &str, context: Option<&str>) -> Keybinding {
    Keybinding {
        key: key.into(),
        action: action.into(),
        context: context.map(str::to_string),
        when: None,
    }
}

#[test]
fn valid_list_produces_no_issues() {
    let list = vec![
        binding("ctrl+a", "select_all", Some("input")),
        binding("enter", "submit", Some("input")),
        binding("esc", "cancel", None),
    ];
    assert!(validate(&list).is_empty());
}

#[test]
fn parse_error_is_reported() {
    let list = vec![binding("", "noop", None)];
    let issues = validate(&list);
    assert!(matches!(issues[0], ValidationIssue::ParseError { .. }));
}

#[test]
fn duplicate_same_action_ignored() {
    // Same key + same action == declared twice; not an error.
    let list = vec![
        binding("ctrl+a", "select_all", Some("input")),
        binding("ctrl+a", "select_all", Some("input")),
    ];
    assert!(validate(&list).is_empty());
}

#[test]
fn duplicate_conflicting_is_reported() {
    let list = vec![
        binding("ctrl+a", "select_all", Some("input")),
        binding("Ctrl+A", "do_other_thing", Some("input")),
    ];
    let issues = validate(&list);
    assert_eq!(issues.len(), 1);
    let ValidationIssue::DuplicateBinding {
        first_action,
        later_action,
        ..
    } = &issues[0]
    else {
        panic!("expected DuplicateBinding, got {:?}", issues[0]);
    };
    assert_eq!(first_action, "select_all");
    assert_eq!(later_action, "do_other_thing");
}

#[test]
fn different_contexts_do_not_conflict() {
    let list = vec![
        binding("enter", "submit", Some("input")),
        binding("enter", "select", Some("search")),
    ];
    assert!(validate(&list).is_empty());
}
