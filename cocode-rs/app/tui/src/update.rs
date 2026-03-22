//! State update functions.
//!
//! This module contains pure functions that update the application state
//! in response to events. Following the Elm Architecture pattern, these
//! functions take the current state and an event, and return the new state.

use cocode_protocol::LoopEvent;
use cocode_protocol::RoleSelection;
use cocode_protocol::ToolResultContent;
use tokio::sync::mpsc;

use crate::command::UserCommand;
use crate::event::TuiCommand;
use crate::file_search::FileSearchEvent;
use crate::i18n::t;
use crate::paste::PasteManager;
use crate::state::AppState;
use crate::state::ChatMessage;
use crate::state::FileSuggestionItem;
use crate::state::FocusTarget;
use crate::state::ModelPickerOverlay;
use crate::state::Overlay;
use crate::state::PermissionOverlay;

/// Handle a TUI command and update the state accordingly.
///
/// This function processes high-level commands from keyboard input
/// and updates the state. It also sends commands to the core agent
/// when needed.
pub async fn handle_command(
    state: &mut AppState,
    cmd: TuiCommand,
    command_tx: &mpsc::Sender<UserCommand>,
    available_models: &[RoleSelection],
    paste_manager: &PasteManager,
) {
    match cmd {
        // ========== Mode Toggles ==========
        TuiCommand::TogglePlanMode => {
            state.cycle_permission_mode();
            let _ = command_tx
                .send(UserCommand::SetPermissionMode {
                    mode: state.session.permission_mode,
                })
                .await;
        }
        TuiCommand::CycleThinkingLevel => {
            state.cycle_thinking_level();
            if let Some(ref sel) = state.session.current_selection {
                let _ = command_tx
                    .send(UserCommand::SetThinkingLevel {
                        level: sel.effective_thinking_level(),
                    })
                    .await;
            }
        }
        TuiCommand::CycleModel => {
            // Show model picker overlay
            if !available_models.is_empty() {
                state
                    .ui
                    .set_overlay(Overlay::ModelPicker(ModelPickerOverlay::new(
                        available_models.to_vec(),
                    )));
            }
        }
        TuiCommand::ShowModelPicker => {
            // Show model picker overlay
            if !available_models.is_empty() {
                state
                    .ui
                    .set_overlay(Overlay::ModelPicker(ModelPickerOverlay::new(
                        available_models.to_vec(),
                    )));
            }
        }

        // ========== Input Actions ==========
        TuiCommand::SubmitInput => {
            let raw_message = state.ui.input.take();
            if !raw_message.trim().is_empty() {
                // Check if this is a slash command (skill or local command)
                let parsed_cmd = cocode_skill::parse_skill_command(raw_message.trim())
                    .map(|(n, a)| (n.to_string(), a.to_string()));

                if let Some((name, args)) = parsed_cmd {
                    // Check if this is a local (built-in) command
                    if let Some(local_cmd) = cocode_skill::find_local_command(&name) {
                        handle_local_command(state, local_cmd, &args, command_tx, available_models)
                            .await;
                    } else {
                        // Prompt skill — send to agent driver
                        let msg_id = format!("user-{}", state.session.messages.len());
                        state
                            .session
                            .add_message(ChatMessage::user(&msg_id, &raw_message));

                        state.ui.input.add_to_history(raw_message);
                        state.ui.input.history_index = None;

                        let _ = command_tx
                            .send(UserCommand::ExecuteSkill { name, args })
                            .await;

                        state.ui.scroll_offset = 0;
                        state.ui.reset_user_scrolled();
                    }
                } else {
                    // Regular message — resolve paste pills and send as input
                    let content = paste_manager.resolve_to_blocks(&raw_message);
                    let display_text = raw_message.clone();

                    // Add user message to chat (display version)
                    let msg_id = format!("user-{}", state.session.messages.len());
                    state
                        .session
                        .add_message(ChatMessage::user(&msg_id, &display_text));

                    // Save to history with frecency tracking
                    state.ui.input.add_to_history(raw_message);
                    state.ui.input.history_index = None;

                    // Send to core with resolved content blocks
                    let _ = command_tx
                        .send(UserCommand::SubmitInput {
                            content,
                            display_text,
                        })
                        .await;

                    // Auto-scroll to bottom and reset user scroll state
                    state.ui.scroll_offset = 0;
                    state.ui.reset_user_scrolled();
                }
            }
        }
        TuiCommand::Interrupt => {
            let _ = command_tx.send(UserCommand::Interrupt).await;
        }
        TuiCommand::ShowRewindSelector => {
            // Request checkpoint list from core, which will open the overlay on response
            let _ = command_tx.send(UserCommand::RequestRewindCheckpoints).await;
        }
        TuiCommand::Rewind => {
            // Legacy: direct rewind of last turn (for programmatic use)
            let _ = command_tx.send(UserCommand::Rewind).await;
        }
        TuiCommand::ClearScreen => {
            // Clear chat history and reset scroll
            state.session.messages.clear();
            state.ui.scroll_offset = 0;
            state.ui.reset_user_scrolled();
            tracing::debug!("Screen cleared - chat history reset");
        }
        TuiCommand::Cancel => {
            // Close overlay if present, otherwise clear input, otherwise double-Esc for rewind
            if let Some(Overlay::PlanExitApproval(ref mut pe)) = state.ui.overlay {
                if pe.feedback_active {
                    // Exit feedback mode, go back to option selection
                    pe.feedback_active = false;
                    pe.feedback_text.clear();
                } else {
                    state.ui.clear_overlay();
                }
            } else if let Some(Overlay::RewindSelector(ref mut rw)) = state.ui.overlay {
                // In mode selection, go back to checkpoint selection instead of closing
                if !rw.go_back() {
                    state.ui.clear_overlay();
                }
            } else if state.has_overlay() {
                state.ui.clear_overlay();
            } else if !state.ui.input.is_empty() {
                state.ui.input.take();
            } else if state.ui.is_double_esc() {
                // Double-Esc detected: open rewind selector
                state.ui.reset_esc_time();
                let _ = command_tx.send(UserCommand::RequestRewindCheckpoints).await;
            } else {
                // First Esc: record time for double-Esc detection
                state.ui.record_esc();
            }
        }

        // ========== Navigation ==========
        TuiCommand::ScrollUp => {
            state.ui.scroll_offset = state.ui.scroll_offset.saturating_add(3);
            state.ui.mark_user_scrolled();
        }
        TuiCommand::ScrollDown => {
            state.ui.scroll_offset = state.ui.scroll_offset.saturating_sub(3);
            if state.ui.scroll_offset < 0 {
                state.ui.scroll_offset = 0;
            }
            // Only mark as user scrolled if we're not at the bottom
            if state.ui.scroll_offset > 0 {
                state.ui.mark_user_scrolled();
            } else {
                // User scrolled to bottom, re-enable auto-scroll
                state.ui.reset_user_scrolled();
            }
        }
        TuiCommand::PageUp => {
            state.ui.scroll_offset = state.ui.scroll_offset.saturating_add(20);
            state.ui.mark_user_scrolled();
        }
        TuiCommand::PageDown => {
            state.ui.scroll_offset = state.ui.scroll_offset.saturating_sub(20);
            if state.ui.scroll_offset < 0 {
                state.ui.scroll_offset = 0;
            }
            if state.ui.scroll_offset > 0 {
                state.ui.mark_user_scrolled();
            } else {
                state.ui.reset_user_scrolled();
            }
        }
        TuiCommand::FocusNext => {
            state.ui.focus = match state.ui.focus {
                FocusTarget::Input => FocusTarget::Chat,
                FocusTarget::Chat => FocusTarget::ToolPanel,
                FocusTarget::ToolPanel => FocusTarget::Input,
            };
        }
        TuiCommand::FocusPrevious => {
            state.ui.focus = match state.ui.focus {
                FocusTarget::Input => FocusTarget::ToolPanel,
                FocusTarget::Chat => FocusTarget::Input,
                FocusTarget::ToolPanel => FocusTarget::Chat,
            };
        }

        // ========== Editing ==========
        TuiCommand::InsertChar(c) => {
            // Handle overlay input if present
            match &mut state.ui.overlay {
                Some(Overlay::ModelPicker(picker)) => {
                    picker.filter.push(c);
                }
                Some(Overlay::OutputStylePicker(picker)) => {
                    picker.filter.push(c);
                }
                Some(Overlay::CommandPalette(palette)) => {
                    palette.insert_char(c);
                }
                Some(Overlay::SessionBrowser(browser)) => {
                    browser.insert_char(c);
                }
                Some(Overlay::PluginManager(manager)) => {
                    manager.insert_char(c);
                }
                Some(Overlay::RewindSelector(rw))
                    if rw.phase == crate::state::RewindSelectorPhase::InputSummarizeContext =>
                {
                    rw.insert_context_char(c);
                }
                Some(Overlay::PlanExitApproval(pe)) if pe.feedback_active => {
                    pe.feedback_text.push(c);
                }
                Some(Overlay::Question(q)) if q.other_input_active => {
                    q.other_text.push(c);
                }
                Some(Overlay::Question(q))
                    if c == ' ' && q.current().is_some_and(|qi| qi.multi_select) =>
                {
                    q.toggle_selected();
                }
                _ => {
                    state.ui.input.insert_char(c);
                }
            }
        }
        TuiCommand::DeleteBackward => match &mut state.ui.overlay {
            Some(Overlay::ModelPicker(picker)) => {
                picker.filter.pop();
            }
            Some(Overlay::OutputStylePicker(picker)) => {
                picker.filter.pop();
            }
            Some(Overlay::CommandPalette(palette)) => {
                palette.delete_char();
            }
            Some(Overlay::SessionBrowser(browser)) => {
                browser.delete_char();
            }
            Some(Overlay::PluginManager(manager)) => {
                manager.delete_char();
            }
            Some(Overlay::RewindSelector(rw))
                if rw.phase == crate::state::RewindSelectorPhase::InputSummarizeContext =>
            {
                rw.delete_context_char();
            }
            Some(Overlay::PlanExitApproval(pe)) if pe.feedback_active => {
                pe.feedback_text.pop();
            }
            Some(Overlay::Question(q)) if q.other_input_active => {
                q.other_text.pop();
            }
            _ => {
                state.ui.input.delete_backward();
            }
        },
        TuiCommand::DeleteForward => {
            state.ui.input.delete_forward();
        }
        TuiCommand::CursorLeft => {
            state.ui.input.move_left();
        }
        TuiCommand::CursorRight => {
            state.ui.input.move_right();
        }
        TuiCommand::CursorUp => {
            // Handle overlay navigation or history
            match &mut state.ui.overlay {
                Some(Overlay::Permission(perm)) => {
                    perm.move_up();
                }
                Some(Overlay::ModelPicker(picker)) => {
                    picker.move_up();
                }
                Some(Overlay::OutputStylePicker(picker)) => {
                    picker.move_up();
                }
                Some(Overlay::CommandPalette(palette)) => {
                    palette.move_up();
                }
                Some(Overlay::SessionBrowser(browser)) => {
                    browser.move_up();
                }
                Some(Overlay::PluginManager(manager)) => {
                    manager.move_up();
                }
                Some(Overlay::RewindSelector(rw)) => {
                    rw.move_up();
                    // Request diff stats for the newly selected checkpoint (lazy).
                    if rw.phase == crate::state::RewindSelectorPhase::SelectCheckpoint
                        && let Some(cp) = rw.selected_checkpoint()
                        && cp.diff_stats.is_none()
                    {
                        let _ = command_tx
                            .send(UserCommand::RequestDiffStats {
                                turn_number: cp.turn_number,
                            })
                            .await;
                    }
                }
                Some(Overlay::PlanExitApproval(plan_exit)) => {
                    plan_exit.move_up();
                }
                Some(Overlay::Question(q)) => {
                    q.move_up();
                }
                Some(Overlay::Help) => {
                    state.ui.help_scroll = (state.ui.help_scroll - 1).max(0);
                }
                _ => {
                    // History navigation
                    handle_history_up(state);
                }
            }
        }
        TuiCommand::CursorDown => {
            // Handle overlay navigation or history
            match &mut state.ui.overlay {
                Some(Overlay::Permission(perm)) => {
                    perm.move_down();
                }
                Some(Overlay::ModelPicker(picker)) => {
                    picker.move_down();
                }
                Some(Overlay::OutputStylePicker(picker)) => {
                    picker.move_down();
                }
                Some(Overlay::CommandPalette(palette)) => {
                    palette.move_down();
                }
                Some(Overlay::SessionBrowser(browser)) => {
                    browser.move_down();
                }
                Some(Overlay::PluginManager(manager)) => {
                    manager.move_down();
                }
                Some(Overlay::RewindSelector(rw)) => {
                    rw.move_down();
                    // Request diff stats for the newly selected checkpoint (lazy).
                    if rw.phase == crate::state::RewindSelectorPhase::SelectCheckpoint
                        && let Some(cp) = rw.selected_checkpoint()
                        && cp.diff_stats.is_none()
                    {
                        let _ = command_tx
                            .send(UserCommand::RequestDiffStats {
                                turn_number: cp.turn_number,
                            })
                            .await;
                    }
                }
                Some(Overlay::PlanExitApproval(plan_exit)) => {
                    plan_exit.move_down();
                }
                Some(Overlay::Question(q)) => {
                    q.move_down();
                }
                Some(Overlay::Help) => {
                    state.ui.help_scroll += 1;
                }
                _ => {
                    // History navigation
                    handle_history_down(state);
                }
            }
        }
        TuiCommand::CursorHome => {
            state.ui.input.move_home();
        }
        TuiCommand::CursorEnd => {
            state.ui.input.move_end();
        }
        TuiCommand::WordLeft => {
            state.ui.input.move_word_left();
        }
        TuiCommand::WordRight => {
            state.ui.input.move_word_right();
        }
        TuiCommand::DeleteWordBackward => {
            state.ui.input.delete_word_backward();
        }
        TuiCommand::DeleteWordForward => {
            state.ui.input.delete_word_forward();
        }
        TuiCommand::InsertNewline => {
            state.ui.input.insert_newline();
        }

        // ========== Approval ==========
        TuiCommand::Approve => {
            if let Some(Overlay::Question(ref mut q_overlay)) = state.ui.overlay {
                // Handle question overlay confirmation
                if q_overlay.other_input_active {
                    // Confirm "Other" text input, then advance
                    let all_done = q_overlay.confirm_current();
                    if all_done {
                        let mut answers = q_overlay.collect_answers();
                        embed_images_in_answers(&mut answers, paste_manager);
                        let request_id = q_overlay.request_id.clone();
                        let _ = command_tx
                            .send(UserCommand::QuestionResponse {
                                request_id,
                                answers,
                            })
                            .await;
                        state.ui.clear_overlay();
                    }
                } else if q_overlay.is_other_selected() {
                    // Activate "Other" text input mode
                    q_overlay.other_input_active = true;
                    q_overlay.other_text.clear();
                } else {
                    let all_done = q_overlay.confirm_current();
                    if all_done {
                        let mut answers = q_overlay.collect_answers();
                        embed_images_in_answers(&mut answers, paste_manager);
                        let request_id = q_overlay.request_id.clone();
                        let _ = command_tx
                            .send(UserCommand::QuestionResponse {
                                request_id,
                                answers,
                            })
                            .await;
                        state.ui.clear_overlay();
                    }
                }
            } else if let Some(Overlay::PlanExitApproval(ref mut plan_exit)) = state.ui.overlay {
                // Handle 5-option plan exit approval
                let option = plan_exit.selected_option();

                if option == cocode_protocol::PlanExitOption::KeepPlanning {
                    if plan_exit.feedback_active {
                        // Second Enter: send denial with feedback text
                        let request_id = plan_exit.request.request_id.clone();
                        let feedback = if plan_exit.feedback_text.trim().is_empty() {
                            None
                        } else {
                            Some(plan_exit.feedback_text.clone())
                        };
                        let _ = command_tx
                            .send(UserCommand::ApprovalResponse {
                                request_id,
                                decision: cocode_protocol::ApprovalDecision::Denied,
                                plan_exit_option: Some(option),
                                feedback,
                            })
                            .await;
                        state.ui.clear_overlay();
                    } else {
                        // First Enter: activate feedback text input
                        plan_exit.feedback_active = true;
                    }
                } else {
                    let request_id = plan_exit.request.request_id.clone();
                    let decision = if option.is_approved() {
                        cocode_protocol::ApprovalDecision::Approved
                    } else {
                        cocode_protocol::ApprovalDecision::Denied
                    };
                    let _ = command_tx
                        .send(UserCommand::ApprovalResponse {
                            request_id,
                            decision,
                            plan_exit_option: Some(option),
                            feedback: None,
                        })
                        .await;
                    state.ui.clear_overlay();
                }
            } else if let Some(Overlay::Permission(ref perm)) = state.ui.overlay {
                let request_id = perm.request.request_id.clone();
                let _ = command_tx
                    .send(UserCommand::ApprovalResponse {
                        request_id,
                        decision: cocode_protocol::ApprovalDecision::Approved,
                        plan_exit_option: None,
                        feedback: None,
                    })
                    .await;
                state.ui.clear_overlay();
            } else if let Some(Overlay::ModelPicker(ref picker)) = state.ui.overlay {
                // Select current model
                let filtered = picker.filtered_items();
                if let Some(selection) = filtered.get(picker.selected as usize) {
                    let selection = (*selection).clone();
                    state.session.current_selection = Some(selection.clone());
                    let _ = command_tx.send(UserCommand::SetModel { selection }).await;
                }
                state.ui.clear_overlay();
            } else if let Some(Overlay::OutputStylePicker(ref picker)) = state.ui.overlay {
                // Select output style
                let filtered = picker.filtered_items();
                if let Some(item) = filtered.get(picker.selected as usize) {
                    let style_name = item.name.clone();
                    state.session.output_style = Some(style_name.clone());
                    let _ = command_tx
                        .send(UserCommand::SetOutputStyle {
                            style: Some(style_name),
                        })
                        .await;
                }
                state.ui.clear_overlay();
            } else if let Some(Overlay::CommandPalette(ref palette)) = state.ui.overlay {
                // Execute selected command
                if let Some(cmd) = palette.selected_command() {
                    let action = cmd.action.clone();
                    state.ui.clear_overlay();
                    execute_command_action(state, &action, command_tx).await;
                } else {
                    state.ui.clear_overlay();
                }
            } else if let Some(Overlay::SessionBrowser(ref browser)) = state.ui.overlay {
                // Load selected session
                if let Some(session) = browser.selected_session() {
                    let session_id = session.id.clone();
                    state.ui.clear_overlay();
                    tracing::info!(session_id, "Load session requested (not yet implemented)");
                } else {
                    state.ui.clear_overlay();
                }
            } else if let Some(Overlay::RewindSelector(ref mut rw)) = state.ui.overlay {
                // Confirm selection (checkpoint, mode, or summarize context)
                use crate::state::RewindAction;
                match rw.confirm() {
                    Some(RewindAction::Rewind { turn_number, mode }) => {
                        rw.set_loading(t!("dialog.rewind_loading_restore").to_string());
                        let _ = command_tx
                            .send(UserCommand::RewindToTurn { turn_number, mode })
                            .await;
                        // Overlay is closed on RewindCompleted event
                    }
                    Some(RewindAction::Summarize {
                        turn_number,
                        context,
                    }) => {
                        rw.set_loading(t!("dialog.rewind_loading_summarize").to_string());
                        let _ = command_tx
                            .send(UserCommand::SummarizeFromTurn {
                                turn_number,
                                context,
                            })
                            .await;
                        // Overlay is closed on SummarizeCompleted/SummarizeFailed event
                    }
                    None => {
                        // Advanced to next phase (mode selection or context input)
                    }
                }
            } else if let Some(Overlay::PluginManager(ref _manager)) = state.ui.overlay {
                // TODO: Handle Enter on selected item (install/enable/disable/etc.)
                // For now, just log the action
                tracing::info!("Plugin manager action requested");
            }
        }
        TuiCommand::Deny => {
            if let Some(Overlay::Question(ref q_overlay)) = state.ui.overlay {
                // Cancel question — send empty answers so the tool unblocks
                let request_id = q_overlay.request_id.clone();
                let _ = command_tx
                    .send(UserCommand::QuestionResponse {
                        request_id,
                        answers: serde_json::json!({}),
                    })
                    .await;
                state.ui.clear_overlay();
            } else if let Some(Overlay::PlanExitApproval(ref plan_exit)) = state.ui.overlay {
                // Quick deny (N key) — send KeepPlanning without feedback
                let request_id = plan_exit.request.request_id.clone();
                let _ = command_tx
                    .send(UserCommand::ApprovalResponse {
                        request_id,
                        decision: cocode_protocol::ApprovalDecision::Denied,
                        plan_exit_option: Some(cocode_protocol::PlanExitOption::KeepPlanning),
                        feedback: None,
                    })
                    .await;
                state.ui.clear_overlay();
            } else if let Some(Overlay::Permission(ref perm)) = state.ui.overlay {
                let request_id = perm.request.request_id.clone();
                let _ = command_tx
                    .send(UserCommand::ApprovalResponse {
                        request_id,
                        decision: cocode_protocol::ApprovalDecision::Denied,
                        plan_exit_option: None,
                        feedback: None,
                    })
                    .await;
                state.ui.clear_overlay();
            }
        }
        TuiCommand::ApproveAll => {
            if let Some(Overlay::Permission(ref perm)) = state.ui.overlay {
                let request_id = perm.request.request_id.clone();
                // Use proposed prefix pattern if available, otherwise approve once
                let decision = if let Some(ref prefix) = perm.request.proposed_prefix_pattern {
                    cocode_protocol::ApprovalDecision::ApprovedWithPrefix {
                        prefix_pattern: prefix.clone(),
                    }
                } else {
                    cocode_protocol::ApprovalDecision::Approved
                };
                let _ = command_tx
                    .send(UserCommand::ApprovalResponse {
                        request_id,
                        decision,
                        plan_exit_option: None,
                        feedback: None,
                    })
                    .await;
                state.ui.clear_overlay();
            }
        }

        // ========== File Autocomplete ==========
        TuiCommand::SelectNextSuggestion => {
            if let Some(ref mut s) = state.ui.file_suggestions {
                s.move_down();
            }
        }
        TuiCommand::SelectPrevSuggestion => {
            if let Some(ref mut s) = state.ui.file_suggestions {
                s.move_up();
            }
        }
        TuiCommand::AcceptSuggestion => {
            if let Some(s) = state.ui.file_suggestions.take()
                && let Some(sel) = s.selected_suggestion()
            {
                state.ui.input.insert_selected_path(s.start_pos, &sel.path);
            }
        }
        TuiCommand::DismissSuggestions => {
            state.ui.clear_file_suggestions();
        }

        // ========== Skill Autocomplete ==========
        TuiCommand::SelectNextSkillSuggestion => {
            if let Some(ref mut s) = state.ui.skill_suggestions {
                s.move_down();
            }
        }
        TuiCommand::SelectPrevSkillSuggestion => {
            if let Some(ref mut s) = state.ui.skill_suggestions {
                s.move_up();
            }
        }
        TuiCommand::AcceptSkillSuggestion => {
            if let Some(s) = state.ui.skill_suggestions.take()
                && let Some(sel) = s.selected_suggestion()
            {
                state.ui.input.insert_selected_skill(s.start_pos, &sel.name);
            }
        }
        TuiCommand::DismissSkillSuggestions => {
            state.ui.clear_skill_suggestions();
        }

        // ========== Agent Autocomplete ==========
        TuiCommand::SelectNextAgentSuggestion => {
            if let Some(ref mut s) = state.ui.agent_suggestions {
                s.move_down();
            }
        }
        TuiCommand::SelectPrevAgentSuggestion => {
            if let Some(ref mut s) = state.ui.agent_suggestions {
                s.move_up();
            }
        }
        TuiCommand::AcceptAgentSuggestion => {
            if let Some(s) = state.ui.agent_suggestions.take()
                && let Some(sel) = s.selected_suggestion()
            {
                state
                    .ui
                    .input
                    .insert_selected_agent(s.start_pos, &sel.agent_type);
            }
        }
        TuiCommand::DismissAgentSuggestions => {
            state.ui.clear_agent_suggestions();
        }

        // ========== Symbol Autocomplete ==========
        TuiCommand::SelectNextSymbolSuggestion => {
            if let Some(ref mut s) = state.ui.symbol_suggestions {
                s.move_down();
            }
        }
        TuiCommand::SelectPrevSymbolSuggestion => {
            if let Some(ref mut s) = state.ui.symbol_suggestions {
                s.move_up();
            }
        }
        TuiCommand::AcceptSymbolSuggestion => {
            if let Some(s) = state.ui.symbol_suggestions.take()
                && let Some(sel) = s.selected_suggestion()
            {
                state
                    .ui
                    .input
                    .insert_selected_symbol(s.start_pos, &sel.file_path, sel.line);
            }
        }
        TuiCommand::DismissSymbolSuggestions => {
            state.ui.clear_symbol_suggestions();
        }

        // ========== Queue ==========
        TuiCommand::QueueInput => {
            // Queue input for later processing (Enter during streaming)
            // This also serves as real-time steering: queued commands are
            // injected into the current turn as system reminders.
            let prompt = state.ui.input.take();
            if !prompt.trim().is_empty() {
                let id = state.session.queue_command(&prompt);
                let _ = command_tx
                    .send(UserCommand::QueueCommand {
                        prompt: prompt.clone(),
                    })
                    .await;
                let count = state.session.queued_count();
                state
                    .ui
                    .toast_info(t!("toast.command_queued", count = count).to_string());
                tracing::debug!(id, count, "Command queued (also serves as steering)");
            }
        }

        // ========== External Editor ==========
        TuiCommand::OpenExternalEditor => {
            // TODO: Implement external editor support
            tracing::info!("External editor requested (not yet implemented)");
        }

        // ========== Plan File Editor (Ctrl+G) ==========
        TuiCommand::OpenPlanEditor => {
            if state.session.plan_mode {
                if let Some(ref plan_file) = state.session.plan_file {
                    let plan_path = plan_file.clone();
                    // Read existing plan content (or empty string for new plans)
                    let content = std::fs::read_to_string(&plan_path).unwrap_or_default();
                    match crate::editor::edit_in_external_editor(&content) {
                        Ok(result) => {
                            if result.modified {
                                // Write the edited content back to the plan file
                                if let Err(e) = std::fs::write(&plan_path, &result.content) {
                                    state.ui.toast_error(format!("Failed to save plan: {e}"));
                                } else {
                                    state.ui.toast_success("Plan saved!".to_string());
                                }
                            }
                        }
                        Err(e) => {
                            state.ui.toast_error(format!("Editor error: {e}"));
                        }
                    }
                } else {
                    state
                        .ui
                        .toast_info("No plan file available yet.".to_string());
                }
            } else {
                state
                    .ui
                    .toast_info("Ctrl+G: Open plan file in editor (plan mode only)".to_string());
            }
        }

        // ========== Clipboard Paste ==========
        TuiCommand::PasteFromClipboard => {
            // Handled in app.rs (needs &mut paste_manager)
        }

        // ========== Help ==========
        TuiCommand::ShowHelp => {
            state.ui.help_scroll = 0;
            state.ui.set_overlay(Overlay::Help);
        }

        // ========== Command Palette ==========
        TuiCommand::ShowCommandPalette => {
            let commands = get_default_commands();
            state.ui.set_overlay(Overlay::CommandPalette(
                crate::state::CommandPaletteOverlay::new(commands),
            ));
        }

        // ========== Session Browser ==========
        TuiCommand::ShowSessionBrowser => {
            // TODO: Load sessions from storage
            let sessions = Vec::new();
            state.ui.set_overlay(Overlay::SessionBrowser(
                crate::state::SessionBrowserOverlay::new(sessions),
            ));
        }
        TuiCommand::LoadSession(_session_id) => {
            // TODO: Implement session loading
            tracing::info!("Load session requested (not yet implemented)");
        }
        TuiCommand::DeleteSession(_session_id) => {
            // TODO: Implement session deletion
            tracing::info!("Delete session requested (not yet implemented)");
        }

        // ========== Plugin Manager ==========
        TuiCommand::ShowPluginManager => {
            // Request plugin data from the session — the response will
            // arrive as `LoopEvent::PluginDataReady` and open the overlay.
            let _ = command_tx.send(UserCommand::RequestPluginData).await;
        }
        TuiCommand::PluginManagerNextTab => {
            if let Some(Overlay::PluginManager(ref mut manager)) = state.ui.overlay {
                manager.next_tab();
            }
        }
        TuiCommand::PluginManagerPrevTab => {
            if let Some(Overlay::PlanExitApproval(ref mut plan_exit)) = state.ui.overlay {
                // Shift+Tab in PlanExitOverlay: select option 0 (ClearAndAcceptEdits)
                let request_id = plan_exit.request.request_id.clone();
                let option = cocode_protocol::PlanExitOption::ClearAndAcceptEdits;
                let _ = command_tx
                    .send(UserCommand::ApprovalResponse {
                        request_id,
                        decision: cocode_protocol::ApprovalDecision::Approved,
                        plan_exit_option: Some(option),
                        feedback: None,
                    })
                    .await;
                state.ui.clear_overlay();
            } else if let Some(Overlay::PluginManager(ref mut manager)) = state.ui.overlay {
                manager.prev_tab();
            }
        }

        // ========== Thinking Toggle ==========
        TuiCommand::ToggleThinking => {
            state.ui.toggle_thinking();
        }

        // ========== Background ==========
        TuiCommand::BackgroundAllTasks => {
            let mut count = 0;

            // Background agents
            for id in cocode_subagent::signal::backgroundable_agent_ids() {
                if cocode_subagent::signal::trigger_background_transition(&id) {
                    count += 1;
                    tracing::info!(agent_id = %id, "Agent transitioned to background");
                }
            }

            // Background bash commands
            for id in cocode_shell::signal::backgroundable_bash_ids() {
                if cocode_shell::signal::trigger_bash_background(&id) {
                    count += 1;
                    tracing::info!(bash_id = %id, "Bash command transitioned to background");
                }
            }

            if count > 0 {
                state
                    .ui
                    .toast_info(t!("toast.tasks_backgrounded", count = count).to_string());
                let _ = command_tx.send(UserCommand::BackgroundAllTasks).await;
            } else {
                tracing::debug!("Ctrl+B: no backgroundable tasks");
            }
        }

        // ========== Tool Collapse ==========
        TuiCommand::ToggleToolCollapse => {
            // Toggle collapse state for all tool calls
            // If any are expanded (not in set), collapse all; otherwise expand all
            let all_ids: Vec<String> = state
                .session
                .messages
                .iter()
                .flat_map(|m| m.tool_calls.iter().map(|tc| tc.tool_name.clone()))
                .collect();
            if state.ui.collapsed_tools.is_empty() {
                // Collapse all
                for id in all_ids {
                    state.ui.collapsed_tools.insert(id);
                }
            } else {
                // Expand all
                state.ui.collapsed_tools.clear();
            }
            tracing::debug!(
                collapsed = state.ui.collapsed_tools.len(),
                "Toggled tool collapse"
            );
        }

        // ========== Quit ==========
        TuiCommand::Quit => {
            state.quit();
        }
    }
}

