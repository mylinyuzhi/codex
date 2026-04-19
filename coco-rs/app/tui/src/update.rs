//! TUI update handler — the Update in TEA.
//!
//! Applies [`TuiCommand`]s to [`AppState`]. Side effects (sending to core)
//! are dispatched via the command channel. Complex per-category handlers
//! live in the private submodules (`overlay`, `show`, `edit`) to keep this
//! dispatcher focused on routing.

use tokio::sync::mpsc;

use crate::command::UserCommand;
use crate::constants;
use crate::events::TuiCommand;
use crate::state::AppState;
use crate::state::FocusTarget;

mod clipboard;
mod edit;
mod overlay;
mod show;

#[cfg(test)]
#[path = "update.test.rs"]
mod tests;

/// Apply a TUI command to the state.
///
/// Returns `true` if the state changed and a redraw is needed.
pub async fn handle_command(
    state: &mut AppState,
    cmd: TuiCommand,
    command_tx: &mpsc::Sender<UserCommand>,
) -> bool {
    // Snapshot input before the command so we can reactively refresh the
    // autocomplete popup whenever input text or cursor moves, without
    // threading a refresh call through every editing arm.
    let text_before = state.ui.input.text.clone();
    let cursor_before = state.ui.input.cursor;

    let changed = match cmd {
        // ── Mode toggles ──
        TuiCommand::TogglePlanMode => {
            state.toggle_plan_mode();
            let _ = command_tx
                .send(UserCommand::SetPermissionMode {
                    mode: state.session.permission_mode,
                })
                .await;
            true
        }
        TuiCommand::CyclePermissionMode => {
            state.cycle_permission_mode();
            let _ = command_tx
                .send(UserCommand::SetPermissionMode {
                    mode: state.session.permission_mode,
                })
                .await;
            true
        }
        TuiCommand::CycleThinkingLevel => {
            let _ = command_tx
                .send(UserCommand::SetThinkingLevel {
                    level: "medium".to_string(),
                })
                .await;
            true
        }
        TuiCommand::ToggleThinking => {
            state.ui.show_thinking = !state.ui.show_thinking;
            true
        }
        TuiCommand::CycleModel => {
            show::cycle_model(state);
            true
        }
        TuiCommand::ToggleFastMode => {
            state.session.fast_mode = !state.session.fast_mode;
            let _ = command_tx.send(UserCommand::ToggleFastMode).await;
            true
        }

        // ── Input actions ──
        TuiCommand::SubmitInput => edit::submit(state, command_tx).await,
        TuiCommand::QueueInput => {
            let text = state.ui.input.take_input();
            if text.is_empty() {
                return true;
            }
            // Local-only slash commands (/copy, /rewind, /checkpoint) must
            // dispatch immediately even while a turn is streaming, rather
            // than being queued to the agent which wouldn't know what to do
            // with them. Shared with `edit::submit` so both paths behave
            // identically.
            if edit::try_local_command(state, text.trim()) {
                return true;
            }
            state.session.queued_commands.push_back(text.clone());
            let _ = command_tx
                .send(UserCommand::QueueCommand { prompt: text })
                .await;
            true
        }
        TuiCommand::Interrupt => {
            state.session.was_interrupted = true;
            let _ = command_tx.send(UserCommand::Interrupt).await;
            true
        }
        TuiCommand::Cancel => {
            // Esc dismisses autocomplete first (so the user can escape out
            // of a trigger without losing their typed input) before
            // touching any overlay.
            if state.ui.overlay.is_none() && state.ui.active_suggestions.is_some() {
                state.ui.active_suggestions = None;
                return true;
            }
            if !overlay::rewind_cancel(state) {
                return true; // phase-back; keep overlay
            }
            if state.has_overlay() {
                state.ui.dismiss_overlay();
            }
            true
        }
        TuiCommand::ClearScreen => {
            state.session.messages.clear();
            // Dropping messages also invalidates the copy cache — without this
            // Ctrl+O after /clear would surface the response the user just
            // wiped, which is surprising. Matches codex-rs's clear-on-reset.
            state.session.last_agent_markdown = None;
            state.ui.scroll_offset = 0;
            true
        }

        // ── Text editing ──
        TuiCommand::InsertChar(c) => {
            state.ui.input.insert_char(c);
            true
        }
        TuiCommand::InsertNewline => {
            state.ui.input.insert_char('\n');
            true
        }
        TuiCommand::DeleteBackward => {
            state.ui.input.backspace();
            true
        }
        TuiCommand::DeleteForward => {
            state.ui.input.delete_forward();
            true
        }
        TuiCommand::DeleteWordBackward => {
            edit::delete_word_backward(state);
            true
        }
        TuiCommand::DeleteWordForward => {
            edit::delete_word_forward(state);
            true
        }
        TuiCommand::KillToEndOfLine => {
            edit::kill_to_end_of_line(state);
            true
        }
        TuiCommand::Yank => {
            edit::yank(state);
            true
        }

        // ── Cursor movement ──
        TuiCommand::CursorLeft => {
            state.ui.input.cursor_left();
            true
        }
        TuiCommand::CursorRight => {
            state.ui.input.cursor_right();
            true
        }
        TuiCommand::CursorUp => {
            edit::history_prev(state);
            true
        }
        TuiCommand::CursorDown => {
            edit::history_next(state);
            true
        }
        TuiCommand::CursorHome => {
            state.ui.input.cursor_home();
            true
        }
        TuiCommand::CursorEnd => {
            state.ui.input.cursor_end();
            true
        }
        TuiCommand::WordLeft => {
            edit::word_left(state);
            true
        }
        TuiCommand::WordRight => {
            edit::word_right(state);
            true
        }

        // ── Scrolling ──
        TuiCommand::ScrollUp => {
            state.ui.scroll_offset += constants::SCROLL_LINE_STEP;
            state.ui.user_scrolled = true;
            true
        }
        TuiCommand::ScrollDown => {
            state.ui.scroll_offset = (state.ui.scroll_offset - constants::SCROLL_LINE_STEP).max(0);
            if state.ui.scroll_offset == 0 {
                state.ui.user_scrolled = false;
            }
            true
        }
        TuiCommand::PageUp => {
            state.ui.scroll_offset += constants::SCROLL_PAGE_STEP;
            state.ui.user_scrolled = true;
            true
        }
        TuiCommand::PageDown => {
            state.ui.scroll_offset = (state.ui.scroll_offset - constants::SCROLL_PAGE_STEP).max(0);
            if state.ui.scroll_offset == 0 {
                state.ui.user_scrolled = false;
            }
            true
        }

        // ── Focus ──
        TuiCommand::FocusNext | TuiCommand::FocusPrevious => {
            state.ui.focus = match state.ui.focus {
                FocusTarget::Input => FocusTarget::Chat,
                FocusTarget::Chat => FocusTarget::Input,
            };
            true
        }
        TuiCommand::FocusNextAgent => {
            let max = state.session.subagents.len() as i32;
            if max > 0 {
                let idx = state.session.focused_subagent_index.unwrap_or(-1);
                state.session.focused_subagent_index = Some((idx + 1).min(max - 1));
            }
            true
        }
        TuiCommand::FocusPrevAgent => {
            if let Some(idx) = state.session.focused_subagent_index {
                state.session.focused_subagent_index = if idx > 0 { Some(idx - 1) } else { None };
            }
            true
        }

        // ── Overlay actions ──
        TuiCommand::Approve => {
            overlay::approve(state, command_tx).await;
            true
        }
        TuiCommand::Deny => {
            overlay::deny(state, command_tx).await;
            true
        }
        TuiCommand::ApproveAll => {
            overlay::approve_all(state, command_tx).await;
            true
        }
        TuiCommand::ClassifierAutoApprove {
            request_id,
            matched_rule: _,
        } => {
            overlay::classifier_auto_approve(state, command_tx, request_id).await;
            true
        }

        // ── Overlay navigation ──
        TuiCommand::OverlayFilter(c) => {
            overlay::filter(state, c);
            true
        }
        TuiCommand::OverlayFilterBackspace => {
            overlay::filter_backspace(state);
            true
        }
        TuiCommand::OverlayNext => {
            overlay::nav(state, 1);
            overlay::request_diff_stats_if_rewind(state, command_tx).await;
            true
        }
        TuiCommand::OverlayPrev => {
            overlay::nav(state, -1);
            overlay::request_diff_stats_if_rewind(state, command_tx).await;
            true
        }
        TuiCommand::OverlayConfirm => {
            overlay::confirm(state, command_tx).await;
            true
        }

        // ── Commands & overlays ──
        TuiCommand::ShowHelp => {
            state.ui.set_overlay(crate::state::Overlay::Help);
            true
        }
        TuiCommand::ShowCommandPalette => {
            show::command_palette(state);
            true
        }
        TuiCommand::ShowSessionBrowser => {
            show::session_browser(state);
            true
        }
        TuiCommand::ShowGlobalSearch => {
            show::global_search(state);
            true
        }
        TuiCommand::ShowQuickOpen => {
            show::quick_open(state);
            true
        }
        TuiCommand::ShowExport => {
            show::export(state);
            true
        }
        TuiCommand::ShowContextViz => {
            state
                .ui
                .set_overlay(crate::state::Overlay::ContextVisualization);
            true
        }
        TuiCommand::ShowRewind => {
            show::rewind(state, command_tx).await;
            true
        }
        TuiCommand::ShowDoctor => {
            show::doctor(state);
            true
        }
        TuiCommand::ShowSettings => {
            show::settings(state);
            true
        }
        TuiCommand::SettingsNextTab => {
            if let Some(crate::state::Overlay::Settings(ref mut s)) = state.ui.overlay {
                s.next_tab();
            }
            true
        }
        TuiCommand::SettingsPrevTab => {
            if let Some(crate::state::Overlay::Settings(ref mut s)) = state.ui.overlay {
                s.prev_tab();
            }
            true
        }
        TuiCommand::ExecuteSkill(name) => {
            let _ = command_tx
                .send(UserCommand::ExecuteSkill { name, args: None })
                .await;
            true
        }

        // ── Task management ──
        TuiCommand::BackgroundAllTasks => {
            let _ = command_tx.send(UserCommand::BackgroundAllTasks).await;
            true
        }
        TuiCommand::KillAllAgents => {
            let _ = command_tx.send(UserCommand::KillAllAgents).await;
            true
        }

        // ── Display toggles ──
        TuiCommand::ToggleToolCollapse => {
            if state.ui.collapsed_tools.is_empty() {
                for tool in &state.session.tool_executions {
                    state.ui.collapsed_tools.insert(tool.call_id.clone());
                }
            } else {
                state.ui.collapsed_tools.clear();
            }
            true
        }
        TuiCommand::ToggleSystemReminders => {
            state.ui.show_system_reminders = !state.ui.show_system_reminders;
            true
        }

        // ── External editor / clipboard ──
        TuiCommand::OpenExternalEditor | TuiCommand::OpenPlanEditor => true,
        TuiCommand::PasteFromClipboard => {
            clipboard::paste_from_clipboard(state).await;
            true
        }
        TuiCommand::CopyLastMessage => {
            clipboard::copy_last_message(state);
            true
        }

        // ── Application ──
        TuiCommand::Quit => {
            let _ = command_tx.send(UserCommand::Shutdown).await;
            state.quit();
            true
        }
    };

    if state.ui.input.text != text_before || state.ui.input.cursor != cursor_before {
        crate::autocomplete::refresh_suggestions(state);
    }

    changed
}
