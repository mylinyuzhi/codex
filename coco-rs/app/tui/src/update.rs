//! TUI update handler — the Update in TEA.
//!
//! Pure function that applies [`TuiCommand`]s to [`AppState`].
//! Side effects (sending to core) are done via the command channel.

use tokio::sync::mpsc;

use crate::command::UserCommand;
use crate::constants;
use crate::events::TuiCommand;
use crate::state::AppState;
use crate::state::FocusTarget;
use crate::state::Overlay;
use crate::state::ui::CommandOption;
use crate::state::ui::CommandPaletteOverlay;
use crate::state::ui::ExportFormat;
use crate::state::ui::ExportOverlay;
use crate::state::ui::GlobalSearchOverlay;
use crate::state::ui::ModelOption;
use crate::state::ui::ModelPickerOverlay;
use crate::state::ui::QuickOpenOverlay;
use crate::state::ui::SessionBrowserOverlay;
use crate::state::ui::SessionOption;
use crate::update_rewind;

/// Apply a TUI command to the state.
///
/// Returns `true` if the state changed and a redraw is needed.
pub async fn handle_command(
    state: &mut AppState,
    cmd: TuiCommand,
    command_tx: &mpsc::Sender<UserCommand>,
) -> bool {
    match cmd {
        // ── Mode toggles ──
        TuiCommand::TogglePlanMode => {
            state.session.plan_mode = !state.session.plan_mode;
            let _ = command_tx
                .send(UserCommand::SetPlanMode {
                    active: state.session.plan_mode,
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
            // Use available_models if populated, else show current model
            let model_list = if state.session.available_models.is_empty() {
                vec![state.session.model.clone()]
            } else {
                state.session.available_models.clone()
            };
            let models: Vec<ModelOption> = model_list
                .iter()
                .map(|m| ModelOption {
                    id: m.clone(),
                    label: m.clone(),
                    description: None,
                })
                .collect();
            let selected = models
                .iter()
                .position(|m| m.id == state.session.model)
                .unwrap_or(0) as i32;
            state
                .ui
                .set_overlay(Overlay::ModelPicker(ModelPickerOverlay {
                    models,
                    filter: String::new(),
                    selected,
                }));
            true
        }
        TuiCommand::ToggleFastMode => {
            state.session.fast_mode = !state.session.fast_mode;
            let _ = command_tx.send(UserCommand::ToggleFastMode).await;
            true
        }

        // ── Input actions ──
        TuiCommand::SubmitInput => {
            let text = state.ui.input.take_input();
            if !text.is_empty() {
                let trimmed = text.trim();
                // Intercept /rewind and /checkpoint to open overlay.
                // TS: /rewind command opens MessageSelector.
                if trimmed == "/rewind"
                    || trimmed == "/checkpoint"
                    || trimmed.starts_with("/rewind ")
                    || trimmed.starts_with("/checkpoint ")
                {
                    let mut overlay = update_rewind::build_rewind_overlay(state);
                    if overlay.messages.is_empty() {
                        state
                            .ui
                            .add_toast(crate::state::ui::Toast::info("No messages to rewind to"));
                    } else {
                        // Parse optional argument: /rewind N or /rewind last
                        let arg = trimmed.split_once(' ').map(|(_, a)| a.trim()).unwrap_or("");
                        if arg == "last" {
                            overlay.selected = overlay.messages.len().saturating_sub(1) as i32;
                        } else if let Ok(n) = arg.parse::<i32>() {
                            // Select turn N (1-based)
                            let idx = (n - 1).clamp(0, overlay.messages.len() as i32 - 1);
                            overlay.selected = idx;
                        }
                        state.ui.set_overlay(Overlay::Rewind(overlay));
                    }
                    return true;
                }
                state.ui.input.add_to_history(text.clone());
                // Resolve paste pills: expand text pills, extract image data.
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
            }
            true
        }
        TuiCommand::QueueInput => {
            let text = state.ui.input.take_input();
            if !text.is_empty() {
                state.session.queued_commands.push(text.clone());
                let _ = command_tx
                    .send(UserCommand::QueueCommand { prompt: text })
                    .await;
            }
            true
        }
        TuiCommand::Interrupt => {
            state.session.was_interrupted = true;
            let _ = command_tx.send(UserCommand::Interrupt).await;
            true
        }
        TuiCommand::Cancel => {
            // Rewind overlay: Esc in RestoreOptions goes back to MessageSelect.
            if let Some(Overlay::Rewind(r)) = &mut state.ui.overlay
                && !update_rewind::handle_rewind_cancel(r)
            {
                return true; // Went back a phase, don't dismiss
            }
            if state.has_overlay() {
                state.ui.dismiss_overlay();
            }
            true
        }
        TuiCommand::ClearScreen => {
            state.session.messages.clear();
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
            while state.ui.input.cursor > 0 {
                let byte_pos = state
                    .ui
                    .input
                    .text
                    .char_indices()
                    .nth((state.ui.input.cursor - 1) as usize)
                    .map(|(_, c)| c);
                if byte_pos.is_none_or(char::is_whitespace) {
                    break;
                }
                state.ui.input.backspace();
            }
            true
        }
        TuiCommand::DeleteWordForward => {
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
            true
        }
        TuiCommand::KillToEndOfLine => {
            let cursor = state.ui.input.cursor as usize;
            let text = &state.ui.input.text;
            let byte_start = text
                .char_indices()
                .nth(cursor)
                .map(|(i, _)| i)
                .unwrap_or(text.len());
            // Kill to end of current line (or to next newline)
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
            true
        }
        TuiCommand::Yank => {
            if !state.ui.kill_ring.is_empty() {
                let yank_text = state.ui.kill_ring.clone();
                for c in yank_text.chars() {
                    state.ui.input.insert_char(c);
                }
            }
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
            if let Some(idx) = state.ui.input.history_index {
                if idx > 0 {
                    state.ui.input.history_index = Some(idx - 1);
                    state.ui.input.text = state.ui.input.history[idx as usize - 1].clone();
                    state.ui.input.cursor_end();
                }
            } else if !state.ui.input.history.is_empty() {
                let idx = state.ui.input.history.len() as i32 - 1;
                state.ui.input.history_index = Some(idx);
                state.ui.input.text = state.ui.input.history[idx as usize].clone();
                state.ui.input.cursor_end();
            }
            true
        }
        TuiCommand::CursorDown => {
            if let Some(idx) = state.ui.input.history_index {
                let max = state.ui.input.history.len() as i32 - 1;
                if idx < max {
                    state.ui.input.history_index = Some(idx + 1);
                    state.ui.input.text = state.ui.input.history[idx as usize + 1].clone();
                    state.ui.input.cursor_end();
                } else {
                    state.ui.input.history_index = None;
                    state.ui.input.text.clear();
                    state.ui.input.cursor = 0;
                }
            }
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
            true
        }
        TuiCommand::WordRight => {
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

        // ── Mouse ──
        TuiCommand::MouseScroll(delta) => {
            if delta > 0 {
                state.ui.scroll_offset += constants::MOUSE_SCROLL_LINES;
                state.ui.user_scrolled = true;
            } else {
                state.ui.scroll_offset =
                    (state.ui.scroll_offset - constants::MOUSE_SCROLL_LINES).max(0);
                if state.ui.scroll_offset == 0 {
                    state.ui.user_scrolled = false;
                }
            }
            true
        }
        TuiCommand::MouseClick { col: _, row: _ } => {
            // Click to position cursor in input area
            // For now just ensure focus is on input
            state.ui.focus = FocusTarget::Input;
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
            handle_overlay_approve(state, command_tx).await;
            true
        }
        TuiCommand::Deny => {
            handle_overlay_deny(state, command_tx).await;
            true
        }
        TuiCommand::ClassifierAutoApprove {
            request_id,
            matched_rule: _,
        } => {
            if let Some(Overlay::Permission(ref p)) = state.ui.overlay
                && p.request_id == request_id
            {
                let _ = command_tx
                    .send(UserCommand::ApprovalResponse {
                        request_id: p.request_id.clone(),
                        approved: true,
                        always_allow: false,
                        feedback: None,
                        updated_input: None,
                        permission_updates: vec![],
                    })
                    .await;
                state.ui.dismiss_overlay();
            }
            true
        }
        TuiCommand::ApproveAll => {
            if let Some(Overlay::Permission(ref p)) = state.ui.overlay
                && p.show_always_allow
            {
                let _ = command_tx
                    .send(UserCommand::ApprovalResponse {
                        request_id: p.request_id.clone(),
                        approved: true,
                        always_allow: true,
                        feedback: None,
                        updated_input: None,
                        permission_updates: vec![],
                    })
                    .await;
                state.ui.dismiss_overlay();
            }
            true
        }

        // ── Overlay navigation ──
        TuiCommand::OverlayFilter(c) => {
            handle_overlay_filter(state, c);
            true
        }
        TuiCommand::OverlayFilterBackspace => {
            handle_overlay_filter_backspace(state);
            true
        }
        TuiCommand::OverlayNext => {
            handle_overlay_nav(state, 1);
            request_diff_stats_if_rewind(state, command_tx).await;
            true
        }
        TuiCommand::OverlayPrev => {
            handle_overlay_nav(state, -1);
            request_diff_stats_if_rewind(state, command_tx).await;
            true
        }
        TuiCommand::OverlayConfirm => {
            handle_overlay_confirm(state, command_tx).await;
            true
        }

        // ── Commands & overlays ──
        TuiCommand::ShowHelp => {
            state.ui.set_overlay(Overlay::Help);
            true
        }
        TuiCommand::ShowCommandPalette => {
            // Use available_commands if populated, else show common defaults
            let cmd_list = if state.session.available_commands.is_empty() {
                vec![
                    ("help".to_string(), Some("Show help".to_string())),
                    ("clear".to_string(), Some("Clear conversation".to_string())),
                    ("compact".to_string(), Some("Compact context".to_string())),
                    ("config".to_string(), Some("Edit configuration".to_string())),
                    ("doctor".to_string(), Some("Run diagnostics".to_string())),
                    ("diff".to_string(), Some("Show changes".to_string())),
                    ("login".to_string(), Some("Login to provider".to_string())),
                    ("mcp".to_string(), Some("MCP server management".to_string())),
                    (
                        "session".to_string(),
                        Some("Session management".to_string()),
                    ),
                ]
            } else {
                state.session.available_commands.clone()
            };
            let commands: Vec<CommandOption> = cmd_list
                .iter()
                .map(|(name, desc)| CommandOption {
                    name: name.clone(),
                    description: desc.clone(),
                })
                .collect();
            state
                .ui
                .set_overlay(Overlay::CommandPalette(CommandPaletteOverlay {
                    commands,
                    filter: String::new(),
                    selected: 0,
                }));
            true
        }
        TuiCommand::ShowSessionBrowser => {
            let sessions: Vec<SessionOption> = state
                .session
                .saved_sessions
                .iter()
                .map(|s| SessionOption {
                    id: s.id.clone(),
                    label: s.label.clone(),
                    message_count: s.message_count,
                    created_at: s.created_at.clone(),
                })
                .collect();
            state
                .ui
                .set_overlay(Overlay::SessionBrowser(SessionBrowserOverlay {
                    sessions,
                    filter: String::new(),
                    selected: 0,
                }));
            true
        }
        TuiCommand::ShowGlobalSearch => {
            state
                .ui
                .set_overlay(Overlay::GlobalSearch(GlobalSearchOverlay {
                    query: String::new(),
                    results: Vec::new(),
                    selected: 0,
                    is_searching: false,
                }));
            true
        }
        TuiCommand::ShowQuickOpen => {
            state.ui.set_overlay(Overlay::QuickOpen(QuickOpenOverlay {
                filter: String::new(),
                files: Vec::new(),
                selected: 0,
            }));
            true
        }
        TuiCommand::ShowExport => {
            state.ui.set_overlay(Overlay::Export(ExportOverlay {
                formats: vec![
                    ExportFormat::Markdown,
                    ExportFormat::Json,
                    ExportFormat::Text,
                ],
                selected: 0,
            }));
            true
        }
        TuiCommand::ShowContextViz => {
            state.ui.set_overlay(Overlay::ContextVisualization);
            true
        }
        TuiCommand::ShowRewind => {
            let overlay = update_rewind::build_rewind_overlay(state);
            if overlay.messages.is_empty() {
                state
                    .ui
                    .add_toast(crate::state::ui::Toast::info("No messages to rewind to"));
            } else {
                // Request async diff stats for the selected message.
                // TS: MessageSelector useEffect loads diffStats on mount.
                if let Some(msg) = overlay.messages.last() {
                    let _ = command_tx
                        .send(UserCommand::RequestDiffStats {
                            message_id: msg.message_id.clone(),
                        })
                        .await;
                }
                state.ui.set_overlay(Overlay::Rewind(overlay));
            }
            true
        }
        TuiCommand::ShowDoctor => {
            state
                .ui
                .set_overlay(Overlay::Doctor(crate::state::ui::DoctorOverlay {
                    checks: Vec::new(),
                }));
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
        TuiCommand::OpenExternalEditor
        | TuiCommand::OpenPlanEditor
        | TuiCommand::PasteFromClipboard => true,

        // ── Application ──
        TuiCommand::Quit => {
            let _ = command_tx.send(UserCommand::Shutdown).await;
            state.quit();
            true
        }
    }
}

/// Handle approve action for current overlay.
async fn handle_overlay_approve(state: &mut AppState, command_tx: &mpsc::Sender<UserCommand>) {
    match &state.ui.overlay {
        Some(Overlay::Permission(p)) => {
            let _ = command_tx
                .send(UserCommand::ApprovalResponse {
                    request_id: p.request_id.clone(),
                    approved: true,
                    always_allow: false,
                    feedback: None,
                    updated_input: None,
                    permission_updates: vec![],
                })
                .await;
            state.ui.dismiss_overlay();
        }
        Some(Overlay::SandboxPermission(s)) => {
            let _ = command_tx
                .send(UserCommand::ApprovalResponse {
                    request_id: s.request_id.clone(),
                    approved: true,
                    always_allow: false,
                    feedback: None,
                    updated_input: None,
                    permission_updates: vec![],
                })
                .await;
            state.ui.dismiss_overlay();
        }
        Some(Overlay::McpServerApproval(m)) => {
            let _ = command_tx
                .send(UserCommand::ApprovalResponse {
                    request_id: m.request_id.clone(),
                    approved: true,
                    always_allow: false,
                    feedback: None,
                    updated_input: None,
                    permission_updates: vec![],
                })
                .await;
            state.ui.dismiss_overlay();
        }
        Some(Overlay::PlanEntry(_) | Overlay::PlanExit(_)) => {
            state.session.plan_mode = !state.session.plan_mode;
            let _ = command_tx
                .send(UserCommand::SetPlanMode {
                    active: state.session.plan_mode,
                })
                .await;
            state.ui.dismiss_overlay();
        }
        Some(Overlay::Trust(_)) => {
            state.ui.dismiss_overlay();
        }
        Some(Overlay::AutoModeOptIn(_)) => {
            state.ui.dismiss_overlay();
        }
        Some(Overlay::BypassPermissions(_)) => {
            state.ui.dismiss_overlay();
        }
        Some(Overlay::WorktreeExit(_)) => {
            state.ui.dismiss_overlay();
        }
        _ => {
            state.ui.dismiss_overlay();
        }
    }
}

/// Handle deny action for current overlay.
async fn handle_overlay_deny(state: &mut AppState, command_tx: &mpsc::Sender<UserCommand>) {
    match &state.ui.overlay {
        Some(Overlay::Permission(p)) => {
            let _ = command_tx
                .send(UserCommand::ApprovalResponse {
                    request_id: p.request_id.clone(),
                    approved: false,
                    always_allow: false,
                    feedback: None,
                    updated_input: None,
                    permission_updates: vec![],
                })
                .await;
            state.ui.dismiss_overlay();
        }
        Some(Overlay::SandboxPermission(s)) => {
            let _ = command_tx
                .send(UserCommand::ApprovalResponse {
                    request_id: s.request_id.clone(),
                    approved: false,
                    always_allow: false,
                    feedback: None,
                    updated_input: None,
                    permission_updates: vec![],
                })
                .await;
            state.ui.dismiss_overlay();
        }
        Some(Overlay::McpServerApproval(m)) => {
            let _ = command_tx
                .send(UserCommand::ApprovalResponse {
                    request_id: m.request_id.clone(),
                    approved: false,
                    always_allow: false,
                    feedback: None,
                    updated_input: None,
                    permission_updates: vec![],
                })
                .await;
            state.ui.dismiss_overlay();
        }
        _ => {
            state.ui.dismiss_overlay();
        }
    }
}

/// Filter text in filterable overlays.
fn handle_overlay_filter(state: &mut AppState, c: char) {
    match &mut state.ui.overlay {
        Some(Overlay::ModelPicker(m)) => {
            m.filter.push(c);
            m.selected = 0;
        }
        Some(Overlay::CommandPalette(cp)) => {
            cp.filter.push(c);
            cp.selected = 0;
        }
        Some(Overlay::SessionBrowser(s)) => {
            s.filter.push(c);
            s.selected = 0;
        }
        Some(Overlay::GlobalSearch(g)) => {
            g.query.push(c);
            g.selected = 0;
        }
        Some(Overlay::QuickOpen(q)) => {
            q.filter.push(c);
            q.selected = 0;
        }
        _ => {}
    }
}

/// Delete filter char in filterable overlays.
fn handle_overlay_filter_backspace(state: &mut AppState) {
    match &mut state.ui.overlay {
        Some(Overlay::ModelPicker(m)) => {
            m.filter.pop();
            m.selected = 0;
        }
        Some(Overlay::CommandPalette(cp)) => {
            cp.filter.pop();
            cp.selected = 0;
        }
        Some(Overlay::SessionBrowser(s)) => {
            s.filter.pop();
            s.selected = 0;
        }
        Some(Overlay::GlobalSearch(g)) => {
            g.query.pop();
            g.selected = 0;
        }
        Some(Overlay::QuickOpen(q)) => {
            q.filter.pop();
            q.selected = 0;
        }
        _ => {}
    }
}

/// Navigate up/down in list overlays.
fn handle_overlay_nav(state: &mut AppState, delta: i32) {
    match &mut state.ui.overlay {
        Some(Overlay::ModelPicker(m)) => {
            let count = filtered_model_count(m);
            m.selected = (m.selected + delta).clamp(0, (count - 1).max(0));
        }
        Some(Overlay::CommandPalette(cp)) => {
            let count = filtered_command_count(cp);
            cp.selected = (cp.selected + delta).clamp(0, (count - 1).max(0));
        }
        Some(Overlay::SessionBrowser(s)) => {
            let count = filtered_session_count(s);
            s.selected = (s.selected + delta).clamp(0, (count - 1).max(0));
        }
        Some(Overlay::GlobalSearch(g)) => {
            let count = g.results.len() as i32;
            g.selected = (g.selected + delta).clamp(0, (count - 1).max(0));
        }
        Some(Overlay::QuickOpen(q)) => {
            let count = q.files.len() as i32;
            q.selected = (q.selected + delta).clamp(0, (count - 1).max(0));
        }
        Some(Overlay::Export(e)) => {
            let count = e.formats.len() as i32;
            e.selected = (e.selected + delta).clamp(0, (count - 1).max(0));
        }
        Some(Overlay::Question(q)) => {
            let count = q.options.len() as i32;
            q.selected = (q.selected + delta).clamp(0, (count - 1).max(0));
        }
        Some(Overlay::Feedback(f)) => {
            let count = f.options.len() as i32;
            f.selected = (f.selected + delta).clamp(0, (count - 1).max(0));
        }
        Some(Overlay::Rewind(r)) => {
            update_rewind::handle_rewind_nav(r, delta);
        }
        Some(Overlay::DiffView(d)) => {
            d.scroll = (d.scroll + delta * constants::SCROLL_LINE_STEP).max(0);
        }
        Some(Overlay::TaskDetail(t)) => {
            t.scroll = (t.scroll + delta * constants::SCROLL_LINE_STEP).max(0);
        }
        // Scrollable read-only overlays — scroll via help_scroll
        Some(
            Overlay::Help
            | Overlay::Doctor(_)
            | Overlay::ContextVisualization
            | Overlay::Bridge(_)
            | Overlay::InvalidConfig(_),
        ) => {
            state.ui.help_scroll =
                (state.ui.help_scroll + delta * constants::SCROLL_LINE_STEP).max(0);
        }
        _ => {}
    }
}

/// Confirm selection in list overlays.
async fn handle_overlay_confirm(state: &mut AppState, command_tx: &mpsc::Sender<UserCommand>) {
    let overlay = state.ui.overlay.take();
    match overlay {
        Some(Overlay::ModelPicker(m)) => {
            if let Some(model) = filtered_models(&m).get(m.selected as usize) {
                let _ = command_tx
                    .send(UserCommand::SetModel {
                        model: model.id.clone(),
                    })
                    .await;
                state.session.model = model.id.clone();
            }
        }
        Some(Overlay::CommandPalette(cp)) => {
            if let Some(cmd) = filtered_commands(&cp).get(cp.selected as usize) {
                // Intercept /rewind and /checkpoint to open overlay instead
                if cmd.name == "rewind" || cmd.name == "checkpoint" {
                    let overlay = update_rewind::build_rewind_overlay(state);
                    state.ui.overlay = Some(Overlay::Rewind(overlay));
                    return;
                }
                let _ = command_tx
                    .send(UserCommand::ExecuteSkill {
                        name: cmd.name.clone(),
                        args: None,
                    })
                    .await;
            }
        }
        Some(Overlay::Rewind(mut r)) => {
            if let Some((message_id, restore_type)) = update_rewind::handle_rewind_confirm(&mut r) {
                let _ = command_tx
                    .send(UserCommand::Rewind {
                        message_id,
                        restore_type,
                    })
                    .await;
            } else {
                // Phase transition; put overlay back.
                state.ui.overlay = Some(Overlay::Rewind(r));
                return;
            }
        }
        Some(Overlay::SessionBrowser(s)) => {
            if let Some(session) = filtered_sessions(&s).get(s.selected as usize) {
                let _ = command_tx
                    .send(UserCommand::SubmitInput {
                        content: format!("/resume {}", session.id),
                        display_text: None,
                        images: Vec::new(),
                    })
                    .await;
            }
        }
        Some(Overlay::Export(e)) => {
            if let Some(fmt) = e.formats.get(e.selected as usize) {
                let cmd = match fmt {
                    ExportFormat::Markdown => "/export markdown",
                    ExportFormat::Json => "/export json",
                    ExportFormat::Text => "/export text",
                };
                let _ = command_tx
                    .send(UserCommand::SubmitInput {
                        content: cmd.to_string(),
                        display_text: None,
                        images: Vec::new(),
                    })
                    .await;
            }
        }
        Some(Overlay::Question(q)) => {
            if let Some(option) = q.options.get(q.selected as usize) {
                let _ = command_tx
                    .send(UserCommand::ApprovalResponse {
                        request_id: q.request_id.clone(),
                        approved: true,
                        always_allow: false,
                        feedback: Some(option.clone()),
                        updated_input: None,
                        permission_updates: vec![],
                    })
                    .await;
            }
        }
        // All remaining overlays: confirm = dismiss
        Some(
            Overlay::Permission(_)
            | Overlay::Help
            | Overlay::Error(_)
            | Overlay::PlanExit(_)
            | Overlay::PlanEntry(_)
            | Overlay::CostWarning(_)
            | Overlay::Elicitation(_)
            | Overlay::SandboxPermission(_)
            | Overlay::GlobalSearch(_)
            | Overlay::QuickOpen(_)
            | Overlay::DiffView(_)
            | Overlay::McpServerApproval(_)
            | Overlay::WorktreeExit(_)
            | Overlay::Doctor(_)
            | Overlay::Bridge(_)
            | Overlay::InvalidConfig(_)
            | Overlay::IdleReturn(_)
            | Overlay::Trust(_)
            | Overlay::AutoModeOptIn(_)
            | Overlay::BypassPermissions(_)
            | Overlay::TaskDetail(_)
            | Overlay::Feedback(_)
            | Overlay::McpServerSelect(_)
            | Overlay::ContextVisualization,
        ) => {
            // Dismiss on confirm (Enter/Esc)
        }
        None => {}
    }
    // Next queued overlay
    state.ui.overlay = state.ui.overlay_queue.pop_front();
}

// ── Filter helpers ──

fn filtered_models(m: &ModelPickerOverlay) -> Vec<&ModelOption> {
    let filter_lower = m.filter.to_lowercase();
    m.models
        .iter()
        .filter(|model| {
            filter_lower.is_empty() || model.label.to_lowercase().contains(&filter_lower)
        })
        .collect()
}

fn filtered_model_count(m: &ModelPickerOverlay) -> i32 {
    filtered_models(m).len() as i32
}

fn filtered_commands(cp: &CommandPaletteOverlay) -> Vec<&CommandOption> {
    let filter_lower = cp.filter.to_lowercase();
    cp.commands
        .iter()
        .filter(|cmd| filter_lower.is_empty() || cmd.name.to_lowercase().contains(&filter_lower))
        .collect()
}

fn filtered_command_count(cp: &CommandPaletteOverlay) -> i32 {
    filtered_commands(cp).len() as i32
}

fn filtered_sessions(s: &SessionBrowserOverlay) -> Vec<&SessionOption> {
    let filter_lower = s.filter.to_lowercase();
    s.sessions
        .iter()
        .filter(|sess| filter_lower.is_empty() || sess.label.to_lowercase().contains(&filter_lower))
        .collect()
}

fn filtered_session_count(s: &SessionBrowserOverlay) -> i32 {
    filtered_sessions(s).len() as i32
}

/// Send RequestDiffStats for the currently selected message if a Rewind overlay is active.
///
/// Called after navigation (OverlayNext/Prev) so diff stats update when user
/// scrolls through messages. TS: MessageSelector useEffect re-computes on selectedIndex change.
async fn request_diff_stats_if_rewind(state: &AppState, command_tx: &mpsc::Sender<UserCommand>) {
    if let Some(Overlay::Rewind(ref r)) = state.ui.overlay {
        if let Some(msg) = r.messages.get(r.selected as usize) {
            let _ = command_tx
                .send(UserCommand::RequestDiffStats {
                    message_id: msg.message_id.clone(),
                })
                .await;
        }
    }
}
