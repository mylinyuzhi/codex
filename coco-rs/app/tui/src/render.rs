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
use crate::i18n::t;
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

    // Lifecycle banners (PR-F1 P0 + PR-F2 P1): shown between the header
    // and main area while their session state is populated. Each banner
    // occupies a single row and stacks vertically. Order: fallback →
    // rate-limit → permission-mode → context-warning → stream-stall →
    // interrupt. The stacking preserves the "severity first" reading
    // order (most urgent banners stay near the header where the eye
    // naturally lands after glancing away from the main area).
    let fallback_rows: u16 = if crate::widgets::ModelFallbackBanner::should_display(
        state.session.model_fallback_banner.as_deref(),
    ) {
        1
    } else {
        0
    };
    let rate_limit_rows: u16 =
        if crate::widgets::RateLimitPanel::should_display(state.session.rate_limit_info.as_ref()) {
            1
        } else {
            0
        };
    let permission_mode_rows: u16 =
        if crate::widgets::PermissionModeBanner::should_display(state.session.permission_mode) {
            1
        } else {
            0
        };
    let context_warning_rows: u16 = if crate::widgets::ContextWarningBanner::should_display(
        state.session.context_usage_percent,
    ) {
        1
    } else {
        0
    };
    let stream_stall_rows: u16 =
        if crate::widgets::StreamStallIndicator::should_display(state.session.stream_stall) {
            1
        } else {
            0
        };
    let interrupt_rows: u16 =
        if crate::widgets::InterruptBanner::should_display(state.session.was_interrupted) {
            1
        } else {
            0
        };

    // ratatui 0.30: `Rect::layout()` returns a fixed-size array so we can
    // destructure directly — no runtime bounds check when reading each slot.
    let [
        header,
        fallback,
        rate_limit,
        permission_mode,
        context_warning,
        stream_stall,
        interrupt,
        main,
        status,
    ] = area.layout(&Layout::vertical([
        Constraint::Length(1),                    // header
        Constraint::Length(fallback_rows),        // model fallback
        Constraint::Length(rate_limit_rows),      // rate limit
        Constraint::Length(permission_mode_rows), // permission mode
        Constraint::Length(context_warning_rows), // context warning
        Constraint::Length(stream_stall_rows),    // stream stall
        Constraint::Length(interrupt_rows),       // interrupt
        Constraint::Min(1),                       // main area
        Constraint::Length(1),                    // status bar
    ]));

    render_header_bar(frame, header, state, theme);
    if fallback_rows > 0
        && let Some(desc) = state.session.model_fallback_banner.as_deref()
    {
        frame.render_widget(
            crate::widgets::ModelFallbackBanner::new(desc, theme),
            fallback,
        );
    }
    if rate_limit_rows > 0
        && let Some(info) = state.session.rate_limit_info.as_ref()
    {
        frame.render_widget(crate::widgets::RateLimitPanel::new(info, theme), rate_limit);
    }
    if permission_mode_rows > 0 {
        frame.render_widget(
            crate::widgets::PermissionModeBanner::new(state.session.permission_mode, theme),
            permission_mode,
        );
    }
    if context_warning_rows > 0
        && let Some(pct) = state.session.context_usage_percent
    {
        frame.render_widget(
            crate::widgets::ContextWarningBanner::new(pct, theme),
            context_warning,
        );
    }
    if stream_stall_rows > 0 {
        frame.render_widget(
            crate::widgets::StreamStallIndicator::new(theme),
            stream_stall,
        );
    }
    if interrupt_rows > 0 {
        frame.render_widget(crate::widgets::InterruptBanner::new(theme), interrupt);
    }
    render_main_area(frame, main, state, theme);
    render_status_bar(frame, status, state, theme);

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
    if state.is_plan_mode() {
        parts.push(Span::styled(" | ", Style::default().fg(theme.border)));
        parts.push(Span::styled(
            t!("status.plan").to_string(),
            Style::default().fg(theme.plan_mode).bold(),
        ));
    }

    // Turn count
    if state.session.turn_count > 0 {
        parts.push(Span::styled(" | ", Style::default().fg(theme.border)));
        parts.push(Span::styled(
            t!("status.turn_short", n = state.session.turn_count).to_string(),
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
            t!("status.context_short", percent = pct).to_string(),
            Style::default().fg(color),
        ));
    }

    // Queued commands count
    if !state.session.queued_commands.is_empty() {
        let count = state.session.queued_commands.len();
        parts.push(Span::styled(" | ", Style::default().fg(theme.border)));
        parts.push(Span::styled(
            t!("status.queued", count = count).to_string(),
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

        let [main, side] = area.layout(&Layout::horizontal([
            Constraint::Percentage(main_pct as u16),
            Constraint::Percentage(side_pct as u16),
        ]));

        render_chat_and_input(frame, main, state, theme);
        render_side_panel(frame, side, state, theme);
    } else {
        render_chat_and_input(frame, area, state, theme);
    }
}

/// Chat area + input area (vertical split).
fn render_chat_and_input(frame: &mut Frame, area: Rect, state: &AppState, theme: &Theme) {
    let input_height = 3.min(constants::MAX_INPUT_HEIGHT as u16);

    let [chat, input] = area.layout(&Layout::vertical([
        Constraint::Min(1),               // chat
        Constraint::Length(input_height), // input
    ]));

    render_conversation(frame, state, chat, theme);
    render_input(frame, state, input, theme);

    // Autocomplete popup sits above the input area. The widget computes its
    // own Y offset upward from the supplied rect, so passing the input area
    // puts it correctly floated over the chat tail.
    if let Some(ref sug) = state.ui.active_suggestions {
        let popup = crate::widgets::SuggestionPopup::new(&sug.items, sug.kind.title(), theme)
            .selected(sug.selected);
        frame.render_widget(popup, input);
    }
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
    } else if state.is_plan_mode() {
        Span::styled("! ", Style::default().fg(theme.plan_mode).bold())
    } else {
        Span::styled("> ", Style::default().fg(theme.primary))
    };

    let display_text = if state.ui.input.is_empty() {
        t!("input.placeholder").to_string()
    } else {
        state.ui.input.text.clone()
    };

    let text_style = if state.ui.input.is_empty() {
        Style::default().fg(theme.text_dim)
    } else {
        Style::default().fg(theme.text)
    };

    let title = if state.is_plan_mode() {
        format!(" {} ", t!("input.title_plan_mode"))
    } else if is_streaming {
        format!(" {} ", t!("input.title_queue"))
    } else {
        format!(" {} ", t!("input.title"))
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
        let [tools, subagents] = area.layout(&Layout::vertical([
            Constraint::Percentage(60),
            Constraint::Percentage(40),
        ]));

        render_tool_panel(frame, tools, state, theme);
        render_subagent_panel(frame, subagents, state, theme);
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
                t!(
                    "status.cache_suffix",
                    tokens = format_token_count(tokens.cache_read_tokens)
                )
                .to_string(),
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
            t!("status.mcp", count = mcp_count).to_string(),
            Style::default().fg(theme.text_dim),
        ));
    }

    // Message count
    parts.push(Span::styled(" | ", Style::default().fg(theme.border)));
    parts.push(Span::styled(
        t!("status.msgs", count = state.session.messages.len()).to_string(),
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
