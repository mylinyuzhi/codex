//! Text-editing, cursor-history, and word-movement handlers.
//!
//! Extracted from `update.rs` to keep the top-level dispatch lean.

use tokio::sync::mpsc;

use crate::command::UserCommand;
use crate::state::AppState;
use crate::state::PromptMode;
use crate::state::SlashCommandName;

pub(super) fn parse_slash_input(trimmed: &str) -> Option<(SlashCommandName, String)> {
    let stripped = trimmed.strip_prefix('/')?;
    if stripped.is_empty() {
        return None;
    }
    let (name, args) = match stripped.split_once(char::is_whitespace) {
        Some((name, rest)) => (name, rest.trim_start()),
        None => (stripped, ""),
    };
    Some((SlashCommandName::new(name).ok()?, args.to_string()))
}

/// Handle a submission whose leading character is a prompt-mode prefix
/// (`!` bash). Dispatches a typed `UserCommand` for the engine bridge
/// to execute; the bridge's `run_prompt_mode_bash` pushes a single
/// `SystemMessage::LocalCommand { command, output }` via
/// `history_push_and_emit` after the shell call completes, so the
/// transcript view shows the invocation through the standard
/// `MessageAppended` path. The TUI never touches the shell directly —
/// keeps the permission model and side-effect surface in one place.
async fn submit_prefixed(
    state: &mut AppState,
    command_tx: &mpsc::Sender<UserCommand>,
    mode: PromptMode,
    text: &str,
) -> bool {
    debug_assert_eq!(mode, PromptMode::Bash);
    let payload = mode.strip_prefix(text).to_string();
    if payload.is_empty() {
        // Empty body after stripping the prefix (e.g. user typed just
        // `!` and hit Enter). Don't echo or dispatch — drop silently.
        return true;
    }

    // Record the *full* prefixed text in history so up-arrow recall
    // returns the user to the same mode without forcing them to retype
    // the prefix character. TS parity: `prependModeCharacterToInput`.
    state.ui.input.add_to_history(text.to_string());

    let user_message_id = uuid::Uuid::new_v4().to_string();
    tracing::info!(
        target: "coco_tui::submit",
        user_message_id = %user_message_id,
        kind = "bash",
        chars = payload.len(),
        "user submitted bash command",
    );
    if let Err(e) = command_tx
        .send(UserCommand::SubmitBash {
            user_message_id,
            command: payload,
        })
        .await
    {
        tracing::warn!(
            target: "coco_tui::submit",
            error = %e,
            "failed to dispatch SubmitBash (command channel closed)",
        );
    }

    state.ui.paste_manager.clear();
    state.ui.scroll_offset = 0;
    state.ui.user_scrolled = false;
    state.session.last_query_completion_at = None;
    state.session.idle_prompt_fired = false;
    true
}

/// Submit current input. Slash commands are sent as typed command requests
/// and resolved by the command layer.
pub(super) async fn submit(state: &mut AppState, command_tx: &mpsc::Sender<UserCommand>) -> bool {
    let text = state.ui.input.take_input();
    if text.is_empty() {
        return true;
    }

    // Prompt-mode routing happens BEFORE slash-command checks
    // because `!` and `#` are prefix-only — they can never collide with
    // `/foo` (different leading byte) so this ordering is safe and
    // matches TS's `getModeFromInput → if bash …` dispatch order.
    let mode = PromptMode::from_text(&text);
    if mode != PromptMode::Normal {
        return submit_prefixed(state, command_tx, mode, &text).await;
    }

    let trimmed = text.trim();
    if let Some((name, args)) = parse_slash_input(trimmed) {
        tracing::info!(
            target: "coco_tui::submit",
            kind = "slash",
            command = %name.as_str(),
            args_chars = args.len(),
            "user submitted slash command",
        );
        state.ui.input.add_to_history(text);
        if let Err(e) = command_tx
            .send(UserCommand::ExecuteSlashCommand { name, args })
            .await
        {
            tracing::warn!(
                target: "coco_tui::submit",
                error = %e,
                "failed to dispatch ExecuteSlashCommand (command channel closed)",
            );
        }
        return true;
    }

    // Snapshot the paste payloads this text references BEFORE the manager
    // is cleared below, so recalling the entry rehydrates its pills.
    let pastes: Vec<_> = state
        .ui
        .paste_manager
        .entries()
        .iter()
        .filter(|e| text.contains(&e.pill))
        .cloned()
        .collect();
    state
        .ui
        .input
        .add_to_history_with_pastes(text.clone(), pastes);
    let resolved = state.ui.paste_manager.resolve_structured(&text);

    // Mint the user-message UUID once at submit time so the agent
    // driver's `Message::User`, the file-history snapshot, and the
    // JSONL transcript all key off the same id. Engine
    // `history_push_and_emit` emits `MessageAppended` carrying this
    // uuid, which the `TranscriptView` then renders.
    let user_message_id = uuid::Uuid::new_v4().to_string();
    tracing::info!(
        target: "coco_tui::submit",
        user_message_id = %user_message_id,
        kind = "prompt",
        chars = resolved.text.len(),
        images = resolved.images.len(),
        display_chars = text.len(),
        "user submitted prompt",
    );

    if let Err(e) = command_tx
        .send(UserCommand::SubmitInput {
            user_message_id,
            content: resolved.text,
            display_text: Some(text),
            images: resolved.images,
        })
        .await
    {
        tracing::warn!(
            target: "coco_tui::submit",
            error = %e,
            "failed to dispatch SubmitInput (command channel closed)",
        );
    }
    state.ui.paste_manager.clear();
    state.ui.scroll_offset = 0;
    state.ui.user_scrolled = false;
    // Reset idle-prompt window: the user has just spoken, so any
    // pending firing must wait for the *next* turn-completion.
    state.session.last_query_completion_at = None;
    state.session.idle_prompt_fired = false;
    true
}

