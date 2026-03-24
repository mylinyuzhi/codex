//! Legacy key event handler — retained for test coverage only.
//!
//! Runtime key handling is done via `keybinding_bridge` + `KeybindingsManager`.
//! These functions verify the expected key→command mappings in tests.

// All handler functions are test-only — runtime uses keybinding_bridge.
#[cfg(test)]
mod legacy {
    use crossterm::event::KeyCode;
    use crossterm::event::KeyEvent;
    use crossterm::event::KeyModifiers;

    use crate::event::TuiCommand;

    pub fn handle_key_event(key: KeyEvent, has_overlay: bool) -> Option<TuiCommand> {
        if has_overlay {
            return handle_overlay_key(key);
        }
        if let Some(cmd) = handle_global_key(key) {
            return Some(cmd);
        }
        handle_input_key(key)
    }

    pub fn handle_key_event_full(
        key: KeyEvent,
        has_overlay: bool,
        has_file_suggestions: bool,
        has_skill_suggestions: bool,
        has_agent_suggestions: bool,
        is_streaming: bool,
    ) -> Option<TuiCommand> {
        if has_overlay {
            return handle_overlay_key(key);
        }
        if has_skill_suggestions
            && let Some(cmd) = handle_suggestion_key(
                key,
                TuiCommand::SelectPrevSkillSuggestion,
                TuiCommand::SelectNextSkillSuggestion,
                TuiCommand::AcceptSkillSuggestion,
                TuiCommand::DismissSkillSuggestions,
            )
        {
            return Some(cmd);
        }
        if has_agent_suggestions
            && let Some(cmd) = handle_suggestion_key(
                key,
                TuiCommand::SelectPrevAgentSuggestion,
                TuiCommand::SelectNextAgentSuggestion,
                TuiCommand::AcceptAgentSuggestion,
                TuiCommand::DismissAgentSuggestions,
            )
        {
            return Some(cmd);
        }
        if has_file_suggestions
            && let Some(cmd) = handle_suggestion_key(
                key,
                TuiCommand::SelectPrevSuggestion,
                TuiCommand::SelectNextSuggestion,
                TuiCommand::AcceptSuggestion,
                TuiCommand::DismissSuggestions,
            )
        {
            return Some(cmd);
        }
        if let Some(cmd) = handle_global_key(key) {
            return Some(cmd);
        }
        handle_input_key_with_streaming(key, is_streaming)
    }

    fn handle_suggestion_key(
        key: KeyEvent,
        prev: TuiCommand,
        next: TuiCommand,
        accept: TuiCommand,
        dismiss: TuiCommand,
    ) -> Option<TuiCommand> {
        match (key.modifiers, key.code) {
            (KeyModifiers::NONE, KeyCode::Up) => Some(prev),
            (KeyModifiers::NONE, KeyCode::Down) => Some(next),
            (KeyModifiers::NONE, KeyCode::Tab) => Some(accept),
            (KeyModifiers::NONE, KeyCode::Enter) => Some(accept),
            (KeyModifiers::NONE, KeyCode::Esc) => Some(dismiss),
            _ => None,
        }
    }

