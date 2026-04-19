//! Keybinding bridge — maps key events to TUI commands.
//!
//! Determines the active keybinding context from state, then resolves
//! key events to commands. Context priority:
//!
//!   overlay > autocomplete > global > input
//!
//! TS: src/keybindings/ + event/handler.rs in cocode-rs

use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyModifiers;

use crate::events::TuiCommand;
use crate::state::AppState;
use crate::state::Overlay;

/// Keybinding context — determines which key mappings are active.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeybindingContext {
    /// Permission/question overlay — Y/N/A approval.
    Confirmation,
    /// Filterable list overlay (model picker, command palette, etc.).
    Picker,
    /// Scrollable content overlay (help, diff view, task detail, doctor).
    Scrollable,
    /// Autocomplete suggestions visible.
    Autocomplete,
    /// Tabbed settings overlay — Tab/Shift+Tab cycle tabs, Up/Down nav.
    Settings,
    /// Default chat input context.
    Chat,
}

/// Determine the active keybinding context from state.
pub fn active_context(state: &AppState) -> KeybindingContext {
    if let Some(ref overlay) = state.ui.overlay {
        return match overlay {
            // Filterable list overlays
            Overlay::ModelPicker(_)
            | Overlay::CommandPalette(_)
            | Overlay::SessionBrowser(_)
            | Overlay::GlobalSearch(_)
            | Overlay::QuickOpen(_)
            | Overlay::Export(_)
            | Overlay::Feedback(_)
            | Overlay::McpServerSelect(_)
            | Overlay::Rewind(_) => KeybindingContext::Picker,

            // Scrollable read-only overlays
            Overlay::Help
            | Overlay::DiffView(_)
            | Overlay::TaskDetail(_)
            | Overlay::Doctor(_)
            | Overlay::ContextVisualization => KeybindingContext::Scrollable,

            // Tabbed settings overlay
            Overlay::Settings(_) => KeybindingContext::Settings,

            // All others are confirmation/approval overlays
            _ => KeybindingContext::Confirmation,
        };
    }

    // Autocomplete popup active: Up/Down/Tab/Esc route to suggestion
    // navigation; all other keys fall through to normal input editing.
    //
    // Gate on non-empty items — async triggers (File/Symbol) install the
    // query before search results arrive, and we must not hijack arrow
    // keys during that window.
    if state
        .ui
        .active_suggestions
        .as_ref()
        .is_some_and(|s| !s.items.is_empty())
    {
        return KeybindingContext::Autocomplete;
    }

    KeybindingContext::Chat
}

/// Map a key event to a TUI command based on the active context.
pub fn map_key(state: &AppState, key: KeyEvent) -> Option<TuiCommand> {
    let ctx = active_context(state);

    match ctx {
        KeybindingContext::Confirmation => map_confirmation_key(key),
        KeybindingContext::Picker => map_picker_key(key),
        KeybindingContext::Scrollable => map_scrollable_key(key),
        // Autocomplete intercepts navigation keys only; other keys fall
        // through to input editing so the user keeps typing and the
        // suggestion popup refreshes reactively.
        KeybindingContext::Autocomplete => map_autocomplete_key(key)
            .or_else(|| map_global_key(state, key))
            .or_else(|| map_input_key(state, key)),
        KeybindingContext::Settings => map_settings_key(key),
        KeybindingContext::Chat => map_global_key(state, key).or_else(|| map_input_key(state, key)),
    }
}

/// Keys for the tabbed Settings overlay: Tab cycles tabs, Up/Down nav,
/// Enter selects, Esc closes.
fn map_settings_key(key: KeyEvent) -> Option<TuiCommand> {
    match key.code {
        KeyCode::Tab => Some(TuiCommand::SettingsNextTab),
        KeyCode::BackTab => Some(TuiCommand::SettingsPrevTab),
        KeyCode::Up => Some(TuiCommand::OverlayPrev),
        KeyCode::Down => Some(TuiCommand::OverlayNext),
        KeyCode::Enter => Some(TuiCommand::OverlayConfirm),
        KeyCode::Esc => Some(TuiCommand::Cancel),
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            Some(TuiCommand::Cancel)
        }
        _ => None,
    }
}

