//! Rewind overlay update logic — extracted to stay under 800 LoC in update.rs.
//!
//! TS: MessageSelector.tsx state management + restore option handling.

use crate::state::AppState;
use crate::state::ChatRole;
use crate::state::rewind::RestoreType;
use crate::state::rewind::RewindOverlay;
use crate::state::rewind::RewindPhase;
use crate::state::rewind::RewindableMessage;
use crate::state::rewind::build_restore_options;

/// Check if a message is a selectable user message for the rewind picker.
///
/// TS: selectableUserMessagesFilter() in MessageSelector.tsx — filters out
/// tool results, synthetic messages, meta messages, compact summaries.
fn is_selectable_user_message(msg: &crate::state::ChatMessage) -> bool {
    if msg.role != ChatRole::User {
        return false;
    }
    if msg.is_meta {
        return false;
    }
    // Filter out tool results displayed as user messages
    if matches!(
        msg.content,
        crate::state::MessageContent::BashOutput { .. }
            | crate::state::MessageContent::Attachment { .. }
    ) {
        return false;
    }
    // Filter out compact boundary markers
    if matches!(msg.content, crate::state::MessageContent::CompactBoundary) {
        return false;
    }
    true
}

/// Build the initial RewindOverlay from current session state.
///
/// Extracts user messages from session.messages, builds RewindableMessage list.
/// TS: MessageSelector receives `messages` prop filtered by selectableUserMessagesFilter.
pub fn build_rewind_overlay(state: &AppState) -> RewindOverlay {
    // TS: tengu_message_selector_opened
    tracing::info!(target: "rewind", event = "selector_opened");
    let mut rewindable: Vec<RewindableMessage> = Vec::new();
    let mut turn_number = 0i32;

    for (i, msg) in state.session.messages.iter().enumerate() {
        if !is_selectable_user_message(msg) {
            continue;
        }
        turn_number += 1;

        let display_text = {
            let text = msg.text_content();
            if text.len() > 50 {
                format!("{}...", &text[..text.floor_char_boundary(47)])
            } else {
                text.to_string()
            }
        };

        rewindable.push(RewindableMessage {
            message_id: msg.id.clone(),
            message_index: i as i32,
            display_text,
            turn_label: format!("Turn {turn_number}"),
            permission_mode: msg.permission_mode,
        });
    }

    let selected = rewindable.len().saturating_sub(1) as i32;

    // Read file_history_enabled from session state (set by tui_runner).
    // TS: fileHistoryEnabled() in fileHistory.ts
    let file_history_enabled = state.session.file_history_enabled;

    RewindOverlay {
        phase: RewindPhase::MessageSelect,
        messages: rewindable,
        selected,
        option_selected: 0,
        available_options: Vec::new(),
        diff_stats: None,
        file_history_enabled,
        // Initial assumption: file changes exist if file_history is enabled.
        // The actual check is async (via RequestDiffStats → DiffStatsLoaded),
        // which updates has_file_changes and rebuilds available_options when
        // the response arrives. This is a safe default — showing code restore
        // options before the async check completes is a better UX than hiding
        // them and then showing (layout shift).
        has_file_changes: file_history_enabled,
    }
}

/// Navigate up/down in the rewind overlay.
///
/// In MessageSelect phase, navigates the message list.
/// In RestoreOptions phase, navigates the option list.
pub fn handle_rewind_nav(overlay: &mut RewindOverlay, delta: i32) {
    match overlay.phase {
        RewindPhase::MessageSelect => {
            let count = overlay.messages.len() as i32;
            overlay.selected = (overlay.selected + delta).clamp(0, (count - 1).max(0));
        }
        RewindPhase::RestoreOptions => {
            let count = overlay.available_options.len() as i32;
            overlay.option_selected =
                (overlay.option_selected + delta).clamp(0, (count - 1).max(0));
        }
        RewindPhase::Confirming => {}
    }
}

