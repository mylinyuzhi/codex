//! Rewind state update logic — extracted to stay under 800 LoC in update.rs.
//!
//! TS: MessageSelector.tsx state management + restore option handling.

use crate::state::AppState;
use crate::state::ChatRole;
use crate::state::rewind::RestoreType;
use crate::state::rewind::RewindPhase;
use crate::state::rewind::RewindState;
use crate::state::rewind::RewindableMessage;
use crate::state::rewind::build_restore_options;

/// Format an epoch-ms timestamp as a relative-time English phrase
/// against a reference `now` (also epoch-ms). Mirrors TS
/// `formatRelativeTimeAgo` (`utils/format.ts`) coarsely — exact text
/// is locale-resolved later via the `t!` macro on display.
pub fn format_relative_time_ago(now_ms: i64, then_ms: i64) -> String {
    let delta_secs = ((now_ms - then_ms).max(0) / 1000) as u64;
    if delta_secs < 60 {
        return "just now".to_string();
    }
    if delta_secs < 60 * 60 {
        let m = delta_secs / 60;
        return if m == 1 {
            "1 minute ago".to_string()
        } else {
            format!("{m} minutes ago")
        };
    }
    if delta_secs < 60 * 60 * 24 {
        let h = delta_secs / 3600;
        return if h == 1 {
            "1 hour ago".to_string()
        } else {
            format!("{h} hours ago")
        };
    }
    let d = delta_secs / (60 * 60 * 24);
    if d == 1 {
        "1 day ago".to_string()
    } else {
        format!("{d} days ago")
    }
}

/// Synthetic XML-tag prefixes that mark non-user-authored content.
/// Mirrors `MessageSelector.tsx:788` filter list.
const SYNTHETIC_XML_PREFIXES: &[&str] = &[
    "<local-command-stdout>",
    "<local-command-stderr>",
    "<bash-stdout>",
    "<bash-stderr>",
    "<task-notification>",
    "<tick>",
    "<teammate-message",
];

/// IDE-injected context tags stripped from restored input. Mirrors TS
/// `stripIdeContextTags()` in `utils/displayTags.ts:49-51`.
const IDE_CONTEXT_TAG_NAMES: &[&str] = &["ide_opened_file", "ide_selection"];

/// XML-tag-block prompt prefixes stripped from picker display text. TS
/// `stripPromptXMLTags` (`utils/messages.ts:2761-2763`).
const PROMPT_XML_TAG_NAMES: &[&str] = &[
    "commit_analysis",
    "context",
    "function_analysis",
    "pr_analysis",
];

/// Strip prompt-only XML tag blocks. Mirrors TS `stripPromptXMLTags`.
pub fn strip_prompt_xml_tags(text: &str) -> String {
    let mut out = text.to_string();
    for tag in PROMPT_XML_TAG_NAMES {
        out = strip_xml_block(&out, tag);
    }
    out
}

fn strip_xml_block(text: &str, tag: &str) -> String {
    let open = format!("<{tag}");
    let close = format!("</{tag}>");
    let mut out = text.to_string();
    loop {
        let Some(start) = out.find(&open) else {
            break;
        };
        let after_open = match out[start..].find('>') {
            Some(p) => start + p + 1,
            None => break,
        };
        let end = match out[after_open..].find(&close) {
            Some(p) => after_open + p + close.len(),
            None => break,
        };
        out.replace_range(start..end, "");
    }
    out
}

/// Strip IDE-injected context tags from a string. TS `stripIdeContextTags`
/// (`utils/displayTags.ts:49-51`). Used by `textForResubmit` so an UP-arrow
/// resubmit keeps user-typed content while dropping IDE-injected noise.
pub fn strip_ide_context_tags(text: &str) -> String {
    let mut out = text.to_string();
    for tag in IDE_CONTEXT_TAG_NAMES {
        out = strip_xml_block(&out, tag);
    }
    // TS trims trailing newline after the closing tag.
    out.trim().to_string()
}

