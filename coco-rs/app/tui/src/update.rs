//! TUI update handler — the Update in TEA.
//!
//! Applies [`TuiCommand`]s to [`AppState`]. Side effects (sending to core)
//! are dispatched via the command channel. Complex per-category handlers
//! live in the private submodules (`state`, `show`, `edit`) to keep this
//! dispatcher focused on routing.

use coco_types::TurnAbortReason;
use tokio::sync::mpsc;

use crate::command::ShutdownReason;
use crate::command::UserCommand;
use crate::events::TuiCommand;
use crate::i18n::t;
use crate::state::AppState;
use crate::state::FocusTarget;
use crate::state::ModalState;
use crate::state::PanePromptState;
use coco_tui_ui::constants;

use exit::ExitEffect;

mod agents_dialog;
mod clipboard;
pub(crate) mod copy;
mod edit;
mod exit;
mod expanded_view;
mod interaction;
mod plugin_dialog;
// `pub(crate)` so the slash-command dispatcher (in
// `server_notification_handler::tui_only`) can call into `cycle_model`
// when `TuiOnlyEvent::OpenModelPicker` arrives. The other `show::*`
// constructors remain crate-internal helpers.
pub(crate) mod show;
mod skills_dialog;
mod stash;
mod transcript;

#[cfg(test)]
#[path = "update.test.rs"]
mod tests;