/// Handle a local (built-in) slash command in the TUI.
async fn handle_local_command(
    state: &mut AppState,
    local_cmd: &cocode_skill::LocalCommandDef,
    _args: &str,
    command_tx: &mpsc::Sender<UserCommand>,
    available_models: &[RoleSelection],
) {
    match local_cmd.name {
        "help" => {
            state.ui.help_scroll = 0;
            state.ui.set_overlay(Overlay::Help);
        }
        "clear" => {
            state.session.messages.clear();
            state.ui.scroll_offset = 0;
            state.ui.reset_user_scrolled();
            tracing::debug!("Screen cleared via /clear command");
        }
        "model" => {
            // Show model picker overlay
            if !available_models.is_empty() {
                state
                    .ui
                    .set_overlay(Overlay::ModelPicker(ModelPickerOverlay::new(
                        available_models.to_vec(),
                    )));
            }
        }
        "status" => {
            // Show status as a system-style message in chat
            let model_display = state
                .session
                .current_selection
                .as_ref()
                .map(|s| s.model.display_name.to_string())
                .unwrap_or_else(|| "none".to_string());
            let thinking = state
                .session
                .current_selection
                .as_ref()
                .map(|s| format!("{:?}", s.effective_thinking_level().effort))
                .unwrap_or_else(|| "none".to_string());
            let status = format!(
                "Model: {model_display}\nThinking: {thinking}\nPlan mode: {}",
                if state.session.plan_mode { "on" } else { "off" },
            );
            let msg_id = format!("status-{}", state.session.messages.len());
            state
                .session
                .add_message(ChatMessage::assistant(&msg_id, &status));
        }
        "exit" | "quit" => {
            state.quit();
        }
        "cancel" => {
            let _ = command_tx.send(UserCommand::Interrupt).await;
        }
        "output-style" => {
            let args = _args.trim();
            if args.is_empty() {
                // No args: open interactive picker overlay
                let _ = command_tx.send(UserCommand::RequestOutputStyles).await;
            } else {
                // Optimistically update TUI state for status bar display
                match args {
                    "off" | "none" | "disable" => {
                        state.session.output_style = None;
                    }
                    "status" | "list" | "help" => {}
                    name => {
                        state.session.output_style = Some(name.to_string());
                    }
                }
                // Dispatch to agent driver for full processing
                let msg_id = format!("user-{}", state.session.messages.len());
                let display = format!("/{} {args}", local_cmd.name);
                state
                    .session
                    .add_message(ChatMessage::user(&msg_id, &display));
                let _ = command_tx
                    .send(UserCommand::ExecuteSkill {
                        name: local_cmd.name.to_string(),
                        args: args.to_string(),
                    })
                    .await;
                state.ui.scroll_offset = 0;
                state.ui.reset_user_scrolled();
            }
        }
        "rewind" | "checkpoint" => {
            // Open the rewind checkpoint selector (same as Ctrl+Z)
            let _ = command_tx.send(UserCommand::RequestRewindCheckpoints).await;
        }
        // Commands that need the agent driver: dispatch via ExecuteSkill
        "compact" | "skills" | "todos" | "agents" => {
            let msg_id = format!("user-{}", state.session.messages.len());
            let display = format!("/{}", local_cmd.name);
            state
                .session
                .add_message(ChatMessage::user(&msg_id, &display));
            let _ = command_tx
                .send(UserCommand::ExecuteSkill {
                    name: local_cmd.name.to_string(),
                    args: _args.to_string(),
                })
                .await;
            state.ui.scroll_offset = 0;
            state.ui.reset_user_scrolled();
        }
        _ => {
            state
                .ui
                .toast_info(format!("/{} is not yet supported in TUI.", local_cmd.name));
        }
    }
}

