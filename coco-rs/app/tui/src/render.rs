//! TUI rendering — the View in TEA.
//!
//! Pure function: takes immutable state, renders to frame.
//! No side effects except pixel drawing.

use ratatui::prelude::*;
use ratatui::widgets::Block;
use ratatui::widgets::Borders;
use ratatui::widgets::Clear;
use ratatui::widgets::Paragraph;

use crate::constants;
use crate::render_overlays;
use crate::state::AppState;
use crate::state::FocusTarget;
use crate::state::Toast;
use crate::state::ToastSeverity;
use crate::theme::Theme;

/// Render the full TUI layout.
pub fn render(frame: &mut Frame, state: &AppState) {
    let area = frame.area();
    let theme = &state.ui.theme;

    let main_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // header
            Constraint::Min(1),    // main area
            Constraint::Length(1), // status bar
        ])
        .split(area);

    render_header_bar(frame, main_chunks[0], state, theme);
    render_main_area(frame, main_chunks[1], state, theme);
    render_status_bar(frame, main_chunks[2], state, theme);

    // Overlays on top
    if let Some(ref overlay) = state.ui.overlay {
        render_overlays::render_overlay(frame, area, overlay, state, theme);
    }

    // Toasts at top-right
    if state.ui.has_toasts() {
        render_toasts(frame, area, &state.ui.toasts, theme);
    }
}

/// Header bar: session info, model, branch.
fn render_header_bar(frame: &mut Frame, area: Rect, state: &AppState, theme: &Theme) {
    let mut parts = Vec::new();

    // Working directory
    if let Some(ref dir) = state.session.working_dir {
        let short = dir.rsplit('/').next().unwrap_or(dir);
        parts.push(Span::styled(
            format!(" {short}"),
            Style::default().fg(theme.primary),
        ));
    }

    // Model
    if !state.session.model.is_empty() {
        parts.push(Span::styled(" | ", Style::default().fg(theme.border)));
        parts.push(Span::styled(
            state.session.model.as_str(),
            Style::default().fg(theme.text_dim),
        ));
    }

    // Fast mode
    if state.session.fast_mode {
        parts.push(Span::styled(" ⚡", Style::default().fg(theme.warning)));
    }

    // Plan mode indicator
    if state.session.plan_mode {
        parts.push(Span::styled(" | ", Style::default().fg(theme.border)));
        parts.push(Span::styled(
            "PLAN",
            Style::default().fg(theme.plan_mode).bold(),
        ));
    }

    // Turn count
    if state.session.turn_count > 0 {
        parts.push(Span::styled(" | ", Style::default().fg(theme.border)));
        parts.push(Span::styled(
            format!("turn {}", state.session.turn_count),
            Style::default().fg(theme.text_dim),
        ));
    }

    // Context usage percentage
    if state.session.context_window_total > 0 {
        let pct = (state.session.context_window_used * 100) / state.session.context_window_total;
        parts.push(Span::styled(" | ", Style::default().fg(theme.border)));
        let color = if pct > 80 {
            theme.warning
        } else {
            theme.text_dim
        };
        parts.push(Span::styled(
            format!("ctx {pct}%"),
            Style::default().fg(color),
        ));
    }

    // Queued commands count
    if !state.session.queued_commands.is_empty() {
        let count = state.session.queued_commands.len();
        parts.push(Span::styled(" | ", Style::default().fg(theme.border)));
        parts.push(Span::styled(
            format!("{count} queued"),
            Style::default().fg(theme.accent),
        ));
    }

    let line = Line::from(parts);
    let header = Paragraph::new(line).style(Style::default().bg(theme.border));
    frame.render_widget(header, area);
}

/// Main area: chat + input, optionally with side panel.
fn render_main_area(frame: &mut Frame, area: Rect, state: &AppState, theme: &Theme) {
    let has_tools =
        !state.session.tool_executions.is_empty() || !state.session.subagents.is_empty();
    let wide_enough = area.width >= constants::SIDE_PANEL_MIN_WIDTH as u16;

    if has_tools && wide_enough {
        let (main_pct, side_pct) = if area.width >= constants::WIDE_TERMINAL_WIDTH as u16 {
            (
                constants::WIDE_TERMINAL_MAIN_PCT,
                constants::WIDE_TERMINAL_SIDE_PCT,
            )
        } else {
            (
                constants::NORMAL_TERMINAL_MAIN_PCT,
                constants::NORMAL_TERMINAL_SIDE_PCT,
            )
        };

        let horiz = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(main_pct as u16),
                Constraint::Percentage(side_pct as u16),
            ])
            .split(area);

        render_chat_and_input(frame, horiz[0], state, theme);
        render_side_panel(frame, horiz[1], state, theme);
    } else {
        render_chat_and_input(frame, area, state, theme);
    }
}

