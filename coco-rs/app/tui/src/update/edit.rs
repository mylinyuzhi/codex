//! Text-editing, cursor-history, and word-movement handlers.
//!
//! Extracted from `update.rs` to keep the top-level dispatch lean.

use tokio::sync::mpsc;

use crate::command::UserCommand;
use crate::i18n::t;
use crate::state::AppState;
use crate::state::Overlay;
use crate::state::ui::Toast;
use crate::update_rewind;

/// Try to handle `trimmed` as a TUI-only slash command that must never reach
/// the agent (`/copy`, `/rewind`, `/checkpoint`). Returns `true` when the
/// input has been consumed locally; callers should skip their normal
/// submit/queue path in that case.
///
/// Shared between [`submit`] (normal Enter) and the `QueueInput` handler
/// (Enter while streaming) so the same commands behave identically in both
/// states — without this shim `/copy` typed mid-stream would be queued to
/// the agent as ordinary input.
pub(super) fn try_local_command(state: &mut AppState, trimmed: &str) -> bool {
    if trimmed == "/copy" {
        super::clipboard::copy_last_message(state);
        return true;
    }
    if trimmed == "/rewind"
        || trimmed == "/checkpoint"
        || trimmed.starts_with("/rewind ")
        || trimmed.starts_with("/checkpoint ")
    {
        let mut overlay = update_rewind::build_rewind_overlay(state);
        if overlay.messages.is_empty() {
            state
                .ui
                .add_toast(Toast::info(t!("toast.no_rewind_messages").to_string()));
        } else {
            let arg = trimmed.split_once(' ').map(|(_, a)| a.trim()).unwrap_or("");
            if arg == "last" {
                overlay.selected = overlay.messages.len().saturating_sub(1) as i32;
            } else if let Ok(n) = arg.parse::<i32>() {
                let idx = (n - 1).clamp(0, overlay.messages.len() as i32 - 1);
                overlay.selected = idx;
            }
            state.ui.set_overlay(Overlay::Rewind(overlay));
        }
        return true;
    }
    // /clear family — handled via `try_local_clear`. We deliberately
    // do NOT intercept those here; the caller funnels them through
    // `submit()` which routes via the async `try_local_clear` path
    // (needs the command channel).
    false
}

/// Try to handle `trimmed` as a `/clear` variant. Called by [`submit`]
/// with access to the core command channel so the engine-side reset
/// can be kicked off asynchronously.
///
/// Scope (TS: `src/commands/clear/conversation.ts::clearConversation`):
/// - `/clear`          — transcript only (session survives for /resume)
/// - `/clear history`  — alias of `/clear`
/// - `/clear all`      — transcript + plan files + plan-mode app_state
pub(super) async fn try_local_clear(
    state: &mut AppState,
    trimmed: &str,
    command_tx: &mpsc::Sender<UserCommand>,
) -> bool {
    let scope = match trimmed {
        "/clear" | "/clear history" => {
            let alias_history = trimmed == "/clear history";
            do_clear_conversation(state, /*clear_plan_state*/ false);
            if alias_history {
                crate::command::ClearScope::History
            } else {
                crate::command::ClearScope::Conversation
            }
        }
        "/clear all" => {
            do_clear_conversation(state, /*clear_plan_state*/ true);
            crate::command::ClearScope::All
        }
        _ => return false,
    };
    // Signal engine to reset its in-process plan-mode flags. Fire and
    // forget — if the channel is full or closed the TUI is already in
    // its own consistent state.
    let _ = command_tx
        .send(UserCommand::ClearConversation { scope })
        .await;
    true
}

/// Perform the local parts of `/clear` that don't require engine
/// cooperation: wipe the TUI transcript, dismiss overlays/toasts, and
/// (for `clear_plan_state=true`) remove plan files on disk + clear the
/// slug cache for this session.
fn do_clear_conversation(state: &mut AppState, clear_plan_state: bool) {
    // 1. Transcript reset.
    state.session.messages.clear();
    state.session.last_agent_markdown = None;
    // 2. Overlay + toast reset — avoid surfacing stale approval prompts
    // or lifecycle banners against a now-empty transcript.
    state.ui.overlay = None;
    state.ui.toasts.clear();
    // 3. Plan-state reset (only /clear all).
    if clear_plan_state {
        if let Some(session_id) = state.session.session_id.clone() {
            // Resolve plans dir using same precedence as the engine: env
            // `COCO_HOME`/config dir, no project override (we're in TUI,
            // settings aren't threaded here). Acceptable gap: if user set
            // a project-local `plansDirectory`, TUI's cleanup misses the
            // custom path. Engine-side SDK RPC would fix this cleanly.
            if let Some(config_home) = dirs::home_dir().map(|h| h.join(".cocode")) {
                let plans_dir = coco_context::resolve_plans_directory(&config_home, None, None);
                let _ = coco_context::delete_all_session_plan_files(&session_id, &plans_dir);
            }
            coco_context::clear_plan_slug(&session_id);
        }
        state
            .ui
            .add_toast(Toast::info(t!("toast.cleared_all").to_string()));
    } else {
        state
            .ui
            .add_toast(Toast::info(t!("toast.cleared_conversation").to_string()));
    }
}