/// Execute a command action from the command palette.
async fn execute_command_action(
    state: &mut AppState,
    action: &crate::state::CommandAction,
    command_tx: &mpsc::Sender<UserCommand>,
) {
    use crate::state::CommandAction;

    match action {
        CommandAction::TogglePlanMode => {
            state.cycle_permission_mode();
            let _ = command_tx
                .send(UserCommand::SetPermissionMode {
                    mode: state.session.permission_mode,
                })
                .await;
        }
        CommandAction::CycleThinkingLevel => {
            state.cycle_thinking_level();
            if let Some(ref sel) = state.session.current_selection {
                let _ = command_tx
                    .send(UserCommand::SetThinkingLevel {
                        level: sel.effective_thinking_level(),
                    })
                    .await;
            }
        }
        CommandAction::ShowModelPicker => {
            // Don't have available_models here, so just log
            tracing::info!("Model picker requested from command palette");
        }
        CommandAction::ShowHelp => {
            state.ui.help_scroll = 0;
            state.ui.set_overlay(Overlay::Help);
        }
        CommandAction::ShowSessionBrowser => {
            let sessions = Vec::new();
            state.ui.set_overlay(Overlay::SessionBrowser(
                crate::state::SessionBrowserOverlay::new(sessions),
            ));
        }
        CommandAction::ShowPluginManager => {
            let _ = command_tx.send(UserCommand::RequestPluginData).await;
        }
        CommandAction::ClearScreen => {
            state.session.messages.clear();
            state.ui.scroll_offset = 0;
            state.ui.reset_user_scrolled();
        }
        CommandAction::Interrupt => {
            let _ = command_tx.send(UserCommand::Interrupt).await;
        }
        CommandAction::Quit => {
            state.quit();
        }
    }
}

