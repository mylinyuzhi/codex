//! Rendering functions for the TUI.
//!
//! This module provides the main render function that draws the UI
//! based on the current application state.

use ratatui::Frame;
use ratatui::layout::Constraint;
use ratatui::layout::Direction;
use ratatui::layout::Layout;
use ratatui::layout::Rect;
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::text::Span;
use ratatui::widgets::Block;
use ratatui::widgets::Borders;
use ratatui::widgets::Clear;
use ratatui::widgets::Paragraph;
use ratatui::widgets::Wrap;

use crate::constants;
use crate::i18n::t;
use crate::state::AppState;
use crate::state::FocusTarget;
use crate::state::Overlay;
use crate::theme::Theme;
use crate::widgets::AgentSuggestionPopup;
use crate::widgets::ChatWidget;
use crate::widgets::FileSuggestionPopup;
use crate::widgets::HeaderBar;
use crate::widgets::InputWidget;
use crate::widgets::QueuedListWidget;
use crate::widgets::SkillSuggestionPopup;
use crate::widgets::StatusBar;
use crate::widgets::SubagentPanel;
use crate::widgets::SymbolSuggestionPopup;
use crate::widgets::ToastWidget;
use crate::widgets::ToolPanel;

/// Render the UI to the terminal frame.
///
/// This function is the main entry point for rendering. It layouts
/// the screen and draws all widgets based on the current state.
pub fn render(frame: &mut Frame, state: &AppState) {
    let area = frame.area();
    let theme = &state.ui.theme;

    // Main layout: Header (1) → Main Area (flex) → Status Bar (1)
    let main_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // Header bar
            Constraint::Min(1),    // Chat + Input + Tools
            Constraint::Length(1), // Status bar
        ])
        .split(area);

    // Header bar
    render_header_bar(frame, main_chunks[0], state, theme);

    // Upper area layout
    render_main_area(frame, main_chunks[1], state, theme);

    // Status bar
    render_status_bar(frame, main_chunks[2], state, theme);

    // Render overlay if present
    if let Some(ref overlay) = state.ui.overlay {
        render_overlay(frame, area, overlay, theme, state.ui.help_scroll);
    }

    // Render toast notifications (always on top)
    if state.ui.has_toasts() {
        render_toasts(frame, area, state, theme);
    }
}

/// Render the header bar at the top.
fn render_header_bar(frame: &mut Frame, area: Rect, state: &AppState, theme: &Theme) {
    let header = HeaderBar::new(theme)
        .session_id(state.session.session_id.as_deref())
        .working_dir(state.session.working_dir.as_deref())
        .turn_count(state.session.turn_count)
        .is_compacting(state.session.is_compacting)
        .fallback_model(state.session.fallback_model.as_deref())
        .active_worktrees(state.session.active_worktrees);
    frame.render_widget(header, area);
}

/// Render toast notifications in the top-right corner.
fn render_toasts(frame: &mut Frame, area: Rect, state: &AppState, theme: &Theme) {
    let toast_widget = ToastWidget::new(&state.ui.toasts, theme);
    let toast_area = toast_widget.calculate_area(area);
    if toast_area.width > 0 && toast_area.height > 0 {
        frame.render_widget(toast_widget, toast_area);
    }
}

/// Render the main content area (chat, tools, input).
fn render_main_area(frame: &mut Frame, area: Rect, state: &AppState, theme: &Theme) {
    // Show side panel whenever tools exist (running or recently completed),
    // including MCP tools, background tasks, and streaming tool uses.
    let has_tools = !state.session.tool_executions.is_empty()
        || !state.session.mcp_tool_calls.is_empty()
        || !state.session.background_tasks.is_empty()
        || state
            .ui
            .streaming
            .as_ref()
            .is_some_and(|s| !s.tool_uses.is_empty());

    // Check if we have subagents to show
    let has_subagents = !state.session.subagents.is_empty();

    // Responsive side panel: hide when terminal width < SIDE_PANEL_MIN_WIDTH
    if (has_tools || has_subagents) && area.width >= constants::SIDE_PANEL_MIN_WIDTH as u16 {
        // Responsive split ratio: wide terminals get a wider main area
        let (main_pct, side_pct) = if area.width >= constants::WIDE_TERMINAL_WIDTH as u16 {
            (
                constants::WIDE_TERMINAL_MAIN_PCT as u16,
                constants::WIDE_TERMINAL_SIDE_PCT as u16,
            )
        } else {
            (
                constants::NORMAL_TERMINAL_MAIN_PCT as u16,
                constants::NORMAL_TERMINAL_SIDE_PCT as u16,
            )
        };

        let horizontal = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(main_pct),
                Constraint::Percentage(side_pct),
            ])
            .split(area);

        render_chat_and_input(frame, horizontal[0], state, theme);
        render_side_panel(frame, horizontal[1], state, theme, has_tools, has_subagents);
    } else {
        // Just chat + input (no side panel or terminal too narrow)
        render_chat_and_input(frame, area, state, theme);
    }
}

/// Render the chat area and input box.
fn render_chat_and_input(frame: &mut Frame, area: Rect, state: &AppState, theme: &Theme) {
    // Calculate input height based on content
    let input_lines = state.ui.input.text().lines().count().max(1);
    let input_height = (input_lines as u16 + 2).min(constants::MAX_INPUT_HEIGHT as u16); // +2 for borders

    // Calculate queued list height (if any queued commands)
    let queued_list = QueuedListWidget::new(&state.session.queued_commands, theme);
    let queued_height = queued_list.required_height();

    let chunks = if queued_height > 0 {
        // Layout: Chat | Queued List | Input
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(1),                // Chat
                Constraint::Length(queued_height), // Queued list
                Constraint::Length(input_height),  // Input
            ])
            .split(area)
    } else {
        // Layout: Chat | Input (no queued commands)
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(1),               // Chat
                Constraint::Length(input_height), // Input
            ])
            .split(area)
    };

    // Chat widget
    let streaming_content = state
        .ui
        .streaming
        .as_ref()
        .map(crate::state::StreamingState::visible_content);
    let streaming_thinking = state.ui.streaming.as_ref().map(|s| s.thinking.as_str());
    let streaming_tool_uses = state
        .ui
        .streaming
        .as_ref()
        .map(|s| s.tool_uses.as_slice())
        .unwrap_or(&[]);

    let chat = ChatWidget::new(&state.session.messages, theme)
        .scroll(state.ui.scroll_offset)
        .streaming(streaming_content)
        .streaming_thinking(streaming_thinking)
        .show_thinking(state.ui.show_thinking)
        .is_thinking(state.ui.is_thinking())
        .spinner_frame(state.ui.spinner_frame())
        .thinking_duration(state.ui.thinking_duration())
        .collapsed_tools(&state.ui.collapsed_tools)
        .width(area.width)
        .user_scrolled(state.ui.user_scrolled)
        .streaming_tool_uses(streaming_tool_uses)
        .show_system_reminders(state.ui.show_system_reminders)
        .transcript_mode(state.ui.transcript_mode);
    frame.render_widget(chat, chunks[0]);

    // Queued list widget (if any queued commands)
    // Get the input chunk index based on whether queued list is shown
    let input_chunk_index = if queued_height > 0 {
        // Render queued list
        let queued_list = QueuedListWidget::new(&state.session.queued_commands, theme);
        frame.render_widget(queued_list, chunks[1]);
        2 // Input is at index 2
    } else {
        1 // Input is at index 1
    };

    // Input widget
    let placeholder = t!("input.placeholder").to_string();
    let input = InputWidget::new(&state.ui.input, theme)
        .focused(state.ui.focus == FocusTarget::Input)
        .plan_mode(state.session.plan_mode)
        .queued_count(state.session.queued_count())
        .placeholder(&placeholder)
        .is_streaming(state.is_streaming());
    frame.render_widget(input, chunks[input_chunk_index]);

    // Suggestion popups are mutually exclusive — only render one at a time.
    // Priority: skill > agent > symbol > file (matches key event handling order).
    // Don't render suggestions when an overlay is active or in plan mode.
    if state.ui.overlay.is_none() && !state.session.plan_mode {
        if let Some(ref suggestions) = state.ui.skill_suggestions {
            let popup = SkillSuggestionPopup::new(suggestions, theme);
            let popup_area = popup.calculate_area(chunks[input_chunk_index], area.height);
            frame.render_widget(popup, popup_area);
        } else if let Some(ref suggestions) = state.ui.agent_suggestions {
            let popup = AgentSuggestionPopup::new(suggestions, theme);
            let popup_area = popup.calculate_area(chunks[input_chunk_index], area.height);
            frame.render_widget(popup, popup_area);
        } else if let Some(ref suggestions) = state.ui.symbol_suggestions {
            let popup = SymbolSuggestionPopup::new(suggestions, theme);
            let popup_area = popup.calculate_area(chunks[input_chunk_index], area.height);
            frame.render_widget(popup, popup_area);
        } else if let Some(ref suggestions) = state.ui.file_suggestions {
            let popup = FileSuggestionPopup::new(suggestions, theme);
            let popup_area = popup.calculate_area(chunks[input_chunk_index], area.height);
            frame.render_widget(popup, popup_area);
        }
    }
}