/// Check if a message is a selectable user message for the rewind picker.
///
/// TS: `selectableUserMessagesFilter()` in `MessageSelector.tsx:767-792`.
/// Rejects tool results / synthetic messages / meta / compact-summary /
/// transcript-only, plus content beginning with the synthetic XML
/// wrappers used for command output / teammate / task / tick envelopes.
fn is_selectable_user_message(msg: &crate::state::ChatMessage) -> bool {
    if msg.role != ChatRole::User {
        return false;
    }
    if msg.is_meta || msg.is_compact_summary || msg.is_visible_in_transcript_only {
        return false;
    }
    // Reject every non-text user content variant — TS filters tool_result
    // first-content-block (`MessageSelector.tsx:771`) and uses
    // `isSyntheticMessage` (line 774) to drop most non-user-authored
    // entries. In coco-rs that maps to: keep only `Text` / `Image` /
    // `BashInput`; drop everything else (BashOutput, Attachment,
    // ChannelMessage, ResourceUpdate, AgentNotification,
    // TeammateMessage, PlanMarker, CompactBoundary, …).
    use crate::state::MessageContent;
    if !matches!(
        msg.content,
        MessageContent::Text(_) | MessageContent::Image { .. } | MessageContent::BashInput { .. }
    ) {
        return false;
    }
    // Filter out synthetic XML-wrapped content (TS: indexOf checks).
    let text = msg.text_content();
    let trimmed = text.trim_start();
    for prefix in SYNTHETIC_XML_PREFIXES {
        if trimmed.starts_with(prefix) {
            return false;
        }
    }
    true
}

/// Build the initial RewindState from current session state, optionally
/// pre-anchored to a specific message.
///
/// When `preselect_message_id` matches a real (non-synthetic) row, the
/// state opens directly in the `RestoreOptions` phase with that row
/// selected. TS: `preselectedMessage` (`MessageSelector.tsx:42-44`,
/// 72-83). Used by the message-actions `edit` flow.
///
/// `preselect_message_id = None` → identical to `build_rewind_state`.
pub fn build_rewind_state_for(state: &AppState, preselect_message_id: Option<&str>) -> RewindState {
    let mut state = build_rewind_state_internal(state);
    let Some(target_id) = preselect_message_id else {
        return state;
    };
    let Some(row_idx) = state
        .messages
        .iter()
        .position(|m| !m.is_current_prompt && m.message_id == target_id)
    else {
        // Unknown id — fall back to the standard pick-list.
        return state;
    };
    state.selected = row_idx as i32;
    state.preselected = true;
    state.available_options = build_restore_options(
        state.file_history_enabled,
        state.has_file_changes,
        state.allow_summarize_up_to,
    );
    state.option_selected = 0;
    state.phase = RewindPhase::RestoreOptions;
    state
}

/// Build the initial RewindState from current session state.
///
/// Sources from the merged transcript view (legacy `session.messages` +
/// engine-derived cells) so engine-pushed user messages — the bulk of
/// the live transcript after `engine-tui-unified-transcript-plan.md`
/// Commit 2 — show up in the rewind picker. Prior to that, the picker
/// only saw TUI optimistic echoes which no longer exist.
/// TS: MessageSelector receives `messages` prop filtered by selectableUserMessagesFilter.
pub fn build_rewind_state(state: &AppState) -> RewindState {
    build_rewind_state_internal(state)
}