/// Chat area + input area (vertical split).
fn render_chat_and_input(frame: &mut Frame, area: Rect, state: &AppState, theme: &Theme) {
    let input_height = 3.min(constants::MAX_INPUT_HEIGHT as u16);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(1),               // chat
            Constraint::Length(input_height), // input
        ])
        .split(area);

    render_conversation(frame, state, chunks[0], theme);
    render_input(frame, state, chunks[1], theme);
}

/// Render conversation history using the ChatWidget.
fn render_conversation(frame: &mut Frame, state: &AppState, area: Rect, theme: &Theme) {
    let mut chat = crate::widgets::ChatWidget::new(&state.session.messages, theme)
        .scroll(state.ui.scroll_offset)
        .streaming(state.ui.streaming.as_ref())
        .show_thinking(state.ui.show_thinking)
        .show_system_reminders(state.ui.show_system_reminders)
        .tool_executions(&state.session.tool_executions)
        .width(area.width);

    if !state.ui.collapsed_tools.is_empty() {
        chat = chat.collapsed_tools(&state.ui.collapsed_tools);
    }

    frame.render_widget(chat, area);
}

/// Render the input area with mode indicator and streaming awareness.
fn render_input(frame: &mut Frame, state: &AppState, area: Rect, theme: &Theme) {
    let is_focused = state.ui.focus == FocusTarget::Input;
    let is_streaming = state.is_streaming();
    let border_color = if is_focused {
        theme.border_focused
    } else {
        theme.border
    };

    // Mode indicator: > normal, ! plan, ~ streaming
    // Use ASCII chars to ensure consistent 2-column width across all terminals.
    let indicator = if is_streaming {
        Span::styled("~ ", Style::default().fg(theme.warning))
    } else if state.session.plan_mode {
        Span::styled("! ", Style::default().fg(theme.plan_mode).bold())
    } else {
        Span::styled("> ", Style::default().fg(theme.primary))
    };

    let display_text = if state.ui.input.is_empty() {
        "Type a message...".to_string()
    } else {
        state.ui.input.text.clone()
    };

    let text_style = if state.ui.input.is_empty() {
        Style::default().fg(theme.text_dim)
    } else {
        Style::default().fg(theme.text)
    };

    let title = if state.session.plan_mode {
        " Plan Mode "
    } else if is_streaming {
        " Queue Input "
    } else {
        " Input "
    };

    let input_line = Line::from(vec![indicator, Span::styled(display_text, text_style)]);
    let input = Paragraph::new(input_line).block(
        Block::default()
            .borders(Borders::TOP)
            .title(title)
            .border_style(Style::default().fg(border_color)),
    );

    frame.render_widget(input, area);

    // Show cursor position (offset by 2 for mode indicator)
    if is_focused && !state.ui.input.is_empty() {
        let indicator_width = 2_u16; // "❯ " is 2 chars wide
        let max_cursor = area.width.saturating_sub(indicator_width + 1) as i32;
        let cursor_x = area.x + indicator_width + state.ui.input.cursor.min(max_cursor) as u16;
        let cursor_y = area.y + 1; // below border
        frame.set_cursor_position((cursor_x, cursor_y));
    }
}

/// Render the side panel with tools and subagents.
fn render_side_panel(frame: &mut Frame, area: Rect, state: &AppState, theme: &Theme) {
    let has_subagents = !state.session.subagents.is_empty();

    if has_subagents {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
            .split(area);

        render_tool_panel(frame, chunks[0], state, theme);
        render_subagent_panel(frame, chunks[1], state, theme);
    } else {
        render_tool_panel(frame, area, state, theme);
    }
}

/// Render active/completed tool executions using ToolPanel widget.
fn render_tool_panel(frame: &mut Frame, area: Rect, state: &AppState, theme: &Theme) {
    let panel = crate::widgets::ToolPanel::new(&state.session.tool_executions, theme);
    frame.render_widget(panel, area);
}