/// Render the side panel (tools and/or subagents).
fn render_side_panel(
    frame: &mut Frame,
    area: Rect,
    state: &AppState,
    theme: &Theme,
    has_tools: bool,
    has_subagents: bool,
) {
    if has_tools && has_subagents {
        // Split vertically between tools and subagents
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Percentage(constants::SIDE_PANEL_TOOL_PCT as u16),
                Constraint::Percentage(constants::SIDE_PANEL_SUBAGENT_PCT as u16),
            ])
            .split(area);

        render_tools(frame, chunks[0], state, theme);
        render_subagents(frame, chunks[1], state, theme);
    } else if has_tools {
        render_tools(frame, area, state, theme);
    } else if has_subagents {
        render_subagents(frame, area, state, theme);
    }
}

/// Render the tools panel.
fn render_tools(frame: &mut Frame, area: Rect, state: &AppState, theme: &Theme) {
    let streaming_tools = state
        .ui
        .streaming
        .as_ref()
        .map(|s| s.tool_uses.as_slice())
        .unwrap_or(&[]);
    let panel = ToolPanel::new(&state.session.tool_executions, theme)
        .max_display(constants::MAX_TOOL_PANEL_DISPLAY as usize)
        .mcp_tool_calls(&state.session.mcp_tool_calls)
        .background_tasks(&state.session.background_tasks)
        .streaming_tools(streaming_tools);
    frame.render_widget(panel, area);
}

/// Render the subagents panel.
fn render_subagents(frame: &mut Frame, area: Rect, state: &AppState, theme: &Theme) {
    let panel = SubagentPanel::new(&state.session.subagents, theme)
        .max_display(constants::MAX_SUBAGENT_PANEL_DISPLAY);
    frame.render_widget(panel, area);
}

/// Render the status bar.
fn render_status_bar(frame: &mut Frame, area: Rect, state: &AppState, theme: &Theme) {
    let is_thinking = state.ui.is_thinking();
    let model_display = state
        .session
        .current_selection
        .as_ref()
        .map(|s| s.model.display_name.as_str())
        .unwrap_or_default();
    let effective_thinking = state
        .session
        .current_selection
        .as_ref()
        .map(cocode_protocol::RoleSelection::effective_thinking_level)
        .unwrap_or_default();

    let status_bar = StatusBar::new(
        model_display,
        &effective_thinking,
        state.session.plan_mode,
        &state.session.token_usage,
        theme,
    )
    .is_thinking(is_thinking)
    .show_thinking_enabled(state.ui.show_thinking)
    .thinking_duration(state.ui.thinking_duration())
    .plan_phase(state.session.plan_phase)
    .mcp_server_count(state.session.connected_mcp_count())
    .queue_counts(
        state.session.queued_count(),
        state.ui.queued_overlay_count(),
    )
    .context_window(
        state.session.context_window_used,
        state.session.context_window_total,
    )
    .estimated_cost(state.session.estimated_cost_cents)
    .working_dir(state.session.working_dir.as_deref())
    .output_style(state.session.output_style.as_deref())
    .spinner_text(state.ui.spinner_text.as_deref())
    .spinner_frame(state.ui.spinner_frame())
    .fast_mode(state.session.fast_mode)
    .sandbox_active(state.session.sandbox_active)
    .sandbox_violation_count(state.session.sandbox_violation_count);
    frame.render_widget(status_bar, area);
}