fn build_rewind_state_internal(state: &AppState) -> RewindState {
    // TS: tengu_message_selector_opened
    tracing::info!(target: "rewind", event = "selector_opened");
    let mut rewindable: Vec<RewindableMessage> = Vec::new();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);

    let messages = state.session.transcript_messages();
    for (i, msg) in messages.iter().enumerate() {
        if !is_selectable_user_message(msg) {
            continue;
        }

        // TS `MessageSelector.tsx:618-624` substitutes the localized
        // `((empty message))` placeholder when `isEmptyMessageText`
        // returns true. We strip the same XML-tag wrapper that TS does
        // before deciding emptiness so a message containing only
        // `<commit_analysis>...</commit_analysis>` still renders the
        // placeholder.
        let display_text = {
            let raw = msg.text_content();
            let stripped = strip_prompt_xml_tags(raw).trim().to_string();
            if stripped.is_empty() {
                crate::i18n::t!("dialog.rewind_empty_message").to_string()
            } else if stripped.len() > 50 {
                format!("{}...", &stripped[..stripped.floor_char_boundary(47)])
            } else {
                stripped
            }
        };

        rewindable.push(RewindableMessage {
            message_id: msg.id.clone(),
            message_index: i as i32,
            display_text,
            relative_time: format_relative_time_ago(now, msg.created_at_ms),
            permission_mode: msg.permission_mode,
            diff_stats: None,
            can_restore_code: None,
            is_current_prompt: false,
        });
    }

    // TS `MessageSelector.tsx:60-66` appends a synthetic current-prompt
    // entry: `[...realMessages, { ...createUserMessage({ content: '' }), uuid }]`.
    // It anchors the default selection to "now" — the user must move up
    // to indicate intent to rewind. Confirm on this row dispatches no
    // rewind (TS line 165: `!messages.includes(message_0) -> onClose()`).
    rewindable.push(RewindableMessage {
        message_id: String::new(),
        message_index: -1,
        display_text: crate::i18n::t!("dialog.rewind_current_prompt").to_string(),
        relative_time: String::new(),
        permission_mode: None,
        // Mark "loaded with zero changes" so the per-row diff path renders
        // nothing for the synthetic row instead of showing a spinner-like
        // gap while file_history fetches resolve.
        diff_stats: Some(crate::state::DiffStatsPreview::default()),
        can_restore_code: Some(false),
        is_current_prompt: true,
    });

    let selected = rewindable.len().saturating_sub(1) as i32;

    // Read file_history_enabled from session state (set by tui_runner).
    // TS: fileHistoryEnabled() in fileHistory.ts
    let file_history_enabled = state.session.file_history_enabled;

    RewindState {
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
        allow_summarize_up_to: state.session.allow_summarize_up_to,
        summarize_feedback: String::new(),
        pending_summarize: None,
        preselected: false,
    }
}

/// Navigate up/down in the rewind state.
///
/// In MessageSelect phase, navigates the message list.
/// In RestoreOptions phase, navigates the option list.
pub fn handle_rewind_nav(state: &mut RewindState, delta: i32) {
    match state.phase {
        RewindPhase::MessageSelect => {
            let count = state.messages.len() as i32;
            state.selected = (state.selected + delta).clamp(0, (count - 1).max(0));
        }
        RewindPhase::RestoreOptions => {
            let count = state.available_options.len() as i32;
            state.option_selected = (state.option_selected + delta).clamp(0, (count - 1).max(0));
        }
        // Feedback / confirming phases ignore arrow navigation.
        RewindPhase::SummarizeFeedback | RewindPhase::Confirming => {}
    }
}

/// Outcome of `handle_rewind_confirm`. TS encodes the same three
/// outcomes implicitly: `restoreConversationDirectly` / option-screen
/// transition / synthetic-row `onClose`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfirmOutcome {
    /// Dispatch the rewind to the engine with this message + restore type.
    Dispatch {
        message_id: String,
        restore: RestoreType,
    },
    /// Phase transition only (no dispatch yet).
    Phase,
    /// Dismiss the state (synthetic current-prompt row, or cancel-on-confirm).
    /// TS: `MessageSelector.tsx:165` — `if (!messages.includes(message_0)) onClose()`.
    Dismiss,
}