/// Get the default list of commands for the command palette.
fn get_default_commands() -> Vec<crate::state::CommandItem> {
    use crate::state::CommandAction;
    use crate::state::CommandItem;

    vec![
        CommandItem {
            name: t!("palette.toggle_plan_mode").to_string(),
            description: t!("palette.toggle_plan_mode_desc").to_string(),
            shortcut: Some("Tab".to_string()),
            action: CommandAction::TogglePlanMode,
        },
        CommandItem {
            name: t!("palette.cycle_thinking").to_string(),
            description: t!("palette.cycle_thinking_desc").to_string(),
            shortcut: Some("Ctrl+T".to_string()),
            action: CommandAction::CycleThinkingLevel,
        },
        CommandItem {
            name: t!("palette.switch_model").to_string(),
            description: t!("palette.switch_model_desc").to_string(),
            shortcut: Some("Ctrl+M".to_string()),
            action: CommandAction::ShowModelPicker,
        },
        CommandItem {
            name: t!("palette.show_help").to_string(),
            description: t!("palette.show_help_desc").to_string(),
            shortcut: Some("?".to_string()),
            action: CommandAction::ShowHelp,
        },
        CommandItem {
            name: t!("palette.session_browser").to_string(),
            description: t!("palette.session_browser_desc").to_string(),
            shortcut: Some("Ctrl+S".to_string()),
            action: CommandAction::ShowSessionBrowser,
        },
        CommandItem {
            name: t!("palette.plugin_manager").to_string(),
            description: t!("palette.plugin_manager_desc").to_string(),
            shortcut: None,
            action: CommandAction::ShowPluginManager,
        },
        CommandItem {
            name: t!("palette.clear_screen").to_string(),
            description: t!("palette.clear_screen_desc").to_string(),
            shortcut: Some("Ctrl+L".to_string()),
            action: CommandAction::ClearScreen,
        },
        CommandItem {
            name: t!("palette.interrupt").to_string(),
            description: t!("palette.interrupt_desc").to_string(),
            shortcut: Some("Ctrl+C".to_string()),
            action: CommandAction::Interrupt,
        },
        CommandItem {
            name: t!("palette.quit").to_string(),
            description: t!("palette.quit_desc").to_string(),
            shortcut: Some("Ctrl+Q".to_string()),
            action: CommandAction::Quit,
        },
    ]
}