/// Render an overlay on top of the main content.
fn render_overlay(
    frame: &mut Frame,
    area: Rect,
    overlay: &Overlay,
    theme: &Theme,
    help_scroll: i32,
) {
    // Calculate centered area
    let overlay_width = match overlay {
        Overlay::PluginManager(_) => {
            (area.width * constants::PLUGIN_MANAGER_OVERLAY_WIDTH_PCT as u16 / 100).clamp(50, 100)
        }
        _ => (area.width * constants::DEFAULT_OVERLAY_WIDTH_PCT as u16 / 100).clamp(
            constants::DEFAULT_OVERLAY_MIN_WIDTH as u16,
            constants::DEFAULT_OVERLAY_MAX_WIDTH as u16,
        ),
    };
    let overlay_height = match overlay {
        Overlay::CostWarning(cw) => {
            // Title + cost + threshold + optional budget + blank + acknowledge + hints
            let base = 7_u16;
            if cw.budget_cents.is_some() {
                base + 1
            } else {
                base
            }
        }
        Overlay::SandboxPermission(_) => constants::SANDBOX_PERMISSION_OVERLAY_HEIGHT as u16,
        Overlay::Permission(_) => constants::PERMISSION_OVERLAY_HEIGHT as u16,
        Overlay::ModelPicker(picker) => (picker.filtered_items().len() as u16 + 4)
            .min(constants::MODEL_PICKER_MAX_HEIGHT as u16),
        Overlay::OutputStylePicker(picker) => (picker.filtered_items().len() as u16 + 4)
            .min(constants::MODEL_PICKER_MAX_HEIGHT as u16),
        Overlay::CommandPalette(palette) => (palette.filtered_commands().len() as u16 + 4)
            .min(constants::MODEL_PICKER_MAX_HEIGHT as u16),
        Overlay::SessionBrowser(browser) => (browser.filtered_sessions().len() as u16 + 4)
            .min(constants::MODEL_PICKER_MAX_HEIGHT as u16),
        Overlay::RewindSelector(rw) => {
            let item_count = match rw.phase {
                crate::state::RewindSelectorPhase::SelectCheckpoint => {
                    // Each checkpoint takes 1 line; only the selected one shows file names
                    let mut lines = rw.checkpoints.len();
                    if let Some(cp) = rw.selected_checkpoint()
                        && !cp.modified_files.is_empty()
                    {
                        lines += cp.modified_files.len().min(3);
                        if cp.modified_files.len() > 3 {
                            lines += 1; // "...and N more"
                        }
                    }
                    lines
                }
                crate::state::RewindSelectorPhase::SelectMode => 4, // 3 rewind modes + summarize
                crate::state::RewindSelectorPhase::InputSummarizeContext => 5, // header + input + hints
            };
            // +6 for header, spacing, warning line, and hints
            (item_count as u16 + 7).min(constants::REWIND_SELECTOR_MAX_HEIGHT as u16)
        }
        Overlay::PlanExitApproval(_) => constants::PLAN_EXIT_OVERLAY_HEIGHT as u16,
        Overlay::Question(q) => {
            // Height: title + question text + options + "Other" + hints + spacing
            let option_count = q.current().map_or(4, |qi| qi.options.len() as u16 + 1);
            (option_count + 8).min(constants::QUESTION_OVERLAY_MAX_HEIGHT as u16)
        }
        Overlay::Elicitation(elicit) => {
            let field_count = match &elicit.mode {
                crate::state::ElicitationMode::Form { fields } => fields.len() as u16,
                crate::state::ElicitationMode::Url { .. } => 2,
            };
            // Title + server name + message + fields + hints + spacing
            (field_count + 8).min(constants::QUESTION_OVERLAY_MAX_HEIGHT as u16)
        }
        Overlay::PluginManager(_) => {
            (area.height * constants::PLUGIN_MANAGER_HEIGHT_PCT as u16 / 100).clamp(20, 40)
        }
        Overlay::Help => constants::HELP_OVERLAY_HEIGHT as u16,
        Overlay::Error(_) => constants::ERROR_OVERLAY_HEIGHT as u16,
    };

    let x = (area.width.saturating_sub(overlay_width)) / 2;
    let y = (area.height.saturating_sub(overlay_height)) / 2;
    let overlay_area = Rect::new(x, y, overlay_width, overlay_height);

    // Clear the area behind the overlay
    frame.render_widget(Clear, overlay_area);

    match overlay {
        Overlay::CostWarning(cw) => render_cost_warning_overlay(frame, overlay_area, cw, theme),
        Overlay::SandboxPermission(sp) => {
            render_sandbox_permission_overlay(frame, overlay_area, sp, theme)
        }
        Overlay::Permission(perm) => render_permission_overlay(frame, overlay_area, perm, theme),
        Overlay::PlanExitApproval(plan_exit) => {
            render_plan_exit_overlay(frame, overlay_area, plan_exit, theme)
        }
        Overlay::ModelPicker(picker) => {
            render_model_picker_overlay(frame, overlay_area, picker, theme)
        }
        Overlay::OutputStylePicker(picker) => {
            render_output_style_picker_overlay(frame, overlay_area, picker, theme)
        }
        Overlay::CommandPalette(palette) => {
            render_command_palette_overlay(frame, overlay_area, palette, theme)
        }
        Overlay::SessionBrowser(browser) => {
            render_session_browser_overlay(frame, overlay_area, browser, theme)
        }
        Overlay::RewindSelector(rw) => {
            render_rewind_selector_overlay(frame, overlay_area, rw, theme)
        }
        Overlay::PluginManager(manager) => {
            render_plugin_manager_overlay(frame, overlay_area, manager, theme)
        }
        Overlay::Question(question) => {
            render_question_overlay(frame, overlay_area, question, theme)
        }
        Overlay::Elicitation(elicit) => {
            render_elicitation_overlay(frame, overlay_area, elicit, theme)
        }
        Overlay::Help => render_help_overlay(frame, overlay_area, theme, help_scroll),
        Overlay::Error(message) => render_error_overlay(frame, overlay_area, message, theme),
    }
}

/// Render the permission approval overlay.
fn render_permission_overlay(
    frame: &mut Frame,
    area: Rect,
    perm: &crate::state::PermissionOverlay,
    theme: &Theme,
) {
    let block = Block::default()
        .title(
            format!(" {} ", t!("dialog.permission_required"))
                .bold()
                .fg(theme.warning),
        )
        .borders(Borders::ALL)
        .border_style(ratatui::style::Style::default().fg(theme.warning));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    // Build content
    let mut lines: Vec<Line> = vec![];

    // Tool name
    lines.push(Line::from(vec![
        Span::raw(format!("{} ", t!("dialog.tool"))).bold(),
        Span::raw(&perm.request.tool_name).fg(theme.primary),
    ]));

    // Description (may contain newlines for multi-argument tools)
    for desc_line in perm.request.description.lines() {
        lines.push(Line::from(
            Span::raw(format!("  {desc_line}")).fg(theme.text_dim),
        ));
    }

    // Security risks
    for risk in &perm.request.risks {
        lines.push(Line::from(
            Span::raw(format!("  ⚠ {}", risk.message)).fg(theme.warning),
        ));
    }
    lines.push(Line::from(""));

    // Options
    let mut options: Vec<String> = vec![
        t!("dialog.approve").to_string(),
        t!("dialog.deny").to_string(),
        t!("dialog.approve_all").to_string(),
    ];
    // Show "allow similar" option with pattern when available
    if perm.request.allow_remember
        && let Some(ref pattern) = perm.request.proposed_prefix_pattern
    {
        options[2] = format!("{} ({})", t!("dialog.approve_all"), pattern);
    }
    for (i, opt) in options.iter().enumerate() {
        let is_selected = perm.selected == i as i32;
        let line = if is_selected {
            Line::from(Span::raw(format!("▸ {opt}")).bold().fg(theme.primary))
        } else {
            Line::from(Span::raw(format!("  {opt}")).fg(theme.text_dim))
        };
        lines.push(line);
    }

    let paragraph = Paragraph::new(lines).wrap(Wrap { trim: false });
    frame.render_widget(paragraph, inner);
}

/// Render the plan exit approval overlay with 4 options.
fn render_plan_exit_overlay(
    frame: &mut Frame,
    area: Rect,
    plan_exit: &crate::state::PlanExitOverlay,
    theme: &Theme,
) {
    let block = Block::default()
        .title(
            format!(" {} ", t!("dialog.plan_exit_title"))
                .bold()
                .fg(theme.primary),
        )
        .borders(Borders::ALL)
        .border_style(ratatui::style::Style::default().fg(theme.primary));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let mut lines: Vec<Line> = vec![];

    // Plan preview (truncated)
    if !plan_exit.plan_preview.is_empty() {
        let preview: String = plan_exit
            .plan_preview
            .lines()
            .take(3)
            .collect::<Vec<_>>()
            .join(" | ");
        let truncated = if preview.len() > 60 {
            format!("{}...", &preview[..57])
        } else {
            preview
        };
        lines.push(Line::from(Span::raw(truncated).fg(theme.text_dim).italic()));
        lines.push(Line::from(""));
    }

    // 5 options
    let options = [
        t!("dialog.plan_exit_clear_accept").to_string(),
        t!("dialog.plan_exit_clear_bypass").to_string(),
        t!("dialog.plan_exit_keep_elevate").to_string(),
        t!("dialog.plan_exit_keep_default").to_string(),
        t!("dialog.plan_exit_keep_planning").to_string(),
    ];
    for (i, opt) in options.iter().enumerate() {
        let is_selected = plan_exit.selected == i as i32;
        let line = if is_selected {
            Line::from(Span::raw(format!("▸ {opt}")).bold().fg(theme.primary))
        } else {
            Line::from(Span::raw(format!("  {opt}")).fg(theme.text_dim))
        };
        lines.push(line);
    }

    // Feedback text input (active when "keep planning" selected + Enter pressed)
    if plan_exit.feedback_active {
        lines.push(Line::from(""));
        lines.push(Line::from(
            Span::raw(t!("dialog.plan_exit_feedback_prompt").to_string()).fg(theme.text_dim),
        ));
        let input_line = if plan_exit.feedback_text.is_empty() {
            Line::from(
                Span::raw(format!(
                    "  > {}",
                    t!("dialog.plan_exit_feedback_placeholder")
                ))
                .dim()
                .italic(),
            )
        } else {
            // Truncate from the left if text exceeds available width
            let max_visible = (inner.width as usize).saturating_sub(6); // "  > " + "▌" + margin
            let display = if plan_exit.feedback_text.len() > max_visible {
                let start = plan_exit.feedback_text.len() - max_visible;
                format!("  > …{}▌", &plan_exit.feedback_text[start..])
            } else {
                format!("  > {}▌", plan_exit.feedback_text)
            };
            Line::from(Span::raw(display))
        };
        lines.push(input_line);
    }

    lines.push(Line::from(""));

    // Hints
    lines.push(Line::from(
        Span::raw(t!("dialog.plan_exit_hints").to_string()).fg(theme.text_dim),
    ));

    let paragraph = Paragraph::new(lines).wrap(Wrap { trim: false });
    frame.render_widget(paragraph, inner);
}

