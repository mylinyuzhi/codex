//! Shared test utilities.

use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyEventKind;
use crossterm::event::KeyEventState;
use crossterm::event::KeyModifiers;

use crate::action::Action;
use crate::context::KeybindingContext;
use crate::key::KeyCombo;
use crate::key::KeySequence;
use crate::resolver::Binding;

/// Create a crossterm key event for testing.
pub fn make_key_event(modifiers: KeyModifiers, code: KeyCode) -> KeyEvent {
    KeyEvent {
        code,
        modifiers,
        kind: KeyEventKind::Press,
        state: KeyEventState::empty(),
    }
}

/// Create a single-key binding for testing.
pub fn make_binding(
    ctx: KeybindingContext,
    mods: KeyModifiers,
    code: KeyCode,
    action: Action,
) -> Binding {
    Binding {
        context: ctx,
        sequence: KeySequence::single(KeyCombo::new(mods, code)),
        action,
    }
}
