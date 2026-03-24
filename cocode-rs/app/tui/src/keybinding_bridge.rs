//! Bridge between the keybindings crate and TUI commands.
//!
//! Maps `KeybindingContext` to current TUI state and converts
//! `Action` results into `TuiCommand` values.

use cocode_keybindings::action::Action;
use cocode_keybindings::context::KeybindingContext;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;

use crate::event::TuiCommand;
use crate::state::AppState;
use crate::state::Overlay;

/// Determine active keybinding contexts from the current TUI state.
///
/// Returns contexts in priority order (most specific first).
/// The resolver uses these to filter bindings — `Global` is always
/// implicitly active and should NOT be included here.
pub fn active_contexts(state: &AppState) -> Vec<KeybindingContext> {
    // Overlay has highest priority
    if let Some(overlay) = &state.ui.overlay {
        return match overlay {
            Overlay::Help => vec![KeybindingContext::Help],
            Overlay::ModelPicker(_) | Overlay::OutputStylePicker(_) => {
                vec![KeybindingContext::ModelPicker]
            }
            Overlay::PluginManager(_) => vec![KeybindingContext::Plugin],
            Overlay::RewindSelector(_) => vec![KeybindingContext::MessageSelector],
            Overlay::Permission(_)
            | Overlay::PlanExitApproval(_)
            | Overlay::Question(_)
            | Overlay::Elicitation(_) => vec![KeybindingContext::Confirmation],
            Overlay::CommandPalette(_) | Overlay::SessionBrowser(_) => {
                vec![KeybindingContext::Select]
            }
            Overlay::Error(_) | Overlay::CostWarning(_) | Overlay::SandboxPermission(_) => {
                vec![KeybindingContext::Confirmation]
            }
        };
    }

    // Autocomplete suggestions (priority: skill > agent > symbol > file)
    if state.ui.has_skill_suggestions() {
        return vec![KeybindingContext::Autocomplete, KeybindingContext::Chat];
    }
    if state.ui.has_agent_suggestions() {
        return vec![KeybindingContext::Autocomplete, KeybindingContext::Chat];
    }
    if state.ui.has_symbol_suggestions() {
        return vec![KeybindingContext::Autocomplete, KeybindingContext::Chat];
    }
    if state.ui.has_file_suggestions() {
        return vec![KeybindingContext::Autocomplete, KeybindingContext::Chat];
    }

    // Default: Chat context
    vec![KeybindingContext::Chat]
}

