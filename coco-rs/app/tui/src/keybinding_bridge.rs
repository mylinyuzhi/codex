//! Keybinding bridge — maps key events to TUI commands.
//!
//! Determines the active keybinding context from state, then resolves
//! key events to commands. Context priority:
//!
//!   state > autocomplete > global > input
//!
//! TS: src/keybindings/ + event/handler.rs in cocode-rs

use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyModifiers;

use crate::events::TuiCommand;
use crate::state::AppState;
use crate::state::ModalState;
use crate::state::PanePromptState;

/// Keybinding context — determines which key mappings are active.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeybindingContext {
    /// Permission/question state — Y/N/A approval.
    Confirmation,
    /// Filterable list state (model picker, command palette, etc.).
    Picker,
    /// Model picker state — filterable list plus effort/role controls.
    ModelPicker,
    /// Scrollable content state (help, diff view, task detail, doctor).
    Scrollable,
    /// Transcript reader state.
    Transcript,
    /// Autocomplete suggestions visible.
    Autocomplete,
    /// Tabbed settings state — Tab/Shift+Tab cycle tabs, Up/Down nav.
    Settings,
    /// Theme tab inside Settings — includes theme-picker-specific actions.
    ThemePicker,
    /// Default chat input context.
    Chat,
}

/// Determine the active keybinding context from state.
pub fn active_context(state: &AppState) -> KeybindingContext {
    if let Some(modal) = state.ui.modal.as_ref() {
        return match modal {
            // Filterable list modals
            ModalState::ModelPicker(_) => KeybindingContext::ModelPicker,

            // Filterable list modals
            ModalState::SessionBrowser(_)
            | ModalState::GlobalSearch(_)
            | ModalState::QuickOpen(_)
            | ModalState::Export(_)
            | ModalState::Feedback(_)
            | ModalState::McpServerSelect(_)
            | ModalState::Rewind(_) => KeybindingContext::Picker,

            // Scrollable read-only modals
            ModalState::Help
            | ModalState::DiffView(_)
            | ModalState::TaskDetail(_)
            | ModalState::Doctor(_)
            | ModalState::ContextVisualization => KeybindingContext::Scrollable,
            ModalState::Transcript(_) => KeybindingContext::Transcript,

            // Tabbed settings state. The Theme tab gets the TS
            // ThemePicker context so `theme:toggleSyntaxHighlighting`
            // works without making syntax highlighting a theme.json field.
            ModalState::Settings(s)
                if s.active_tab == crate::widgets::settings_panel::SettingsTab::Theme =>
            {
                KeybindingContext::ThemePicker
            }
            ModalState::Settings(_) => KeybindingContext::Settings,

            // All others are confirmation/approval surfaces
            _ => KeybindingContext::Confirmation,
        };
    }

    if matches!(
        state.ui.interaction.active_prompt,
        Some(
            PanePromptState::Permission(_)
                | PanePromptState::Question(_)
                | PanePromptState::SandboxPermission(_)
                | PanePromptState::CostWarning(_)
                | PanePromptState::PlanEntry(_)
                | PanePromptState::PlanExit(_)
                | PanePromptState::PlanApproval(_)
                | PanePromptState::McpServerApproval(_)
        )
    ) {
        return KeybindingContext::Confirmation;
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
///
/// Resolution order:
///
/// 1. Run the [`coco_keybindings`] resolver against the TS default
///    bindings. If it fires an action with a TUI handler in
///    [`crate::keybinding_dispatch`], use it.
/// 2. If the resolver explicitly consumed the keystroke (chord
///    cancelled, null unbind), return `None` so it doesn't fall through.
/// 3. Otherwise, dispatch through the legacy hardcoded cascade for
///    TUI-only shortcuts (Ctrl+S session browser, F1 help, Ctrl+,
///    settings, …) that aren't in the TS schema yet.
pub fn map_key(state: &AppState, key: KeyEvent) -> Option<TuiCommand> {
    let ctx = active_context(state);

    if matches!(ctx, KeybindingContext::Transcript) {
        if matches!(key.code, KeyCode::BackTab) {
            return None;
        }
        if let Some(cmd) = map_transcript_key(key) {
            return Some(cmd);
        }
    }

    // Layer 1: TS-defined bindings via the resolver.
    match state.ui.kb_handle.resolve_key(key, ctx) {
        crate::keybinding_resolver::ResolverResult::Action(action) => {
            if let Some(cmd) = crate::keybinding_dispatch::dispatch_action(&action, state) {
                return Some(cmd);
            }
            // Resolver knew about this action but the TUI doesn't have
            // a handler yet. Swallow the keystroke instead of falling
            // through to the cascade — that path would do the wrong
            // thing for a user who customized the chord.
            return None;
        }
        crate::keybinding_resolver::ResolverResult::Pending => {
            // Caller should render a "ctrl+x …" chord status hint.
            // We don't have a dedicated TuiCommand for that yet, so
            // swallow the keystroke. Status-bar wiring is P8 follow-up.
            return None;
        }
        crate::keybinding_resolver::ResolverResult::Consumed => return None,
        crate::keybinding_resolver::ResolverResult::NotResolved => {}
    }

    // Layer 2: legacy hardcoded cascade (TUI-only shortcuts).
    match ctx {
        KeybindingContext::Confirmation => map_confirmation_key(key),
        KeybindingContext::ModelPicker => map_model_picker_key(key),
        KeybindingContext::Picker => map_picker_key(key),
        KeybindingContext::Scrollable => map_scrollable_key(key),
        KeybindingContext::Transcript => map_transcript_key(key),
        // Autocomplete intercepts navigation keys only; other keys fall
        // through to input editing so the user keeps typing and the
        // suggestion popup refreshes reactively.
        KeybindingContext::Autocomplete => map_autocomplete_key(key)
            .or_else(|| map_global_key(state, key))
            .or_else(|| map_input_key(state, key)),
        KeybindingContext::Settings | KeybindingContext::ThemePicker => map_settings_key(key),
        KeybindingContext::Chat => map_global_key(state, key).or_else(|| map_input_key(state, key)),
    }
}

/// Keys for the model picker: Up/Down chooses a model, Left/Right chooses
/// effort, Tab/Shift+Tab chooses role, printable chars edit the filter.
fn map_model_picker_key(key: KeyEvent) -> Option<TuiCommand> {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let shift = key.modifiers.contains(KeyModifiers::SHIFT);
    match key.code {
        KeyCode::Home => Some(TuiCommand::SurfaceJumpStart),
        KeyCode::End => Some(TuiCommand::SurfaceJumpEnd),
        KeyCode::Up if shift => Some(TuiCommand::SurfaceJumpStart),
        KeyCode::Down if shift => Some(TuiCommand::SurfaceJumpEnd),
        KeyCode::Up => Some(TuiCommand::SurfacePrev),
        KeyCode::Down => Some(TuiCommand::SurfaceNext),
        KeyCode::Left => Some(TuiCommand::ModelPickerCycleEffort(-1)),
        KeyCode::Right => Some(TuiCommand::ModelPickerCycleEffort(1)),
        KeyCode::Tab => Some(TuiCommand::SettingsNextTab),
        KeyCode::BackTab => Some(TuiCommand::SettingsPrevTab),
        KeyCode::Enter => Some(TuiCommand::SurfaceConfirm),
        KeyCode::Esc => Some(TuiCommand::Cancel),
        KeyCode::Backspace => Some(TuiCommand::SurfaceFilterBackspace),
        KeyCode::Char('c') if ctrl => Some(TuiCommand::Cancel),
        KeyCode::Char('p') if ctrl => Some(TuiCommand::SurfacePrev),
        KeyCode::Char('n') if ctrl => Some(TuiCommand::SurfaceNext),
        KeyCode::Char(c) => Some(TuiCommand::SurfaceFilter(c)),
        _ => None,
    }
}

/// Keys for the tabbed Settings state: Tab cycles tabs, Up/Down nav,
/// Enter selects, Esc closes.
fn map_settings_key(key: KeyEvent) -> Option<TuiCommand> {
    match key.code {
        KeyCode::Tab => Some(TuiCommand::SettingsNextTab),
        KeyCode::BackTab => Some(TuiCommand::SettingsPrevTab),
        KeyCode::Up => Some(TuiCommand::SurfacePrev),
        KeyCode::Down => Some(TuiCommand::SurfaceNext),
        KeyCode::Enter => Some(TuiCommand::SurfaceConfirm),
        KeyCode::Esc => Some(TuiCommand::Cancel),
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            Some(TuiCommand::Cancel)
        }
        _ => None,
    }
}

/// Keys for permission/question/approval prompts.
fn map_confirmation_key(key: KeyEvent) -> Option<TuiCommand> {
    match key.code {
        KeyCode::Char('y' | 'Y') => Some(TuiCommand::Approve),
        KeyCode::Char('n' | 'N') => Some(TuiCommand::Deny),
        KeyCode::Char('a' | 'A') => Some(TuiCommand::ApproveAll),
        // Tab cycles multi-option confirmations (PlanExit approval
        // target: Restore / AcceptEdits / Bypass). For simple Y/N
        // dialogs the handler is a no-op.
        KeyCode::Tab => Some(TuiCommand::SurfaceNext),
        KeyCode::BackTab => Some(TuiCommand::SurfacePrev),
        KeyCode::Up | KeyCode::Char('k') => Some(TuiCommand::SurfacePrev),
        KeyCode::Down | KeyCode::Char('j') => Some(TuiCommand::SurfaceNext),
        KeyCode::Enter => Some(TuiCommand::SurfaceConfirm),
        KeyCode::Esc => Some(TuiCommand::Cancel),
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            Some(TuiCommand::Cancel)
        }
        _ => None,
    }
}