/// Handle input history navigation (up arrow).
fn handle_history_up(state: &mut AppState) {
    let history_len = state.ui.input.history_len();
    if history_len == 0 {
        return;
    }

    let new_index = match state.ui.input.history_index {
        None => Some(0), // Start from most recent (history is sorted by frecency)
        Some(idx) if (idx as usize) < history_len - 1 => Some(idx + 1),
        Some(idx) => Some(idx),
    };

    if let Some(idx) = new_index {
        // Clone text to avoid borrow issues
        let text = state
            .ui
            .input
            .history_text(idx as usize)
            .map(std::string::ToString::to_string);
        if let Some(text) = text {
            state.ui.input.set_text(text);
            state.ui.input.history_index = Some(idx);
        }
    }
}

/// Handle input history navigation (down arrow).
fn handle_history_down(state: &mut AppState) {
    let history_len = state.ui.input.history_len();
    if history_len == 0 {
        return;
    }

    match state.ui.input.history_index {
        Some(idx) if idx > 0 => {
            let new_idx = idx - 1;
            // Clone text to avoid borrow issues
            let text = state
                .ui
                .input
                .history_text(new_idx as usize)
                .map(std::string::ToString::to_string);
            if let Some(text) = text {
                state.ui.input.set_text(text);
                state.ui.input.history_index = Some(new_idx);
            }
        }
        Some(_) | None => {
            // At the most recent or not in history, clear input
            state.ui.input.take();
            state.ui.input.history_index = None;
        }
    }
}

