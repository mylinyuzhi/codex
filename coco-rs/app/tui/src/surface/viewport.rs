//! Interactive viewport renderer for the native-scrollback surface.

use ratatui::layout::Constraint;
use ratatui::layout::Layout;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::text::Span;
use ratatui::widgets::Block;
use ratatui::widgets::Borders;
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
use crate::surface::modal::SurfaceFramePlan;
use crate::surface::modal::render_modal_surface;
use crate::surface::modal::required_text_surface_height_for_box;
use crate::surface::terminal::SurfaceFrame;
use crate::widgets::SuggestionPopup;
use crate::widgets::ToastWidget;
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

    if state.ui.has_toasts() {
        render_toasts(frame, area, &state.ui.toasts, styles);
    }

    if let Some(modal) = state.ui.modal.as_ref() {
        render_modal_surface(
            frame,
            area,
            Some(layout.input),
            modal,
            state,
            transcript_layout,
            styles,
        );
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
        interaction_pane_bottom_reservation(state, width, max_height, activity_rows, queue_rows);
    let prompt_rows = interaction_prompt_height(state, width, max_height);
    let input_height = bottom.input_height;
    let stash_rows = bottom.stash_rows;
    let bottom_height = bottom.bottom_height;
    let other_fixed_rows = activity_rows + queue_rows + prompt_rows + input_height + stash_rows;
    let fixed_rows = other_fixed_rows + bottom_height;
    let desired = fixed_rows + live_content_height.min(max_height.saturating_sub(fixed_rows));
    desired.min(max_height)
}

#[derive(Debug, Clone, Copy)]
struct InteractionPaneBottomReservation {
    input_height: u16,
    stash_rows: u16,
    bottom_height: u16,
}

