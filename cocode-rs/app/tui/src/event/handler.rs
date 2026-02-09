//! Key event handler for mapping keyboard input to commands.
//!
//! This module converts raw crossterm key events into high-level
//! [`TuiCommand`]s that can be processed by the application.

use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyModifiers;

use super::TuiCommand;

/// Handle a key event and return the corresponding command.
///
/// This function maps keyboard input to application commands based on
/// the current focus state and modifiers.
///
/// # Arguments
///
/// * `key` - The key event to handle
/// * `has_overlay` - Whether an overlay (e.g., permission prompt) is active
/// * `has_file_suggestions` - Whether file suggestions are being displayed
///
/// # Returns
///
/// The command to execute, if any.
pub fn handle_key_event(key: KeyEvent, has_overlay: bool) -> Option<TuiCommand> {
    // Handle overlay-specific keys first
    if has_overlay {
        return handle_overlay_key(key);
    }

    // Handle global shortcuts (with modifiers)
    if let Some(cmd) = handle_global_key(key) {
        return Some(cmd);
    }

    // Handle input editing keys
    handle_input_key(key)
}

/// Handle a key event with file and skill suggestion state.
///
/// When suggestions are active, some keys are redirected to
/// suggestion navigation. Skill suggestions take priority over file suggestions.
pub fn handle_key_event_with_suggestions(
    key: KeyEvent,
    has_overlay: bool,
    has_file_suggestions: bool,
) -> Option<TuiCommand> {
    handle_key_event_full(key, has_overlay, has_file_suggestions, false, false, false)
}

/// Handle a key event with full context including streaming state.
///
/// This is the most complete handler that supports:
/// - Overlay handling
/// - File, skill, agent, and symbol suggestion navigation
/// - Queue/steering behavior based on streaming state
pub fn handle_key_event_full(
    key: KeyEvent,
    has_overlay: bool,
    has_file_suggestions: bool,
    has_skill_suggestions: bool,
    has_agent_suggestions: bool,
    is_streaming: bool,
) -> Option<TuiCommand> {
    handle_key_event_full_with_symbols(
        key,
        has_overlay,
        has_file_suggestions,
        has_skill_suggestions,
        has_agent_suggestions,
        false,
        is_streaming,
    )
}

/// Handle a key event with full context including symbol suggestions.
///
/// Priority: overlay > skill > agent > symbol > file > global > input
pub fn handle_key_event_full_with_symbols(
    key: KeyEvent,
    has_overlay: bool,
    has_file_suggestions: bool,
    has_skill_suggestions: bool,
    has_agent_suggestions: bool,
    has_symbol_suggestions: bool,
    is_streaming: bool,
) -> Option<TuiCommand> {
    // Handle overlay-specific keys first
    if has_overlay {
        return handle_overlay_key(key);
    }

    // Handle skill suggestion navigation (highest priority)
    if has_skill_suggestions {
        if let Some(cmd) = handle_skill_suggestion_key(key) {
            return Some(cmd);
        }
    }

    // Handle agent suggestion navigation
    if has_agent_suggestions {
        if let Some(cmd) = handle_agent_suggestion_key(key) {
            return Some(cmd);
        }
    }

    // Handle symbol suggestion navigation
    if has_symbol_suggestions {
        if let Some(cmd) = handle_symbol_suggestion_key(key) {
            return Some(cmd);
        }
    }

    // Handle file suggestion navigation
    if has_file_suggestions {
        if let Some(cmd) = handle_suggestion_key(key) {
            return Some(cmd);
        }
    }

    // Handle global shortcuts (with modifiers)
    if let Some(cmd) = handle_global_key(key) {
        return Some(cmd);
    }

    // Handle input editing keys with streaming context
    handle_input_key_with_streaming(key, is_streaming)
}

/// Handle keys for file suggestion navigation.
fn handle_suggestion_key(key: KeyEvent) -> Option<TuiCommand> {
    match (key.modifiers, key.code) {
        // Navigate suggestions
        (KeyModifiers::NONE, KeyCode::Up) => Some(TuiCommand::SelectPrevSuggestion),
        (KeyModifiers::NONE, KeyCode::Down) => Some(TuiCommand::SelectNextSuggestion),

        // Accept suggestion
        (KeyModifiers::NONE, KeyCode::Tab) => Some(TuiCommand::AcceptSuggestion),
        (KeyModifiers::NONE, KeyCode::Enter) => Some(TuiCommand::AcceptSuggestion),

        // Dismiss suggestions
        (KeyModifiers::NONE, KeyCode::Esc) => Some(TuiCommand::DismissSuggestions),

        // Other keys pass through to normal handling
        _ => None,
    }
}