/// Delete one word backwards from the cursor.
///
/// Delegates to `TextArea::delete_backward_word`, which puts the killed
/// span into the TextArea's kill buffer (yankable via Ctrl+Y).
pub(super) fn delete_word_backward(state: &mut AppState) {
    state.ui.input.textarea.delete_backward_word();
}

/// Delete one word forward from the cursor.
///
/// Delegates to `TextArea::delete_forward_word` (alt+d / ctrl+delete).
pub(super) fn delete_word_forward(state: &mut AppState) {
    state.ui.input.textarea.delete_forward_word();
}

/// Kill from cursor to end of current line (Emacs Ctrl+K).
///
/// TextArea owns the single-entry kill buffer; consecutive kills accumulate
/// readline-style so `Ctrl+Y` recovers the full deleted region.
pub(super) fn kill_to_end_of_line(state: &mut AppState) {
    state.ui.input.textarea.kill_to_end_of_line();
}

/// Kill from BOL to cursor (Emacs Ctrl+U / readline `unix-line-discard`).
pub(super) fn kill_to_beginning_of_line(state: &mut AppState) {
    state.ui.input.textarea.kill_to_beginning_of_line();
}

/// Yank (paste) the kill buffer at the cursor (Emacs Ctrl+Y).
pub(super) fn yank(state: &mut AppState) {
    state.ui.input.textarea.yank();
}

/// Up arrow: step toward less-relevant history entries (away from the top
/// frecency match). First Up surfaces the most relevant entry; subsequent
/// Ups walk down the frecency-sorted list.
pub(super) fn history_prev(state: &mut AppState) {
    let len = state.ui.input.history.len();
    if len == 0 {
        return;
    }
    let new_idx = match state.ui.input.history_index {
        None => 0,
        Some(i) if i + 1 < len => i + 1,
        Some(i) => i, // already at least-relevant tail; stay put
    };
    state.ui.input.history_index = Some(new_idx);
    let entry = &state.ui.input.history[new_idx];
    let text = entry.text.clone();
    // Rehydrate the paste manager so any pills in the recalled text
    // resolve to their original payloads at submit.
    state.ui.paste_manager.replace_entries(entry.pastes.clone());
    state.ui.input.textarea.set_text(&text);
    state
        .ui
        .input
        .textarea
        .move_cursor_to_end_of_line(coco_tui_ui::widgets::EolBehavior::StayPut);
}

/// Down arrow: step back toward the most-relevant entry; leaving the list
/// at index 0 clears the input (matches TS PromptInput behaviour).
pub(super) fn history_next(state: &mut AppState) {
    let Some(idx) = state.ui.input.history_index else {
        return;
    };
    if idx > 0 {
        let new_idx = idx - 1;
        state.ui.input.history_index = Some(new_idx);
        let entry = &state.ui.input.history[new_idx];
        let text = entry.text.clone();
        state.ui.paste_manager.replace_entries(entry.pastes.clone());
        state.ui.input.textarea.set_text(&text);
        state
            .ui
            .input
            .textarea
            .move_cursor_to_end_of_line(coco_tui_ui::widgets::EolBehavior::StayPut);
    } else {
        state.ui.input.history_index = None;
        state.ui.input.textarea.set_text("");
        state.ui.paste_manager.clear();
    }
}

/// Move cursor one word to the left (grapheme-aware via TextArea).
pub(super) fn word_left(state: &mut AppState) {
    let target = state.ui.input.textarea.beginning_of_previous_word();
    state.ui.input.textarea.set_cursor(target);
}

/// Move cursor one word to the right (grapheme-aware via TextArea).
pub(super) fn word_right(state: &mut AppState) {
    let target = state.ui.input.textarea.end_of_next_word();
    state.ui.input.textarea.set_cursor(target);
}
