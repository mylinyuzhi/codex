//! Rewind state update logic — extracted to stay under 800 LoC in update.rs.
//!
//! TS: MessageSelector.tsx state management + restore option handling.
//!
//! Per-row `+X -Y` summaries do NOT come from this module — they ride
//! the [`coco_types::TuiOnlyEvent::RewindRowMetadataReady`] event
//! emitted by the CLI driver, which computes them from
//! [`coco_context::FileHistoryState`] snapshot pairs (see
//! `app/cli/src/tui_runner.rs::RequestDiffStatsBatch`). TS reads
//! `msg.toolUseResult.structuredPatch`; we have no typed
//! tool-output side channel and use the file-history snapshots
//! instead — same observable numbers, single source of truth shared
//! with the selected restore preview.

use coco_messages::Message;

use crate::state::AppState;
use crate::state::rewind::RestoreType;
use crate::state::rewind::RewindPhase;
use crate::state::rewind::RewindState;
use crate::state::rewind::RewindableMessage;
use crate::state::rewind::build_restore_options;
use crate::state::transcript_view::CellKind;
use crate::state::transcript_view::RenderedCell;

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

/// Synthetic XML-tag substrings that mark non-user-authored content.
/// Mirrors `MessageSelector.tsx:788`'s `messageText.indexOf(`<TAG>`) !== -1`
/// filter — tags may appear anywhere in the text, not just at the
/// start, because user-visible messages can carry composed envelopes
/// (e.g. queued-command attachments wrap original prompt + synthetic
/// status frames).
const SYNTHETIC_XML_FRAGMENTS: &[&str] = &[
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

const BASH_INPUT_TAG: &str = "bash-input";
const COMMAND_MESSAGE_TAG: &str = "command-message";
const COMMAND_ARGS_TAG: &str = "command-args";
const SKILL_FORMAT_TAG: &str = "skill-format";

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

/// Extract the body of `<tag>...</tag>` or `<tag attr=...>...</tag>`.
///
/// Tag-name boundary is enforced: an `<{tag}>` or `<{tag} ` (followed
/// by a whitespace + attributes) open is required, so looking up
/// `bash-input` will not match `<bash-input-error>`. Mirrors TS
/// `extractTag` regex `<${escapedTag}(?:\\s+[^>]*?)?>`
/// (`utils/messages.ts:655`). Case-sensitive (TS uses `gi`, but every
/// tag we look up is a hard-coded lowercase constant).
fn extract_xml_block(text: &str, tag: &str) -> Option<String> {
    let close = format!("</{tag}>");
    let mut cursor = 0;
    while cursor < text.len() {
        let rel = text[cursor..].find(&format!("<{tag}"))?;
        let open_start = cursor + rel;
        let after_open_tag = open_start + 1 + tag.len();
        let next_byte = text.as_bytes().get(after_open_tag).copied();
        let after_open = match next_byte {
            Some(b'>') => after_open_tag + 1,
            Some(b' ') | Some(b'\t') | Some(b'\n') | Some(b'\r') => {
                // Skip past the attribute list to the next `>`.
                let attr_end = text[after_open_tag..].find('>')?;
                after_open_tag + attr_end + 1
            }
            // Followed by neither `>` nor whitespace — wrong tag
            // (e.g. `<bash-input-error` when looking for `bash-input`).
            // Skip past this `<` and keep searching.
            _ => {
                cursor = open_start + 1;
                continue;
            }
        };
        let end_rel = text[after_open..].find(&close)?;
        return Some(text[after_open..after_open + end_rel].to_string());
    }
    None
}

fn display_text_for_rewind_row(text: &str) -> String {
    let stripped = strip_prompt_xml_tags(text).trim().to_string();
    if stripped.is_empty() {
        return crate::i18n::t!("dialog.rewind_empty_message").to_string();
    }
    if let Some(input) = extract_xml_block(&stripped, BASH_INPUT_TAG)
        && !input.trim().is_empty()
    {
        return format!("! {}", input.trim());
    }
    if let Some(command) = extract_xml_block(&stripped, COMMAND_MESSAGE_TAG) {
        let command = command.trim();
        if !command.is_empty() {
            let is_skill = extract_xml_block(&stripped, SKILL_FORMAT_TAG)
                .is_some_and(|value| value.trim() == "true");
            if is_skill {
                return format!("Skill({command})");
            }
            let args = extract_xml_block(&stripped, COMMAND_ARGS_TAG)
                .map(|value| value.trim().to_string())
                .unwrap_or_default();
            return if args.is_empty() {
                format!("/{command}")
            } else {
                format!("/{command} {args}")
            };
        }
    }
    stripped
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

/// Check if a cell is a selectable user message for the rewind picker.
///
/// TS: `selectableUserMessagesFilter()` in `MessageSelector.tsx:767-792`.
/// Rejects tool results / synthetic messages / virtual user pushes /
/// compact-summary / transcript-only rows, plus content beginning with
/// the synthetic XML wrappers used for command output / teammate / task /
/// tick envelopes.
fn is_selectable_user_cell(cell: &RenderedCell) -> bool {
    let CellKind::UserText { text } = &cell.kind else {
        return false;
    };
    let Message::User(u) = cell.source.as_ref() else {
        return false;
    };
    if u.is_virtual || u.is_compact_summary || u.is_visible_in_transcript_only {
        return false;
    }
    // TS `MessageSelector.tsx:787-790` uses `indexOf(...) !== -1` for
    // each synthetic tag. The fragment may appear after user prose
    // when frames have been composed together (e.g. queued-command
    // attachments). `contains` matches that semantics.
    for fragment in SYNTHETIC_XML_FRAGMENTS {
        if text.contains(fragment) {
            return false;
        }
    }
    true
}

/// Build the initial RewindState pre-anchored to a specific user
/// message by UUID.
///
/// When `target_uuid` matches a real (non-synthetic) row, the state
/// opens directly in the `RestoreOptions` phase with that row selected
/// and `preselected = true` (Esc / Nevermind dismiss fully). TS:
/// `preselectedMessage` (`MessageSelector.tsx:42-44, 72-83`). Used by
/// message-actions edit gesture (TS `screens/REPL.tsx:3783-3784`).
///
/// Miss path (`target_uuid` not present in any real cell): returns
/// the bare picker state with `preselected = false`.
///
/// UUID match is structural (`Uuid == Uuid`), not stringly typed —
/// case-insensitive on input is handled at the parse boundary in
/// `RewindHandler`.
pub fn build_rewind_state_for_uuid(state: &AppState, target_uuid: uuid::Uuid) -> RewindState {
    let mut state = build_rewind_state_internal(state);
    tracing::debug!(
        target: "rewind::tui",
        %target_uuid,
        rows = state.messages.len(),
        "build_rewind_state_for_uuid: searching for preselect row",
    );
    let Some(row_idx) = state
        .messages
        .iter()
        .position(|m| !m.is_current_prompt && m.message_id == target_uuid)
    else {
        tracing::warn!(
            target: "rewind::tui",
            %target_uuid,
            real_rows = state.messages.iter().filter(|m| !m.is_current_prompt).count(),
            "build_rewind_state_for_uuid: preselect uuid not in transcript; \
             returning bare picker state (caller surfaces toast)",
        );
        return state;
    };
    tracing::info!(
        target: "rewind::tui",
        %target_uuid,
        row_idx,
        "build_rewind_state_for_uuid: preselect resolved to row",
    );
    state.selected = row_idx as i32;
    state.preselected = true;
    state.diff_stats = None;
    state.diff_stats_message_id = None;
    state.has_file_changes = false;
    state.available_options = build_restore_options(
        state.file_history_enabled,
        /*has_file_changes*/ false,
        state.allow_summarize_up_to,
    );
    state.option_selected = 0;
    state.phase = RewindPhase::RestoreOptions;
    state
}

/// Build the initial RewindState from current session state.
///
/// Sources from the engine-authoritative `transcript.cells()` so
/// engine-pushed user messages — the entire live transcript after
/// `engine-tui-unified-transcript-plan.md` Commit 2 — show up in the
/// rewind picker.
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

    let cells = state.session.transcript.cells();
    let file_history_enabled = state.session.file_history_enabled;
    for (i, cell) in cells.iter().enumerate() {
        if !is_selectable_user_cell(cell) {
            continue;
        }
        let CellKind::UserText { text } = &cell.kind else {
            continue;
        };
        let Message::User(user) = cell.source.as_ref() else {
            continue;
        };

        // TS `MessageSelector.tsx:618-624` substitutes the localized
        // `((empty message))` placeholder when `isEmptyMessageText`
        // returns true. We strip the same XML-tag wrapper that TS does
        // before deciding emptiness so a message containing only
        // `<commit_analysis>...</commit_analysis>` still renders the
        // placeholder.
        let display_text = display_text_for_rewind_row(text);

        rewindable.push(RewindableMessage {
            message_id: cell.message_uuid,
            message_index: i as i32,
            display_text,
            relative_time: format_relative_time_ago(now, timestamp_to_ms(&user.timestamp)),
            permission_mode: user.permission_mode,
            // Per-row `+X -Y` rides on `RewindRowMetadataReady`
            // emitted by the CLI driver — see module-level note. The
            // batch's `Some` / `None` choice also doubles as the
            // canonical `can_restore` resolution per row, so we leave
            // `can_restore_code` unknown until the event arrives.
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
        // `Uuid::nil()` is the canonical sentinel — `is_current_prompt`
        // is the gate that prevents this row from ever being matched
        // against a real preselect uuid (see lookup in
        // `build_rewind_state_for_uuid`).
        message_id: uuid::Uuid::nil(),
        message_index: -1,
        display_text: crate::i18n::t!("dialog.rewind_current_prompt").to_string(),
        relative_time: String::new(),
        permission_mode: None,
        // Synthetic row never enters `row_diff_stats_line` — the
        // renderer early-returns on `is_current_prompt`. Stats and
        // can_restore are immaterial.
        diff_stats: None,
        can_restore_code: None,
        is_current_prompt: true,
    });

    let selected = rewindable.len().saturating_sub(1) as i32;

    RewindState {
        phase: RewindPhase::MessageSelect,
        messages: rewindable,
        selected,
        option_selected: 0,
        available_options: Vec::new(),
        diff_stats: None,
        diff_stats_message_id: None,
        file_history_enabled,
        // Unknown until RewindRestorePreviewReady arrives. Do not
        // expose code-restore options from an optimistic default.
        has_file_changes: false,
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
    /// Diff stats are required before entering restore options.
    RequestDiffStats { message_id: String },
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
                    message_id: msg.message_id.to_string(),
                    restore: RestoreType::ConversationOnly,
                };
            }
            if msg.can_restore_code == Some(false) {
                state.diff_stats = None;
                state.diff_stats_message_id = Some(msg.message_id);
                state.has_file_changes = false;
                state.available_options = build_restore_options(
                    state.file_history_enabled,
                    /*has_file_changes*/ false,
                    state.allow_summarize_up_to,
                );
                state.option_selected = 0;
                state.phase = RewindPhase::RestoreOptions;
                return ConfirmOutcome::Phase;
            }
            let Some(diff_stats) = (state.diff_stats_message_id == Some(msg.message_id))
                .then(|| state.diff_stats.clone())
                .flatten()
            else {
                return ConfirmOutcome::RequestDiffStats {
                    message_id: msg.message_id.to_string(),
                };
            };
            let has_file_changes = !diff_stats.file_paths.is_empty();
            state.diff_stats = Some(diff_stats);
            state.diff_stats_message_id = Some(msg.message_id);
            state.has_file_changes = has_file_changes;
            state.available_options = build_restore_options(
                state.file_history_enabled,
                has_file_changes,
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
                        state.diff_stats_message_id = None;
                        state.option_selected = 0;
                        state.phase = RewindPhase::MessageSelect;
                        ConfirmOutcome::Phase
                    }
                }
                _ => ConfirmOutcome::Dispatch {
                    message_id: msg.message_id.to_string(),
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
                message_id: msg.message_id.to_string(),
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
            state.diff_stats_message_id = None;
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

/// Check if all cells after `from_index` are synthetic/non-meaningful.
///
/// TS: `messagesAfterAreOnlySynthetic` in `MessageSelector.tsx:799`.
/// Cell-side predicates mirror the 2 "meaningful → return false" arms:
/// - any virtual user message is skipped (synthetic engine push)
/// - tool results / system / attachment / progress rows are skipped
/// - assistant text with non-empty body (and non-`[redacted]`) is meaningful
/// - any `ToolUse` cell is meaningful even with empty text
/// - any other real user message is meaningful → not safe to truncate
pub fn cells_after_are_only_synthetic(cells: &[RenderedCell], from_index: usize) -> bool {
    for cell in cells.iter().skip(from_index + 1) {
        match &cell.kind {
            CellKind::UserText { .. } => {
                if matches!(cell.source.as_ref(), Message::User(u) if u.is_virtual) {
                    continue;
                }
                return false;
            }
            CellKind::AssistantText { text, .. } => {
                if !text.is_empty() && text != "[redacted]" {
                    return false;
                }
            }
            CellKind::ToolUse { .. } => {
                return false;
            }
            CellKind::AssistantThinking { .. }
            | CellKind::AssistantRedactedThinking
            | CellKind::ToolResult { .. }
            | CellKind::System(_)
            | CellKind::UserAttachment
            | CellKind::Attachment
            | CellKind::Progress
            | CellKind::Tombstone => continue,
        }
    }
    true
}

/// Find the last selectable user message index for auto-restore.
pub fn find_last_user_cell_index(cells: &[RenderedCell]) -> Option<usize> {
    cells.iter().rposition(is_selectable_user_cell)
}

/// Parse the engine `UserMessage.timestamp` string (ISO 8601 or epoch
/// integer) into epoch-ms. Returns 0 for empty / unparseable values
/// so the rewind picker renders "just now" rather than erroring out.
fn timestamp_to_ms(ts: &str) -> i64 {
    if ts.is_empty() {
        return 0;
    }
    if let Ok(n) = ts.parse::<i64>() {
        return n;
    }
    // Best-effort RFC 3339 parse: pull seconds via chrono if present,
    // else fall back to 0. The engine emits ISO timestamps; tests
    // typically leave this empty.
    chrono_iso_to_ms(ts).unwrap_or(0)
}

/// Lightweight RFC 3339 → epoch-ms conversion. Mirrors what
/// `chrono::DateTime::parse_from_rfc3339(...).timestamp_millis()` does
/// without adding a `chrono` dependency: we accept a strict subset
/// (`YYYY-MM-DDTHH:MM:SS[.frac][Z|±HH:MM]`). Returns `None` for
/// anything else — callers fall back to 0.
fn chrono_iso_to_ms(ts: &str) -> Option<i64> {
    let bytes = ts.as_bytes();
    if bytes.len() < 19 || bytes[4] != b'-' || bytes[7] != b'-' || bytes[10] != b'T' {
        return None;
    }
    let year: i64 = ts.get(0..4)?.parse().ok()?;
    let month: i64 = ts.get(5..7)?.parse().ok()?;
    let day: i64 = ts.get(8..10)?.parse().ok()?;
    let hour: i64 = ts.get(11..13)?.parse().ok()?;
    let minute: i64 = ts.get(14..16)?.parse().ok()?;
    let second: i64 = ts.get(17..19)?.parse().ok()?;
    // Days since Unix epoch (1970-01-01) — civil-from-days algorithm.
    let y = year - i64::from(month <= 2);
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (month + (if month > 2 { -3 } else { 9 })) + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146097 + doe - 719468;
    let secs = days * 86400 + hour * 3600 + minute * 60 + second;
    Some(secs * 1000)
}

#[cfg(test)]
#[path = "update_rewind.test.rs"]
mod tests;