/// Render subagent instances using SubagentPanel widget.
fn render_subagent_panel(frame: &mut Frame, area: Rect, state: &AppState, theme: &Theme) {
    let panel = crate::widgets::SubagentPanel::new(&state.session.subagents, theme)
        .focused_index(state.session.focused_subagent_index);
    frame.render_widget(panel, area);
}

/// Render the status bar.
fn render_status_bar(frame: &mut Frame, area: Rect, state: &AppState, theme: &Theme) {
    let mut parts = Vec::new();

    // Model
    parts.push(Span::styled(
        format!(" {}", state.session.model),
        Style::default().fg(theme.primary).bold(),
    ));

    // Fast mode
    if state.session.fast_mode {
        parts.push(Span::styled(" ⚡", Style::default().fg(theme.warning)));
    }

    // Permission mode
    parts.push(Span::styled(" | ", Style::default().fg(theme.border)));
    parts.push(Span::styled(
        format!("{:?}", state.session.permission_mode),
        Style::default().fg(theme.text_dim),
    ));

    // Token usage
    let tokens = &state.session.token_usage;
    let total = tokens.input_tokens + tokens.output_tokens;
    if total > 0 {
        parts.push(Span::styled(" | ", Style::default().fg(theme.border)));
        let formatted = format_token_count(total);
        parts.push(Span::styled(formatted, Style::default().fg(theme.text_dim)));
        if tokens.cache_read_tokens > 0 {
            parts.push(Span::styled(
                format!(" ({}cached)", format_token_count(tokens.cache_read_tokens)),
                Style::default().fg(theme.text_dim),
            ));
        }
    }

    // Cost
    if state.session.estimated_cost_cents > 0 {
        parts.push(Span::styled(" | ", Style::default().fg(theme.border)));
        let cost = state.session.estimated_cost_cents as f64 / 100.0;
        parts.push(Span::styled(
            format!("${cost:.2}"),
            Style::default().fg(theme.text_dim),
        ));
    }

    // MCP servers
    let mcp_count = state.session.connected_mcp_count();
    if mcp_count > 0 {
        parts.push(Span::styled(" | ", Style::default().fg(theme.border)));
        parts.push(Span::styled(
            format!("{mcp_count} MCP"),
            Style::default().fg(theme.text_dim),
        ));
    }

    // Message count
    parts.push(Span::styled(" | ", Style::default().fg(theme.border)));
    parts.push(Span::styled(
        format!("{} msgs", state.session.messages.len()),
        Style::default().fg(theme.text_dim),
    ));

    // Token warning
    if state.session.context_window_total > 0 {
        let pct = (state.session.context_window_used * 100) / state.session.context_window_total;
        if pct > 90 {
            parts.push(Span::styled(
                " ⚠ context nearly full",
                Style::default().fg(theme.error),
            ));
        }
    }

    let line = Line::from(parts);
    let bar = Paragraph::new(line).style(Style::default().bg(theme.border));
    frame.render_widget(bar, area);
}

/// Render toast notifications at top-right corner.
fn render_toasts(
    frame: &mut Frame,
    area: Rect,
    toasts: &std::collections::VecDeque<Toast>,
    theme: &Theme,
) {
    let toast_width: u16 = 40;
    let mut y = 1_u16;

    for toast in toasts.iter() {
        if y >= area.height - 2 {
            break;
        }

        let (icon, color) = match toast.severity {
            ToastSeverity::Info => ("ℹ", theme.text_dim),
            ToastSeverity::Success => ("✓", theme.success),
            ToastSeverity::Warning => ("⚠", theme.warning),
            ToastSeverity::Error => ("✗", theme.error),
        };

        let x = area.width.saturating_sub(toast_width + 1);
        let toast_area = Rect::new(x, y, toast_width, 1);

        let text = format!(" {icon} {} ", toast.message);
        let span = Span::styled(text, Style::default().fg(color));
        frame.render_widget(Clear, toast_area);
        frame.render_widget(Paragraph::new(span), toast_area);

        y += 1;
    }
}

/// Format token count with K/M suffix.
pub(crate) fn format_token_count(count: i64) -> String {
    if count >= 1_000_000 {
        format!("{:.1}M", count as f64 / 1_000_000.0)
    } else if count >= 1_000 {
        format!("{:.1}K", count as f64 / 1_000.0)
    } else {
        format!("{count}")
    }
}

// NOTE: Overlay rendering extracted to render_overlays.rs
// to keep this module under 800 LoC per CLAUDE.md guidance.