/// Keys for filterable list modals (model picker, command palette, etc.).
fn map_picker_key(key: KeyEvent) -> Option<TuiCommand> {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let shift = key.modifiers.contains(KeyModifiers::SHIFT);
    match key.code {
        // Top/bottom — TS `messageSelector:top|bottom` (defaultBindings.ts:256-263).
        KeyCode::Home => Some(TuiCommand::SurfaceJumpStart),
        KeyCode::End => Some(TuiCommand::SurfaceJumpEnd),
        KeyCode::Up if shift => Some(TuiCommand::SurfaceJumpStart),
        KeyCode::Down if shift => Some(TuiCommand::SurfaceJumpEnd),
        KeyCode::Up => Some(TuiCommand::SurfacePrev),
        KeyCode::Down => Some(TuiCommand::SurfaceNext),
        KeyCode::Enter => Some(TuiCommand::SurfaceConfirm),
        KeyCode::Esc => Some(TuiCommand::Cancel),
        KeyCode::Backspace => Some(TuiCommand::SurfaceFilterBackspace),
        KeyCode::Char('c') if ctrl => Some(TuiCommand::Cancel),
        // Vim + emacs nav aliases — TS `messageSelector:up|down` accepts
        // k / j / ctrl+p / ctrl+n. For text-input pickers (model picker,
        // command palette) Char(c) routes into the filter; we keep that
        // path by short-circuiting only on ctrl+p / ctrl+n which would
        // otherwise be no-ops in those modals.
        KeyCode::Char('p') if ctrl => Some(TuiCommand::SurfacePrev),
        KeyCode::Char('n') if ctrl => Some(TuiCommand::SurfaceNext),
        KeyCode::Char(c) => Some(TuiCommand::SurfaceFilter(c)),
        _ => None,
    }
}