/// Handle keys for skill suggestion navigation.
fn handle_skill_suggestion_key(key: KeyEvent) -> Option<TuiCommand> {
    match (key.modifiers, key.code) {
        // Navigate suggestions
        (KeyModifiers::NONE, KeyCode::Up) => Some(TuiCommand::SelectPrevSkillSuggestion),
        (KeyModifiers::NONE, KeyCode::Down) => Some(TuiCommand::SelectNextSkillSuggestion),

        // Accept suggestion
        (KeyModifiers::NONE, KeyCode::Tab) => Some(TuiCommand::AcceptSkillSuggestion),
        (KeyModifiers::NONE, KeyCode::Enter) => Some(TuiCommand::AcceptSkillSuggestion),

        // Dismiss suggestions
        (KeyModifiers::NONE, KeyCode::Esc) => Some(TuiCommand::DismissSkillSuggestions),

        // Other keys pass through to normal handling
        _ => None,
    }
}

/// Handle keys for symbol suggestion navigation.
fn handle_symbol_suggestion_key(key: KeyEvent) -> Option<TuiCommand> {
    match (key.modifiers, key.code) {
        // Navigate suggestions
        (KeyModifiers::NONE, KeyCode::Up) => Some(TuiCommand::SelectPrevSymbolSuggestion),
        (KeyModifiers::NONE, KeyCode::Down) => Some(TuiCommand::SelectNextSymbolSuggestion),

        // Accept suggestion
        (KeyModifiers::NONE, KeyCode::Tab) => Some(TuiCommand::AcceptSymbolSuggestion),
        (KeyModifiers::NONE, KeyCode::Enter) => Some(TuiCommand::AcceptSymbolSuggestion),

        // Dismiss suggestions
        (KeyModifiers::NONE, KeyCode::Esc) => Some(TuiCommand::DismissSymbolSuggestions),

        // Other keys pass through to normal handling
        _ => None,
    }
}

/// Handle keys for agent suggestion navigation.
fn handle_agent_suggestion_key(key: KeyEvent) -> Option<TuiCommand> {
    match (key.modifiers, key.code) {
        // Navigate suggestions
        (KeyModifiers::NONE, KeyCode::Up) => Some(TuiCommand::SelectPrevAgentSuggestion),
        (KeyModifiers::NONE, KeyCode::Down) => Some(TuiCommand::SelectNextAgentSuggestion),

        // Accept suggestion
        (KeyModifiers::NONE, KeyCode::Tab) => Some(TuiCommand::AcceptAgentSuggestion),
        (KeyModifiers::NONE, KeyCode::Enter) => Some(TuiCommand::AcceptAgentSuggestion),

        // Dismiss suggestions
        (KeyModifiers::NONE, KeyCode::Esc) => Some(TuiCommand::DismissAgentSuggestions),

        // Other keys pass through to normal handling
        _ => None,
    }
}

/// Handle keys when an overlay (permission prompt, model picker) is active.
fn handle_overlay_key(key: KeyEvent) -> Option<TuiCommand> {
    match key.code {
        // Approval shortcuts
        KeyCode::Char('y') | KeyCode::Char('Y') => Some(TuiCommand::Approve),
        KeyCode::Char('n') | KeyCode::Char('N') => Some(TuiCommand::Deny),
        KeyCode::Char('a') | KeyCode::Char('A')
            if key.modifiers.contains(KeyModifiers::CONTROL) =>
        {
            Some(TuiCommand::ApproveAll)
        }

        // Navigation
        KeyCode::Up | KeyCode::Char('k') => Some(TuiCommand::CursorUp),
        KeyCode::Down | KeyCode::Char('j') => Some(TuiCommand::CursorDown),
        KeyCode::Enter => Some(TuiCommand::Approve),

        // Cancel
        KeyCode::Esc => Some(TuiCommand::Cancel),
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            Some(TuiCommand::Cancel)
        }

        // Character input for filter-based overlays
        KeyCode::Char(c) if key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT => {
            Some(TuiCommand::InsertChar(c))
        }

        // Backspace for filter
        KeyCode::Backspace => Some(TuiCommand::DeleteBackward),

        _ => None,
    }
}