/// Render the question overlay (AskUserQuestion tool).
fn render_question_overlay(
    frame: &mut Frame,
    area: Rect,
    question_overlay: &crate::state::QuestionOverlay,
    theme: &Theme,
) {
    let block = Block::default()
        .title(format!(" {} ", t!("dialog.question_title")))
        .borders(Borders::ALL)
        .border_style(ratatui::style::Style::default().fg(theme.primary));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let mut lines = Vec::new();

    if let Some(q) = question_overlay.current() {
        // Question number indicator
        let total = question_overlay.questions.len();
        let current = question_overlay.current_question + 1;
        lines.push(Line::from(
            Span::raw(format!("[{}/{}] [{}]", current, total, q.header)).fg(theme.text_dim),
        ));
        lines.push(Line::from(""));

        // Question text
        lines.push(Line::from(Span::raw(&q.question).bold().fg(theme.text)));
        lines.push(Line::from(""));

        // Options
        for (i, opt) in q.options.iter().enumerate() {
            let is_selected = q.selected == i as i32;
            let prefix = if q.multi_select {
                if q.checked.get(i).copied().unwrap_or(false) {
                    "[x]"
                } else {
                    "[ ]"
                }
            } else if is_selected {
                " ▸ "
            } else {
                "   "
            };

            let label_text = format!("{prefix} {} — {}", opt.label, opt.description);
            let line = if is_selected {
                Line::from(Span::raw(label_text).bold().fg(theme.primary))
            } else {
                Line::from(Span::raw(label_text).fg(theme.text_dim))
            };
            lines.push(line);
        }

        // "Other" option
        let is_other_selected = q.selected as usize == q.options.len();
        if question_overlay.other_input_active {
            lines.push(Line::from(
                Span::raw(format!(
                    " ▸ {}: {}_",
                    t!("dialog.question_other"),
                    question_overlay.other_text
                ))
                .bold()
                .fg(theme.primary),
            ));
        } else if is_other_selected {
            lines.push(Line::from(
                Span::raw(format!(" ▸ {}", t!("dialog.question_other")))
                    .bold()
                    .fg(theme.primary),
            ));
        } else {
            lines.push(Line::from(
                Span::raw(format!("   {}", t!("dialog.question_other"))).fg(theme.text_dim),
            ));
        }
    } else {
        lines.push(Line::from(
            Span::raw("No questions to display").fg(theme.text_dim),
        ));
    }

    lines.push(Line::from(""));

    // Hints
    lines.push(Line::from(
        Span::raw(t!("dialog.question_hints").to_string()).fg(theme.text_dim),
    ));

    let paragraph = Paragraph::new(lines).wrap(Wrap { trim: false });
    frame.render_widget(paragraph, inner);
}

/// Render the elicitation overlay for MCP server input requests.
fn render_elicitation_overlay(
    frame: &mut Frame,
    area: Rect,
    elicit: &crate::state::ElicitationOverlay,
    theme: &Theme,
) {
    let block = Block::default()
        .title(
            format!(" MCP: {} ", elicit.server_name)
                .bold()
                .fg(theme.primary),
        )
        .borders(Borders::ALL)
        .border_style(ratatui::style::Style::default().fg(theme.primary));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let mut lines = Vec::new();

    // Message
    lines.push(Line::from(Span::raw(&elicit.message).bold().fg(theme.text)));
    lines.push(Line::from(""));

    match &elicit.mode {
        crate::state::ElicitationMode::Form { fields } => {
            for (i, field) in fields.iter().enumerate() {
                let is_selected = elicit.selected == i as i32;
                let required_marker = if field.required { "*" } else { "" };
                let prefix = if is_selected { " > " } else { "   " };

                let label_line = format!("{prefix}{}{required_marker}:", field.label);
                let label = if is_selected {
                    Span::raw(label_line).bold().fg(theme.primary)
                } else {
                    Span::raw(label_line).fg(theme.text_dim)
                };
                lines.push(Line::from(label));

                // Show current value
                let value_text = match &field.field_type {
                    crate::state::ElicitationFieldType::Text { value, .. } => {
                        if is_selected {
                            format!("     [{value}_]")
                        } else {
                            format!("     [{value}]")
                        }
                    }
                    crate::state::ElicitationFieldType::Number { value, .. } => {
                        if is_selected {
                            format!("     [{value}_]")
                        } else {
                            format!("     [{value}]")
                        }
                    }
                    crate::state::ElicitationFieldType::Select {
                        options, selected, ..
                    } => {
                        let current = selected
                            .and_then(|i| options.get(i as usize))
                            .map_or("-", |s| s.as_str());
                        format!("     [{current}]")
                    }
                    crate::state::ElicitationFieldType::Boolean { value, .. } => {
                        if *value {
                            "     [x] Yes".to_string()
                        } else {
                            "     [ ] No".to_string()
                        }
                    }
                    crate::state::ElicitationFieldType::MultiSelect { options, checked } => {
                        let selected: Vec<&str> = options
                            .iter()
                            .zip(checked.iter())
                            .filter(|&(_, &c)| c)
                            .map(|(opt, _)| opt.as_str())
                            .collect();
                        if selected.is_empty() {
                            "     (none selected)".to_string()
                        } else {
                            format!("     [{}]", selected.join(", "))
                        }
                    }
                };
                let value_color = if is_selected {
                    theme.text
                } else {
                    theme.text_dim
                };
                lines.push(Line::from(Span::raw(value_text).fg(value_color)));
            }
        }
        crate::state::ElicitationMode::Url { url } => {
            lines.push(Line::from(
                Span::raw("Open this URL in your browser:").fg(theme.text_dim),
            ));
            lines.push(Line::from(Span::raw(url).fg(theme.primary).underlined()));
        }
    }

    lines.push(Line::from(""));
    let hints = match &elicit.mode {
        crate::state::ElicitationMode::Url { .. } => "Enter: Continue | N: Decline | Esc: Cancel",
        crate::state::ElicitationMode::Form { .. } => {
            "Enter: Accept | N: Decline | Esc: Cancel | Up/Down: Navigate | Space: Toggle"
        }
    };
    lines.push(Line::from(Span::raw(hints).fg(theme.text_dim)));

    let paragraph = Paragraph::new(lines).wrap(Wrap { trim: false });
    frame.render_widget(paragraph, inner);
}