    fn handle_overlay_key(key: KeyEvent) -> Option<TuiCommand> {
        match key.code {
            KeyCode::Char('y') | KeyCode::Char('Y') => Some(TuiCommand::Approve),
            KeyCode::Char('n') | KeyCode::Char('N') => Some(TuiCommand::Deny),
            KeyCode::Char('a') | KeyCode::Char('A')
                if key.modifiers.contains(KeyModifiers::CONTROL) =>
            {
                Some(TuiCommand::ApproveAll)
            }
            KeyCode::Tab if key.modifiers.is_empty() => Some(TuiCommand::PluginManagerNextTab),
            KeyCode::BackTab => Some(TuiCommand::PluginManagerPrevTab),
            KeyCode::Up | KeyCode::Char('k') => Some(TuiCommand::CursorUp),
            KeyCode::Down | KeyCode::Char('j') => Some(TuiCommand::CursorDown),
            KeyCode::Enter => Some(TuiCommand::Approve),
            KeyCode::Esc => Some(TuiCommand::Cancel),
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                Some(TuiCommand::Cancel)
            }
            KeyCode::Char('v') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                Some(TuiCommand::PasteFromClipboard)
            }
            KeyCode::Char('v') if key.modifiers.contains(KeyModifiers::ALT) => {
                Some(TuiCommand::PasteFromClipboard)
            }
            KeyCode::Char(c)
                if key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT =>
            {
                Some(TuiCommand::InsertChar(c))
            }
            KeyCode::Backspace => Some(TuiCommand::DeleteBackward),
            KeyCode::Delete => Some(TuiCommand::DeleteForward),
            _ => None,
        }
    }

    fn handle_global_key(key: KeyEvent) -> Option<TuiCommand> {
        match (key.modifiers, key.code) {
            (KeyModifiers::NONE, KeyCode::Tab) => Some(TuiCommand::TogglePlanMode),
            (KeyModifiers::SHIFT, KeyCode::BackTab) => Some(TuiCommand::CyclePermissionMode),
            (KeyModifiers::CONTROL, KeyCode::Char('t')) => Some(TuiCommand::CycleThinkingLevel),
            (KeyModifiers::CONTROL, KeyCode::Char('m')) => Some(TuiCommand::CycleModel),
            (KeyModifiers::CONTROL, KeyCode::Char('b')) => Some(TuiCommand::BackgroundAllTasks),
            (KeyModifiers::CONTROL, KeyCode::Char('f')) => Some(TuiCommand::KillAllAgents),
            (KeyModifiers::CONTROL, KeyCode::Char('c')) => Some(TuiCommand::Interrupt),
            (KeyModifiers::CONTROL, KeyCode::Char('l')) => Some(TuiCommand::ClearScreen),
            (KeyModifiers::CONTROL, KeyCode::Char('e')) => Some(TuiCommand::OpenExternalEditor),
            (KeyModifiers::CONTROL, KeyCode::Char('g')) => Some(TuiCommand::OpenPlanEditor),
            (KeyModifiers::CONTROL, KeyCode::Char('p')) => Some(TuiCommand::ShowCommandPalette),
            (KeyModifiers::CONTROL, KeyCode::Char('s')) => Some(TuiCommand::ShowSessionBrowser),
            (m, KeyCode::Char('T'))
                if m.contains(KeyModifiers::CONTROL) && m.contains(KeyModifiers::SHIFT) =>
            {
                Some(TuiCommand::ToggleThinking)
            }
            (m, KeyCode::Char('E'))
                if m.contains(KeyModifiers::CONTROL) && m.contains(KeyModifiers::SHIFT) =>
            {
                Some(TuiCommand::ToggleToolCollapse)
            }
            (m, KeyCode::Char('R'))
                if m.contains(KeyModifiers::CONTROL) && m.contains(KeyModifiers::SHIFT) =>
            {
                Some(TuiCommand::ToggleSystemReminders)
            }
            (m, KeyCode::Char('F'))
                if m.contains(KeyModifiers::CONTROL) && m.contains(KeyModifiers::SHIFT) =>
            {
                Some(TuiCommand::ToggleFastMode)
            }
            (KeyModifiers::NONE, KeyCode::F(1)) => Some(TuiCommand::ShowHelp),
            (KeyModifiers::SHIFT, KeyCode::Char('?')) => Some(TuiCommand::ShowHelp),
            (KeyModifiers::CONTROL, KeyCode::Char('q')) => Some(TuiCommand::Quit),
            (KeyModifiers::CONTROL, KeyCode::Char('v')) => Some(TuiCommand::PasteFromClipboard),
            (KeyModifiers::ALT, KeyCode::Char('v')) => Some(TuiCommand::PasteFromClipboard),
            (KeyModifiers::CONTROL, KeyCode::Char('a')) => Some(TuiCommand::SelectAll),
            (_, KeyCode::Esc) => Some(TuiCommand::Cancel),
            (_, KeyCode::PageUp) => Some(TuiCommand::PageUp),
            (_, KeyCode::PageDown) => Some(TuiCommand::PageDown),
            (KeyModifiers::CONTROL, KeyCode::Up) => Some(TuiCommand::PageUp),
            (KeyModifiers::CONTROL, KeyCode::Down) => Some(TuiCommand::PageDown),
            _ => None,
        }
    }

    fn handle_input_key(key: KeyEvent) -> Option<TuiCommand> {
        handle_input_key_with_streaming(key, false)
    }

    fn handle_input_key_with_streaming(key: KeyEvent, is_streaming: bool) -> Option<TuiCommand> {
        match (key.modifiers, key.code) {
            (KeyModifiers::NONE | KeyModifiers::CONTROL, KeyCode::Enter) => {
                if is_streaming {
                    Some(TuiCommand::QueueInput)
                } else {
                    Some(TuiCommand::SubmitInput)
                }
            }
            (KeyModifiers::SHIFT, KeyCode::Enter) => Some(TuiCommand::InsertNewline),
            (KeyModifiers::ALT, KeyCode::Enter) => Some(TuiCommand::InsertNewline),
            (KeyModifiers::NONE | KeyModifiers::SHIFT, KeyCode::Char(c)) => {
                Some(TuiCommand::InsertChar(c))
            }
            (KeyModifiers::NONE, KeyCode::Backspace) => Some(TuiCommand::DeleteBackward),
            (KeyModifiers::CONTROL, KeyCode::Backspace) => Some(TuiCommand::DeleteWordBackward),
            (KeyModifiers::NONE, KeyCode::Delete) => Some(TuiCommand::DeleteForward),
            (KeyModifiers::CONTROL, KeyCode::Delete) => Some(TuiCommand::DeleteWordForward),
            (KeyModifiers::CONTROL, KeyCode::Char('k')) => Some(TuiCommand::KillToEndOfLine),
            (KeyModifiers::CONTROL, KeyCode::Char('y')) => Some(TuiCommand::Yank),
            (KeyModifiers::NONE, KeyCode::Left) => Some(TuiCommand::CursorLeft),
            (KeyModifiers::NONE, KeyCode::Right) => Some(TuiCommand::CursorRight),
            (KeyModifiers::NONE, KeyCode::Up) => Some(TuiCommand::CursorUp),
            (KeyModifiers::NONE, KeyCode::Down) => Some(TuiCommand::CursorDown),
            (KeyModifiers::NONE, KeyCode::Home) => Some(TuiCommand::CursorHome),
            (KeyModifiers::NONE, KeyCode::End) => Some(TuiCommand::CursorEnd),
            (KeyModifiers::CONTROL, KeyCode::Left) => Some(TuiCommand::WordLeft),
            (KeyModifiers::CONTROL, KeyCode::Right) => Some(TuiCommand::WordRight),
            (KeyModifiers::ALT, KeyCode::Up) => Some(TuiCommand::ScrollUp),
            (KeyModifiers::ALT, KeyCode::Down) => Some(TuiCommand::ScrollDown),
            _ => None,
        }
    }
}

#[cfg(test)]
#[path = "handler.test.rs"]
mod tests;