/// Convert a keybinding action into a TUI command.
///
/// Some actions require state to pick the right command variant
/// (e.g., autocomplete accept depends on which suggestion type is active).
pub fn action_to_command(action: &Action, state: &AppState) -> Option<TuiCommand> {
    match action {
        // ===== App =====
        Action::AppInterrupt => Some(TuiCommand::Interrupt),
        Action::AppExit | Action::ExtQuit => Some(TuiCommand::Quit),
        Action::AppToggleTranscript
        | Action::AppToggleTodos
        | Action::AppToggleBrief
        | Action::AppToggleTeammatePreview
        | Action::AppToggleTerminal
        | Action::AppGlobalSearch
        | Action::AppQuickOpen => None, // not yet implemented in TUI

        // ===== Chat =====
        Action::ChatCancel => Some(TuiCommand::Cancel),
        Action::ChatSubmit => {
            if state.is_streaming() {
                Some(TuiCommand::QueueInput)
            } else {
                Some(TuiCommand::SubmitInput)
            }
        }
        Action::ChatKillAgents => Some(TuiCommand::KillAllAgents),
        Action::ChatExternalEditor => Some(TuiCommand::OpenExternalEditor),
        Action::ChatModelPicker | Action::ChatCycleMode => Some(TuiCommand::CycleModel),
        Action::ChatFastMode => None, // not yet implemented
        Action::ChatThinkingToggle => Some(TuiCommand::CycleThinkingLevel),
        Action::ChatNewline | Action::ExtInsertNewline => Some(TuiCommand::InsertNewline),
        Action::ChatUndo => None,  // not yet implemented
        Action::ChatStash => None, // not yet implemented
        Action::ChatImagePaste => Some(TuiCommand::PasteFromClipboard),

        // ===== History =====
        Action::HistorySearch => None, // not yet implemented
        Action::HistoryPrevious => Some(TuiCommand::CursorUp),
        Action::HistoryNext => Some(TuiCommand::CursorDown),

        // ===== Task =====
        Action::TaskBackground => Some(TuiCommand::BackgroundAllTasks),

        // ===== Confirm/Permission =====
        Action::ConfirmYes => Some(TuiCommand::Approve),
        Action::ConfirmNo => Some(TuiCommand::Deny),
        Action::ConfirmPrevious => Some(TuiCommand::CursorUp),
        Action::ConfirmNext => Some(TuiCommand::CursorDown),
        Action::ConfirmToggle | Action::ConfirmNextField | Action::ConfirmPreviousField => None,
        Action::ConfirmCycleMode => None,
        Action::ConfirmToggleExplanation => None,
        Action::PermissionToggleDebug => None,

        // ===== Autocomplete =====
        // The correct TUI command depends on which autocomplete is active
        Action::AutocompleteAccept => Some(autocomplete_accept(state)),
        Action::AutocompleteDismiss => Some(autocomplete_dismiss(state)),
        Action::AutocompletePrevious => Some(autocomplete_prev(state)),
        Action::AutocompleteNext => Some(autocomplete_next(state)),

        // ===== Select =====
        Action::SelectNext => Some(TuiCommand::CursorDown),
        Action::SelectPrevious => Some(TuiCommand::CursorUp),
        Action::SelectAccept => Some(TuiCommand::Approve),
        Action::SelectCancel => Some(TuiCommand::Cancel),

        // ===== Tabs =====
        Action::TabsNext => Some(TuiCommand::FocusNext),
        Action::TabsPrevious => Some(TuiCommand::FocusPrevious),

        // ===== Attachments =====
        Action::AttachmentsNext
        | Action::AttachmentsPrevious
        | Action::AttachmentsRemove
        | Action::AttachmentsExit => None, // not yet implemented

        // ===== Footer =====
        Action::FooterNext
        | Action::FooterPrevious
        | Action::FooterSelect
        | Action::FooterOpenSelected
        | Action::FooterClearSelection => None, // not yet implemented

        // ===== Message Selector =====
        Action::MessageSelectorNext | Action::MessageSelectorDown => Some(TuiCommand::CursorDown),
        Action::MessageSelectorPrevious | Action::MessageSelectorUp => Some(TuiCommand::CursorUp),
        Action::MessageSelectorAccept | Action::MessageSelectorSelect => Some(TuiCommand::Approve),
        Action::MessageSelectorCancel => Some(TuiCommand::Cancel),
        Action::MessageSelectorTop | Action::MessageSelectorBottom => None, // not yet implemented

        // ===== Diff =====
        Action::DiffAccept => Some(TuiCommand::Approve),
        Action::DiffReject | Action::DiffDismiss => Some(TuiCommand::Cancel),
        Action::DiffNext | Action::DiffNextFile | Action::DiffNextSource => {
            Some(TuiCommand::CursorDown)
        }
        Action::DiffPrevious | Action::DiffPreviousFile | Action::DiffPreviousSource => {
            Some(TuiCommand::CursorUp)
        }
        Action::DiffBack | Action::DiffViewDetails => None, // not yet implemented

        // ===== Model Picker =====
        Action::ModelPickerNext => Some(TuiCommand::CursorDown),
        Action::ModelPickerPrevious => Some(TuiCommand::CursorUp),
        Action::ModelPickerAccept => Some(TuiCommand::Approve),
        Action::ModelPickerCancel => Some(TuiCommand::Cancel),
        Action::ModelPickerDecreaseEffort | Action::ModelPickerIncreaseEffort => None,

        // ===== Transcript =====
        Action::TranscriptScrollUp => Some(TuiCommand::ScrollUp),
        Action::TranscriptScrollDown => Some(TuiCommand::ScrollDown),
        Action::TranscriptClose | Action::TranscriptExit => Some(TuiCommand::Cancel),
        Action::TranscriptToggleShowAll => None, // not yet implemented

        // ===== History Search =====
        Action::HistorySearchPrevious | Action::HistorySearchNext => None,
        Action::HistorySearchAccept | Action::HistorySearchExecute => None,
        Action::HistorySearchCancel => Some(TuiCommand::Cancel),

        // ===== Theme =====
        Action::ThemeNext => Some(TuiCommand::CursorDown),
        Action::ThemePrevious => Some(TuiCommand::CursorUp),
        Action::ThemeAccept => Some(TuiCommand::Approve),
        Action::ThemeCancel => Some(TuiCommand::Cancel),
        Action::ThemeToggleSyntaxHighlighting => None,

        // ===== Help =====
        Action::HelpClose => Some(TuiCommand::Cancel),
        Action::HelpScrollUp => Some(TuiCommand::ScrollUp),
        Action::HelpScrollDown => Some(TuiCommand::ScrollDown),

        // ===== Settings =====
        Action::SettingsNext => Some(TuiCommand::CursorDown),
        Action::SettingsPrevious => Some(TuiCommand::CursorUp),
        Action::SettingsToggle => Some(TuiCommand::Approve),
        Action::SettingsClose => Some(TuiCommand::Cancel),
        Action::SettingsSearch | Action::SettingsRetry => None,

        // ===== Plugin =====
        Action::PluginNextTab => Some(TuiCommand::PluginManagerNextTab),
        Action::PluginPreviousTab => Some(TuiCommand::PluginManagerPrevTab),
        Action::PluginNext => Some(TuiCommand::CursorDown),
        Action::PluginPrevious => Some(TuiCommand::CursorUp),
        Action::PluginAccept => Some(TuiCommand::Approve),
        Action::PluginClose => Some(TuiCommand::Cancel),
        Action::PluginToggle | Action::PluginInstall => None,

        // ===== Voice =====
        Action::VoicePushToTalk => None, // not yet implemented

        // ===== Extensions =====
        Action::ExtTogglePlanMode => Some(TuiCommand::TogglePlanMode),
        Action::ExtCycleThinkingLevel => Some(TuiCommand::CycleThinkingLevel),
        Action::ExtCycleModel => Some(TuiCommand::CycleModel),
        Action::ExtShowCommandPalette => Some(TuiCommand::ShowCommandPalette),
        Action::ExtShowSessionBrowser => Some(TuiCommand::ShowSessionBrowser),
        Action::ExtClearScreen => Some(TuiCommand::ClearScreen),
        Action::ExtOpenPlanEditor => Some(TuiCommand::OpenPlanEditor),
        Action::ExtBackgroundAllTasks => Some(TuiCommand::BackgroundAllTasks),
        Action::ExtToggleToolCollapse => Some(TuiCommand::ToggleToolCollapse),
        Action::ExtToggleSystemReminders => Some(TuiCommand::ToggleSystemReminders),
        Action::ExtShowRewindSelector => Some(TuiCommand::ShowRewindSelector),
        Action::ExtSelectAll => Some(TuiCommand::SelectAll),
        Action::ExtKillToEndOfLine => Some(TuiCommand::KillToEndOfLine),
        Action::ExtYank => Some(TuiCommand::Yank),
        Action::ExtShowHelp => Some(TuiCommand::ShowHelp),
        Action::ExtApproveAll => Some(TuiCommand::ApproveAll),
        Action::ExtToggleThinking => Some(TuiCommand::ToggleThinking),
        Action::ExtPageUp => Some(TuiCommand::PageUp),
        Action::ExtPageDown => Some(TuiCommand::PageDown),
        Action::ExtScrollUp => Some(TuiCommand::ScrollUp),
        Action::ExtScrollDown => Some(TuiCommand::ScrollDown),
        Action::ExtDeleteBackward => Some(TuiCommand::DeleteBackward),
        Action::ExtDeleteWordBackward => Some(TuiCommand::DeleteWordBackward),
        Action::ExtDeleteForward => Some(TuiCommand::DeleteForward),
        Action::ExtDeleteWordForward => Some(TuiCommand::DeleteWordForward),
        Action::ExtCursorLeft => Some(TuiCommand::CursorLeft),
        Action::ExtCursorRight => Some(TuiCommand::CursorRight),
        Action::ExtCursorUp => Some(TuiCommand::CursorUp),
        Action::ExtCursorDown => Some(TuiCommand::CursorDown),
        Action::ExtCursorHome => Some(TuiCommand::CursorHome),
        Action::ExtCursorEnd => Some(TuiCommand::CursorEnd),
        Action::ExtWordLeft => Some(TuiCommand::WordLeft),
        Action::ExtWordRight => Some(TuiCommand::WordRight),

        // ===== Command =====
        Action::Command(_) => None, // skill execution handled separately
    }
}