/// Render the model picker overlay.
fn render_model_picker_overlay(
    frame: &mut Frame,
    area: Rect,
    picker: &crate::state::ModelPickerOverlay,
    theme: &Theme,
) {
    let title = if picker.filter.is_empty() {
        format!(" {} ", t!("dialog.select_model"))
    } else {
        format!(
            " {} ",
            t!("dialog.select_model_filter", filter = &picker.filter)
        )
    };

    let block = Block::default()
        .title(title.bold())
        .borders(Borders::ALL)
        .border_style(ratatui::style::Style::default().fg(theme.border_focused));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    // Build model list
    let items = picker.filtered_items();
    let mut lines: Vec<Line> = vec![];

    for (i, selection) in items.iter().enumerate() {
        let is_selected = picker.selected == i as i32;
        let is_current = picker
            .current_slug
            .as_ref()
            .is_some_and(|s| s == &selection.model.slug);

        // Show display_name with provider prefix and thinking indicator
        let name = &selection.model.display_name;
        let provider = &selection.model.provider;
        let thinking_tag = selection
            .thinking_level
            .as_ref()
            .filter(|t| t.effort != cocode_protocol::ReasoningEffort::None)
            .map(|t| format!(" [{}]", t.effort))
            .unwrap_or_default();
        let current_tag = if is_current { " (current)" } else { "" };

        let prefix = if is_selected { "▸ " } else { "  " };
        let line = if is_selected {
            Line::from(vec![
                Span::raw(prefix).bold().fg(theme.primary),
                Span::raw(format!("{provider}/")).fg(theme.text_dim),
                Span::raw(name).bold().fg(theme.primary),
                Span::raw(thinking_tag).fg(theme.thinking),
                Span::raw(current_tag).fg(theme.success),
            ])
        } else if is_current {
            Line::from(vec![
                Span::raw(prefix),
                Span::raw(format!("{provider}/")).fg(theme.text_dim),
                Span::raw(name).fg(theme.success),
                Span::raw(thinking_tag).fg(theme.thinking),
                Span::raw(current_tag).fg(theme.success),
            ])
        } else {
            Line::from(vec![
                Span::raw(prefix),
                Span::raw(format!("{provider}/")).fg(theme.text_dim),
                Span::raw(name.to_string()),
                Span::raw(thinking_tag).fg(theme.thinking),
            ])
        };
        lines.push(line);
    }

    if items.is_empty() {
        lines.push(Line::from(
            Span::raw(t!("dialog.no_models_match").to_string())
                .fg(theme.text_dim)
                .italic(),
        ));
    }

    // Add hints at bottom
    lines.push(Line::from(""));
    lines.push(Line::from(
        Span::raw(t!("dialog.model_picker_hints").to_string()).fg(theme.text_dim),
    ));

    let paragraph = Paragraph::new(lines).wrap(Wrap { trim: false });
    frame.render_widget(paragraph, inner);
}

/// Render the output style picker overlay.
fn render_output_style_picker_overlay(
    frame: &mut Frame,
    area: Rect,
    picker: &crate::state::OutputStylePickerOverlay,
    theme: &Theme,
) {
    let title = if picker.filter.is_empty() {
        format!(" {} ", t!("dialog.select_output_style"))
    } else {
        format!(
            " {} ",
            t!("dialog.select_output_style_filter", filter = &picker.filter)
        )
    };

    let block = Block::default()
        .title(title.bold())
        .borders(Borders::ALL)
        .border_style(ratatui::style::Style::default().fg(theme.border_focused));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let items = picker.filtered_items();
    let mut lines: Vec<Line> = vec![];

    for (i, item) in items.iter().enumerate() {
        let desc = item
            .description
            .as_deref()
            .map(|d| format!(" - {d}"))
            .unwrap_or_default();
        let display = format!("{} [{}]{desc}", item.name, item.source);
        let is_selected = picker.selected == i as i32;
        let line = if is_selected {
            Line::from(Span::raw(format!("▸ {display}")).bold().fg(theme.primary))
        } else {
            Line::from(Span::raw(format!("  {display}")))
        };
        lines.push(line);
    }

    if items.is_empty() {
        lines.push(Line::from(
            Span::raw(t!("dialog.no_styles_match").to_string())
                .fg(theme.text_dim)
                .italic(),
        ));
    }

    // Add hints at bottom
    lines.push(Line::from(""));
    lines.push(Line::from(
        Span::raw(t!("dialog.output_style_picker_hints").to_string()).fg(theme.text_dim),
    ));

    let paragraph = Paragraph::new(lines).wrap(Wrap { trim: false });
    frame.render_widget(paragraph, inner);
}

/// Render the help overlay with categorized shortcuts.
fn render_help_overlay(frame: &mut Frame, area: Rect, theme: &Theme, scroll_offset: i32) {
    let block = Block::default()
        .title(format!(" {} ", t!("dialog.keyboard_shortcuts")).bold())
        .borders(Borders::ALL)
        .border_style(ratatui::style::Style::default().fg(theme.border_focused));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let category_style =
        |text: String| -> Line<'static> { Line::from(Span::raw(text).bold().fg(theme.primary)) };

    let shortcut = |key: &'static str, desc: String| -> Line<'static> {
        let pad = 14_usize.saturating_sub(key.len());
        Line::from(vec![
            Span::raw(format!("  {key}")).bold(),
            Span::raw(format!("{}{desc}", " ".repeat(pad))).fg(theme.text_dim),
        ])
    };

    let lines: Vec<Line> = vec![
        // Mode
        category_style(format!("── {} ──", t!("help.category_mode"))),
        shortcut("Tab", t!("help.tab").to_string()),
        shortcut("Ctrl+T", t!("help.ctrl_t").to_string()),
        shortcut("Ctrl+Shift+T", t!("help.ctrl_shift_t").to_string()),
        shortcut("Ctrl+M", t!("help.ctrl_m").to_string()),
        Line::from(""),
        // Editing
        category_style(format!("── {} ──", t!("help.category_editing"))),
        shortcut("Enter", t!("help.enter").to_string()),
        shortcut("Shift+Enter", t!("help.shift_enter").to_string()),
        shortcut("Ctrl+V", t!("help.ctrl_v").to_string()),
        shortcut("Ctrl+E", t!("help.ctrl_e").to_string()),
        shortcut("Ctrl+Bksp", t!("help.ctrl_bksp").to_string()),
        Line::from(""),
        // Navigation
        category_style(format!("── {} ──", t!("help.category_navigation"))),
        shortcut("↑/↓", t!("help.up_down").to_string()),
        shortcut("Alt+↑/↓", t!("help.alt_up_down").to_string()),
        shortcut("PgUp/PgDn", t!("help.pgup_pgdn").to_string()),
        shortcut("Ctrl+←/→", t!("help.ctrl_left_right").to_string()),
        Line::from(""),
        // Tools
        category_style(format!("── {} ──", t!("help.category_tools"))),
        shortcut("Ctrl+C", t!("help.ctrl_c").to_string()),
        shortcut("Ctrl+B", t!("help.ctrl_b").to_string()),
        shortcut("Ctrl+F", t!("help.ctrl_f").to_string()),
        shortcut("Ctrl+Shift+E", t!("help.ctrl_shift_e").to_string()),
        shortcut("Ctrl+Shift+R", t!("help.ctrl_shift_r").to_string()),
        Line::from(""),
        // UI
        category_style(format!("── {} ──", t!("help.category_ui"))),
        shortcut("? / F1", t!("help.question_f1").to_string()),
        shortcut("Ctrl+P", t!("help.ctrl_p").to_string()),
        shortcut("Ctrl+S", t!("help.ctrl_s").to_string()),
        shortcut("Ctrl+L", t!("help.ctrl_l").to_string()),
        shortcut("Ctrl+Q", t!("help.ctrl_q").to_string()),
        shortcut("Esc", t!("help.esc").to_string()),
        shortcut("Esc Esc", t!("help.esc_esc").to_string()),
        Line::from(""),
        Line::from(Span::raw(t!("dialog.press_esc_close").to_string()).fg(theme.text_dim)),
    ];

    let paragraph = Paragraph::new(lines).scroll((scroll_offset.max(0) as u16, 0));
    frame.render_widget(paragraph, inner);
}