/// Keys for scrollable read-only modals (help, diff, doctor, etc.).
fn map_scrollable_key(key: KeyEvent) -> Option<TuiCommand> {
    match key.code {
        KeyCode::Esc | KeyCode::Char('q') => Some(TuiCommand::Cancel),
        KeyCode::Up | KeyCode::Char('k') => Some(TuiCommand::SurfacePrev),
        KeyCode::Down | KeyCode::Char('j') => Some(TuiCommand::SurfaceNext),
        KeyCode::PageUp => Some(TuiCommand::PageUp),
        KeyCode::PageDown => Some(TuiCommand::PageDown),
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            Some(TuiCommand::Cancel)
        }
        _ => None,
    }
}

fn map_transcript_key(key: KeyEvent) -> Option<TuiCommand> {
    match key.code {
        KeyCode::Esc | KeyCode::Char('q') => Some(TuiCommand::Cancel),
        KeyCode::Up | KeyCode::Char('k') => Some(TuiCommand::TranscriptScrollLines(-1)),
        KeyCode::Down | KeyCode::Char('j') => Some(TuiCommand::TranscriptScrollLines(1)),
        KeyCode::Home => Some(TuiCommand::TranscriptJumpStart),
        KeyCode::End => Some(TuiCommand::TranscriptJumpEnd),
        KeyCode::PageUp => Some(TuiCommand::TranscriptPage(-1)),
        KeyCode::PageDown => Some(TuiCommand::TranscriptPage(1)),
        KeyCode::Tab => Some(TuiCommand::TranscriptSelectNext),
        KeyCode::Enter => Some(TuiCommand::TranscriptToggleCell),
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            Some(TuiCommand::Cancel)
        }
        _ => None,
    }
}