/// Handle Enter/confirm in the rewind state.
///
/// Returns `ConfirmOutcome` so the dispatcher knows whether to send the
/// rewind, keep the state open in a new phase, or dismiss it.
///
/// TS: MessageSelector onSelect -> onRestoreOptionSelect
pub fn handle_rewind_confirm(state: &mut RewindState) -> ConfirmOutcome {
    use crate::state::rewind::SummarizeDirection;
    match state.phase {
        RewindPhase::MessageSelect => {
            let Some(msg) = state.messages.get(state.selected as usize) else {
                return ConfirmOutcome::Phase;
            };
            // Synthetic `(current)` row — TS `MessageSelector.tsx:165`.
            if msg.is_current_prompt {
                tracing::info!(target: "rewind", event = "selector_cancelled_via_current_row");
                return ConfirmOutcome::Dismiss;
            }
            // TS: tengu_message_selector_selected
            tracing::info!(
                target: "rewind",
                event = "message_selected",
                index_from_end = state.messages.len() as i32 - state.selected - 1,
            );
            // TS `MessageSelector.tsx:169-172`: when file history is
            // disabled the selector skips the option screen entirely
            // and dispatches `restoreConversationDirectly`. Mirror by
            // returning ConversationOnly straight away.
            if !state.file_history_enabled {
                return ConfirmOutcome::Dispatch {
                    message_id: msg.message_id.clone(),
                    restore: RestoreType::ConversationOnly,
                };
            }
            state.available_options = build_restore_options(
                state.file_history_enabled,
                state.has_file_changes,
                state.allow_summarize_up_to,
            );
            state.option_selected = 0;
            state.phase = RewindPhase::RestoreOptions;
            ConfirmOutcome::Phase
        }
        RewindPhase::RestoreOptions => {
            let Some(msg) = state.messages.get(state.selected as usize) else {
                return ConfirmOutcome::Phase;
            };
            let Some(opt) = state
                .available_options
                .get(state.option_selected as usize)
                .cloned()
            else {
                return ConfirmOutcome::Phase;
            };
            // TS: tengu_message_selector_restore_option_selected
            tracing::info!(
                target: "rewind",
                event = "restore_option_selected",
                option = ?opt,
            );
            // Summarize variants need the optional feedback box first.
            match &opt {
                RestoreType::SummarizeFrom { .. } => {
                    state.pending_summarize = Some(SummarizeDirection::From);
                    state.summarize_feedback.clear();
                    state.phase = RewindPhase::SummarizeFeedback;
                    ConfirmOutcome::Phase
                }
                RestoreType::SummarizeUpTo { .. } => {
                    state.pending_summarize = Some(SummarizeDirection::UpTo);
                    state.summarize_feedback.clear();
                    state.phase = RewindPhase::SummarizeFeedback;
                    ConfirmOutcome::Phase
                }
                // TS `MessageSelector.tsx:185-188`: Nevermind cancels
                // the option pick. When launched preselected there is
                // no message list to fall back to, so it dismisses
                // (TS line 186: `if (preselectedMessage) onClose()`).
                RestoreType::Nevermind => {
                    if state.preselected {
                        ConfirmOutcome::Dismiss
                    } else {
                        state.available_options.clear();
                        state.diff_stats = None;
                        state.option_selected = 0;
                        state.phase = RewindPhase::MessageSelect;
                        ConfirmOutcome::Phase
                    }
                }
                _ => ConfirmOutcome::Dispatch {
                    message_id: msg.message_id.clone(),
                    restore: opt,
                },
            }
        }
        RewindPhase::SummarizeFeedback => {
            let Some(msg) = state.messages.get(state.selected as usize) else {
                return ConfirmOutcome::Phase;
            };
            // TS `allowEmptySubmitToCancel: true` — empty submit cancels
            // the summarize choice and returns to the option list.
            let fb = state.summarize_feedback.trim();
            if fb.is_empty() {
                state.summarize_feedback.clear();
                state.pending_summarize = None;
                state.phase = RewindPhase::RestoreOptions;
                return ConfirmOutcome::Phase;
            }
            // Peek (don't take) — keep `pending_summarize` set so the
            // renderer's Confirming phase can show "Summarizing…".
            let Some(dir) = state.pending_summarize else {
                return ConfirmOutcome::Phase;
            };
            let feedback = Some(fb.to_string());
            let restore = match dir {
                SummarizeDirection::From => RestoreType::SummarizeFrom { feedback },
                SummarizeDirection::UpTo => RestoreType::SummarizeUpTo { feedback },
            };
            ConfirmOutcome::Dispatch {
                message_id: msg.message_id.clone(),
                restore,
            }
        }
        RewindPhase::Confirming => ConfirmOutcome::Phase,
    }
}