/// Render the command palette overlay.
fn render_command_palette_overlay(
    frame: &mut Frame,
    area: Rect,
    palette: &crate::state::CommandPaletteOverlay,
    theme: &Theme,
) {
    let title = if palette.query.is_empty() {
        format!(" {} ", t!("dialog.command_palette"))
    } else {
        format!(
            " {} ",
            t!("dialog.command_palette_filter", filter = &palette.query)
        )
    };

    let block = Block::default()
        .title(title.bold())
        .borders(Borders::ALL)
        .border_style(ratatui::style::Style::default().fg(theme.border_focused));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    // Build command list
    let commands = palette.filtered_commands();
    let mut lines: Vec<Line> = vec![];

    for (i, cmd) in commands.iter().enumerate() {
        let is_selected = palette.selected == i as i32;
        let shortcut_text = cmd
            .shortcut
            .as_ref()
            .map(|s| format!(" ({s})"))
            .unwrap_or_default();

        let line = if is_selected {
            Line::from(vec![
                Span::raw("▸ ").bold().fg(theme.primary),
                Span::raw(&cmd.name).bold().fg(theme.primary),
                Span::raw(shortcut_text).fg(theme.text_dim),
            ])
        } else {
            Line::from(vec![
                Span::raw("  "),
                Span::raw(&cmd.name),
                Span::raw(shortcut_text).fg(theme.text_dim),
            ])
        };
        lines.push(line);

        // Add description for selected item
        if is_selected {
            lines.push(Line::from(vec![
                Span::raw("    "),
                Span::raw(&cmd.description).fg(theme.text_dim).italic(),
            ]));
        }
    }

    if commands.is_empty() {
        lines.push(Line::from(
            Span::raw(t!("dialog.no_commands_match").to_string())
                .fg(theme.text_dim)
                .italic(),
        ));
    }

    // Add hints at bottom
    lines.push(Line::from(""));
    lines.push(Line::from(
        Span::raw(t!("dialog.command_palette_hints").to_string()).fg(theme.text_dim),
    ));

    let paragraph = Paragraph::new(lines).wrap(Wrap { trim: false });
    frame.render_widget(paragraph, inner);
}

/// Render the session browser overlay.
fn render_session_browser_overlay(
    frame: &mut Frame,
    area: Rect,
    browser: &crate::state::SessionBrowserOverlay,
    theme: &Theme,
) {
    let title = if browser.filter.is_empty() {
        format!(" {} ", t!("dialog.session_browser"))
    } else {
        format!(
            " {} ",
            t!("dialog.session_browser_filter", filter = &browser.filter)
        )
    };

    let block = Block::default()
        .title(title.bold())
        .borders(Borders::ALL)
        .border_style(ratatui::style::Style::default().fg(theme.border_focused));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    // Build session list
    let sessions = browser.filtered_sessions();
    let mut lines: Vec<Line> = vec![];

    if sessions.is_empty() {
        lines.push(Line::from(
            Span::raw(t!("dialog.no_saved_sessions").to_string())
                .fg(theme.text_dim)
                .italic(),
        ));
    } else {
        for (i, session) in sessions.iter().enumerate() {
            let is_selected = browser.selected == i as i32;
            let msg_count = t!(
                "dialog.session_message_count",
                count = session.message_count
            )
            .to_string();
            let line = if is_selected {
                Line::from(vec![
                    Span::raw("▸ ").bold().fg(theme.primary),
                    Span::raw(&session.title).bold().fg(theme.primary),
                    Span::raw(format!(" {msg_count}")).fg(theme.text_dim),
                ])
            } else {
                Line::from(vec![
                    Span::raw("  "),
                    Span::raw(&session.title),
                    Span::raw(format!(" {msg_count}")).fg(theme.text_dim),
                ])
            };
            lines.push(line);
        }
    }

    // Add hints at bottom
    lines.push(Line::from(""));
    lines.push(Line::from(
        Span::raw(t!("dialog.session_browser_hints").to_string()).fg(theme.text_dim),
    ));

    let paragraph = Paragraph::new(lines).wrap(Wrap { trim: false });
    frame.render_widget(paragraph, inner);
}

