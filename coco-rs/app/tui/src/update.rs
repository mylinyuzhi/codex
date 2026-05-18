//! TUI update handler — the Update in TEA.
//!
//! Applies [`TuiCommand`]s to [`AppState`]. Side effects (sending to core)
//! are dispatched via the command channel. Complex per-category handlers
//! live in the private submodules (`overlay`, `show`, `edit`) to keep this
//! dispatcher focused on routing.

use tokio::sync::mpsc;

use crate::command::ShutdownReason;
use crate::command::UserCommand;
use crate::constants;
use crate::events::TuiCommand;
use crate::i18n::t;
use crate::state::AppState;
use crate::state::FocusTarget;

use exit::ExitEffect;

mod clipboard;
mod edit;
mod exit;
mod expanded_view;
mod overlay;
// `pub(crate)` so the slash-command dispatcher (in
// `server_notification_handler::tui_only`) can call into `cycle_model`
// when `TuiOnlyEvent::OpenModelPicker` arrives. The other `show::*`
// constructors remain crate-internal helpers.
pub(crate) mod show;
mod stash;
mod transcript;

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
    let text_before = state.ui.input.text().to_string();
    let cursor_before = state.ui.input.textarea.cursor();

    let changed = match cmd {
        // ── Mode toggles ──
        TuiCommand::TogglePlanMode => {
            state.toggle_plan_mode();
            let mode = state.session.permission_mode;
            let _ = command_tx
                .send(UserCommand::SetPermissionMode { mode })
                .await;
            // Transient toast so the toggle is acknowledged even when
            // the user's eyes are on the input bar rather than the
            // mode banner. Plan-on uses plan_mode color (info-equivalent
            // in the Toast palette); plan-off uses info as well so the
            // off-state doesn't read as a failure.
            let key = if mode == coco_types::PermissionMode::Plan {
                "toast.plan_mode_on"
            } else {
                "toast.plan_mode_off"
            };
            state
                .ui
                .add_toast(crate::state::ui::Toast::info(t!(key).to_string()));
            true
        }
        TuiCommand::CyclePermissionMode => {
            // Compute the next mode without committing — the cycle helper
            // applies eagerly, so we'd lose the chance to intercept
            // high-stakes targets (BypassPermissions / Auto) and force
            // a confirmation dialog. Mirrors TS: Shift+Tab landing on
            // bypass surfaces `BypassPermissionsModeDialog` before the
            // mode actually flips.
            let next = state.session.permission_mode.next_in_cycle(
                state.session.bypass_permissions_available,
                state.session.auto_mode_available,
            );
            match next {
                coco_types::PermissionMode::BypassPermissions => {
                    let current_label = format!("{:?}", state.session.permission_mode);
                    state
                        .ui
                        .set_overlay(crate::state::Overlay::BypassPermissions(
                            crate::state::BypassPermissionsOverlay {
                                current_mode: current_label,
                            },
                        ));
                }
                coco_types::PermissionMode::Auto => {
                    state.ui.set_overlay(crate::state::Overlay::AutoModeOptIn(
                        crate::state::AutoModeOptInOverlay {
                            description: t!("dialog.auto_mode_description").to_string(),
                        },
                    ));
                }
                _ => {
                    state.session.permission_mode = next;
                    let _ = command_tx
                        .send(UserCommand::SetPermissionMode { mode: next })
                        .await;
                    state.ui.add_toast(crate::state::ui::Toast::info(
                        t!(
                            "toast.permission_mode_set",
                            mode = permission_mode_label(next).as_str()
                        )
                        .to_string(),
                    ));
                }
            }
            true
        }
        TuiCommand::CycleThinkingLevel => {
            // Find the catalog entry for the current Main role's
            // (provider, model_id) pair and cycle through ITS declared
            // `supported_efforts`. This honors per-model declarations
            // (DeepSeek's 4-state ladder is different from Anthropic's
            // 4-budget ladder is different from OpenAI's 5-level
            // ladder) — Ctrl+T is no longer a hardcoded ordering.
            let supported: Vec<coco_types::ReasoningEffort> = state
                .session
                .model_catalog
                .iter()
                .find(|e| e.provider == state.session.provider && e.model_id == state.session.model)
                .map(|e| e.supported_efforts.clone())
                .unwrap_or_default();

            // No declared support → silent no-op. Common when the
            // active model is registered without a
            // `supported_thinking_levels` entry (e.g. a user-added
            // model in `~/.coco/models.json` without metadata) or when
            // `state.session.{provider, model}` haven't been seeded
            // yet (pre-bootstrap mock paths).
            if supported.is_empty() {
                return true;
            }

            // Current effort not in supported list → restart at index 0.
            // Self-correcting: a stale `thinking_effort` (e.g. from a
            // prior model that supported XHigh) snaps back to a legal
            // value on the next Ctrl+T press instead of going nowhere.
            let current_idx = supported
                .iter()
                .position(|e| *e == state.session.thinking_effort)
                .unwrap_or(0);
            let next = supported[(current_idx + 1) % supported.len()];

            state.session.thinking_effort = next;
            let _ = command_tx
                .send(UserCommand::SetThinkingLevel {
                    level: next.to_string(),
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
        TuiCommand::SubmitInput => {
            // TS `useTextInput.ts:250-255`: a trailing backslash + Enter
            // inserts a newline instead of submitting (poor-man's
            // line-continuation). Match here so the heredoc-style escape
            // works in both ordinary and vim-Insert mode.
            if state.ui.input.textarea.text().ends_with('\\') {
                let len = state.ui.input.textarea.text().len();
                state.ui.input.textarea.replace_range(len - 1..len, "\n");
                return true;
            }
            edit::submit(state, command_tx).await
        }
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
            // Resolve paste pills the same way `submit` does so mid-turn
            // pastes (text expansion + image attachments) survive queueing.
            // TS parity: `handlePromptSubmit.ts:336-343` enqueues with
            // `pastedContents` so images flow into the queued attachment.
            let resolved = state.ui.paste_manager.resolve_structured(&text);
            // The TUI display is now a projection of the engine queue
            // state — the round-trip `CommandQueued` notification
            // (server_notification_handler/protocol.rs) repopulates
            // `state.session.queued_commands` from the engine's
            // authoritative count. Keep no optimistic local push: a
            // double-push (here + on the event) would double the
            // displayed count.
            let _ = command_tx
                .send(UserCommand::QueueCommand {
                    prompt: resolved.text,
                    images: resolved.images,
                })
                .await;
            state.ui.paste_manager.clear();
            true
        }
        TuiCommand::Interrupt => {
            let now = std::time::Instant::now();
            let timing =
                ExitTiming::from_pending_until(state.ui.ctrl_c_tracker.pending_until(), now);
            let effect = exit::on_interrupt(state, now);
            apply_exit_effect(state, command_tx, ExitSource::CtrlC, timing, effect).await;
            true
        }
        TuiCommand::RequestExit => {
            let now = std::time::Instant::now();
            let timing =
                ExitTiming::from_pending_until(state.ui.ctrl_d_tracker.pending_until(), now);
            let effect = exit::on_request_exit(state, now);
            apply_exit_effect(state, command_tx, ExitSource::CtrlD, timing, effect).await;
            true
        }
        TuiCommand::Cancel => {
            // Vim insert-mode Esc → transition to Normal mode and walk
            // the cursor back one grapheme (vim convention). Mirrors
            // codex-rs textarea.rs:654-660. This wins over every other
            // Cancel branch because Esc in Insert is a mode transition,
            // not a UI dismissal. Gated on `vim.enabled` so non-vim
            // users keep the standard Esc → Cancel behavior.
            if state.ui.input.vim.insert_escape_active()
                && crate::vim::wiring::handle_insert_escape(
                    &mut state.ui.input.textarea,
                    &mut state.ui.input.vim,
                )
            {
                return true;
            }
            // Esc dismisses autocomplete first (so the user can escape out
            // of a trigger without losing their typed input) before
            // touching any overlay.
            if !state.ui.has_overlay() && state.ui.active_suggestions.is_some() {
                state.ui.active_suggestions = None;
                return true;
            }
            // Escape while viewing the teammate activity pane mirrors TS
            // `useBackgroundTaskNavigation`: interrupt the focused
            // teammate's current turn only. Ctrl+C / KillAllAgents remains
            // the lifecycle stop path.
            if !state.ui.has_overlay()
                && matches!(
                    state.session.expanded_view,
                    coco_types::ExpandedView::Teammates
                )
                && let Some(index) = state.session.focused_subagent_index
                && let Some(agent) = state.session.subagents.get(index as usize)
                && matches!(agent.status, crate::state::session::SubagentStatus::Running)
            {
                let _ = command_tx
                    .send(UserCommand::InterruptAgentCurrentWork {
                        agent_id: agent.agent_id.clone(),
                    })
                    .await;
                return true;
            }
            // No overlay + active suggestions + text present → ESC
            // double-press clears input + saves to history. Mirrors TS
            // `useTextInput.ts:126-153`: single Esc shows a toast; second
            // Esc within `DOUBLE_PRESS_TIMEOUT` clears.
            if !state.ui.has_overlay()
                && state.ui.active_suggestions.is_none()
                && !state.ui.input.is_empty()
            {
                use crate::double_press::Outcome;
                if state.ui.esc_tracker.poll((), std::time::Instant::now()) == Outcome::Double {
                    let taken = state.ui.input.take_input();
                    state.ui.input.add_to_history(taken);
                    state.ui.input.history_index = None;
                } else {
                    state.ui.add_toast(crate::state::ui::Toast::info(
                        crate::i18n::t!("toast.esc_again_to_clear").to_string(),
                    ));
                }
                return true;
            }
            // No overlay + no suggestions + idle conditions met → run
            // the double-Esc tracker so a second Esc opens the rewind
            // picker. TS: `useDoublePress` in `PromptInput.tsx`. The
            // poll lives here (not in `keybinding_dispatch`) because
            // dispatch only has `&AppState`; the tracker needs a
            // mutable borrow.
            if state.rewind_available_from_input() {
                use crate::double_press::Outcome;
                if state.ui.esc_tracker.poll((), std::time::Instant::now()) == Outcome::Double {
                    show::rewind(state, command_tx).await;
                    return true;
                }
            }
            if !overlay::rewind_cancel(state) {
                return true; // phase-back; keep overlay
            }
            // /memory cancel surfaces a toast (TS:
            // `commands/memory/memory.tsx::onCancel` → "Cancelled memory editing").
            if matches!(
                state.ui.active_overlay(),
                Some(crate::state::Overlay::MemoryDialog(_))
            ) {
                let text = crate::i18n::t!("toast.memory_cancelled").to_string();
                state
                    .session
                    .add_message(crate::state::session::ChatMessage::system_text(
                        uuid::Uuid::new_v4().to_string(),
                        text.clone(),
                    ));
                state.ui.add_toast(crate::state::ui::Toast::info(text));
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
            // Route into the rewind summarize-feedback box when that
            // overlay phase is active so typing builds the feedback
            // string instead of leaking to the input bar.
            if let Some(crate::state::Overlay::Rewind(r)) = state.ui.active_overlay_mut()
                && r.phase == crate::state::rewind::RewindPhase::SummarizeFeedback
            {
                r.summarize_feedback.push(c);
            } else if state.ui.input.vim.normal_dispatch_active() {
                // Vim Normal mode: route the printable key through the
                // vim state machine (h/j/k/l/i/a/o/w/b/d/y/p/x/...).
                // Mirrors codex-rs textarea.rs:518-530 pattern. Gated on
                // `vim.enabled` so non-vim users insert characters as
                // typed instead of triggering vim motions.
                let action = crate::vim::wiring::dispatch_vim_key(
                    c,
                    &mut state.ui.input.textarea,
                    &mut state.ui.input.vim,
                );
                let should_submit = crate::vim::wiring::apply_action(
                    action,
                    &mut state.ui.input.textarea,
                    &mut state.ui.input.vim,
                );
                if should_submit {
                    // Vim `Enter` in Normal mode submits — delegate to
                    // the same path Enter takes in non-vim mode.
                    edit::submit(state, command_tx).await;
                }
            } else {
                let mut buf = [0u8; 4];
                state.ui.input.textarea.insert_str(c.encode_utf8(&mut buf));
            }
            true
        }
        TuiCommand::InsertNewline => {
            state.ui.input.textarea.insert_str("\n");
            true
        }
        TuiCommand::DeleteBackward => {
            if let Some(crate::state::Overlay::Rewind(r)) = state.ui.active_overlay_mut()
                && r.phase == crate::state::rewind::RewindPhase::SummarizeFeedback
            {
                r.summarize_feedback.pop();
            } else {
                state.ui.input.textarea.delete_backward(1);
            }
            true
        }
        TuiCommand::DeleteForward => {
            state.ui.input.textarea.delete_forward(1);
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
        TuiCommand::KillToBeginningOfLine => {
            edit::kill_to_beginning_of_line(state);
            true
        }
        TuiCommand::Yank => {
            edit::yank(state);
            true
        }

        // ── Cursor movement ──
        TuiCommand::CursorLeft => {
            state.ui.input.textarea.move_cursor_left();
            true
        }
        TuiCommand::CursorRight => {
            state.ui.input.textarea.move_cursor_right();
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
            state
                .ui
                .input
                .textarea
                .move_cursor_to_beginning_of_line(crate::widgets::BolBehavior::StayPut);
            true
        }
        TuiCommand::CursorEnd => {
            state
                .ui
                .input
                .textarea
                .move_cursor_to_end_of_line(crate::widgets::EolBehavior::StayPut);
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
        TuiCommand::OverlayJumpStart => {
            overlay::nav(state, i32::MIN / 2);
            overlay::request_diff_stats_if_rewind(state, command_tx).await;
            true
        }
        TuiCommand::OverlayJumpEnd => {
            overlay::nav(state, i32::MAX / 2);
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
        TuiCommand::ShowRewindFor { message_id } => {
            show::rewind_for(state, command_tx, message_id).await;
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
        TuiCommand::ToggleSyntaxHighlighting => {
            overlay::toggle_syntax_highlighting(state);
            true
        }
        TuiCommand::SettingsNextTab => {
            // Tab cycles between contexts depending on the active overlay.
            // Settings overlay → next tab. Question overlay → cycle focus
            // (questions → footer items). ModelPicker → cycle the
            // role pill. Other overlays ignore Tab.
            if let Some(crate::state::Overlay::Settings(s)) = state.ui.active_overlay_mut() {
                s.next_tab();
            } else if matches!(
                state.ui.active_overlay(),
                Some(crate::state::Overlay::Question(_))
            ) {
                overlay::question_cycle_focus(state, 1);
            } else if matches!(
                state.ui.active_overlay(),
                Some(crate::state::Overlay::ModelPicker(_))
            ) {
                show::cycle_model_role(state, 1);
            }
            true
        }
        TuiCommand::SettingsPrevTab => {
            if let Some(crate::state::Overlay::Settings(s)) = state.ui.active_overlay_mut() {
                s.prev_tab();
            } else if matches!(
                state.ui.active_overlay(),
                Some(crate::state::Overlay::Question(_))
            ) {
                overlay::question_cycle_focus(state, -1);
            } else if matches!(
                state.ui.active_overlay(),
                Some(crate::state::Overlay::ModelPicker(_))
            ) {
                show::cycle_model_role(state, -1);
            }
            true
        }
        TuiCommand::ModelPickerCycleEffort(delta) => {
            overlay::cycle_model_effort(state, delta);
            true
        }
        TuiCommand::ModelPickerCycleRole(delta) => {
            show::cycle_model_role(state, delta);
            true
        }
        TuiCommand::ExecuteSkill(name) => {
            let _ = command_tx
                .send(UserCommand::ExecuteSkill { name, args: None })
                .await;
            true
        }
        TuiCommand::ExecuteSlashCommand(name) => {
            // Synthesize a SubmitInput as if the user typed `/foo<Enter>`.
            // The agent driver's existing slash-command parser handles
            // it the same way. `display_text: None` keeps the chat
            // history clean — the rendered slash command shows up via
            // the agent driver's own ChatMessage emission.
            let user_message_id = uuid::Uuid::new_v4().to_string();
            let content = format!("/{name}");
            if edit::try_local_command(state, &content) {
                return true;
            }
            let _ = command_tx
                .send(UserCommand::SubmitInput {
                    user_message_id,
                    content: content.clone(),
                    display_text: Some(content),
                    images: Vec::new(),
                })
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
        TuiCommand::OpenExternalEditor => {
            let _ = command_tx
                .send(UserCommand::OpenPromptEditor {
                    initial_content: state.ui.input.text().to_string(),
                })
                .await;
            true
        }
        TuiCommand::OpenPlanEditor => {
            let _ = command_tx.send(UserCommand::OpenPlanEditor).await;
            true
        }
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
            tracing::info!(
                exit_case = %ShutdownReason::ImmediateQuit,
                "immediate quit requested"
            );
            let _ = command_tx
                .send(UserCommand::Shutdown {
                    reason: ShutdownReason::ImmediateQuit,
                })
                .await;
            state.quit();
            true
        }

        // ── Stash ──
        TuiCommand::StashInputDraft => {
            stash::swap_input_draft(state);
            true
        }

        // ── Expanded right-rail view ──
        TuiCommand::ToggleExpandedTasksView => {
            expanded_view::cycle(state);
            true
        }
        TuiCommand::ToggleTeammateMessagePreview => {
            state.ui.show_teammate_message_preview = !state.ui.show_teammate_message_preview;
            true
        }
        TuiCommand::ToggleTranscript => {
            transcript::toggle(state);
            true
        }
        TuiCommand::TranscriptSelectNext => {
            transcript::select_expandable(state, 1);
            true
        }
        TuiCommand::TranscriptToggleCell => {
            transcript::toggle_selected_cell(state);
            true
        }
        TuiCommand::TranscriptScrollLines(delta) => {
            transcript::scroll_lines(state, delta);
            true
        }
        TuiCommand::TranscriptPage(delta) => {
            transcript::page(state, delta);
            true
        }
        TuiCommand::TranscriptJumpStart => {
            transcript::jump_start(state);
            true
        }
        TuiCommand::TranscriptJumpEnd => {
            transcript::jump_end(state);
            true
        }
    };

    if state.ui.input.text() != text_before || state.ui.input.textarea.cursor() != cursor_before {
        crate::autocomplete::refresh_suggestions(state);
    }

    changed
}

#[derive(Debug, Clone, Copy)]
enum ExitSource {
    CtrlC,
    CtrlD,
}

impl ExitSource {
    fn label(self) -> &'static str {
        match self {
            Self::CtrlC => "Ctrl-C",
            Self::CtrlD => "Ctrl-D",
        }
    }

    fn shutdown_reason(self) -> ShutdownReason {
        match self {
            Self::CtrlC => ShutdownReason::DoublePressCtrlC,
            Self::CtrlD => ShutdownReason::DoublePressCtrlD,
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct ExitTiming {
    expired_by_ms: Option<u128>,
}

impl ExitTiming {
    fn from_pending_until(
        pending_until: Option<std::time::Instant>,
        now: std::time::Instant,
    ) -> Self {
        Self {
            expired_by_ms: pending_until
                .and_then(|until| now.checked_duration_since(until))
                .map(|d| d.as_millis()),
        }
    }
}

/// Translate an [`ExitEffect`] (pure decision from `update::exit`) into
/// the matching side effects: `UserCommand` sends + terminal
/// `state.quit()`. **Does not** decide auto-restore — that's the
/// `TurnInterrupted` event handler's job in
/// `server_notification_handler::protocol::on_turn_interrupted`,
/// mirroring TS where `restoreMessageSync` runs inside `.finally`
/// after the abort completes (`REPL.tsx:3010-3022`).
async fn apply_exit_effect(
    state: &mut AppState,
    command_tx: &mpsc::Sender<UserCommand>,
    source: ExitSource,
    timing: ExitTiming,
    effect: ExitEffect,
) {
    match effect {
        ExitEffect::InterruptOnly => {
            tracing::info!(
                key = source.label(),
                exit_case = "interrupt_active_turn",
                "exit key interrupted active turn"
            );
            state.session.was_interrupted = true;
            let _ = command_tx.send(UserCommand::Interrupt).await;
        }
        ExitEffect::ClearInput => {
            // Idle Ctrl+C with text in the input: clear + save to history.
            // The exit hint is already armed by `on_interrupt`, so the
            // *next* Ctrl+C within the window goes through the Quit path.
            let taken = state.ui.input.take_input();
            state.ui.input.add_to_history(taken);
            state.ui.input.history_index = None;
            let prompt = state
                .ui
                .pending_exit_hint()
                .map(|key| t!("status.exit_prompt", key = key.label()).to_string())
                .unwrap_or_else(|| t!("status.exit_prompt", key = source.label()).to_string());
            tracing::info!(
                key = source.label(),
                exit_case = "clear_input",
                prompt,
                "exit key cleared draft input"
            );
        }
        ExitEffect::ArmOnly => {
            // First idle Ctrl+C / Ctrl+D: no interrupt, no overlay.
            // Tracker already updated by `exit::*`; renderer reads
            // `state.ui.pending_exit_hint()` to show the footer hint.
            let prompt = state
                .ui
                .pending_exit_hint()
                .map(|key| t!("status.exit_prompt", key = key.label()).to_string())
                .unwrap_or_else(|| t!("status.exit_prompt", key = source.label()).to_string());
            tracing::info!(
                key = source.label(),
                exit_case = "arm_exit_prompt",
                rearmed_after_timeout = timing.expired_by_ms.is_some(),
                expired_by_ms = timing.expired_by_ms.unwrap_or(0),
                window_ms = crate::constants::DOUBLE_PRESS_TIMEOUT.as_millis(),
                prompt,
                "exit prompt armed"
            );
        }
        ExitEffect::Quit => {
            let reason = source.shutdown_reason();
            tracing::info!(
                key = source.label(),
                exit_case = %reason,
                "exit confirmed; shutting down"
            );
            let _ = command_tx.send(UserCommand::Shutdown { reason }).await;
            state.quit();
        }
    }
}

/// Localised label for a permission mode, used in toasts/banners so the
/// user sees the same wording the help overlay and status row use.
/// `pub(crate)` so the overlay deny handler can reuse it without
/// duplicating the match — keeps mode wording consistent across the
/// TogglePlanMode / Cycle / overlay-decline surfaces.
pub(crate) fn permission_mode_label(mode: coco_types::PermissionMode) -> String {
    let key = match mode {
        coco_types::PermissionMode::Default => "permission_mode.default",
        coco_types::PermissionMode::Plan => "permission_mode.plan",
        coco_types::PermissionMode::AcceptEdits => "permission_mode.accept_edits",
        coco_types::PermissionMode::BypassPermissions => "permission_mode.bypass",
        coco_types::PermissionMode::Auto => "permission_mode.auto",
        coco_types::PermissionMode::DontAsk => "permission_mode.dont_ask",
        coco_types::PermissionMode::Bubble => "permission_mode.bubble",
    };
    t!(key).to_string()
}