/// Keys for autocomplete suggestions.
fn map_autocomplete_key(key: KeyEvent) -> Option<TuiCommand> {
    match key.code {
        KeyCode::Up => Some(TuiCommand::SurfacePrev),
        KeyCode::Down => Some(TuiCommand::SurfaceNext),
        KeyCode::Tab | KeyCode::Enter => Some(TuiCommand::SurfaceConfirm),
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
        // Ctrl shortcuts. `ctrl+c` / `ctrl+d` are owned exclusively
        // by the resolver (defaults → `app:interrupt` / `app:exit`,
        // reserved against rebinding); both go through `update::exit`
        // for double-press confirmation, so they MUST NOT appear in
        // this fallback cascade — a hard-coded `Interrupt`/`Quit`
        // here would bypass the confirmation.
        //
        // `ctrl+q` is a coco-rs-only power-user immediate-quit
        // shortcut with no equivalent in TS. It is not in
        // `coco-keybindings` defaults (so the resolver leaves it
        // unhandled) and not reserved, so the fallback below is
        // what gives it meaning.
        KeyCode::Char('q') if ctrl => Some(TuiCommand::Quit),
        KeyCode::Char('l') if ctrl => Some(TuiCommand::ClearScreen),
        // Ctrl+T / F2 are owned by the keybindings resolver
        // (Chat: `chat:cycleThinking` → `CycleThinkingLevel`, and
        // `chat:thinkingToggle` → `ToggleThinking`). No legacy fallback
        // needed; users who unbind both can rebind via
        // `~/.coco/keybindings.json`.
        //
        // Bare Ctrl+E is deliberately NOT mapped here: TS uses the chord
        // `ctrl+x ctrl+e` for the external editor precisely so it doesn't
        // shadow readline's `Ctrl+E = end-of-line` (see
        // `defaultBindings.ts:82-83`). The `ctrl+x ctrl+e` chord plus the
        // bare `ctrl+g` shortcut for external editor are already wired
        // through the resolver via `keybindings/defaults.rs:113-114`.
        // Falling through here lets `map_input_key` map bare Ctrl+E to
        // `CursorEnd` as the user expects.
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
        // Submit / queue — each match arm carries the structured `keymap`
        // entry id it implements; the keymap/ module is the source of
        // truth that `/help`, the help state, and subagent retrieval
        // all read from.
        // keymap = "input:newline"
        KeyCode::Enter if shift || alt => Some(TuiCommand::InsertNewline),
        KeyCode::Enter if is_streaming => Some(TuiCommand::QueueInput),
        // keymap = "input:submit"
        KeyCode::Enter => Some(TuiCommand::SubmitInput),

        // Editing
        // keymap = "input:delete_word_backward"
        KeyCode::Backspace if ctrl || alt => Some(TuiCommand::DeleteWordBackward),
        // keymap = "input:delete_backward"
        KeyCode::Backspace => Some(TuiCommand::DeleteBackward),
        // keymap = "input:delete_word_forward"
        KeyCode::Delete if ctrl => Some(TuiCommand::DeleteWordForward),
        // keymap = "input:delete_forward"
        KeyCode::Delete => Some(TuiCommand::DeleteForward),
        // keymap = "input:delete_backward" (alternate combo). Some terminals
        // deliver Ctrl+H as a literal control char rather than Backspace.
        KeyCode::Char('h') if ctrl => Some(TuiCommand::DeleteBackward),

        // Cursor
        // keymap = "input:word_left" (alternate combo)
        KeyCode::Left if ctrl || alt => Some(TuiCommand::WordLeft),
        // keymap = "input:cursor_left" (alternate combo)
        KeyCode::Left => Some(TuiCommand::CursorLeft),
        // keymap = "input:word_right" (alternate combo)
        KeyCode::Right if ctrl || alt => Some(TuiCommand::WordRight),
        // keymap = "input:cursor_right" (alternate combo)
        KeyCode::Right => Some(TuiCommand::CursorRight),
        KeyCode::Up if alt => Some(TuiCommand::ScrollUp),
        // keymap = "input:history"
        KeyCode::Up => Some(TuiCommand::CursorUp),
        KeyCode::Down if alt => Some(TuiCommand::ScrollDown),
        // keymap = "input:history"
        KeyCode::Down => Some(TuiCommand::CursorDown),
        // keymap = "input:cursor_home" (alternate combo)
        KeyCode::Home => Some(TuiCommand::CursorHome),
        // keymap = "input:cursor_end" (alternate combo)
        KeyCode::End => Some(TuiCommand::CursorEnd),

        // Emacs / readline (matches TS PromptInput.tsx + GNU readline
        // conventions). Each arm is the canonical implementation of the
        // `KeymapEntry` named in the comment above.
        // keymap = "input:cursor_home"
        KeyCode::Char('a') if ctrl => Some(TuiCommand::CursorHome),
        // keymap = "input:cursor_end"
        KeyCode::Char('e') if ctrl => Some(TuiCommand::CursorEnd),
        // keymap = "input:cursor_left"
        KeyCode::Char('b') if ctrl => Some(TuiCommand::CursorLeft),
        // keymap = "input:cursor_right"
        KeyCode::Char('f') if ctrl => Some(TuiCommand::CursorRight),
        // keymap = "input:kill_to_eol"
        KeyCode::Char('k') if ctrl => Some(TuiCommand::KillToEndOfLine),
        // keymap = "input:kill_to_bol"
        KeyCode::Char('u') if ctrl => Some(TuiCommand::KillToBeginningOfLine),
        // keymap = "input:delete_word_backward"
        KeyCode::Char('w') if ctrl => Some(TuiCommand::DeleteWordBackward),
        // keymap = "input:yank"
        KeyCode::Char('y') if ctrl => Some(TuiCommand::Yank),
        // keymap = "input:newline" (alternate combo)
        KeyCode::Char('j') if ctrl => Some(TuiCommand::InsertNewline),
        // keymap = "input:word_left" (alternate combo)
        KeyCode::Char('b') if alt => Some(TuiCommand::WordLeft),
        // keymap = "input:word_right" (alternate combo)
        KeyCode::Char('f') if alt => Some(TuiCommand::WordRight),

        // Escape always emits Cancel; the *second* Esc within
        // `DOUBLE_PRESS_TIMEOUT` is what opens rewind, handled inside
        // `update::handle_command`'s Cancel arm via
        // `state.ui.esc_tracker.poll(...)`. The dispatch layer can't
        // poll a tracker through `&AppState`.
        KeyCode::Esc => Some(TuiCommand::Cancel),

        // Character input
        KeyCode::Char(c) => Some(TuiCommand::InsertChar(c)),

        _ => None,
    }
}

#[cfg(test)]
#[path = "keybinding_bridge.test.rs"]
mod tests;