/// Render the plugin manager overlay with 4 tabs.
fn render_plugin_manager_overlay(
    frame: &mut Frame,
    area: Rect,
    manager: &crate::state::PluginManagerOverlay,
    theme: &Theme,
) {
    use crate::state::PluginManagerTab;

    // Build tab bar
    let tabs = [
        (
            PluginManagerTab::Discover,
            t!("dialog.plugin_tab_discover").to_string(),
        ),
        (
            PluginManagerTab::Installed,
            t!("dialog.plugin_tab_installed").to_string(),
        ),
        (
            PluginManagerTab::Marketplaces,
            t!("dialog.plugin_tab_marketplaces").to_string(),
        ),
        (
            PluginManagerTab::Errors,
            t!("dialog.plugin_tab_errors").to_string(),
        ),
    ];

    let tab_title: String = tabs
        .iter()
        .map(|(tab, label)| {
            if *tab == manager.tab {
                format!("[{label}]")
            } else {
                format!(" {label} ")
            }
        })
        .collect::<Vec<_>>()
        .join(" ");

    let title = if manager.filter.is_empty() {
        format!(" {} ", t!("dialog.plugin_manager"))
    } else {
        format!(
            " {} ",
            t!("dialog.plugin_manager_filter", filter = &manager.filter)
        )
    };

    let block = Block::default()
        .title(title.bold())
        .borders(Borders::ALL)
        .border_style(ratatui::style::Style::default().fg(theme.border_focused));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let mut lines: Vec<Line> = vec![];

    // Tab bar
    let tab_spans: Vec<Span> = tabs
        .iter()
        .map(|(tab, label)| {
            if *tab == manager.tab {
                Span::raw(format!(" {label} ")).bold().fg(theme.primary)
            } else {
                Span::raw(format!(" {label} ")).fg(theme.text_dim)
            }
        })
        .collect();
    lines.push(Line::from(tab_spans));
    lines.push(Line::from(""));

    // Tab content
    match manager.tab {
        PluginManagerTab::Discover => {
            let items = manager.filtered_discover();
            if items.is_empty() {
                lines.push(Line::from(
                    Span::raw(t!("dialog.plugin_no_discover").to_string())
                        .fg(theme.text_dim)
                        .italic(),
                ));
            } else {
                for (i, plugin) in items.iter().enumerate() {
                    let is_selected = manager.selected == i as i32;
                    let line = if is_selected {
                        Line::from(vec![
                            Span::raw("▸ ").bold().fg(theme.primary),
                            Span::raw(&plugin.name).bold().fg(theme.primary),
                            Span::raw(format!(" v{}", plugin.version)).fg(theme.text_dim),
                            Span::raw(format!(" - {}", plugin.description)).fg(theme.text_dim),
                        ])
                    } else {
                        Line::from(vec![
                            Span::raw("  "),
                            Span::raw(&plugin.name),
                            Span::raw(format!(" v{}", plugin.version)).fg(theme.text_dim),
                            Span::raw(format!(" - {}", plugin.description)).fg(theme.text_dim),
                        ])
                    };
                    lines.push(line);
                }
            }
        }
        PluginManagerTab::Installed => {
            let items = manager.filtered_installed();
            if items.is_empty() {
                lines.push(Line::from(
                    Span::raw(t!("dialog.plugin_no_installed").to_string())
                        .fg(theme.text_dim)
                        .italic(),
                ));
            } else {
                for (i, plugin) in items.iter().enumerate() {
                    let is_selected = manager.selected == i as i32;
                    let status = if plugin.enabled {
                        Span::raw(" ●").fg(theme.success)
                    } else {
                        Span::raw(" ○").fg(theme.text_dim)
                    };
                    let line = if is_selected {
                        Line::from(vec![
                            Span::raw("▸ ").bold().fg(theme.primary),
                            Span::raw(&plugin.name).bold().fg(theme.primary),
                            status,
                            Span::raw(format!(" [{}]", plugin.scope)).fg(theme.text_dim),
                            Span::raw(format!(
                                " ({}s/{}h/{}a)",
                                plugin.skills_count, plugin.hooks_count, plugin.agents_count
                            ))
                            .fg(theme.text_dim),
                        ])
                    } else {
                        Line::from(vec![
                            Span::raw("  "),
                            Span::raw(&plugin.name),
                            status,
                            Span::raw(format!(" [{}]", plugin.scope)).fg(theme.text_dim),
                            Span::raw(format!(
                                " ({}s/{}h/{}a)",
                                plugin.skills_count, plugin.hooks_count, plugin.agents_count
                            ))
                            .fg(theme.text_dim),
                        ])
                    };
                    lines.push(line);
                }
            }
        }
        PluginManagerTab::Marketplaces => {
            if manager.marketplace_items.is_empty() {
                lines.push(Line::from(
                    Span::raw(t!("dialog.plugin_no_marketplaces").to_string())
                        .fg(theme.text_dim)
                        .italic(),
                ));
            } else {
                for (i, market) in manager.marketplace_items.iter().enumerate() {
                    let is_selected = manager.selected == i as i32;
                    let auto = if market.auto_update { " ↻" } else { "" };
                    let line = if is_selected {
                        Line::from(vec![
                            Span::raw("▸ ").bold().fg(theme.primary),
                            Span::raw(&market.name).bold().fg(theme.primary),
                            Span::raw(format!(" ({})  {}", market.source_type, market.source))
                                .fg(theme.text_dim),
                            Span::raw(format!(" [{} plugins]{auto}", market.plugin_count))
                                .fg(theme.text_dim),
                        ])
                    } else {
                        Line::from(vec![
                            Span::raw("  "),
                            Span::raw(&market.name),
                            Span::raw(format!(" ({})  {}", market.source_type, market.source))
                                .fg(theme.text_dim),
                            Span::raw(format!(" [{} plugins]{auto}", market.plugin_count))
                                .fg(theme.text_dim),
                        ])
                    };
                    lines.push(line);
                }
            }
        }
        PluginManagerTab::Errors => {
            if manager.error_items.is_empty() {
                lines.push(Line::from(
                    Span::raw(t!("dialog.plugin_no_errors").to_string())
                        .fg(theme.success)
                        .italic(),
                ));
            } else {
                for (i, err) in manager.error_items.iter().enumerate() {
                    let is_selected = manager.selected == i as i32;
                    let line = if is_selected {
                        Line::from(vec![
                            Span::raw("▸ ").bold().fg(theme.error),
                            Span::raw(&err.source).bold().fg(theme.error),
                            Span::raw(format!(": {}", err.error)).fg(theme.text_dim),
                        ])
                    } else {
                        Line::from(vec![
                            Span::raw("  "),
                            Span::raw(&err.source).fg(theme.error),
                            Span::raw(format!(": {}", err.error)).fg(theme.text_dim),
                        ])
                    };
                    lines.push(line);
                }
            }
        }
    }

    // Hints at bottom
    lines.push(Line::from(""));
    lines.push(Line::from(
        Span::raw(t!("dialog.plugin_manager_hints").to_string()).fg(theme.text_dim),
    ));

    let paragraph = Paragraph::new(lines).wrap(Wrap { trim: false });
    frame.render_widget(paragraph, inner);

    // Suppress unused variable warning
    let _ = tab_title;
}

/// Render the rewind selector overlay.
fn render_rewind_selector_overlay(
    frame: &mut Frame,
    area: Rect,
    rw: &crate::state::RewindSelectorOverlay,
    theme: &Theme,
) {
    use crate::state::RewindSelectorPhase;

    // Show loading state if an operation is in progress
    if rw.loading {
        let action = rw.loading_action.as_deref().unwrap_or("Processing...");
        let block = Block::default()
            .title(format!(" {action} ").bold().fg(theme.primary))
            .borders(Borders::ALL)
            .border_style(ratatui::style::Style::default().fg(theme.primary));
        let inner = block.inner(area);
        frame.render_widget(block, area);
        let spinner_chars = ['|', '/', '-', '\\'];
        let idx = (std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .subsec_millis()
            / 250) as usize
            % spinner_chars.len();
        let lines = vec![
            Line::from(""),
            Line::from(Span::raw(format!("  {} {action}", spinner_chars[idx])).fg(theme.primary)),
        ];
        let paragraph = Paragraph::new(lines).wrap(Wrap { trim: false });
        frame.render_widget(paragraph, inner);
        return;
    }

    let title = match rw.phase {
        RewindSelectorPhase::SelectCheckpoint => t!("dialog.rewind_select_checkpoint"),
        RewindSelectorPhase::SelectMode => t!("dialog.rewind_select_mode"),
        RewindSelectorPhase::InputSummarizeContext => t!("dialog.rewind_summarize_context"),
    };

    let block = Block::default()
        .title(format!(" {title} ").bold().fg(theme.primary))
        .borders(Borders::ALL)
        .border_style(ratatui::style::Style::default().fg(theme.primary));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let mut lines: Vec<Line> = Vec::new();

    match rw.phase {
        RewindSelectorPhase::SelectCheckpoint => {
            // Show checkpoints newest-first with file names
            let display_items = rw.display_items();
            for (i, cp) in display_items.iter().enumerate() {
                let is_selected = i as i32 == rw.selected;
                let prefix = if is_selected { "▸ " } else { "  " };
                let file_info = if let Some(ref ds) = cp.diff_stats {
                    format!(
                        " ({} {}, +{} -{})",
                        cp.file_count,
                        t!("dialog.rewind_files"),
                        ds.insertions,
                        ds.deletions
                    )
                } else if cp.file_count > 0 {
                    format!(" ({} {})", cp.file_count, t!("dialog.rewind_files"))
                } else {
                    String::new()
                };
                let label = format!(
                    "{prefix}{}{file_info}",
                    if cp.user_message_preview.is_empty() {
                        format!("Turn {}", cp.turn_number)
                    } else {
                        cp.user_message_preview.clone()
                    }
                );
                let line = if is_selected {
                    Line::from(Span::raw(label).bold().fg(theme.primary))
                } else {
                    Line::from(Span::raw(label).fg(theme.text))
                };
                lines.push(line);

                // Show modified file names for selected checkpoint
                if is_selected && !cp.modified_files.is_empty() {
                    let max_files = 3;
                    for file in cp.modified_files.iter().take(max_files) {
                        // Show just the file name, not full path
                        let display_name = std::path::Path::new(file)
                            .file_name()
                            .and_then(|n| n.to_str())
                            .unwrap_or(file);
                        lines.push(Line::from(
                            Span::raw(format!("    {display_name}")).fg(theme.text_dim),
                        ));
                    }
                    if cp.modified_files.len() > max_files {
                        let remaining = cp.modified_files.len() - max_files;
                        lines.push(Line::from(
                            Span::raw(format!(
                                "    {}",
                                t!("dialog.rewind_more_files", count = remaining)
                            ))
                            .fg(theme.text_dim),
                        ));
                    }
                }
            }
            lines.push(Line::from(""));
            lines.push(Line::from(
                Span::raw(t!("dialog.rewind_checkpoint_hints").to_string()).fg(theme.text_dim),
            ));
        }
        RewindSelectorPhase::SelectMode => {
            // Show the selected checkpoint info
            if let Some(cp) = rw.selected_checkpoint() {
                let header = format!(
                    "{}: Turn {} ({})",
                    t!("dialog.rewind_target"),
                    cp.turn_number,
                    if cp.user_message_preview.is_empty() {
                        "...".to_string()
                    } else if cp.user_message_preview.chars().count() > 40 {
                        let truncated: String = cp.user_message_preview.chars().take(40).collect();
                        format!("{truncated}...")
                    } else {
                        cp.user_message_preview.clone()
                    }
                );
                lines.push(Line::from(Span::raw(header).fg(theme.text_dim)));
                lines.push(Line::from(""));
            }

            let modes = [
                t!("dialog.rewind_mode_code_and_conversation").to_string(),
                t!("dialog.rewind_mode_conversation_only").to_string(),
                t!("dialog.rewind_mode_code_only").to_string(),
                t!("dialog.rewind_mode_summarize").to_string(),
            ];
            for (i, label) in modes.iter().enumerate() {
                let is_selected = i as i32 == rw.mode_selected;
                let prefix = if is_selected { "▸ " } else { "  " };
                let line = if is_selected {
                    Line::from(
                        Span::raw(format!("{prefix}{label}"))
                            .bold()
                            .fg(theme.primary),
                    )
                } else {
                    Line::from(Span::raw(format!("{prefix}{label}")).fg(theme.text))
                };
                lines.push(line);
            }
            lines.push(Line::from(""));
            lines.push(Line::from(
                Span::raw(t!("dialog.rewind_warning_bash").to_string()).fg(theme.text_dim),
            ));
            lines.push(Line::from(
                Span::raw(t!("dialog.rewind_mode_hints").to_string()).fg(theme.text_dim),
            ));
        }
        RewindSelectorPhase::InputSummarizeContext => {
            // Show context input for summarize
            if let Some(turn) = rw.summarize_turn {
                let header = format!("{}: Turn {}", t!("dialog.rewind_target"), turn,);
                lines.push(Line::from(Span::raw(header).fg(theme.text_dim)));
                lines.push(Line::from(""));
            }
            lines.push(Line::from(
                Span::raw(t!("dialog.rewind_summarize_context_prompt").to_string()).fg(theme.text),
            ));
            lines.push(Line::from(""));

            // Show the text input with cursor
            let input_text = if rw.summarize_context.is_empty() {
                t!("dialog.rewind_summarize_context_placeholder").to_string()
            } else {
                rw.summarize_context.clone()
            };
            let input_style = if rw.summarize_context.is_empty() {
                ratatui::style::Style::default().fg(theme.text_dim)
            } else {
                ratatui::style::Style::default().fg(theme.text)
            };
            lines.push(Line::from(
                Span::raw(format!("  > {input_text}")).style(input_style),
            ));

            lines.push(Line::from(""));
            lines.push(Line::from(
                Span::raw(t!("dialog.rewind_summarize_context_hints").to_string())
                    .fg(theme.text_dim),
            ));
        }
    }

    let paragraph = Paragraph::new(lines).wrap(Wrap { trim: false });
    frame.render_widget(paragraph, inner);
}

