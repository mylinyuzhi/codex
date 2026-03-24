use crossterm::event::KeyCode;
use crossterm::event::KeyModifiers;
use pretty_assertions::assert_eq;

use crate::action::Action;
use crate::context::KeybindingContext;
use crate::test_helpers::make_binding;

use super::*;

fn make_ctrl_t_binding(action: Action) -> crate::resolver::Binding {
    make_binding(
        KeybindingContext::Chat,
        KeyModifiers::CONTROL,
        KeyCode::Char('t'),
        action,
    )
}

#[test]
fn test_merge_empty_user() {
    let defaults = vec![make_ctrl_t_binding(Action::ExtCycleThinkingLevel)];
    let user = vec![];
    let merged = merge_bindings(defaults, user);
    assert_eq!(merged.len(), 1);
    assert_eq!(merged[0].action, Action::ExtCycleThinkingLevel);
}

#[test]
fn test_merge_user_appended_after_defaults() {
    let defaults = vec![make_ctrl_t_binding(Action::ExtCycleThinkingLevel)];
    let user = vec![make_ctrl_t_binding(Action::AppToggleTodos)];
    let merged = merge_bindings(defaults, user);
    assert_eq!(merged.len(), 2);
    // User binding comes after default.
    assert_eq!(merged[0].action, Action::ExtCycleThinkingLevel);
    assert_eq!(merged[1].action, Action::AppToggleTodos);
}

#[test]
fn test_merge_both_empty() {
    let merged = merge_bindings(vec![], vec![]);
    assert!(merged.is_empty());
}