/// Handle a symbol search event.
///
/// This function processes results from the symbol search manager
/// and updates the autocomplete suggestions.
pub fn handle_symbol_search_event(
    state: &mut AppState,
    event: crate::symbol_search::SymbolSearchEvent,
) {
    match event {
        crate::symbol_search::SymbolSearchEvent::IndexReady { symbol_count } => {
            tracing::info!(symbol_count, "Symbol index ready");
        }
        crate::symbol_search::SymbolSearchEvent::SearchResult {
            query,
            start_pos: _,
            suggestions,
        } => {
            // Only update if we're still showing suggestions for this query
            if let Some(ref current) = state.ui.symbol_suggestions
                && current.query == query
            {
                state.ui.update_symbol_suggestions(suggestions);
            }
        }
    }
}

/// Handle a file search event.
///
/// This function processes results from the file search manager
/// and updates the autocomplete suggestions.
pub fn handle_file_search_event(state: &mut AppState, event: FileSearchEvent) {
    match event {
        FileSearchEvent::SearchResult {
            query,
            start_pos: _,
            suggestions,
        } => {
            // Only update if we're still showing suggestions for this query
            if let Some(ref current) = state.ui.file_suggestions
                && current.query == query
            {
                let items: Vec<FileSuggestionItem> = suggestions
                    .into_iter()
                    .map(|s| FileSuggestionItem {
                        path: s.path,
                        display_text: s.display_text,
                        score: s.score,
                        match_indices: s.match_indices,
                        is_directory: s.is_directory,
                    })
                    .collect();
                state.ui.update_file_suggestions(items);
            }
        }
    }
}