/// Render the cost warning overlay.
fn render_cost_warning_overlay(
    frame: &mut Frame,
    area: Rect,
    cw: &crate::state::CostWarningOverlay,
    theme: &Theme,
) {
    let block = Block::default()
        .title(
            format!(" {} ", t!("dialog.cost_warning_title"))
                .bold()
                .fg(theme.warning),
        )
        .borders(Borders::ALL)
        .border_style(ratatui::style::Style::default().fg(theme.warning));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let format_cost = |cents: i32| -> String {
        if cents >= 100 {
            format!("${:.2}", cents as f64 / 100.0)
        } else {
            format!("{cents}c")
        }
    };

    let mut lines: Vec<Line> = vec![];

    lines.push(Line::from(vec![
        Span::raw(format!("{} ", t!("dialog.cost_current"))).bold(),
        Span::raw(format_cost(cw.current_cost_cents)).fg(theme.warning),
    ]));

    lines.push(Line::from(vec![
        Span::raw(format!("{} ", t!("dialog.cost_threshold"))),
        Span::raw(format_cost(cw.threshold_cents)).fg(theme.text_dim),
    ]));

    if let Some(budget) = cw.budget_cents {
        lines.push(Line::from(vec![
            Span::raw(format!("{} ", t!("dialog.cost_budget"))),
            Span::raw(format_cost(budget)).fg(theme.error),
        ]));
    }

    lines.push(Line::from(""));
    lines.push(Line::from(
        Span::raw(t!("dialog.cost_warning_hints").to_string()).fg(theme.text_dim),
    ));

    let paragraph = Paragraph::new(lines).wrap(Wrap { trim: false });
    frame.render_widget(paragraph, inner);
}

/// Render the sandbox permission overlay.
fn render_sandbox_permission_overlay(
    frame: &mut Frame,
    area: Rect,
    sp: &crate::state::SandboxPermissionOverlay,
    theme: &Theme,
) {
    let block = Block::default()
        .title(
            format!(" {} ", t!("dialog.sandbox_permission_title"))
                .bold()
                .fg(theme.error),
        )
        .borders(Borders::ALL)
        .border_style(ratatui::style::Style::default().fg(theme.error));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let mut lines: Vec<Line> = vec![];

    // Access type
    lines.push(Line::from(vec![
        Span::raw(format!("{} ", t!("dialog.sandbox_access_type"))).bold(),
        Span::raw(sp.access_type.label()).fg(theme.warning),
    ]));

    // Tool name
    lines.push(Line::from(vec![
        Span::raw(format!("{} ", t!("dialog.tool"))).bold(),
        Span::raw(&sp.request.tool_name).fg(theme.primary),
    ]));

    // Description
    for desc_line in sp.request.description.lines() {
        lines.push(Line::from(
            Span::raw(format!("  {desc_line}")).fg(theme.text_dim),
        ));
    }

    // Security risks
    for risk in &sp.request.risks {
        lines.push(Line::from(
            Span::raw(format!("  ! {}", risk.message)).fg(theme.error),
        ));
    }
    lines.push(Line::from(""));

    // Options
    let options = [
        t!("dialog.approve").to_string(),
        t!("dialog.deny").to_string(),
        t!("dialog.approve_all").to_string(),
    ];
    for (i, opt) in options.iter().enumerate() {
        let is_selected = sp.selected == i as i32;
        let line = if is_selected {
            Line::from(Span::raw(format!("▸ {opt}")).bold().fg(theme.primary))
        } else {
            Line::from(Span::raw(format!("  {opt}")).fg(theme.text_dim))
        };
        lines.push(line);
    }

    lines.push(Line::from(""));
    lines.push(Line::from(
        Span::raw(t!("dialog.sandbox_permission_hints").to_string()).fg(theme.text_dim),
    ));

    let paragraph = Paragraph::new(lines).wrap(Wrap { trim: false });
    frame.render_widget(paragraph, inner);
}

/// Render an error overlay.
fn render_error_overlay(frame: &mut Frame, area: Rect, message: &str, theme: &Theme) {
    let block = Block::default()
        .title(format!(" {} ", t!("dialog.error")).bold().fg(theme.error))
        .borders(Borders::ALL)
        .border_style(ratatui::style::Style::default().fg(theme.error));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let lines: Vec<Line> = vec![
        Line::from(Span::raw(message)),
        Line::from(""),
        Line::from(Span::raw(t!("dialog.press_esc_enter_dismiss").to_string()).fg(theme.text_dim)),
    ];

    let paragraph = Paragraph::new(lines).wrap(Wrap { trim: false });
    frame.render_widget(paragraph, inner);
}

#[cfg(test)]
#[path = "render.test.rs"]
mod tests;
