//! Interactive viewport renderer for the native-scrollback surface.

use ratatui::layout::Constraint;
use ratatui::layout::Layout;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::text::Span;
use ratatui::widgets::Clear;
use ratatui::widgets::Paragraph;
use ratatui::widgets::Wrap;

use crate::FrameLayout;
use crate::constants;
use crate::presentation::activity::TurnActivityView;
use crate::presentation::activity::inline_activity_height;
use crate::presentation::activity::turn_activity_view;
use crate::presentation::footer::FooterSpan;
use crate::presentation::footer::FooterTone;
use crate::presentation::footer::FooterView;
use crate::presentation::footer::footer_view;
use crate::presentation::input::InlinePopupView;
use crate::presentation::input::inline_popup_view;
use crate::presentation::styles::UiStyles;
use crate::state::AppState;
use crate::state::FocusTarget;
use crate::state::Toast;
use crate::state::ToastSeverity;
use crate::surface::overlay::OverlaySurfacePlacement;
use crate::surface::overlay::SurfaceFramePlan;
use crate::surface::overlay::render_surface_overlay;
use crate::surface::overlay::required_overlay_height;
use crate::surface::terminal::SurfaceFrame;
use crate::widgets::SuggestionPopup;
use crate::widgets::TranscriptLayoutIndex;

/// Render the retained native-scrollback viewport.
///
/// Finalized transcript messages normally live above the viewport in native
/// scrollback. Compatibility fallback mode renders them inside the retained
/// viewport instead so terminals without usable scrollback do not drop history.
pub(crate) fn render_interactive_viewport(
    frame: &mut SurfaceFrame<'_>,
    state: &AppState,
    plan: SurfaceFramePlan,
    transcript_layout: &mut TranscriptLayoutIndex,
) -> FrameLayout {
    let mut layout = FrameLayout::default();
    let area = frame.area();
    let styles = UiStyles::new(&state.ui.theme);

    render_live_viewport(frame, area, state, styles, plan, &mut layout);

    if let Some(overlay) = state.ui.active_overlay() {
        render_surface_overlay(
            frame,
            area,
            Some(layout.input),
            overlay,
            state,
            transcript_layout,
            styles,
        );
    }

    if state.ui.has_toasts() {
        render_toasts(frame, area, &state.ui.toasts, styles);
    }

    layout
}

pub(crate) fn interactive_viewport_desired_height(
    state: &AppState,
    width: u16,
    max_height: u16,
    plan: SurfaceFramePlan,
) -> u16 {
    if width == 0 || max_height == 0 {
        return 0;
    }

    let styles = UiStyles::new(&state.ui.theme);
    let live_content_height = build_live_tail_lines(state, styles, width, plan).len() as u16;
    let activity = turn_activity_view(state, width);
    let activity_rows = inline_activity_height(&activity, max_height, width);
    let queue_rows: u16 =
        if crate::widgets::QueueStatusWidget::should_display(&state.session.queued_commands) {
            1
        } else {
            0
        };
    let bottom =
        inline_decision_bottom_reservation(state, width, max_height, activity_rows, queue_rows);
    let input_height = bottom.input_height;
    let stash_rows = bottom.stash_rows;
    let bottom_height = bottom.bottom_height;
    let other_fixed_rows = activity_rows + queue_rows + input_height + stash_rows;
    let fixed_rows = other_fixed_rows + bottom_height;
    let desired = fixed_rows + live_content_height.min(max_height.saturating_sub(fixed_rows));
    let overlay_height = state
        .ui
        .active_overlay()
        .filter(|_| {
            plan.overlay_placement.is_some_and(|placement| {
                matches!(placement, OverlaySurfacePlacement::InlineDecision)
            })
        })
        .map(|overlay| required_overlay_height(overlay, state, styles, width, max_height))
        .unwrap_or(0);
    let protected_bottom_rows = input_height + stash_rows + bottom_height;
    desired
        .max(overlay_height.saturating_add(protected_bottom_rows))
        .min(max_height)
}

pub(crate) fn inline_decision_protected_bottom_rows(
    state: &AppState,
    width: u16,
    max_height: u16,
) -> u16 {
    let activity = turn_activity_view(state, width);
    let activity_rows = inline_activity_height(&activity, max_height, width);
    let queue_rows: u16 =
        if crate::widgets::QueueStatusWidget::should_display(&state.session.queued_commands) {
            1
        } else {
            0
        };
    let bottom =
        inline_decision_bottom_reservation(state, width, max_height, activity_rows, queue_rows);
    bottom.input_height + bottom.stash_rows + bottom.bottom_height
}

#[derive(Debug, Clone, Copy)]
struct InlineDecisionBottomReservation {
    input_height: u16,
    stash_rows: u16,
    bottom_height: u16,
}