/// Map an unhandled key event to a character-input command.
///
/// Called when the keybindings manager returns `Unhandled` — the
/// keystroke did not match any binding, so treat it as text input.
/// Only plain characters (no modifiers or SHIFT-only) become input.
pub fn unhandled_key_to_command(key: &KeyEvent, state: &AppState) -> Option<TuiCommand> {
    use crossterm::event::KeyModifiers;

    let is_char_input = key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT;

    // In overlay mode, allow character input for filter-based overlays
    if state.has_overlay() {
        return match key.code {
            KeyCode::Char(c) if is_char_input => Some(TuiCommand::InsertChar(c)),
            KeyCode::Backspace => Some(TuiCommand::DeleteBackward),
            KeyCode::Delete => Some(TuiCommand::DeleteForward),
            _ => None,
        };
    }

    // In chat mode, treat unmodified characters as input
    match key.code {
        KeyCode::Char(c) if is_char_input => Some(TuiCommand::InsertChar(c)),
        _ => None,
    }
}

// -- Autocomplete helpers --

fn autocomplete_accept(state: &AppState) -> TuiCommand {
    if state.ui.has_skill_suggestions() {
        TuiCommand::AcceptSkillSuggestion
    } else if state.ui.has_agent_suggestions() {
        TuiCommand::AcceptAgentSuggestion
    } else if state.ui.has_symbol_suggestions() {
        TuiCommand::AcceptSymbolSuggestion
    } else {
        TuiCommand::AcceptSuggestion
    }
}