/// Keys for permission/question/elicitation/approval overlays.
fn map_confirmation_key(key: KeyEvent) -> Option<TuiCommand> {
    match key.code {
        KeyCode::Char('y' | 'Y') => Some(TuiCommand::Approve),
        KeyCode::Char('n' | 'N') => Some(TuiCommand::Deny),
        KeyCode::Char('a' | 'A') => Some(TuiCommand::ApproveAll),
        // Tab cycles multi-option confirmations (PlanExit approval
        // target: Restore / AcceptEdits / Bypass). For simple Y/N
        // dialogs the handler is a no-op.
        KeyCode::Tab => Some(TuiCommand::OverlayNext),
        KeyCode::BackTab => Some(TuiCommand::OverlayPrev),
        KeyCode::Up | KeyCode::Char('k') => Some(TuiCommand::OverlayPrev),
        KeyCode::Down | KeyCode::Char('j') => Some(TuiCommand::OverlayNext),
        KeyCode::Enter => Some(TuiCommand::OverlayConfirm),
        KeyCode::Esc => Some(TuiCommand::Cancel),
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            Some(TuiCommand::Cancel)
        }
        _ => None,
    }
}

/// Keys for filterable list overlays (model picker, command palette, etc.).
fn map_picker_key(key: KeyEvent) -> Option<TuiCommand> {
    match key.code {
        KeyCode::Up => Some(TuiCommand::OverlayPrev),
        KeyCode::Down => Some(TuiCommand::OverlayNext),
        KeyCode::Enter => Some(TuiCommand::OverlayConfirm),
        KeyCode::Esc => Some(TuiCommand::Cancel),
        KeyCode::Backspace => Some(TuiCommand::OverlayFilterBackspace),
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            Some(TuiCommand::Cancel)
        }
        KeyCode::Char(c) => Some(TuiCommand::OverlayFilter(c)),
        _ => None,
    }
}

/// Keys for scrollable read-only overlays (help, diff, doctor, etc.).
fn map_scrollable_key(key: KeyEvent) -> Option<TuiCommand> {
    match key.code {
        KeyCode::Esc | KeyCode::Char('q') => Some(TuiCommand::Cancel),
        KeyCode::Up | KeyCode::Char('k') => Some(TuiCommand::OverlayPrev),
        KeyCode::Down | KeyCode::Char('j') => Some(TuiCommand::OverlayNext),
        KeyCode::PageUp => Some(TuiCommand::PageUp),
        KeyCode::PageDown => Some(TuiCommand::PageDown),
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            Some(TuiCommand::Cancel)
        }
        _ => None,
    }
}

/// Keys for autocomplete suggestions.
fn map_autocomplete_key(key: KeyEvent) -> Option<TuiCommand> {
    match key.code {
        KeyCode::Up => Some(TuiCommand::OverlayPrev),
        KeyCode::Down => Some(TuiCommand::OverlayNext),
        KeyCode::Tab | KeyCode::Enter => Some(TuiCommand::OverlayConfirm),
        KeyCode::Esc => Some(TuiCommand::Cancel),
        _ => None,
    }
}

/// Global keyboard shortcuts (active in Chat context).
fn map_global_key(state: &AppState, key: KeyEvent) -> Option<TuiCommand> {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let shift = key.modifiers.contains(KeyModifiers::SHIFT);
    let alt = key.modifiers.contains(KeyModifiers::ALT);

    match key.code {
        // Ctrl shortcuts
        KeyCode::Char('c') if ctrl => Some(TuiCommand::Interrupt),
        KeyCode::Char('q') if ctrl => Some(TuiCommand::Quit),
        KeyCode::Char('l') if ctrl => Some(TuiCommand::ClearScreen),
        KeyCode::Char('t') if ctrl && shift => Some(TuiCommand::ToggleThinking),
        KeyCode::Char('t') if ctrl => Some(TuiCommand::CycleThinkingLevel),
        KeyCode::Char('e') if ctrl && shift => Some(TuiCommand::ToggleToolCollapse),
        KeyCode::Char('e') if ctrl => Some(TuiCommand::OpenExternalEditor),
        KeyCode::Char('r') if ctrl && shift => Some(TuiCommand::ToggleSystemReminders),
        // Spec (crate-coco-tui.md §Keyboard Shortcuts): Ctrl+F = kill all agents,
        // Ctrl+Shift+F = toggle fast mode. Global search is reached via
        // /search in the command palette — no dedicated hotkey.
        KeyCode::Char('f') if ctrl && shift => Some(TuiCommand::ToggleFastMode),
        KeyCode::Char('f') if ctrl => Some(TuiCommand::KillAllAgents),
        KeyCode::Char('p') if ctrl => Some(TuiCommand::ShowCommandPalette),
        KeyCode::Char('s') if ctrl => Some(TuiCommand::ShowSessionBrowser),
        // Ctrl+O: copy the last agent response (codex-rs parity). Fallback
        // to Quick Open when Shift is also held so power users still have
        // access to it — Shift+Ctrl+O = open, Ctrl+O = copy.
        KeyCode::Char('o') if ctrl && shift => Some(TuiCommand::ShowQuickOpen),
        KeyCode::Char('o') if ctrl => Some(TuiCommand::CopyLastMessage),
        KeyCode::Char('b') if ctrl => Some(TuiCommand::BackgroundAllTasks),
        KeyCode::Char('v') if ctrl || alt => Some(TuiCommand::PasteFromClipboard),
        KeyCode::Char('m') if ctrl => Some(TuiCommand::CycleModel),
        KeyCode::Char('g') if ctrl => Some(TuiCommand::OpenPlanEditor),
        KeyCode::Char('w') if ctrl => Some(TuiCommand::ShowContextViz),
        KeyCode::Char(',') if ctrl => Some(TuiCommand::ShowSettings),

        // Tab
        KeyCode::Tab => Some(TuiCommand::TogglePlanMode),
        KeyCode::BackTab => Some(TuiCommand::CyclePermissionMode),

        // Help
        KeyCode::Char('?') if state.ui.input.is_empty() => Some(TuiCommand::ShowHelp),
        KeyCode::F(1) => Some(TuiCommand::ShowHelp),

        // Scrolling
        KeyCode::PageUp => Some(TuiCommand::PageUp),
        KeyCode::PageDown => Some(TuiCommand::PageDown),

        // Focus
        KeyCode::F(6) => Some(TuiCommand::FocusNext),

        _ => None,
    }
}