fn inline_decision_bottom_reservation(
    state: &AppState,
    _width: u16,
    max_height: u16,
    activity_rows: u16,
    queue_rows: u16,
) -> InlineDecisionBottomReservation {
    let stash_rows: u16 =
        if crate::widgets::StashNotice::should_display(state.ui.stashed_input.as_ref()) {
            1
        } else {
            0
        };
    let input_height = 3.min(constants::MAX_INPUT_HEIGHT as u16);
    let inline_popup = inline_popup_view(state);
    let popup_items = inline_popup
        .as_ref()
        .map(InlinePopupView::item_count)
        .unwrap_or(0);
    let popup_active = popup_items > 0;
    let status_height: u16 = 1;
    let other_fixed_rows = activity_rows + queue_rows + input_height + stash_rows;
    let avail_below_input = max_height.saturating_sub(other_fixed_rows);
    let bottom_height: u16 = if popup_active {
        (popup_items as u16)
            .min(SuggestionPopup::DEFAULT_MAX_VISIBLE)
            .min(avail_below_input)
            .max(status_height)
    } else {
        status_height.min(avail_below_input)
    };
    InlineDecisionBottomReservation {
        input_height,
        stash_rows,
        bottom_height,
    }
}

fn render_live_viewport(
    frame: &mut SurfaceFrame<'_>,
    area: Rect,
    state: &AppState,
    styles: UiStyles<'_>,
    plan: SurfaceFramePlan,
    layout: &mut FrameLayout,
) {
    let input_height = 3.min(constants::MAX_INPUT_HEIGHT as u16);
    let activity = turn_activity_view(state, area.width);
    let activity_rows = inline_activity_height(&activity, area.height, area.width);
    let queue_rows: u16 =
        if crate::widgets::QueueStatusWidget::should_display(&state.session.queued_commands) {
            1
        } else {
            0
        };
    let stash_rows: u16 =
        if crate::widgets::StashNotice::should_display(state.ui.stashed_input.as_ref()) {
            1
        } else {
            0
        };

    let live_lines = build_live_tail_lines(state, styles, area.width, plan);
    let live_content_height = live_lines.len() as u16;
    let inline_popup = inline_popup_view(state);
    let popup_items = inline_popup
        .as_ref()
        .map(InlinePopupView::item_count)
        .unwrap_or(0);
    let popup_active = popup_items > 0;
    let status_height: u16 = 1;
    let other_fixed_rows = activity_rows + queue_rows + input_height + stash_rows;
    let avail_below_input = area.height.saturating_sub(other_fixed_rows);
    let bottom_height: u16 = if popup_active {
        (popup_items as u16)
            .min(SuggestionPopup::DEFAULT_MAX_VISIBLE)
            .min(avail_below_input)
            .max(status_height)
    } else {
        status_height.min(avail_below_input)
    };
    let avail_for_live_tail = avail_below_input.saturating_sub(bottom_height);
    let live_tail_height = live_content_height.min(avail_for_live_tail);

    let [
        _filler,
        live_tail,
        activity_area,
        queue,
        input,
        stash,
        bottom,
    ] = area.layout(&Layout::vertical([
        Constraint::Min(0),
        Constraint::Length(live_tail_height),
        Constraint::Length(activity_rows),
        Constraint::Length(queue_rows),
        Constraint::Length(input_height),
        Constraint::Length(stash_rows),
        Constraint::Length(bottom_height),
    ]));

    layout.input = input;

    render_live_tail_lines(
        frame,
        live_tail,
        live_lines,
        live_content_height,
        avail_for_live_tail,
        state,
    );
    if activity_area.height > 0 && matches!(&activity, TurnActivityView::Surface(_)) {
        frame.render_widget(
            crate::widgets::ActivityPanel::new(activity, styles),
            activity_area,
        );
    }
    if queue_rows > 0 {
        frame.render_widget(
            crate::widgets::QueueStatusWidget::new(&state.session.queued_commands, styles),
            queue,
        );
    }
    render_input(frame, state, input, styles);
    if stash_rows > 0
        && let Some(s) = state.ui.stashed_input.as_ref()
    {
        frame.render_widget(crate::widgets::StashNotice::new(s, styles), stash);
    }

    if popup_active {
        let anchor = Rect::new(bottom.x, bottom.y + bottom.height, bottom.width, 0);
        if let Some(popup_view) = inline_popup {
            let popup = crate::widgets::SuggestionPopup::new(&popup_view.items, styles)
                .selected(popup_view.selected)
                .max_visible(bottom_height as usize);
            frame.render_widget(popup, anchor);
        }
    } else if bottom_height > 0 {
        render_status_bar(frame, bottom, state, styles);
    }
}

