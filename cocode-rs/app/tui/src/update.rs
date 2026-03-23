//! State update functions.
//!
//! This module contains pure functions that update the application state
//! in response to events. Following the Elm Architecture pattern, these
//! functions take the current state and an event, and return the new state.

use cocode_protocol::RoleSelection;
use tokio::sync::mpsc;

use crate::command::UserCommand;
use crate::constants;
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
            let mode = state.session.permission_mode;
            state
                .ui
                .toast_info(t!("toast.permission_mode", mode = format!("{mode:?}")).to_string());
            let _ = command_tx
                .send(UserCommand::SetPermissionMode { mode })
                .await;
        }
        TuiCommand::CycleThinkingLevel => {
            state.cycle_thinking_level();
            if let Some(ref sel) = state.session.current_selection {
                let level = sel.effective_thinking_level();
                state.ui.toast_info(
                    t!(
                        "toast.thinking_level",
                        level = format!("{:?}", level.effort)
                    )
                    .to_string(),
                );
                let _ = command_tx
                    .send(UserCommand::SetThinkingLevel { level })
                    .await;
            }
        }
        TuiCommand::CycleModel => {
            if !available_models.is_empty() {
                let current = state
                    .session
                    .current_selection
                    .as_ref()
                    .map(|s| s.model.slug.clone());
                state.ui.set_overlay(Overlay::ModelPicker(
                    ModelPickerOverlay::new(available_models.to_vec()).with_current(current),
                ));
            }
        }
        TuiCommand::ShowModelPicker => {
            if !available_models.is_empty() {
                let current = state
                    .session
                    .current_selection
                    .as_ref()
                    .map(|s| s.model.slug.clone());
                state.ui.set_overlay(Overlay::ModelPicker(
                    ModelPickerOverlay::new(available_models.to_vec()).with_current(current),
                ));
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
            state.ui.toast_info(t!("toast.interrupted").to_string());
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
            } else if let Some(Overlay::Elicitation(ref elicit)) = state.ui.overlay {
                // Cancelling elicitation must send "cancel" response so the MCP
                // server's blocked Promise resolves — just closing the overlay
                // would hang the server indefinitely.
                let request_id = elicit.request_id.clone();
                let _ = command_tx
                    .send(UserCommand::ElicitationResponse {
                        request_id,
                        action: "cancel".to_string(),
                        content: None,
                    })
                    .await;
                state.ui.clear_overlay();
            } else if state.has_overlay() {
                state.ui.clear_overlay();
            } else if !state.ui.input.is_empty() {
                state.ui.input.take();
            } else if !state.session.queued_commands.is_empty() {
                // Pop queued commands back into input for editing
                // (matches Claude Code's cancel Branch 3 behavior)
                let merged: String = state
                    .session
                    .queued_commands
                    .drain(..)
                    .map(|c| c.prompt)
                    .collect::<Vec<_>>()
                    .join("\n");
                state.ui.input.set_text(&merged);
                let _ = command_tx.send(UserCommand::ClearQueues).await;
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
            state.ui.scroll_by(constants::SCROLL_LINE_STEP);
        }
        TuiCommand::ScrollDown => {
            state.ui.scroll_by(-constants::SCROLL_LINE_STEP);
        }
        TuiCommand::PageUp => {
            state.ui.scroll_by(constants::SCROLL_PAGE_STEP);
        }
        TuiCommand::PageDown => {
            state.ui.scroll_by(-constants::SCROLL_PAGE_STEP);
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
                Some(Overlay::Question(q)) => {
                    // Space toggles multi-select; all other chars consumed by overlay
                    if c == ' ' && q.current().is_some_and(|qi| qi.multi_select) {
                        q.toggle_selected();
                    }
                }
                Some(Overlay::Elicitation(elicit)) => {
                    // Space toggles boolean/select/multiselect fields
                    if c == ' ' {
                        elicit.toggle_or_cycle();
                    } else {
                        elicit.insert_char(c);
                    }
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
            Some(Overlay::Elicitation(elicit)) => {
                elicit.delete_char();
            }
            _ => {
                state.ui.input.delete_backward();
            }
        },
        TuiCommand::DeleteForward => {
            if let Some(Overlay::SessionBrowser(_)) = state.ui.overlay {
                state.ui.toast_info(
                    t!("toast.unsupported_command", name = "delete-session").to_string(),
                );
            } else {
                state.ui.input.delete_forward();
            }
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
                Some(Overlay::Elicitation(elicit)) => {
                    elicit.move_up();
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
                Some(Overlay::Elicitation(elicit)) => {
                    elicit.move_down();
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
        TuiCommand::KillToEndOfLine => {
            state.ui.input.kill_to_end_of_line();
        }
        TuiCommand::Yank => {
            state.ui.input.yank();
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
            } else if let Some(Overlay::Elicitation(ref elicit)) = state.ui.overlay {
                // Submit elicitation form
                let request_id = elicit.request_id.clone();
                let content = elicit.collect_values();
                let _ = command_tx
                    .send(UserCommand::ElicitationResponse {
                        request_id,
                        action: "accept".to_string(),
                        content: Some(content),
                    })
                    .await;
                state.ui.clear_overlay();
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
            } else if let Some(Overlay::Elicitation(ref elicit)) = state.ui.overlay {
                let request_id = elicit.request_id.clone();
                let _ = command_tx
                    .send(UserCommand::ElicitationResponse {
                        request_id,
                        action: "decline".to_string(),
                        content: None,
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
        // OpenExternalEditor is intercepted in app.rs before reaching here.
        TuiCommand::OpenExternalEditor => {}

        // ========== Select All ==========
        TuiCommand::SelectAll => {
            state.ui.input.select_all();
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
                                    state.ui.toast_error(
                                        t!("toast.plan_save_failed", error = e).to_string(),
                                    );
                                } else {
                                    state.ui.toast_success(t!("toast.plan_saved").to_string());
                                }
                            }
                        }
                        Err(e) => {
                            state
                                .ui
                                .toast_error(t!("toast.editor_error", error = e).to_string());
                        }
                    }
                } else {
                    state.ui.toast_info(t!("toast.no_plan_file").to_string());
                }
            } else {
                state
                    .ui
                    .toast_info(t!("toast.plan_editor_hint").to_string());
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
            // Session listing requires core SessionManager integration.
            // For now, show an informational toast.
            state
                .ui
                .toast_info(t!("toast.sessions_unavailable").to_string());
        }
        TuiCommand::LoadSession(_session_id) => {
            state
                .ui
                .toast_info(t!("toast.sessions_unavailable").to_string());
        }
        TuiCommand::DeleteSession(_session_id) => {
            state
                .ui
                .toast_info(t!("toast.sessions_unavailable").to_string());
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
                state
                    .ui
                    .toast_info("No running tasks to background".to_string());
            }
        }

        // ========== Kill All Agents ==========
        TuiCommand::KillAllAgents => {
            let _ = command_tx.send(UserCommand::KillAllAgents).await;
            state
                .ui
                .toast_info(t!("toast.killing_all_agents").to_string());
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

        // ========== System Reminders ==========
        TuiCommand::ToggleSystemReminders => {
            state.ui.toggle_system_reminders();
            let key = if state.ui.show_system_reminders {
                "toast.system_reminders_shown"
            } else {
                "toast.system_reminders_hidden"
            };
            state.ui.toast_info(t!(key).to_string());
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
                .toast_info(t!("toast.unsupported_command", name = local_cmd.name).to_string());
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
            state
                .ui
                .toast_info(t!("toast.sessions_unavailable").to_string());
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
        CommandAction::KillAllAgents => {
            let _ = command_tx.send(UserCommand::KillAllAgents).await;
            state
                .ui
                .toast_info(t!("toast.killing_all_agents").to_string());
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
            name: t!("palette.kill_all_agents").to_string(),
            description: t!("palette.kill_all_agents_desc").to_string(),
            shortcut: Some("Ctrl+F".to_string()),
            action: CommandAction::KillAllAgents,
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

// `handle_agent_event` is defined in `agent_event_handler.rs` for module
// size management. Re-exported here for backward compatibility with
// existing callers (e.g., `app.rs`).
pub use crate::agent_event_handler::handle_agent_event;

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