/// Handle Enter/confirm in the rewind overlay.
///
/// Returns `Some((message_id, restore_type))` when a final selection is made,
/// `None` when transitioning between phases or no valid selection.
///
/// TS: MessageSelector onSelect -> onRestoreOptionSelect
pub fn handle_rewind_confirm(overlay: &mut RewindOverlay) -> Option<(String, RestoreType)> {
    match overlay.phase {
        RewindPhase::MessageSelect => {
            if let Some(msg) = overlay.messages.get(overlay.selected as usize) {
                // TS: tengu_message_selector_selected
                tracing::info!(
                    target: "rewind",
                    event = "message_selected",
                    index_from_end = overlay.messages.len() as i32 - overlay.selected - 1,
                );
                // Transition to RestoreOptions phase.
                overlay.available_options =
                    build_restore_options(overlay.file_history_enabled, overlay.has_file_changes);
                overlay.option_selected = 0;
                overlay.phase = RewindPhase::RestoreOptions;
                let _ = msg;
            }
            None
        }
        RewindPhase::RestoreOptions => {
            let msg = overlay.messages.get(overlay.selected as usize)?;
            let opt = *overlay
                .available_options
                .get(overlay.option_selected as usize)?;
            // TS: tengu_message_selector_restore_option_selected
            tracing::info!(
                target: "rewind",
                event = "restore_option_selected",
                option = ?opt,
            );
            Some((msg.message_id.clone(), opt))
        }
        RewindPhase::Confirming => None,
    }
}

/// Handle Esc/cancel in the rewind overlay.
///
/// Returns `true` if the overlay should be fully dismissed (was in MessageSelect).
/// Returns `false` if it went back to a previous phase (RestoreOptions -> MessageSelect).
///
/// TS: Esc in restore options goes back to message list; Esc in message list closes.
pub fn handle_rewind_cancel(overlay: &mut RewindOverlay) -> bool {
    match overlay.phase {
        RewindPhase::MessageSelect => {
            // TS: tengu_message_selector_cancelled
            tracing::info!(target: "rewind", event = "selector_cancelled");
            true
        }
        RewindPhase::RestoreOptions => {
            overlay.phase = RewindPhase::MessageSelect;
            overlay.available_options.clear();
            overlay.diff_stats = None;
            false
        }
        RewindPhase::Confirming => true,
    }
}

/// Compute visible message window (centered on selected).
///
/// TS: MAX_VISIBLE_MESSAGES = 7, firstVisibleIndex calculation.
pub fn visible_range(overlay: &RewindOverlay) -> (usize, usize) {
    let max_visible = crate::constants::REWIND_MAX_VISIBLE as usize;
    let count = overlay.messages.len();
    if count <= max_visible {
        return (0, count);
    }
    let center = overlay.selected as usize;
    let half = max_visible / 2;
    let start = center.saturating_sub(half).min(count - max_visible);
    (start, start + max_visible)
}

/// Check if all messages after `from_index` are synthetic/non-meaningful.
///
/// TS: messagesAfterAreOnlySynthetic() in MessageSelector.tsx.
/// Returns true if it's safe to auto-restore without losing meaningful work.
pub fn messages_after_are_only_synthetic(
    messages: &[crate::state::ChatMessage],
    from_index: usize,
) -> bool {
    for msg in messages.iter().skip(from_index + 1) {
        match msg.role {
            ChatRole::User => {
                if msg.is_meta {
                    continue;
                }
                // Real user message — not synthetic
                return false;
            }
            crate::state::ChatRole::Assistant => {
                // Assistant message with actual text content is meaningful
                let text = msg.text_content();
                if !text.is_empty() && text != "[redacted]" {
                    return false;
                }
            }
            // Tool results, system messages — synthetic
            _ => continue,
        }
    }
    true
}

/// Find the last selectable user message index for auto-restore.
pub fn find_last_user_message_index(messages: &[crate::state::ChatMessage]) -> Option<usize> {
    messages.iter().rposition(is_selectable_user_message)
}

#[cfg(test)]
#[path = "update_rewind.test.rs"]
mod tests;