/// Handle global shortcuts that work regardless of focus.
fn handle_global_key(key: KeyEvent) -> Option<TuiCommand> {
    match (key.modifiers, key.code) {
        // Plan mode toggle (Tab)
        (KeyModifiers::NONE, KeyCode::Tab) => Some(TuiCommand::TogglePlanMode),

        // Thinking level cycle (Ctrl+T)
        (KeyModifiers::CONTROL, KeyCode::Char('t')) => Some(TuiCommand::CycleThinkingLevel),

        // Model cycle/picker (Ctrl+M)
        (KeyModifiers::CONTROL, KeyCode::Char('m')) => Some(TuiCommand::CycleModel),

        // Interrupt (Ctrl+C)
        (KeyModifiers::CONTROL, KeyCode::Char('c')) => Some(TuiCommand::Interrupt),

        // Clear screen (Ctrl+L)
        (KeyModifiers::CONTROL, KeyCode::Char('l')) => Some(TuiCommand::ClearScreen),

        // External editor (Ctrl+E)
        (KeyModifiers::CONTROL, KeyCode::Char('e')) => Some(TuiCommand::OpenExternalEditor),

        // Command palette (Ctrl+P)
        (KeyModifiers::CONTROL, KeyCode::Char('p')) => Some(TuiCommand::ShowCommandPalette),

        // Session browser (Ctrl+S)
        (KeyModifiers::CONTROL, KeyCode::Char('s')) => Some(TuiCommand::ShowSessionBrowser),

        // Toggle thinking display (Ctrl+Shift+T)
        (m, KeyCode::Char('T'))
            if m.contains(KeyModifiers::CONTROL) && m.contains(KeyModifiers::SHIFT) =>
        {
            Some(TuiCommand::ToggleThinking)
        }

        // Show help (? or F1)
        (KeyModifiers::NONE, KeyCode::F(1)) => Some(TuiCommand::ShowHelp),
        (KeyModifiers::SHIFT, KeyCode::Char('?')) => Some(TuiCommand::ShowHelp),

        // Quit (Ctrl+Q)
        (KeyModifiers::CONTROL, KeyCode::Char('q')) => Some(TuiCommand::Quit),

        // Smart paste from clipboard: image first, text fallback (Ctrl+V)
        (KeyModifiers::CONTROL, KeyCode::Char('v')) => Some(TuiCommand::PasteFromClipboard),

        // Alt+V: Windows fallback where Ctrl+V may be intercepted by terminal
        (KeyModifiers::ALT, KeyCode::Char('v')) => Some(TuiCommand::PasteFromClipboard),

        // Cancel (Escape)
        (_, KeyCode::Esc) => Some(TuiCommand::Cancel),

        // Page up/down with modifiers
        (_, KeyCode::PageUp) => Some(TuiCommand::PageUp),
        (_, KeyCode::PageDown) => Some(TuiCommand::PageDown),
        (KeyModifiers::CONTROL, KeyCode::Up) => Some(TuiCommand::PageUp),
        (KeyModifiers::CONTROL, KeyCode::Down) => Some(TuiCommand::PageDown),

        _ => None,
    }
}

/// Handle input editing keys.
fn handle_input_key(key: KeyEvent) -> Option<TuiCommand> {
    // Delegate to streaming-aware handler with streaming=false
    handle_input_key_with_streaming(key, false)
}

/// Handle input editing keys with streaming context.
///
/// When `is_streaming` is true:
/// - Enter / Ctrl+Enter queues the input for later (QueueInput)
///
/// When `is_streaming` is false:
/// - Enter / Ctrl+Enter submits immediately (SubmitInput)
///
/// Both modes:
/// - Shift+Enter inserts a newline (for multi-line input)
/// - Alt+Enter inserts a newline (for multi-line input)
fn handle_input_key_with_streaming(key: KeyEvent, is_streaming: bool) -> Option<TuiCommand> {
    match (key.modifiers, key.code) {
        // Enter / Ctrl+Enter: Submit or Queue depending on streaming state
        (KeyModifiers::NONE | KeyModifiers::CONTROL, KeyCode::Enter) => {
            if is_streaming {
                Some(TuiCommand::QueueInput)
            } else {
                Some(TuiCommand::SubmitInput)
            }
        }

        // Shift+Enter: Insert newline (aligned with Claude Code behavior)
        (KeyModifiers::SHIFT, KeyCode::Enter) => Some(TuiCommand::InsertNewline),

        // Alt+Enter: Insert newline (for multi-line input)
        (KeyModifiers::ALT, KeyCode::Enter) => Some(TuiCommand::InsertNewline),

        // Character input
        (KeyModifiers::NONE | KeyModifiers::SHIFT, KeyCode::Char(c)) => {
            Some(TuiCommand::InsertChar(c))
        }

        // Backspace
        (KeyModifiers::NONE, KeyCode::Backspace) => Some(TuiCommand::DeleteBackward),
        (KeyModifiers::CONTROL, KeyCode::Backspace) => Some(TuiCommand::DeleteWordBackward),

        // Delete
        (KeyModifiers::NONE, KeyCode::Delete) => Some(TuiCommand::DeleteForward),
        (KeyModifiers::CONTROL, KeyCode::Delete) => Some(TuiCommand::DeleteWordForward),

        // Cursor movement
        (KeyModifiers::NONE, KeyCode::Left) => Some(TuiCommand::CursorLeft),
        (KeyModifiers::NONE, KeyCode::Right) => Some(TuiCommand::CursorRight),
        (KeyModifiers::NONE, KeyCode::Up) => Some(TuiCommand::CursorUp),
        (KeyModifiers::NONE, KeyCode::Down) => Some(TuiCommand::CursorDown),
        (KeyModifiers::NONE, KeyCode::Home) => Some(TuiCommand::CursorHome),
        (KeyModifiers::NONE, KeyCode::End) => Some(TuiCommand::CursorEnd),

        // Word movement (Ctrl+Arrow)
        (KeyModifiers::CONTROL, KeyCode::Left) => Some(TuiCommand::WordLeft),
        (KeyModifiers::CONTROL, KeyCode::Right) => Some(TuiCommand::WordRight),

        // Scroll (without modifiers, for chat area)
        (KeyModifiers::ALT, KeyCode::Up) => Some(TuiCommand::ScrollUp),
        (KeyModifiers::ALT, KeyCode::Down) => Some(TuiCommand::ScrollDown),

        _ => None,
    }
}

#[cfg(test)]
#[path = "handler.test.rs"]
mod tests;