fn build_live_tail_lines(
    state: &AppState,
    styles: UiStyles<'_>,
    width: u16,
    plan: SurfaceFramePlan,
) -> Vec<Line<'static>> {
    let committed_messages = if plan.finalized_history_in_viewport() {
        state.session.messages.as_slice()
    } else {
        &[]
    };
    let mut chat = crate::widgets::ChatWidget::new(committed_messages, styles)
        .scroll(state.ui.scroll_offset)
        .streaming(state.ui.streaming.as_ref())
        .show_thinking(state.ui.show_thinking)
        .show_system_reminders(state.ui.show_system_reminders)
        .tool_executions(&state.session.tool_executions)
        .width(width)
        .syntax_highlighting(state.ui.display_settings.syntax_highlighting)
        .kb_handle(&state.ui.kb_handle);
    if !state.ui.collapsed_tools.is_empty() {
        chat = chat.collapsed_tools(&state.ui.collapsed_tools);
    }
    chat.build_lines_owned()
}

fn render_live_tail_lines(
    frame: &mut SurfaceFrame<'_>,
    area: Rect,
    lines: Vec<Line<'static>>,
    content_height: u16,
    avail: u16,
    state: &AppState,
) {
    if area.height == 0 {
        return;
    }
    let scroll_offset = if content_height > avail {
        let overflow = content_height - avail;
        if state.ui.user_scrolled {
            state.ui.scroll_offset.max(0).min(overflow as i32) as u16
        } else {
            overflow
        }
    } else {
        0
    };
    let paragraph = Paragraph::new(lines)
        .wrap(Wrap { trim: false })
        .scroll((scroll_offset, 0));
    frame.render_widget(paragraph, area);
}

fn render_input(frame: &mut SurfaceFrame<'_>, state: &AppState, area: Rect, styles: UiStyles<'_>) {
    let is_focused = state.ui.focus == FocusTarget::Input;
    let command_palette_filter: Option<&str> = match state.ui.active_overlay() {
        Some(crate::state::Overlay::CommandPalette(cp)) => Some(cp.filter.as_str()),
        _ => None,
    };
    let input = crate::widgets::InputWidget::new(&state.ui.input, styles)
        .focused(is_focused)
        .plan_mode(state.is_plan_mode())
        .is_streaming(state.is_streaming())
        .prompt_suggestion(state.session.prompt_suggestions.last().map(String::as_str))
        .has_editable_queue(!state.session.queued_commands.is_empty())
        .command_palette_filter(command_palette_filter);

    frame.render_widget(input, area);
}

fn render_status_bar(
    frame: &mut SurfaceFrame<'_>,
    area: Rect,
    state: &AppState,
    styles: UiStyles<'_>,
) {
    let view = footer_view(state);
    let line = match view {
        FooterView::ExitPrompt { key, text } => {
            tracing::info!(
                key = key.label(),
                prompt = %text,
                width = area.width,
                "status bar rendering exit prompt"
            );
            Line::from(Span::styled(
                text,
                Style::default().fg(styles.warning()).bold(),
            ))
        }
        FooterView::Status { spans } => Line::from(
            spans
                .iter()
                .map(|span| footer_span(span, styles))
                .collect::<Vec<_>>(),
        ),
    };
    frame.render_widget(Paragraph::new(line), area);
}

fn footer_span(span: &FooterSpan, styles: UiStyles<'_>) -> Span<'static> {
    let color = match span.tone {
        FooterTone::Primary => styles.primary(),
        FooterTone::Dim => styles.dim(),
        FooterTone::Border => styles.border(),
        FooterTone::Warning => styles.warning(),
        FooterTone::Accent => styles.accent(),
        FooterTone::Plan => styles.plan(),
        FooterTone::Error => styles.error(),
    };
    let rendered = Span::styled(span.text.clone(), Style::default().fg(color));
    if span.bold { rendered.bold() } else { rendered }
}

fn render_toasts(
    frame: &mut SurfaceFrame<'_>,
    area: Rect,
    toasts: &std::collections::VecDeque<Toast>,
    styles: UiStyles<'_>,
) {
    let toast_width: u16 = 40;
    let mut y = area.y.saturating_add(1);
    let max_y = area.bottom().saturating_sub(2);

    for toast in toasts.iter() {
        if y >= max_y {
            break;
        }

        let (icon, color) = match toast.severity {
            ToastSeverity::Info => ("ℹ", styles.dim()),
            ToastSeverity::Success => ("✓", styles.success()),
            ToastSeverity::Warning => ("⚠", styles.warning()),
            ToastSeverity::Error => ("✗", styles.error()),
        };

        let x = area.x + area.width.saturating_sub(toast_width + 1);
        let toast_area = Rect::new(x, y, toast_width, 1);

        let text = format!(" {icon} {} ", toast.message);
        let span = Span::styled(text, Style::default().fg(color));
        frame.render_widget(Clear, toast_area);
        frame.render_widget(Paragraph::new(span), toast_area);

        y += 1;
    }
}

#[cfg(test)]
#[path = "viewport.test.rs"]
mod tests;
