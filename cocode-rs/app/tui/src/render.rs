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
        .fallback_model(state.session.fallback_model.as_deref());
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
    // Check if we have running tools to show
    let has_tools = !state.session.tool_executions.is_empty()
        && state
            .session
            .tool_executions
            .iter()
            .any(|t| t.status == crate::state::ToolStatus::Running);

    // Check if we have subagents to show
    let has_subagents = !state.session.subagents.is_empty();

    // Responsive side panel: hide when terminal width < 100
    if (has_tools || has_subagents) && area.width >= 100 {
        // Responsive split ratio: 75/25 for wide terminals, 70/30 default
        let (main_pct, side_pct) = if area.width >= 160 {
            (75, 25)
        } else {
            (70, 30)
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
    let input_height = (input_lines as u16 + 2).min(10); // +2 for borders, max 10

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
    let streaming_content = state.ui.streaming.as_ref().map(|s| s.content.as_str());
    let streaming_thinking = state.ui.streaming.as_ref().map(|s| s.thinking.as_str());

    let chat = ChatWidget::new(&state.session.messages, theme)
        .scroll(state.ui.scroll_offset)
        .streaming(streaming_content)
        .streaming_thinking(streaming_thinking)
        .show_thinking(state.ui.show_thinking)
        .is_thinking(state.ui.is_thinking())
        .animation_frame(state.ui.animation_frame())
        .thinking_duration(state.ui.thinking_duration())
        .collapsed_tools(&state.ui.collapsed_tools)
        .width(area.width);
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
        .placeholder(&placeholder);
    frame.render_widget(input, chunks[input_chunk_index]);

    // Suggestion popups are mutually exclusive — only render one at a time.
    // Priority: skill > agent > symbol > file (matches key event handling order).
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
                Constraint::Percentage(50), // Tools
                Constraint::Percentage(50), // Subagents
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
    let panel = ToolPanel::new(&state.session.tool_executions, theme).max_display(8);
    frame.render_widget(panel, area);
}

/// Render the subagents panel.
fn render_subagents(frame: &mut Frame, area: Rect, state: &AppState, theme: &Theme) {
    let panel = SubagentPanel::new(&state.session.subagents, theme).max_display(5);
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
    .queue_counts(state.session.queued_count(), 0)
    .context_window(
        state.session.context_window_used,
        state.session.context_window_total,
    )
    .estimated_cost(state.session.estimated_cost_cents)
    .working_dir(state.session.working_dir.as_deref());
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
    let overlay_width = (area.width * 60 / 100).clamp(40, 80);
    let overlay_height = match overlay {
        Overlay::Permission(_) => 12,
        Overlay::ModelPicker(picker) => (picker.filtered_items().len() as u16 + 4).min(20),
        Overlay::CommandPalette(palette) => (palette.filtered_commands().len() as u16 + 4).min(20),
        Overlay::SessionBrowser(browser) => (browser.filtered_sessions().len() as u16 + 4).min(20),
        Overlay::Help => 30,
        Overlay::Error(_) => 8,
    };

    let x = (area.width.saturating_sub(overlay_width)) / 2;
    let y = (area.height.saturating_sub(overlay_height)) / 2;
    let overlay_area = Rect::new(x, y, overlay_width, overlay_height);

    // Clear the area behind the overlay
    frame.render_widget(Clear, overlay_area);

    match overlay {
        Overlay::Permission(perm) => render_permission_overlay(frame, overlay_area, perm, theme),
        Overlay::ModelPicker(picker) => {
            render_model_picker_overlay(frame, overlay_area, picker, theme)
        }
        Overlay::CommandPalette(palette) => {
            render_command_palette_overlay(frame, overlay_area, palette, theme)
        }
        Overlay::SessionBrowser(browser) => {
            render_session_browser_overlay(frame, overlay_area, browser, theme)
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
    lines.push(Line::from(""));

    // Description
    lines.push(Line::from(Span::raw(&perm.request.description)));
    lines.push(Line::from(""));

    // Options
    let options = [
        t!("dialog.approve").to_string(),
        t!("dialog.deny").to_string(),
        t!("dialog.approve_all").to_string(),
    ];
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
        let display = selection.model.to_string();
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
        shortcut("Ctrl+Shift+E", t!("help.ctrl_shift_e").to_string()),
        Line::from(""),
        // UI
        category_style(format!("── {} ──", t!("help.category_ui"))),
        shortcut("? / F1", t!("help.question_f1").to_string()),
        shortcut("Ctrl+P", t!("help.ctrl_p").to_string()),
        shortcut("Ctrl+S", t!("help.ctrl_s").to_string()),
        shortcut("Ctrl+L", t!("help.ctrl_l").to_string()),
        shortcut("Ctrl+Q", t!("help.ctrl_q").to_string()),
        shortcut("Esc", t!("help.esc").to_string()),
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
