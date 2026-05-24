use crossterm::event::KeyCode;
use crossterm::event::KeyModifiers;

use crate::action::Action;
use crate::context::KeybindingContext;

use super::*;

#[test]
fn test_default_bindings_not_empty() {
    let bindings = default_bindings();
    assert!(!bindings.is_empty());
}

#[test]
fn test_default_bindings_include_ctrl_c() {
    let bindings = default_bindings();
    let has_ctrl_c = bindings
        .iter()
        .any(|b| b.action == Action::AppInterrupt && b.context == KeybindingContext::Global);
    assert!(has_ctrl_c, "must include Ctrl+C -> AppInterrupt");
}

#[test]
fn test_default_bindings_include_enter_submit() {
    let bindings = default_bindings();
    let has_enter = bindings
        .iter()
        .any(|b| b.action == Action::ChatSubmit && b.context == KeybindingContext::Chat);
    assert!(has_enter, "must include Enter -> ChatSubmit");
}

#[test]
fn test_default_bindings_include_esc_esc_chord() {
    let bindings = default_bindings();
    let has_chord = bindings
        .iter()
        .any(|b| b.action == Action::ExtShowRewindSelector && b.sequence.is_chord());
    assert!(has_chord, "must include Esc Esc -> ShowRewindSelector");
}

#[test]
fn test_default_bindings_include_autocomplete() {
    let bindings = default_bindings();
    let count = bindings
        .iter()
        .filter(|b| b.context == KeybindingContext::Autocomplete)
        .count();
    assert!(
        count >= 4,
        "autocomplete needs at least accept, dismiss, prev, next"
    );
}

#[test]
fn test_default_bindings_include_confirmation() {
    let bindings = default_bindings();
    let has_yes = bindings
        .iter()
        .any(|b| b.action == Action::ConfirmYes && b.context == KeybindingContext::Confirmation);
    let has_no = bindings
        .iter()
        .any(|b| b.action == Action::ConfirmNo && b.context == KeybindingContext::Confirmation);
    assert!(has_yes, "must have Y -> ConfirmYes");
    assert!(has_no, "must have N -> ConfirmNo");
}

#[test]
fn test_default_bindings_page_navigation_not_help() {
    let bindings = default_bindings();
    let page_bindings: Vec<_> = bindings
        .iter()
        .filter(|b| {
            matches!(
                b.sequence.keys().first().map(|k| k.code),
                Some(KeyCode::PageUp | KeyCode::PageDown)
            )
        })
        .collect();
    // None should map to ExtShowHelp (that was the placeholder bug).
    for b in &page_bindings {
        assert_ne!(
            b.action,
            Action::ExtShowHelp,
            "PageUp/PageDown must not map to ExtShowHelp"
        );
    }
}

#[test]
fn test_default_bindings_ctrl_shift_t_is_toggle_thinking() {
    let bindings = default_bindings();
    let ctrl_shift = KeyModifiers::CONTROL.union(KeyModifiers::SHIFT);
    let has_thinking = bindings.iter().any(|b| {
        b.context == KeybindingContext::Chat
            && b.action == Action::ExtToggleThinking
            && b.sequence.keys().first().map(|k| k.modifiers) == Some(ctrl_shift)
    });
    assert!(has_thinking, "Ctrl+Shift+T must map to ExtToggleThinking");
}

#[test]
fn test_default_bindings_no_global_esc() {
    let bindings = default_bindings();
    let global_esc = bindings.iter().any(|b| {
        b.context == KeybindingContext::Global
            && b.sequence.keys().first().map(|k| k.code) == Some(KeyCode::Esc)
            && !b.sequence.is_chord()
    });
    assert!(
        !global_esc,
        "single Esc must not be in Global (breaks Esc-Esc chord)"
    );
}

#[test]
fn test_default_bindings_chat_has_single_esc() {
    let bindings = default_bindings();
    let chat_esc = bindings.iter().any(|b| {
        b.context == KeybindingContext::Chat
            && b.action == Action::ChatCancel
            && !b.sequence.is_chord()
            && b.sequence.keys().first().map(|k| k.code) == Some(KeyCode::Esc)
    });
    assert!(chat_esc, "Chat context must have single Esc -> ChatCancel");
}