/// Submit current input (or intercept local-only slash commands via
/// [`try_local_command`]). Returns `true` so the caller can propagate the
/// "state changed" signal.
pub(super) async fn submit(state: &mut AppState, command_tx: &mpsc::Sender<UserCommand>) -> bool {
    let text = state.ui.input.take_input();
    if text.is_empty() {
        return true;
    }

    let trimmed = text.trim();
    if try_local_command(state, trimmed) {
        return true;
    }
    // /clear needs the command channel to signal the engine; handle
    // separately. Keep `try_local_command` sync for QueueInput callers.
    if try_local_clear(state, trimmed, command_tx).await {
        return true;
    }

    state.ui.input.add_to_history(text.clone());
    let resolved = state.ui.paste_manager.resolve_structured(&text);
    let _ = command_tx
        .send(UserCommand::SubmitInput {
            content: resolved.text,
            display_text: Some(text),
            images: resolved.images,
        })
        .await;
    state.ui.paste_manager.clear();
    state.ui.scroll_offset = 0;
    state.ui.user_scrolled = false;
    true
}

/// Delete one word backwards from the cursor.
pub(super) fn delete_word_backward(state: &mut AppState) {
    while state.ui.input.cursor > 0 {
        let prev_char = state
            .ui
            .input
            .text
            .chars()
            .nth((state.ui.input.cursor - 1) as usize);
        if prev_char.is_none_or(char::is_whitespace) {
            break;
        }
        state.ui.input.backspace();
    }
}

/// Delete one word forward from the cursor.
pub(super) fn delete_word_forward(state: &mut AppState) {
    let len = state.ui.input.text.chars().count() as i32;
    while state.ui.input.cursor < len {
        let c = state
            .ui
            .input
            .text
            .chars()
            .nth(state.ui.input.cursor as usize);
        state.ui.input.delete_forward();
        if c.is_none_or(char::is_whitespace) {
            break;
        }
    }
}

/// Kill from cursor to end of current line (Emacs Ctrl+K) into the kill ring.
pub(super) fn kill_to_end_of_line(state: &mut AppState) {
    let cursor = state.ui.input.cursor as usize;
    let text = &state.ui.input.text;
    let byte_start = text
        .char_indices()
        .nth(cursor)
        .map(|(i, _)| i)
        .unwrap_or(text.len());
    let remaining = &text[byte_start..];
    let kill_end = remaining
        .find('\n')
        .map(|pos| byte_start + pos)
        .unwrap_or(text.len());
    let killed = text[byte_start..kill_end].to_string();
    if !killed.is_empty() {
        state.ui.kill_ring = killed;
        state.ui.input.text = format!("{}{}", &text[..byte_start], &text[kill_end..]);
    }
}

/// Yank (paste) the kill ring at the cursor.
pub(super) fn yank(state: &mut AppState) {
    if state.ui.kill_ring.is_empty() {
        return;
    }
    let yank_text = state.ui.kill_ring.clone();
    for c in yank_text.chars() {
        state.ui.input.insert_char(c);
    }
}

/// Up arrow: step toward less-relevant history entries (away from the top
/// frecency match). First Up surfaces the most relevant entry; subsequent
/// Ups walk down the frecency-sorted list.
pub(super) fn history_prev(state: &mut AppState) {
    let len = state.ui.input.history.len() as i32;
    if len == 0 {
        return;
    }
    let new_idx = match state.ui.input.history_index {
        None => 0,
        Some(i) if i + 1 < len => i + 1,
        Some(i) => i, // already at least-relevant tail; stay put
    };
    state.ui.input.history_index = Some(new_idx);
    state.ui.input.text = state.ui.input.history[new_idx as usize].text.clone();
    state.ui.input.cursor_end();
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
        state.ui.input.text = state.ui.input.history[new_idx as usize].text.clone();
        state.ui.input.cursor_end();
    } else {
        state.ui.input.history_index = None;
        state.ui.input.text.clear();
        state.ui.input.cursor = 0;
    }
}

/// Move cursor one word to the left.
pub(super) fn word_left(state: &mut AppState) {
    while state.ui.input.cursor > 0 {
        state.ui.input.cursor_left();
        let c = state
            .ui
            .input
            .text
            .chars()
            .nth(state.ui.input.cursor as usize);
        if c.is_none_or(char::is_whitespace) {
            break;
        }
    }
}

/// Move cursor one word to the right.
pub(super) fn word_right(state: &mut AppState) {
    let len = state.ui.input.text.chars().count() as i32;
    while state.ui.input.cursor < len {
        state.ui.input.cursor_right();
        let c = state
            .ui
            .input
            .text
            .chars()
            .nth(state.ui.input.cursor as usize);
        if c.is_none_or(char::is_whitespace) {
            break;
        }
    }
}