/// Handle an event from the core agent loop.
///
/// This function processes events from the agent and updates the
/// application state accordingly. It handles streaming content,
/// tool execution updates, and other agent lifecycle events.
pub fn handle_agent_event(state: &mut AppState, event: LoopEvent) {
    match event {
        // ========== Turn Lifecycle ==========
        LoopEvent::TurnStarted {
            turn_id,
            turn_number,
        } => {
            state.ui.start_streaming(turn_id);
            // Clear previous thinking duration when starting a new turn
            state.ui.clear_thinking_duration();
            // Reset thinking tokens for new turn
            state.session.reset_thinking_tokens();
            // Track current turn number and tag the last user message
            state.session.current_turn_number = Some(turn_number);
            if let Some(msg) = state.session.messages.last_mut()
                && msg.role == crate::state::MessageRole::User
                && msg.turn_number.is_none()
            {
                msg.turn_number = Some(turn_number);
            }
            // Start query timing tracker
            state.ui.query_timing.start();
        }
        LoopEvent::TurnCompleted { turn_id, usage } => {
            // Check for slow query before stopping the timer
            if state.ui.query_timing.is_slow_query()
                && let Some(duration) = state.ui.query_timing.actual_duration()
            {
                state
                    .ui
                    .toast_info(format!("Query took {:.1}s", duration.as_secs_f64()));
            }
            state.ui.query_timing.stop();
            // Stop thinking timer if still running
            if state.ui.is_thinking() {
                state.ui.stop_thinking();
            }
            // Finalize the streaming message
            if let Some(streaming) = state.ui.streaming.take() {
                let mut message = ChatMessage::assistant(&turn_id, &streaming.content);
                if !streaming.thinking.is_empty() {
                    message.thinking = Some(streaming.thinking);
                }
                message.turn_number = state.session.current_turn_number;
                message.complete();
                state.session.add_message(message);
            }
            // Track reasoning/thinking tokens
            if let Some(reasoning_tokens) = usage.reasoning_tokens {
                state.session.add_thinking_tokens(reasoning_tokens as i32);
            }
            state.session.update_tokens(usage);
        }

        // ========== Content Streaming ==========
        LoopEvent::TextDelta { delta, .. } => {
            // When we get the first text delta, thinking is done
            if state.ui.is_thinking() {
                state.ui.stop_thinking();
            }
            state.ui.append_streaming(&delta);
        }
        LoopEvent::ThinkingDelta { delta, .. } => {
            // Start thinking timer on first thinking delta
            state.ui.start_thinking();
            state.ui.append_streaming_thinking(&delta);
        }
        LoopEvent::ToolCallDelta { call_id, delta } => {
            // Accumulate partial tool call JSON for streaming display
            state.ui.append_tool_call_delta(&call_id, &delta);
        }

        // ========== Tool Execution ==========
        LoopEvent::ToolUseQueued {
            call_id,
            name,
            input: _,
        } => {
            // Track tool use during streaming for spinner/status display
            state.ui.add_streaming_tool_use(call_id, name);
        }
        LoopEvent::ToolUseStarted { call_id, name, .. } => {
            // When tool execution begins, transition mode to ToolUse
            state.ui.set_stream_mode_tool_use();
            state.session.start_tool(call_id, name);
        }
        LoopEvent::ToolProgress { call_id, progress } => {
            if let Some(msg) = progress.message {
                state.session.update_tool_progress(&call_id, msg);
            }
        }
        LoopEvent::ToolUseCompleted {
            call_id,
            output,
            is_error,
        } => {
            let output_str = match output {
                ToolResultContent::Text(s) => s,
                ToolResultContent::Structured(v) => v.to_string(),
            };
            state.session.complete_tool(&call_id, output_str, is_error);
            // Cleanup old completed tools
            state.session.cleanup_completed_tools(10);
        }

        // ========== Permission ==========
        LoopEvent::ApprovalRequired { request } => {
            // Pause query timing while user reviews permission
            state.ui.query_timing.on_permission_dialog_open();
            if request.tool_name == cocode_protocol::ToolName::ExitPlanMode.as_str() {
                // Use the 4-option plan exit overlay for ExitPlanMode
                state.ui.set_overlay(Overlay::PlanExitApproval(
                    crate::state::PlanExitOverlay::new(request),
                ));
            } else {
                state
                    .ui
                    .set_overlay(Overlay::Permission(PermissionOverlay::new(request)));
            }
        }

        // ========== User Questions ==========
        LoopEvent::QuestionAsked {
            request_id,
            questions,
        } => {
            // Pause query timing while user answers questions
            state.ui.query_timing.on_permission_dialog_open();
            state
                .ui
                .set_overlay(Overlay::Question(crate::state::QuestionOverlay::new(
                    request_id, &questions,
                )));
        }

        // ========== Token Usage ==========
        LoopEvent::StreamRequestEnd { usage } => {
            // Track reasoning/thinking tokens separately
            if let Some(reasoning_tokens) = usage.reasoning_tokens {
                state.session.add_thinking_tokens(reasoning_tokens as i32);
            }
            state.session.update_tokens(usage);
        }

        // ========== Plan Mode ==========
        LoopEvent::PlanModeEntered { plan_file } => {
            state.session.plan_mode = true;
            state.session.permission_mode = cocode_protocol::PermissionMode::Plan;
            state.session.plan_file = plan_file;
        }
        LoopEvent::PlanModeExited { .. } => {
            state.session.plan_mode = false;
            state.session.plan_file = None;
            if state.session.permission_mode == cocode_protocol::PermissionMode::Plan {
                state.session.permission_mode = cocode_protocol::PermissionMode::Default;
            }
        }

        // ========== Context Cleared ==========
        LoopEvent::ContextCleared { new_mode } => {
            // Clear all TUI conversation state
            state.session.messages.clear();
            state.session.tool_executions.clear();
            state.session.subagents.clear();
            state.session.plan_mode = false;
            state.session.plan_file = None;
            state.session.permission_mode = new_mode;
            state.ui.scroll_offset = 0;
            state.ui.user_scrolled = false;
            tracing::info!(?new_mode, "Context cleared after plan exit");
        }

        // ========== Permission Mode ==========
        LoopEvent::PermissionModeChanged { mode } => {
            state.session.permission_mode = mode;
            state.session.plan_mode = mode == cocode_protocol::PermissionMode::Plan;
        }

        // ========== Subagent Events ==========
        LoopEvent::SubagentSpawned {
            agent_id,
            agent_type,
            description,
            color,
        } => {
            state
                .session
                .start_subagent(agent_id, agent_type, description, color);
        }
        LoopEvent::SubagentProgress { agent_id, progress } => {
            state.session.update_subagent_progress(&agent_id, progress);
        }
        LoopEvent::SubagentCompleted { agent_id, result } => {
            state.session.complete_subagent(&agent_id, result);
            // Cleanup old completed subagents
            state.session.cleanup_completed_subagents(5);
        }
        LoopEvent::SubagentBackgrounded {
            agent_id,
            output_file,
        } => {
            state.session.background_subagent(&agent_id, output_file);
        }

        // ========== Errors ==========
        LoopEvent::Error { error } => {
            state.ui.query_timing.stop();
            state
                .ui
                .set_overlay(Overlay::Error(format!("{}: {}", error.code, error.message)));
        }
        LoopEvent::Interrupted => {
            // Stop streaming and timing if active
            state.ui.stop_streaming();
            state.ui.query_timing.stop();
            tracing::info!("Operation interrupted");
        }

        // ========== Context/Compaction ==========
        LoopEvent::ContextUsageWarning {
            percent_left,
            estimated_tokens,
            warning_threshold,
        } => {
            // Format tokens with k/M suffix
            let format_tokens = |n: i32| -> String {
                if n >= 1_000_000 {
                    format!("{:.1}M", n as f64 / 1_000_000.0)
                } else if n >= 1_000 {
                    format!("{:.0}k", n as f64 / 1_000.0)
                } else {
                    n.to_string()
                }
            };
            // Calculate remaining tokens from threshold vs used
            let remain = format_tokens(warning_threshold.saturating_sub(estimated_tokens));
            let total = format_tokens(warning_threshold);
            let percent = (percent_left * 100.0) as i32;
            let msg = t!(
                "toast.context_warning",
                percent = percent,
                remain = remain,
                total = total
            )
            .to_string();
            state.ui.toast_warning(msg);
            tracing::debug!(percent_left, "Context usage warning");
        }
        LoopEvent::CompactionStarted => {
            state.ui.toast_info(t!("toast.compacting").to_string());
            state.session.is_compacting = true;
            tracing::debug!("Compaction started");
        }
        LoopEvent::CompactionCompleted {
            removed_messages,
            summary_tokens,
        } => {
            let msg = t!(
                "toast.compacted",
                messages = removed_messages,
                tokens = summary_tokens
            )
            .to_string();
            state.ui.toast_success(msg);
            state.session.is_compacting = false;
            tracing::info!(removed_messages, summary_tokens, "Compaction completed");
        }
        LoopEvent::CompactionFailed { error, .. } => {
            state
                .ui
                .toast_error(t!("toast.compaction_failed", error = error).to_string());
            state.session.is_compacting = false;
            tracing::error!(error, "Compaction failed");
        }

        // ========== Model Fallback ==========
        LoopEvent::ModelFallbackStarted { from, to, reason } => {
            let msg = t!("toast.model_fallback", from = from, to = to).to_string();
            state.ui.toast_warning(msg);
            state.session.fallback_model = Some(to.clone());
            tracing::info!(from, to, reason, "Model fallback started");
        }
        LoopEvent::ModelFallbackCompleted => {
            state.session.fallback_model = None;
            tracing::debug!("Model fallback completed");
        }

        // ========== Queue ==========
        LoopEvent::CommandQueued { id, preview } => {
            tracing::debug!(id, preview, "Command queued (from core)");
        }
        LoopEvent::CommandDequeued { id } => {
            // Remove from local queue if present
            state.session.queued_commands.retain(|c| c.id != id);
            tracing::debug!(id, "Command dequeued");
        }
        LoopEvent::QueueStateChanged { queued } => {
            tracing::debug!(queued, "Queue state changed");
        }

        // ========== MCP Events ==========
        LoopEvent::McpStartupUpdate { server, status } => {
            use cocode_protocol::McpStartupStatus;
            match status {
                McpStartupStatus::Ready => {
                    state
                        .ui
                        .toast_success(t!("toast.mcp_ready", server = server).to_string());
                }
                McpStartupStatus::Failed => {
                    state
                        .ui
                        .toast_error(t!("toast.mcp_failed", server = server).to_string());
                }
                _ => {}
            }
        }
        LoopEvent::McpStartupComplete { servers, failed } => {
            if !servers.is_empty() {
                let count = servers.len();
                state
                    .ui
                    .toast_success(t!("toast.mcp_connected", count = count).to_string());
            }
            for (name, error) in failed {
                state
                    .ui
                    .toast_error(t!("toast.mcp_error", name = name, error = error).to_string());
            }
        }

        // ========== Plugin Data ==========
        LoopEvent::PluginDataReady {
            installed,
            marketplaces,
        } => {
            use crate::state::MarketplaceSummary;
            use crate::state::PluginSummary;

            let installed_items: Vec<PluginSummary> = installed
                .into_iter()
                .map(|p| PluginSummary {
                    name: p.name,
                    description: p.description,
                    version: p.version,
                    enabled: p.enabled,
                    scope: p.scope,
                    skills_count: p.skills_count,
                    hooks_count: p.hooks_count,
                    agents_count: p.agents_count,
                })
                .collect();
            let marketplace_items: Vec<MarketplaceSummary> = marketplaces
                .into_iter()
                .map(|m| MarketplaceSummary {
                    name: m.name,
                    source_type: m.source_type,
                    source: m.source,
                    auto_update: m.auto_update,
                    plugin_count: m.plugin_count,
                })
                .collect();
            state.ui.set_overlay(Overlay::PluginManager(
                crate::state::PluginManagerOverlay::new(
                    installed_items,
                    marketplace_items,
                    Vec::new(),
                ),
            ));
        }

        // ========== Output Styles ==========
        LoopEvent::OutputStylesReady { styles } => {
            use crate::state::OutputStylePickerItem;

            let items: Vec<OutputStylePickerItem> = styles
                .into_iter()
                .map(|s| OutputStylePickerItem {
                    name: s.name,
                    source: s.source,
                    description: s.description,
                })
                .collect();
            if items.is_empty() {
                state
                    .ui
                    .toast_info("No output styles available.".to_string());
            } else {
                state.ui.set_overlay(Overlay::OutputStylePicker(
                    crate::state::OutputStylePickerOverlay::new(items),
                ));
            }
        }

        // ========== Rewind ==========
        LoopEvent::RewindCompleted {
            rewound_turn,
            restored_files,
            mode,
            restored_prompt,
            ..
        } => {
            use crate::i18n::t;
            use cocode_protocol::RewindMode;

            // Remove TUI chat messages if conversation was rewound
            if mode != RewindMode::CodeOnly {
                while let Some(msg) = state.session.messages.last() {
                    if msg.turn_number.is_some_and(|n| n >= rewound_turn) {
                        state.session.messages.pop();
                    } else {
                        break;
                    }
                }
            }

            // Restore the original prompt into the input field
            if let Some(prompt) = restored_prompt
                && !prompt.is_empty()
            {
                state.ui.input.set_text(&prompt);
            }

            // Close the rewind overlay if open
            if matches!(state.ui.overlay, Some(Overlay::RewindSelector(_))) {
                state.ui.clear_overlay();
            }

            state.ui.toast_success(t!(
                "toast.rewind_success",
                turn = rewound_turn,
                files = restored_files
            ));
        }
        LoopEvent::RewindFailed { error } => {
            use crate::i18n::t;
            // Close the rewind overlay if open
            if matches!(state.ui.overlay, Some(Overlay::RewindSelector(_))) {
                state.ui.clear_overlay();
            }
            state
                .ui
                .toast_error(t!("toast.rewind_failed", error = error));
        }
        LoopEvent::RewindCheckpointsReady { checkpoints } => {
            use crate::i18n::t;
            if checkpoints.is_empty() {
                state.ui.toast_info(t!("toast.rewind_no_checkpoints"));
            } else {
                let mut overlay = crate::state::RewindSelectorOverlay::new(checkpoints);
                // Mark that the initial selection needs diff stats fetched.
                // The first navigation event (or confirm) will trigger the request.
                overlay.needs_initial_diff_stats = true;
                state.ui.set_overlay(Overlay::RewindSelector(overlay));
            }
        }
        LoopEvent::DiffStatsReady { turn_number, stats } => {
            // Update the matching checkpoint in the rewind selector overlay.
            if let Some(Overlay::RewindSelector(ref mut rw)) = state.ui.overlay {
                for cp in &mut rw.checkpoints {
                    if cp.turn_number == turn_number {
                        cp.diff_stats = Some(stats);
                        break;
                    }
                }
            }
        }

        // ========== Summarize ==========
        LoopEvent::SummarizeCompleted {
            from_turn,
            summary_tokens: _,
        } => {
            // Close the rewind overlay if open (loading state)
            if matches!(state.ui.overlay, Some(Overlay::RewindSelector(_))) {
                state.ui.clear_overlay();
            }
            state
                .ui
                .toast_success(t!("toast.summarize_success", turn = from_turn));
        }
        LoopEvent::SummarizeFailed { error } => {
            // Close the rewind overlay if open (loading state)
            if matches!(state.ui.overlay, Some(Overlay::RewindSelector(_))) {
                state.ui.clear_overlay();
            }
            state
                .ui
                .toast_error(t!("toast.summarize_failed", error = error));
        }

        // ========== Hooks ==========
        LoopEvent::HookExecuted {
            hook_type,
            hook_name,
        } => {
            tracing::debug!(
                hook_type = %hook_type,
                hook_name = %hook_name,
                "Hook executed"
            );
        }

        // Other events we don't need to handle in the TUI
        _ => {}
    }
}

/// Extract images from paste pills in question answers and embed them as `_images` metadata.
///
/// Scans each answer value for paste pills (e.g., `[Image #1]`). When found,
/// resolves them via the `PasteManager` to get base64-encoded image data and
/// injects an `_images` array into the answers JSON object.
fn embed_images_in_answers(answers: &mut serde_json::Value, paste_manager: &PasteManager) {
    let obj = match answers.as_object_mut() {
        Some(obj) => obj,
        None => return,
    };

    let mut images = Vec::new();
    for value in obj.values() {
        let text = match value.as_str() {
            Some(t) => t,
            None => continue,
        };
        for block in paste_manager.resolve_to_blocks(text) {
            if let cocode_api::UserContentPart::File(file_part) = block
                && file_part.media_type.starts_with("image/")
            {
                let data = file_part.data.to_base64();
                images.push(serde_json::json!({
                    "data": data,
                    "media_type": file_part.media_type,
                }));
            }
        }
    }

    if !images.is_empty() {
        obj.insert("_images".to_string(), serde_json::Value::Array(images));
    }
}

#[cfg(test)]
#[path = "update.test.rs"]
mod tests;