/// Handle Esc/cancel in the rewind state.
///
/// Returns `true` if the state should be fully dismissed (was in MessageSelect).
/// Returns `false` if it went back to a previous phase (RestoreOptions -> MessageSelect).
///
/// TS: Esc in restore options goes back to message list; Esc in message list closes.
pub fn handle_rewind_cancel(state: &mut RewindState) -> bool {
    match state.phase {
        RewindPhase::MessageSelect => {
            // TS: tengu_message_selector_cancelled
            tracing::info!(target: "rewind", event = "selector_cancelled");
            true
        }
        RewindPhase::RestoreOptions => {
            // TS `MessageSelector.tsx:248-253`: when launched preselected
            // there is no message list to fall back to — Esc closes the
            // state entirely.
            if state.preselected {
                tracing::info!(target: "rewind", event = "selector_cancelled_preselected");
                return true;
            }
            state.phase = RewindPhase::MessageSelect;
            state.available_options.clear();
            state.diff_stats = None;
            false
        }
        RewindPhase::SummarizeFeedback => {
            // Esc in the feedback box goes back to the option list,
            // discarding the typed feedback. TS: SummarizeOption's
            // `allowEmptySubmitToCancel` plus Esc routing.
            state.summarize_feedback.clear();
            state.pending_summarize = None;
            state.phase = RewindPhase::RestoreOptions;
            false
        }
        RewindPhase::Confirming => true,
    }
}

/// Check if all messages after `from_index` are synthetic/non-meaningful.
///
/// TS: `messagesAfterAreOnlySynthetic` in `MessageSelector.tsx:799`. We
/// mirror its 6 skip predicates and 2 "meaningful → return false" arms
/// byte-by-byte so auto-restore behavior matches across providers:
/// - synthetic interrupt markers and user-meta rows are skipped
/// - tool results / system / attachment / progress rows are skipped
/// - assistant messages count as meaningful when they contain text OR
///   any `tool_use` block (the latter mirrors TS's
///   `block.type === 'tool_use'` short-circuit)
/// - any other user message is meaningful → not safe to truncate
pub fn messages_after_are_only_synthetic(
    messages: &[crate::state::ChatMessage],
    from_index: usize,
) -> bool {
    use crate::state::MessageContent;
    for msg in messages.iter().skip(from_index + 1) {
        match msg.role {
            ChatRole::User => {
                if msg.is_meta {
                    continue;
                }
                // Synthetic interruption marker — TS `isSyntheticMessage`
                // matches text against `INTERRUPT_MESSAGE` /
                // `INTERRUPT_MESSAGE_FOR_TOOL_USE` (utils/messages.ts:302).
                if matches!(msg.content, MessageContent::InterruptionMarker { .. }) {
                    continue;
                }
                // Real user message — not synthetic
                return false;
            }
            crate::state::ChatRole::Assistant => {
                // Tool calls count as meaningful even with empty text
                // (TS: `block.type === 'tool_use'`).
                if matches!(msg.content, MessageContent::ToolUse { .. }) {
                    return false;
                }
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