/// Verify all bindings from handler.rs have equivalents in defaults.
#[test]
fn test_default_bindings_cover_handler_rs() {
    let bindings = default_bindings();

    // Essential handler.rs bindings that must exist.
    let required = [
        (KeyModifiers::CONTROL, KeyCode::Char('c'), "AppInterrupt"),
        (KeyModifiers::NONE, KeyCode::Tab, "ExtTogglePlanMode"),
        (
            KeyModifiers::CONTROL,
            KeyCode::Char('t'),
            "ExtCycleThinkingLevel",
        ),
        (KeyModifiers::CONTROL, KeyCode::Char('m'), "ExtCycleModel"),
        (
            KeyModifiers::CONTROL,
            KeyCode::Char('b'),
            "ExtBackgroundAllTasks",
        ),
        (KeyModifiers::CONTROL, KeyCode::Char('f'), "ChatKillAgents"),
        (
            KeyModifiers::CONTROL,
            KeyCode::Char('e'),
            "ChatExternalEditor",
        ),
        (
            KeyModifiers::CONTROL,
            KeyCode::Char('p'),
            "ExtShowCommandPalette",
        ),
        (
            KeyModifiers::CONTROL,
            KeyCode::Char('s'),
            "ExtShowSessionBrowser",
        ),
        (KeyModifiers::CONTROL, KeyCode::Char('l'), "ExtClearScreen"),
        (KeyModifiers::CONTROL, KeyCode::Char('q'), "ExtQuit"),
        (KeyModifiers::CONTROL, KeyCode::Char('v'), "ChatImagePaste"),
        (KeyModifiers::ALT, KeyCode::Char('v'), "ChatImagePaste"),
        (KeyModifiers::NONE, KeyCode::Enter, "ChatSubmit"),
        (KeyModifiers::SHIFT, KeyCode::Enter, "ExtInsertNewline"),
        (KeyModifiers::NONE, KeyCode::Backspace, "ExtDeleteBackward"),
        (
            KeyModifiers::CONTROL,
            KeyCode::Backspace,
            "ExtDeleteWordBackward",
        ),
        (KeyModifiers::NONE, KeyCode::Delete, "ExtDeleteForward"),
        (
            KeyModifiers::CONTROL,
            KeyCode::Delete,
            "ExtDeleteWordForward",
        ),
        (
            KeyModifiers::CONTROL,
            KeyCode::Char('k'),
            "ExtKillToEndOfLine",
        ),
        (KeyModifiers::CONTROL, KeyCode::Char('y'), "ExtYank"),
        (KeyModifiers::NONE, KeyCode::Left, "ExtCursorLeft"),
        (KeyModifiers::NONE, KeyCode::Right, "ExtCursorRight"),
        (KeyModifiers::NONE, KeyCode::Home, "ExtCursorHome"),
        (KeyModifiers::NONE, KeyCode::End, "ExtCursorEnd"),
        (KeyModifiers::CONTROL, KeyCode::Left, "ExtWordLeft"),
        (KeyModifiers::CONTROL, KeyCode::Right, "ExtWordRight"),
        (KeyModifiers::NONE, KeyCode::PageUp, "ExtPageUp"),
        (KeyModifiers::NONE, KeyCode::PageDown, "ExtPageDown"),
        // Additional bindings from handler.rs not in original list
        (KeyModifiers::CONTROL, KeyCode::Enter, "ChatSubmit"),
        (KeyModifiers::ALT, KeyCode::Enter, "ExtInsertNewline"),
        (KeyModifiers::ALT, KeyCode::Up, "ExtScrollUp"),
        (KeyModifiers::ALT, KeyCode::Down, "ExtScrollDown"),
        (KeyModifiers::CONTROL, KeyCode::Up, "ExtPageUp"),
        (KeyModifiers::CONTROL, KeyCode::Down, "ExtPageDown"),
        (
            KeyModifiers::CONTROL,
            KeyCode::Char('g'),
            "ExtOpenPlanEditor",
        ),
        (KeyModifiers::CONTROL, KeyCode::Char('a'), "ExtSelectAll"),
        (KeyModifiers::NONE, KeyCode::F(1), "ExtShowHelp"),
        (KeyModifiers::NONE, KeyCode::Up, "ExtCursorUp"),
        (KeyModifiers::NONE, KeyCode::Down, "ExtCursorDown"),
    ];

    for (mods, code, action_name) in required {
        let found = bindings.iter().any(|b| {
            b.sequence.keys().len() == 1
                && b.sequence.keys()[0].modifiers == mods
                && b.sequence.keys()[0].code == code
        });
        assert!(
            found,
            "missing binding for {action_name} ({mods:?}+{code:?})"
        );
    }
}