fn interaction_pane_bottom_reservation(
    state: &AppState,
    _width: u16,
    max_height: u16,
    activity_rows: u16,
    queue_rows: u16,
) -> InteractionPaneBottomReservation {
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
    let prompt_rows = interaction_prompt_height(state, _width, max_height);
    let other_fixed_rows = activity_rows + queue_rows + prompt_rows + input_height + stash_rows;
    let avail_below_input = max_height.saturating_sub(other_fixed_rows);
    let bottom_height: u16 = if popup_active {
        (popup_items as u16)
            .min(SuggestionPopup::DEFAULT_MAX_VISIBLE)
            .min(avail_below_input)
            .max(status_height)
    } else {
        status_height.min(avail_below_input)
    };
    InteractionPaneBottomReservation {
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
    let prompt_rows = interaction_prompt_height(state, area.width, area.height);

    let live_lines = build_live_tail_lines(state, styles, area.width, plan);
    let live_content_height = live_lines.len() as u16;
    let inline_popup = inline_popup_view(state);
    let popup_items = inline_popup
        .as_ref()
        .map(InlinePopupView::item_count)
        .unwrap_or(0);
    let popup_active = popup_items > 0;
    let status_height: u16 = 1;
    let other_fixed_rows = activity_rows + queue_rows + prompt_rows + input_height + stash_rows;
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
        prompt,
        input,
        stash,
        bottom,
    ] = area.layout(&Layout::vertical([
        Constraint::Min(0),
        Constraint::Length(live_tail_height),
        Constraint::Length(activity_rows),
        Constraint::Length(queue_rows),
        Constraint::Length(prompt_rows),
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
    if prompt_rows > 0 {
        render_interaction_prompt(frame, prompt, state, styles);
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
    // Phase 3d (§4): the renderer consumes engine cells directly.
    // Compatibility-fallback mode keeps finalized history inside the
    // viewport; otherwise the native scrollback owns it and this layer
    // renders an empty tail.
    let committed_cells: &[crate::state::transcript_view::RenderedCell] =
        if plan.finalized_history_in_viewport() {
            state.session.transcript.cells()
        } else {
            &[]
        };
    let mut chat = crate::widgets::ChatWidget::new(committed_cells, styles)
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
    let input = crate::widgets::InputWidget::new(&state.ui.input, styles)
        .focused(is_focused)
        .plan_mode(state.is_plan_mode())
        .is_streaming(state.is_streaming())
        .prompt_suggestion(state.session.prompt_suggestions.last().map(String::as_str))
        .has_editable_queue(!state.session.queued_commands.is_empty())
        .command_palette_filter(None);

    frame.render_widget(input, area);
}

fn interaction_prompt_box_width(area_width: u16) -> u16 {
    area_width.min(constants::MAX_INTERACTION_PROMPT_WIDTH)
}

/// Center a fixed-width strip inside `area`, preserving full height. Unlike
/// `layout::centered_fixed_area` (which subtracts 2 from height for modal
/// margins), this keeps every row available for the prompt body.
fn center_horizontally(area: Rect, box_width: u16) -> Rect {
    let width = box_width.min(area.width);
    let x_offset = area.width.saturating_sub(width) / 2;
    Rect::new(area.x + x_offset, area.y, width, area.height)
}

fn interaction_prompt_height(state: &AppState, width: u16, max_height: u16) -> u16 {
    let Some(prompt) = state.ui.interaction.active_prompt.as_ref() else {
        return 0;
    };
    let styles = UiStyles::new(&state.ui.theme);
    let text_surface = crate::surface_content::prompt_text_surface(prompt);
    let box_width = interaction_prompt_box_width(width);
    required_text_surface_height_for_box(text_surface, state, styles, box_width, max_height)
        .min(max_height.saturating_sub(4))
        .max(3)
}

fn render_interaction_prompt(
    frame: &mut SurfaceFrame<'_>,
    area: Rect,
    state: &AppState,
    styles: UiStyles<'_>,
) {
    if area.height == 0 {
        return;
    }
    let Some(prompt) = state.ui.interaction.active_prompt.as_ref() else {
        return;
    };
    let text_surface = crate::surface_content::prompt_text_surface(prompt);
    let (title, body, border_color) =
        crate::surface_content::surface_content(text_surface, state, styles);
    let body = compact_prompt_body(&body, area.height.saturating_sub(2) as usize);
    let box_area = center_horizontally(area, interaction_prompt_box_width(area.width));
    frame.render_widget(Clear, box_area);
    frame.render_widget(
        Paragraph::new(body).wrap(Wrap { trim: false }).block(
            Block::default()
                .borders(Borders::ALL)
                .title(title)
                .border_style(Style::default().fg(border_color)),
        ),
        box_area,
    );
}

fn compact_prompt_body(body: &str, max_lines: usize) -> String {
    let lines = body.lines().collect::<Vec<_>>();
    if max_lines == 0 || lines.len() <= max_lines {
        return body.to_string();
    }
    if max_lines == 1 {
        return lines.last().copied().unwrap_or_default().to_string();
    }
    if max_lines == 2 {
        return format!(
            "{}\n{}",
            lines.first().copied().unwrap_or_default(),
            lines.last().copied().unwrap_or_default()
        );
    }
    if let Some(blank_idx) = lines.iter().rposition(|line| line.trim().is_empty()) {
        let tail = &lines[blank_idx.saturating_add(1)..];
        if !tail.is_empty() {
            if tail.len() < max_lines {
                let head_count = max_lines.saturating_sub(tail.len() + 1);
                let mut compact = Vec::new();
                compact.extend(lines.iter().take(head_count).copied());
                compact.push("...");
                compact.extend(tail.iter().copied());
                return compact.join("\n");
            }
            if tail.len() == max_lines {
                return tail.join("\n");
            }
        }
    }
    let tail_count = (max_lines / 2).max(1);
    let head_count = max_lines.saturating_sub(tail_count + 1).max(1);
    let mut compact = Vec::new();
    compact.extend(lines.iter().take(head_count).copied());
    compact.push("...");
    compact.extend(lines.iter().rev().take(tail_count).rev().copied());
    compact.join("\n")
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
    toasts: &std::collections::VecDeque<crate::state::Toast>,
    styles: UiStyles<'_>,
) {
    let owned = toasts.iter().cloned().collect::<Vec<_>>();
    frame.render_widget(ToastWidget::new(&owned, styles), area);
}

#[cfg(test)]
#[path = "viewport.test.rs"]
mod tests;
