use std::str::FromStr;

use pretty_assertions::assert_eq;

use super::*;

#[test]
fn test_action_as_str() {
    assert_eq!(Action::AppInterrupt.as_str(), "app:interrupt");
    assert_eq!(Action::ChatSubmit.as_str(), "chat:submit");
    assert_eq!(Action::ConfirmYes.as_str(), "confirm:yes");
    assert_eq!(Action::AutocompleteAccept.as_str(), "autocomplete:accept");
    assert_eq!(Action::ExtTogglePlanMode.as_str(), "ext:togglePlanMode");
}

#[test]
fn test_action_from_str_roundtrip() {
    let actions = [
        "app:interrupt",
        "app:toggleTerminal",
        "app:globalSearch",
        "app:quickOpen",
        "chat:submit",
        "chat:killAgents",
        "confirm:yes",
        "autocomplete:accept",
        "attachments:exit",
        "footer:openSelected",
        "footer:clearSelection",
        "messageSelector:up",
        "messageSelector:down",
        "messageSelector:top",
        "messageSelector:bottom",
        "messageSelector:select",
        "diff:dismiss",
        "diff:previousSource",
        "diff:nextSource",
        "diff:back",
        "diff:viewDetails",
        "diff:previousFile",
        "diff:nextFile",
        "modelPicker:decreaseEffort",
        "modelPicker:increaseEffort",
        "transcript:toggleShowAll",
        "transcript:exit",
        "historySearch:execute",
        "theme:toggleSyntaxHighlighting",
        "settings:search",
        "settings:retry",
        "plugin:toggle",
        "plugin:install",
        "ext:togglePlanMode",
        "ext:cycleThinkingLevel",
    ];
    for s in actions {
        let action = Action::from_str(s).unwrap();
        assert_eq!(action.as_str(), s, "roundtrip failed for {s}");
    }
}

#[test]
fn test_command_action() {
    let action = Action::from_str("command:doctor").unwrap();
    assert_eq!(
        action,
        Action::Command(CommandAction {
            name: "doctor".to_string()
        })
    );
    assert_eq!(action.to_string(), "command:doctor");
}

#[test]
fn test_unknown_action() {
    assert!(Action::from_str("unknown:action").is_err());
    assert!(Action::from_str("").is_err());
    assert!(Action::from_str("nocolon").is_err());
}

#[test]
fn test_display_for_named_action() {
    assert_eq!(Action::ChatCancel.to_string(), "chat:cancel");
}

#[test]
fn test_display_for_command_action() {
    let action = Action::Command(CommandAction {
        name: "my-skill".to_string(),
    });
    assert_eq!(action.to_string(), "command:my-skill");
}