/// Input editing keys (lowest priority).
fn map_input_key(state: &AppState, key: KeyEvent) -> Option<TuiCommand> {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let alt = key.modifiers.contains(KeyModifiers::ALT);
    let shift = key.modifiers.contains(KeyModifiers::SHIFT);
    let is_streaming = state.is_streaming();

    match key.code {
        // Submit / queue
        KeyCode::Enter if shift || alt => Some(TuiCommand::InsertNewline),
        KeyCode::Enter if is_streaming => Some(TuiCommand::QueueInput),
        KeyCode::Enter => Some(TuiCommand::SubmitInput),

        // Editing
        KeyCode::Backspace if ctrl || alt => Some(TuiCommand::DeleteWordBackward),
        KeyCode::Backspace => Some(TuiCommand::DeleteBackward),
        KeyCode::Delete if ctrl => Some(TuiCommand::DeleteWordForward),
        KeyCode::Delete => Some(TuiCommand::DeleteForward),

        // Cursor
        KeyCode::Left if ctrl || alt => Some(TuiCommand::WordLeft),
        KeyCode::Left => Some(TuiCommand::CursorLeft),
        KeyCode::Right if ctrl || alt => Some(TuiCommand::WordRight),
        KeyCode::Right => Some(TuiCommand::CursorRight),
        KeyCode::Up if alt => Some(TuiCommand::ScrollUp),
        KeyCode::Up => Some(TuiCommand::CursorUp),
        KeyCode::Down if alt => Some(TuiCommand::ScrollDown),
        KeyCode::Down => Some(TuiCommand::CursorDown),
        KeyCode::Home => Some(TuiCommand::CursorHome),
        KeyCode::End => Some(TuiCommand::CursorEnd),

        // Emacs
        KeyCode::Char('a') if ctrl => Some(TuiCommand::CursorHome),
        KeyCode::Char('e') if ctrl => Some(TuiCommand::CursorEnd),
        KeyCode::Char('k') if ctrl => Some(TuiCommand::KillToEndOfLine),
        KeyCode::Char('y') if ctrl => Some(TuiCommand::Yank),
        KeyCode::Char('j') if ctrl => Some(TuiCommand::InsertNewline),
        // Emacs word-nav: Alt+b / Alt+f (TS PromptInput.tsx)
        KeyCode::Char('b') if alt => Some(TuiCommand::WordLeft),
        KeyCode::Char('f') if alt => Some(TuiCommand::WordRight),

        // Escape — double-Esc opens rewind when input is empty + messages exist.
        // TS: useDoublePress() in PromptInput.tsx
        KeyCode::Esc => {
            let now = std::time::Instant::now();
            let is_double = state
                .ui
                .last_esc_time
                .is_some_and(|t| now.duration_since(t) < crate::constants::DOUBLE_ESC_THRESHOLD);
            if is_double
                && state.ui.input.is_empty()
                && !state.session.messages.is_empty()
                && state.ui.overlay.is_none()
            {
                Some(TuiCommand::ShowRewind)
            } else {
                Some(TuiCommand::Cancel)
            }
        }

        // Character input
        KeyCode::Char(c) => Some(TuiCommand::InsertChar(c)),

        _ => None,
    }
}

#[cfg(test)]
#[path = "keybinding_bridge.test.rs"]
mod tests;