fn autocomplete_dismiss(state: &AppState) -> TuiCommand {
    if state.ui.has_skill_suggestions() {
        TuiCommand::DismissSkillSuggestions
    } else if state.ui.has_agent_suggestions() {
        TuiCommand::DismissAgentSuggestions
    } else if state.ui.has_symbol_suggestions() {
        TuiCommand::DismissSymbolSuggestions
    } else {
        TuiCommand::DismissSuggestions
    }
}

fn autocomplete_prev(state: &AppState) -> TuiCommand {
    if state.ui.has_skill_suggestions() {
        TuiCommand::SelectPrevSkillSuggestion
    } else if state.ui.has_agent_suggestions() {
        TuiCommand::SelectPrevAgentSuggestion
    } else if state.ui.has_symbol_suggestions() {
        TuiCommand::SelectPrevSymbolSuggestion
    } else {
        TuiCommand::SelectPrevSuggestion
    }
}

fn autocomplete_next(state: &AppState) -> TuiCommand {
    if state.ui.has_skill_suggestions() {
        TuiCommand::SelectNextSkillSuggestion
    } else if state.ui.has_agent_suggestions() {
        TuiCommand::SelectNextAgentSuggestion
    } else if state.ui.has_symbol_suggestions() {
        TuiCommand::SelectNextSymbolSuggestion
    } else {
        TuiCommand::SelectNextSuggestion
    }
}

#[cfg(test)]
#[path = "keybinding_bridge.test.rs"]
mod tests;