/// How a picker/dialog reports being closed with Esc. Mirrors TS local-jsx
/// `onDone('… dismissed', { display: 'system' })` — every picker leaves a
/// transcript trace of what closed.
enum PickerDismiss {
    /// Picker opened by a slash command → render `❯ /<name>` + `⎿ <message>`,
    /// matching the command's own confirm feedback (e.g. theme "Theme set to").
    Slash {
        name: &'static str,
        message: &'static str,
    },
    /// Keybinding-only overlay (no slash command) → a standalone system line,
    /// with no misleading `/cmd` echo.
    System { message: &'static str },
}

/// Dismiss feedback for a modal closed via Esc. `None` for prompt-style and
/// viewer modals that own their own decline UX. Wording mirrors TS where a
/// counterpart exists (`Theme picker dismissed`, `Skills dialog dismissed`,
/// `Cancelled memory editing`, …).
fn picker_dismiss(modal: &ModalState) -> Option<PickerDismiss> {
    use ModalState as M;
    use PickerDismiss::Slash;
    use PickerDismiss::System;
    Some(match modal {
        M::Help => Slash {
            name: "help",
            message: "Help dialog dismissed",
        },
        M::ModelPicker(_) => Slash {
            name: "model",
            message: "Model picker dismissed",
        },
        M::ThemePicker(_) => Slash {
            name: "theme",
            message: "Theme picker dismissed",
        },
        M::SkillsDialog(_) => Slash {
            name: "skills",
            message: "Skills dialog dismissed",
        },
        M::PluginDialog(_) => Slash {
            name: "plugin",
            message: "Plugin dialog dismissed",
        },
        M::AgentsDialog(_) => Slash {
            name: "agents",
            message: "Agents dialog dismissed",
        },
        M::McpServerSelect(_) => Slash {
            name: "mcp",
            message: "MCP dialog dismissed",
        },
        M::MemoryDialog(_) => Slash {
            name: "memory",
            message: "Cancelled memory editing",
        },
        M::Settings(_) => Slash {
            name: "status",
            message: "Status dialog dismissed",
        },
        M::DiffView(_) => Slash {
            name: "diff",
            message: "Diff dialog dismissed",
        },
        M::Export(_) => Slash {
            name: "export",
            message: "Export dialog dismissed",
        },
        M::SessionBrowser(_) => Slash {
            name: "resume",
            message: "Session browser dismissed",
        },
        M::CopyPicker(_) => Slash {
            name: "copy",
            message: "Copy cancelled",
        },
        M::QuickOpen(_) => System {
            message: "Quick open dismissed",
        },
        M::GlobalSearch(_) => System {
            message: "Search dismissed",
        },
        // Prompt / viewer / system modals own their own decline UX; Rewind
        // runs a dedicated multi-phase cancel (`interaction::rewind_cancel`).
        M::Error(_)
        | M::Transcript(_)
        | M::Rewind(_)
        | M::Doctor(_)
        | M::WorktreeExit(_)
        | M::Bridge(_)
        | M::InvalidConfig(_)
        | M::IdleReturn(_)
        | M::Trust(_)
        | M::AutoModeOptIn(_)
        | M::BypassPermissions(_)
        | M::TaskDetail(_)
        | M::TeamRoster(_)
        | M::PluginHint(_)
        | M::Feedback(_) => return None,
    })
}

/// Close the active modal and surface its dismiss feedback. Shared by both Esc
/// routes: `TuiCommand::Cancel` and `TuiCommand::Deny` — the theme picker and
/// settings reuse the Settings keybinding context, whose Esc resolves to `Deny`
/// (`interaction::deny`), so the close logic must live in one place reachable
/// from both. Restores the theme picker's live preview before closing.
pub(crate) async fn close_modal_with_feedback(
    state: &mut AppState,
    command_tx: &mpsc::Sender<UserCommand>,
) {
    // Theme picker: Esc cancels the live preview by restoring the theme that was
    // active when the picker opened. Read `original_setting` before the take.
    if let Some(ModalState::ThemePicker(p)) = state.ui.modal.as_ref() {
        let original = p.original_setting.clone();
        if let Err(err) = state.ui.apply_theme_setting(original) {
            tracing::warn!(
                error = %err,
                "theme picker: failed to restore original theme on cancel"
            );
        }
    }
    // Plugin-hint Esc dismissal is treated as "no" (TS auto-dismiss →
    // onResponse('no')): record show-once so the prompt never reappears.
    if let Some(ModalState::PluginHint(ph)) = state.ui.modal.as_ref() {
        coco_plugins::mark_hint_plugin_shown(&ph.plugin_id);
    }
    // Capture the dismiss feedback before the modal is taken, emit after close.
    let dismiss = state.ui.modal.as_ref().and_then(picker_dismiss);
    state.ui.dismiss_modal();
    match dismiss {
        Some(PickerDismiss::Slash { name, message }) => {
            let messages = coco_messages::build_slash_command_messages(
                name, /*args*/ "", message, /*is_sensitive*/ false,
            );
            let _ = command_tx
                .send(UserCommand::PushSlashResult { messages })
                .await;
        }
        Some(PickerDismiss::System { message }) => {
            let _ = command_tx
                .send(UserCommand::PushSystemMessage {
                    kind: crate::command::SystemPushKind::Informational {
                        level: coco_messages::SystemMessageLevel::Info,
                        title: String::new(),
                        message: message.to_string(),
                    },
                })
                .await;
        }
        None => {}
    }
}

/// Apply a TUI command to the state.
///
/// Returns `true` if the state changed and a redraw is needed.
pub async fn handle_command(
    state: &mut AppState,
    cmd: TuiCommand,
    command_tx: &mpsc::Sender<UserCommand>,
) -> bool {
    // Breadcrumb every dispatch so user-bug repros include which TuiCommand
    // ran. InsertChar / SurfaceFilter fire per-keystroke and would flood
    // debug at typing rate — drop them to trace.
    match &cmd {
        TuiCommand::InsertChar(_) | TuiCommand::SurfaceFilter(_) => {
            tracing::trace!(target: "coco_tui::command", cmd = ?cmd, "TuiCommand dispatch");
        }
        _ => {
            tracing::debug!(
                target: "coco_tui::command",
                cmd = ?cmd,
                is_streaming = state.is_streaming(),
                has_modal = state.ui.modal.is_some(),
                has_prompt = state.ui.interaction.active_prompt.is_some(),
                "TuiCommand dispatch",
            );
        }
    }
    // Snapshot input before the command so we can reactively refresh the
    // autocomplete popup whenever input text or cursor moves, without
    // threading a refresh call through every editing arm.
    let text_before = state.ui.input.text().to_string();
    let cursor_before = state.ui.input.textarea.cursor();

    // Intercept editable-dialog keys before the main dispatch.
    // The skills dialog has a richer state machine (select / filter
    // modes) than the generic modal cancel/submit path; deferring to
    // it here keeps the per-arm InsertChar/Backspace/etc. branches
    // free of dialog-specific logic.
    if let skills_dialog::Handled::Yes(changed) =
        skills_dialog::intercept(state, &cmd, command_tx).await
    {
        return changed;
    }

    // The `/agents` 2-tab dialog has its own ←/→ tab cycle + `x`
    // cancel-task path that the generic modal dispatch doesn't model.
    // Same fall-through contract as the skills dialog.
    if let agents_dialog::Handled::Yes(changed) =
        agents_dialog::intercept(state, &cmd, command_tx).await
    {
        return changed;
    }

    if let plugin_dialog::Handled::Yes(changed) =
        plugin_dialog::intercept(state, &cmd, command_tx).await
    {
        return changed;
    }

    let changed = match cmd {
        TuiCommand::Noop => false,
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
                    state.ui.show_modal(ModalState::BypassPermissions(
                        crate::state::BypassPermissionsState {
                            current_mode: current_label,
                        },
                    ));
                }
                coco_types::PermissionMode::Auto => {
                    state.ui.show_modal(ModalState::AutoModeOptIn(
                        crate::state::AutoModeOptInState {
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
            if state.is_streaming() {
                let handled = queue_current_input(state, command_tx).await;
                if state.session.has_submit_interruptible_tool_in_progress {
                    let _ = command_tx
                        .send(UserCommand::Interrupt(TurnAbortReason::SubmitInterrupt))
                        .await;
                }
                return handled;
            }
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
        TuiCommand::SubmitInterrupt => {
            let handled = queue_current_input(state, command_tx).await;
            let _ = command_tx
                .send(UserCommand::Interrupt(TurnAbortReason::SubmitInterrupt))
                .await;
            handled
        }
        TuiCommand::QueueInput => queue_current_input(state, command_tx).await,
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
            // touching any state.
            if !state.ui.has_blocking_interaction() && state.ui.completion.active.is_some() {
                state.ui.completion.dismiss_active();
                state.ui.sync_popup_from_active_suggestions();
                return true;
            }
            // Escape while viewing the teammate activity pane mirrors TS
            // `useBackgroundTaskNavigation`: interrupt the focused
            // teammate's current turn only. Ctrl+C / KillAllAgents remains
            // the lifecycle stop path.
            if !state.ui.has_blocking_interaction()
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
            // No state + active suggestions + text present → ESC
            // double-press clears input + saves to history. Mirrors TS
            // `useTextInput.ts:126-153`: single Esc shows a toast; second
            // Esc within `DOUBLE_PRESS_TIMEOUT` clears.
            if !state.ui.has_blocking_interaction()
                && state.ui.completion.active.is_none()
                && !state.ui.input.is_empty()
            {
                use coco_tui_ui::double_press::Outcome;
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
            // No state + no suggestions + idle conditions met → run
            // the double-Esc tracker so a second Esc opens the rewind
            // picker. TS: `useDoublePress` in `PromptInput.tsx`. The
            // poll lives here (not in `keybinding_dispatch`) because
            // dispatch only has `&AppState`; the tracker needs a
            // mutable borrow.
            if state.rewind_available_from_input() {
                use coco_tui_ui::double_press::Outcome;
                if state.ui.esc_tracker.poll((), std::time::Instant::now()) == Outcome::Double {
                    show::rewind(state, command_tx).await;
                    return true;
                }
            }
            if !interaction::rewind_cancel(state) {
                return true; // phase-back; keep state
            }
            // Every picker reports its own dismissal to the transcript (TS
            // local-jsx `onDone('… dismissed', { display: 'system' })`). The
            // theme picker / settings route Esc through `Deny` instead, so the
            // shared helper also runs there (`interaction::deny`).
            if state.ui.modal.is_some() {
                close_modal_with_feedback(state, command_tx).await;
            } else if state.has_active_surface() {
                state.ui.dismiss_prompt();
            }
            true
        }
        TuiCommand::ClearScreen => {
            // Phase 3d (§5): clear the engine-derived transcript so the
            // visible chat empties. The engine retains the full
            // conversation; future `MessageAppended` events repopulate
            // the cell view from the next turn forward.
            state.session.transcript.on_session_reset();
            // Dropping messages also invalidates the copy cache — without this
            // /copy after /clear would surface the response the user just
            // wiped, which is surprising. Matches codex-rs's clear-on-reset.
            state.session.last_agent_markdown = None;
            state.ui.scroll_offset = 0;
            true
        }

        // ── Text editing ──
        TuiCommand::InsertChar(c) => {
            state.ui.input.clear_inline_hint();
            // Route into the rewind summarize-feedback box when that
            // state phase is active so typing builds the feedback
            // string instead of leaking to the input bar.
            if let Some(ModalState::Rewind(r)) = state.ui.modal.as_mut()
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
            state.ui.input.clear_inline_hint();
            state.ui.input.textarea.insert_str("\n");
            true
        }
        TuiCommand::DeleteBackward => {
            state.ui.input.clear_inline_hint();
            if let Some(ModalState::Rewind(r)) = state.ui.modal.as_mut()
                && r.phase == crate::state::rewind::RewindPhase::SummarizeFeedback
            {
                r.summarize_feedback.pop();
            } else {
                state.ui.input.textarea.delete_backward(1);
            }
            true
        }
        TuiCommand::DeleteForward => {
            state.ui.input.clear_inline_hint();
            state.ui.input.textarea.delete_forward(1);
            true
        }
        TuiCommand::DeleteWordBackward => {
            state.ui.input.clear_inline_hint();
            edit::delete_word_backward(state);
            true
        }
        TuiCommand::DeleteWordForward => {
            state.ui.input.clear_inline_hint();
            edit::delete_word_forward(state);
            true
        }
        TuiCommand::KillToEndOfLine => {
            state.ui.input.clear_inline_hint();
            edit::kill_to_end_of_line(state);
            true
        }
        TuiCommand::KillToBeginningOfLine => {
            state.ui.input.clear_inline_hint();
            edit::kill_to_beginning_of_line(state);
            true
        }
        TuiCommand::Yank => {
            state.ui.input.clear_inline_hint();
            edit::yank(state);
            true
        }

        // ── Cursor movement ──
        TuiCommand::CursorLeft => {
            state.ui.input.clear_inline_hint();
            state.ui.input.textarea.move_cursor_left();
            true
        }
        TuiCommand::CursorRight => {
            state.ui.input.clear_inline_hint();
            state.ui.input.textarea.move_cursor_right();
            true
        }
        TuiCommand::CursorUp => {
            state.ui.input.clear_inline_hint();
            if state.ui.input.is_empty()
                && let Some(first) = state.session.queued_commands.front()
            {
                let _ = command_tx
                    .send(UserCommand::EditQueuedCommand {
                        id: first.id.clone(),
                    })
                    .await;
                return true;
            }
            edit::history_prev(state);
            true
        }
        TuiCommand::CursorDown => {
            state.ui.input.clear_inline_hint();
            edit::history_next(state);
            true
        }
        TuiCommand::CursorHome => {
            state.ui.input.clear_inline_hint();
            state
                .ui
                .input
                .textarea
                .move_cursor_to_beginning_of_line(coco_tui_ui::widgets::BolBehavior::StayPut);
            true
        }
        TuiCommand::CursorEnd => {
            state.ui.input.clear_inline_hint();
            state
                .ui
                .input
                .textarea
                .move_cursor_to_end_of_line(coco_tui_ui::widgets::EolBehavior::StayPut);
            true
        }
        TuiCommand::WordLeft => {
            state.ui.input.clear_inline_hint();
            edit::word_left(state);
            true
        }
        TuiCommand::WordRight => {
            state.ui.input.clear_inline_hint();
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

        // ── Surface actions ──
        TuiCommand::Approve => {
            interaction::approve(state, command_tx).await;
            true
        }
        TuiCommand::Deny => {
            interaction::deny(state, command_tx).await;
            true
        }
        TuiCommand::ApproveAll => {
            interaction::approve_all(state, command_tx).await;
            true
        }
        TuiCommand::ClassifierAutoApprove {
            request_id,
            matched_rule: _,
        } => {
            interaction::classifier_auto_approve(state, command_tx, request_id).await;
            true
        }
        TuiCommand::AutocompleteAccept => {
            let _ = crate::completion::accept_suggestion(
                state,
                crate::completion::AcceptMode::ExtendCommonPrefix,
            );
            true
        }
        TuiCommand::AcceptPromptSuggestion => {
            accept_prompt_suggestion(state);
            true
        }
        TuiCommand::SubmitPromptSuggestion => {
            if accept_prompt_suggestion(state) {
                edit::submit(state, command_tx).await
            } else {
                true
            }
        }
        TuiCommand::AutocompleteSubmit => {
            if crate::completion::accept_suggestion(
                state,
                crate::completion::AcceptMode::SubmitSelected,
            )
            .is_some_and(|a| a.should_submit)
            {
                edit::submit(state, command_tx).await
            } else {
                true
            }
        }

        // ── Surface navigation ──
        TuiCommand::SurfaceFilter(c) => {
            interaction::filter(state, c);
            true
        }
        TuiCommand::SurfaceFilterBackspace => {
            interaction::filter_backspace(state);
            true
        }
        TuiCommand::SurfaceNext => {
            interaction::nav(state, 1);
            interaction::request_diff_stats_if_rewind(state, command_tx).await;
            true
        }
        TuiCommand::SurfacePrev => {
            interaction::nav(state, -1);
            interaction::request_diff_stats_if_rewind(state, command_tx).await;
            true
        }
        TuiCommand::SurfaceJumpStart => {
            interaction::nav(state, i32::MIN / 2);
            interaction::request_diff_stats_if_rewind(state, command_tx).await;
            true
        }
        TuiCommand::SurfaceJumpEnd => {
            interaction::nav(state, i32::MAX / 2);
            interaction::request_diff_stats_if_rewind(state, command_tx).await;
            true
        }
        TuiCommand::SurfaceConfirm => {
            interaction::confirm(state, command_tx).await;
            true
        }
        TuiCommand::CopyPickerWriteToFile => {
            match state.ui.take_modal() {
                Some(ModalState::CopyPicker(cp)) => {
                    if let Some(message) = copy::write_picker_selection_to_file(state, cp) {
                        copy::enqueue_copy_output(message, command_tx);
                    }
                    state.ui.finish_taken_modal();
                }
                Some(other) => state.ui.restore_modal(other),
                None => {}
            }
            true
        }

        // ── Commands & surfaces ──
        TuiCommand::ShowHelp => {
            state.ui.show_modal(ModalState::Help);
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
        TuiCommand::OpenTeamRoster => {
            show::team_roster(state);
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
        TuiCommand::ShowRewind => {
            show::rewind(state, command_tx).await;
            true
        }
        TuiCommand::ShowRewindFor { target_uuid } => {
            show::rewind_for(state, command_tx, target_uuid).await;
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
            interaction::toggle_syntax_highlighting(state);
            true
        }
        TuiCommand::SettingsNextTab => {
            // Tab cycles between contexts depending on the active state.
            // Settings state → next tab. Question state → cycle focus
            // (questions → footer items). ModelPicker → cycle the
            // role pill. Other surfaces ignore Tab.
            if let Some(ModalState::Settings(s)) = state.ui.modal.as_mut() {
                s.next_tab();
            } else if matches!(
                state.ui.interaction.active_prompt,
                Some(PanePromptState::Question(_))
            ) {
                interaction::question_cycle_focus(state, 1);
            } else if matches!(state.ui.modal, Some(ModalState::ModelPicker(_))) {
                show::cycle_model_role(state, 1);
            }
            true
        }
        TuiCommand::SettingsPrevTab => {
            if let Some(ModalState::Settings(s)) = state.ui.modal.as_mut() {
                s.prev_tab();
            } else if matches!(
                state.ui.interaction.active_prompt,
                Some(PanePromptState::Question(_))
            ) {
                interaction::question_cycle_focus(state, -1);
            } else if matches!(state.ui.modal, Some(ModalState::ModelPicker(_))) {
                show::cycle_model_role(state, -1);
            }
            true
        }
        TuiCommand::ModelPickerCycleEffort(delta) => {
            interaction::cycle_model_effort(state, delta);
            true
        }
        TuiCommand::TeamRosterCycleMode(delta) => {
            // Cycle the focused teammate's mode and apply it immediately
            // (TS: cycling in the teams dialog persists at once).
            if let Some((name, mode)) = interaction::team_roster_cycle_mode(state, delta) {
                let _ = command_tx
                    .send(UserCommand::SetTeammateMode { name, mode })
                    .await;
            }
            true
        }
        TuiCommand::TeamRosterCycleAllModes(delta) => {
            // Cycle ALL teammates in tandem and persist in one batch
            // (TS `cycleAllTeammateModes`).
            let updates = interaction::team_roster_cycle_all_modes(state, delta);
            if !updates.is_empty() {
                let _ = command_tx
                    .send(UserCommand::SetTeammateModes { updates })
                    .await;
            }
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
            let _ = command_tx
                .send(UserCommand::ExecuteSlashCommand {
                    name,
                    args: String::new(),
                })
                .await;
            true
        }

        // ── Task management ──
        TuiCommand::BackgroundAllTasks => {
            // TS-parity single-press Ctrl+B (`SessionBackgroundHint.tsx`):
            // background every foreground BgAgent. There is no wire
            // event for the foreground→background transition, so the
            // TUI mirror flips its own rows optimistically before
            // dispatching the engine command. `is_backgrounded` is a
            // sticky UI flag; the eventual `TaskCompleted` carries the
            // real terminal status into `agent.status`.
            if has_foreground_tasks(state) {
                for agent in state.session.subagents.iter_mut() {
                    if matches!(agent.kind, crate::state::SubagentKind::Subagent)
                        && matches!(agent.status, crate::state::SubagentStatus::Running)
                    {
                        agent.is_backgrounded = true;
                    }
                }
                let _ = command_tx.send(UserCommand::BackgroundAllTasks).await;
            }
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
            if state.session.is_busy() || state.ui.streaming.is_some() {
                state.ui.add_toast(crate::state::ui::Toast::warning(
                    "External editor is unavailable while a turn is running",
                ));
                return true;
            }
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

fn accept_prompt_suggestion(state: &mut AppState) -> bool {
    if !state.ui.input.is_empty() || !state.session.queued_commands.is_empty() {
        return false;
    }
    let Some(suggestion) = state.session.prompt_suggestions.last().cloned() else {
        return false;
    };
    if suggestion.is_empty() {
        return false;
    }
    state.ui.input.set_text(&suggestion);
    state
        .ui
        .input
        .textarea
        .set_cursor(state.ui.input.text().len());
    state.session.prompt_suggestions.clear();
    true
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
            let _ = command_tx
                .send(UserCommand::Interrupt(TurnAbortReason::UserCancel))
                .await;
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
            // First idle Ctrl+C / Ctrl+D: no interrupt, no state.
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
                window_ms = coco_tui_ui::constants::DOUBLE_PRESS_TIMEOUT.as_millis(),
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

async fn queue_current_input(state: &mut AppState, command_tx: &mpsc::Sender<UserCommand>) -> bool {
    let text = state.ui.input.take_input();
    if text.is_empty() {
        return true;
    }
    let resolved = state.ui.paste_manager.resolve_structured(&text);
    let _ = command_tx
        .send(UserCommand::QueueCommand {
            prompt: resolved.text,
            images: resolved.images,
        })
        .await;
    state.ui.paste_manager.clear();
    true
}

/// Whether any foreground tools / subagents are still running. Drives
/// the live Ctrl+B path in `TuiCommand::BackgroundAllTasks`. A
/// subagent flipped to `is_backgrounded` no longer counts; `Queued`
/// tool executions are excluded for parity with TS `hasForegroundTasks`.
fn has_foreground_tasks(state: &AppState) -> bool {
    let any_running_subagent =
        state.session.subagents.iter().any(|s| {
            matches!(s.status, crate::state::SubagentStatus::Running) && !s.is_backgrounded
        });
    let any_running_tool = state
        .session
        .tool_executions
        .iter()
        .any(|t| matches!(t.status, crate::state::ToolStatus::Running));
    any_running_subagent || any_running_tool
}

/// Localised label for a permission mode, used in toasts/banners so the
/// user sees the same wording the help state and status row use.
/// `pub(crate)` so the state deny handler can reuse it without
/// duplicating the match — keeps mode wording consistent across the
/// TogglePlanMode / Cycle / state-decline surfaces.
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
