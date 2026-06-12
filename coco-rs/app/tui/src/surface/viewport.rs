//! Interactive viewport renderer for the native-scrollback surface.

use ratatui::layout::Constraint;
use ratatui::layout::Layout;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::Line;
use ratatui::widgets::Block;
use ratatui::widgets::Borders;
use ratatui::widgets::Clear;
use ratatui::widgets::Paragraph;
use ratatui::widgets::Wrap;

use crate::FrameLayout;
use crate::presentation::activity::TurnActivityView;
use crate::presentation::activity::inline_activity_height;
use crate::presentation::activity::turn_activity_view;
use crate::presentation::input::InlinePopupView;
use crate::presentation::input::inline_popup_view;
use crate::state::AppState;
use crate::state::FocusTarget;
use crate::state::PanePromptState;
use crate::surface::modal::SurfaceFramePlan;
use crate::surface::modal::render_modal_surface;
use crate::surface::modal::required_text_surface_height_for_box;
use crate::widgets::SuggestionPopup;
use crate::widgets::ToastWidget;
use crate::widgets::TranscriptLayoutIndex;
use coco_tui_ui::constants;
use coco_tui_ui::engine::terminal::SurfaceFrame;
use coco_tui_ui::style::UiStyles;

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
    precomputed_live: Option<Vec<Line<'static>>>,
) -> FrameLayout {
    let mut layout = FrameLayout::default();
    let area = frame.area();
    let styles = UiStyles::new(&state.ui.theme);

    render_live_viewport(
        frame,
        area,
        state,
        styles,
        plan,
        &mut layout,
        precomputed_live,
    );

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
    precomputed_live_height: Option<u16>,
) -> u16 {
    if width == 0 || max_height == 0 {
        return 0;
    }

    // The live tail is built once per frame in `Tui::draw` and threaded in
    // here as `precomputed_live_height`; only fall back to rebuilding it when
    // no precomputed value is supplied (alt-screen / tests).
    let live_content_height = precomputed_live_height.unwrap_or_else(|| {
        let styles = UiStyles::new(&state.ui.theme);
        build_live_tail_lines(state, styles, width, plan).len() as u16
    });
    let activity = turn_activity_view(state, width);
    let activity_rows = inline_activity_height(&activity, max_height, width);
    let queue_rows: u16 =
        if crate::widgets::QueueStatusWidget::should_display(&state.session.queued_commands) {
            1
        } else {
            0
        };
    // Mirror render_live_viewport's reservations so the sizing pass and the
    // paint pass agree on viewport height (these were previously omitted here).
    let status_indicator_rows: u16 =
        if state.ui.ephemeral.turn_active() || state.session.is_compacting {
            1
        } else {
            0
        };
    let background_pills_rows: u16 =
        if crate::widgets::build_background_pills_view(state).is_empty() {
            0
        } else {
            1
        };
    let bottom = interaction_pane_bottom_reservation(
        state,
        width,
        max_height,
        status_indicator_rows,
        background_pills_rows,
        activity_rows,
        queue_rows,
    );
    let prompt_rows = interaction_prompt_height(state, width, max_height);
    let input_height = bottom.input_height;
    let stash_rows = bottom.stash_rows;
    let bottom_height = bottom.bottom_height;
    let other_fixed_rows = status_indicator_rows
        + background_pills_rows
        + activity_rows
        + queue_rows
        + prompt_rows
        + input_height
        + stash_rows;
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

fn input_height_for_state(state: &AppState) -> u16 {
    if matches!(
        state.ui.interaction.active_prompt,
        Some(PanePromptState::Question(_))
    ) {
        0
    } else {
        3.min(constants::MAX_INPUT_HEIGHT as u16)
    }
}

#[allow(clippy::too_many_arguments)]
fn interaction_pane_bottom_reservation(
    state: &AppState,
    _width: u16,
    max_height: u16,
    status_indicator_rows: u16,
    background_pills_rows: u16,
    activity_rows: u16,
    queue_rows: u16,
) -> InteractionPaneBottomReservation {
    let stash_rows: u16 =
        if crate::widgets::StashNotice::should_display(state.ui.stashed_input.as_ref()) {
            1
        } else {
            0
        };
    let input_height = input_height_for_state(state);
    let inline_popup = inline_popup_view(state);
    let popup_items = inline_popup
        .as_ref()
        .map(InlinePopupView::item_count)
        .unwrap_or(0);
    let popup_active = popup_items > 0;
    let status_height: u16 = 1;
    let prompt_rows = interaction_prompt_height(state, _width, max_height);
    // Match render_live_viewport's avail_below_input base exactly (it subtracts
    // the status-indicator and background-pill rows too) so the sizing pass and
    // the paint pass derive the same bottom_height when a popup is active.
    let other_fixed_rows = status_indicator_rows
        + background_pills_rows
        + activity_rows
        + queue_rows
        + prompt_rows
        + input_height
        + stash_rows;
    let avail_below_input = max_height.saturating_sub(other_fixed_rows);
    let bottom_height: u16 = if popup_active {
        popup_row_budget(avail_below_input)
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
    precomputed_live: Option<Vec<Line<'static>>>,
) {
    let input_height = input_height_for_state(state);
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

    let live_lines =
        precomputed_live.unwrap_or_else(|| build_live_tail_lines(state, styles, area.width, plan));
    let live_content_height = live_lines.len() as u16;
    let inline_popup = inline_popup_view(state);
    let popup_items = inline_popup
        .as_ref()
        .map(InlinePopupView::item_count)
        .unwrap_or(0);
    let popup_active = popup_items > 0;
    let status_height: u16 = 1;
    // Single-row main-turn status indicator (spinner + verb + elapsed
    // + tokens) above the activity panel — visible only while a turn
    // is running.
    let status_indicator_rows: u16 =
        if state.ui.ephemeral.turn_active() || state.session.is_compacting {
            1
        } else {
            0
        };
    // Background pills bar — shown when any subagent is backgrounded.
    // TS keeps the row populated for the lifetime of the backgrounded
    // task (completed teammates render with `is_idle = true`); we
    // mirror that — no completion-flash window.
    let pills_view = crate::widgets::build_background_pills_view(state);
    let background_pills_rows: u16 = if pills_view.is_empty() { 0 } else { 1 };
    let other_fixed_rows = status_indicator_rows
        + activity_rows
        + background_pills_rows
        + queue_rows
        + prompt_rows
        + input_height
        + stash_rows;
    let avail_below_input = area.height.saturating_sub(other_fixed_rows);
    let bottom_height: u16 = if popup_active {
        popup_row_budget(avail_below_input)
    } else {
        status_height.min(avail_below_input)
    };
    let avail_for_live_tail = avail_below_input.saturating_sub(bottom_height);
    let live_tail_height = live_content_height.min(avail_for_live_tail);

    let [
        _filler,
        live_tail,
        status_indicator_area,
        activity_area,
        background_pills_area,
        queue,
        prompt,
        input,
        stash,
        bottom,
    ] = area.layout(&Layout::vertical([
        Constraint::Min(0),
        Constraint::Length(live_tail_height),
        Constraint::Length(status_indicator_rows),
        Constraint::Length(activity_rows),
        Constraint::Length(background_pills_rows),
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
    if status_indicator_area.height > 0
        && (state.ui.ephemeral.turn_active() || state.session.is_compacting)
    {
        // Elapsed reads from the UI ephemeral helper so the displayed
        // clock subtracts paused time (permission-prompt blocks). The
        // turn-start anchor is on the running-turn record itself —
        // no Option<Instant> threading required.
        let view = if state.ui.ephemeral.turn_active() {
            let elapsed_ms = state.ui.ephemeral.elapsed_ms(std::time::Instant::now());
            let effort = state.session.thinking_effort;
            let effort_level = effort.is_explicit_level().then(|| effort.as_str());
            // TS `hasRunningTeammates` derives from
            // `tasks.some(t => isInProcessTeammateTask(t) && !t.isIdle)`. In
            // coco-rs an "in-process teammate" maps to
            // `SubagentKind::Teammate` and the closest "not idle" predicate
            // is `status == Running`.
            let has_running_teammates = state.session.subagents.iter().any(|a| {
                matches!(a.kind, crate::state::SubagentKind::Teammate)
                    && matches!(a.status, crate::state::session::SubagentStatus::Running)
            });
            let teammate_tokens: i64 = state
                .session
                .subagents
                .iter()
                .filter(|a| {
                    matches!(a.kind, crate::state::SubagentKind::Teammate)
                        && matches!(a.status, crate::state::session::SubagentStatus::Running)
                })
                .map(|a| a.total_tokens)
                .sum();
            coco_tui_ui::widgets::StatusIndicatorView {
                verb: state.ui.ephemeral.current_verb().unwrap_or("Working"),
                elapsed_ms,
                input_tokens: None,
                output_tokens: state
                    .ui
                    .ephemeral
                    .live_output_tokens()
                    .saturating_add(teammate_tokens),
                effort_level,
                show_interrupt_hint: true,
                force_show_tokens: false,
                has_running_teammates,
            }
        } else {
            let elapsed_ms = state
                .session
                .compaction_started_at
                .map(|started| started.elapsed().as_millis() as i64)
                .unwrap_or(0);
            coco_tui_ui::widgets::StatusIndicatorView {
                verb: state
                    .session
                    .compaction_phase
                    .map(crate::state::session::CompactionPhaseLabel::status_label)
                    .unwrap_or("Compacting conversation"),
                elapsed_ms,
                input_tokens: None,
                output_tokens: 0,
                effort_level: None,
                show_interrupt_hint: true,
                force_show_tokens: false,
                has_running_teammates: false,
            }
        };
        frame.render_widget(
            coco_tui_ui::widgets::StatusIndicator::new(view, styles),
            status_indicator_area,
        );
    }
    if activity_area.height > 0 && matches!(&activity, TurnActivityView::Surface(_)) {
        frame.render_widget(
            crate::widgets::ActivityPanel::new(activity, styles),
            activity_area,
        );
    }
    if background_pills_area.height > 0 {
        frame.render_widget(
            crate::widgets::BackgroundPills::new(&pills_view, styles),
            background_pills_area,
        );
    }
    if queue_rows > 0 {
        frame.render_widget(
            crate::widgets::QueueStatusWidget::new(&state.session.queued_commands, styles),
            queue,
        );
    }
    if prompt_rows > 0 {
        if matches!(
            state.ui.interaction.active_prompt,
            Some(PanePromptState::Question(_))
        ) {
            layout.question_prompt = prompt;
        }
        render_interaction_prompt(frame, prompt, state, styles);
    }
    render_input(frame, state, input, styles);
    if stash_rows > 0
        && let Some(s) = state.ui.stashed_input.as_ref()
    {
        frame.render_widget(crate::widgets::StashNotice::new(s, styles), stash);
    }

    if popup_active {
        if let Some(popup_view) = inline_popup {
            let popup = crate::widgets::SuggestionPopup::new(popup_view.items, styles)
                .selected(popup_view.selected)
                .max_visible(bottom_height as usize);
            frame.render_widget(popup, bottom);
        }
    } else if bottom_height > 0 {
        frame.render_widget(
            crate::status_bar::StatusBarWidget::new(state, styles),
            bottom,
        );
    }
}

fn popup_row_budget(avail_below_input: u16) -> u16 {
    SuggestionPopup::DEFAULT_MAX_VISIBLE.min(avail_below_input)
}

pub(crate) fn build_live_tail_lines(
    state: &AppState,
    styles: UiStyles<'_>,
    width: u16,
    plan: SurfaceFramePlan,
) -> Vec<Line<'static>> {
    // Phase 3d (§4): the renderer consumes engine cells directly.
    // Compatibility-fallback mode keeps finalized history inside the
    // viewport; otherwise the native scrollback owns it and this layer
    // renders an empty tail.
    let committed_cells: &[crate::transcript::cells::RenderedCell] =
        if plan.finalized_history_in_viewport() {
            state.session.transcript.cells()
        } else {
            &[]
        };
    let mut chat = crate::transcript::render::CellsRenderer::new(committed_cells, styles)
        .streaming(state.ui.streaming.as_ref())
        .show_thinking(state.ui.show_thinking)
        .show_system_reminders(state.ui.show_system_reminders)
        .tool_executions(&state.session.tool_executions)
        .width(width)
        .syntax_highlighting(state.ui.display_settings.syntax_highlighting)
        .cwd(state.session.working_dir.as_deref())
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
        .streaming(state.is_streaming())
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
    if let PanePromptState::Question(q) = prompt {
        return question_prompt_max_height(q, width, styles)
            .min(max_height.saturating_sub(4))
            .max(3);
    }
    let box_width = interaction_prompt_box_width(width);
    let Some(text_surface) = crate::surface_content::prompt_text_surface(prompt) else {
        return 0;
    };
    required_text_surface_height_for_box(text_surface, state, styles, box_width, max_height)
        .min(max_height.saturating_sub(4))
        .max(3)
}

fn question_prompt_max_height(
    q: &crate::state::QuestionPromptState,
    box_width: u16,
    styles: UiStyles<'_>,
) -> u16 {
    use crate::state::QuestionPage;

    // One clone reused across pages (this runs several times per frame while
    // a question prompt is open): `set_question_page` overwrites the page and
    // focus deterministically from the untouched `questions` data, so
    // re-pointing one projection copy is equivalent to cloning per page.
    let mut projected = q.clone();
    let mut max_height = 0;
    for idx in 0..q.questions.len() {
        projected.set_question_page(idx);
        let view = crate::presentation::request::project_question(&projected);
        max_height = max_height.max(view.desired_height(box_width, styles));
    }
    if q.questions.len() > 1 {
        projected.current_question = QuestionPage::Submit;
        projected.focus_target = crate::state::QuestionFocusTarget::SubmitAction(
            crate::state::SubmitAction::SubmitAnswers,
        );
        projected.sync_other_focus();
        let view = crate::presentation::request::project_question(&projected);
        max_height = max_height.max(view.desired_height(box_width, styles));
    }
    max_height
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
    // AskUserQuestion renders through the dedicated area-based widget, pinned to
    // the lower-left above the composer (mirrors TS/codex bottom-pane) instead
    // of horizontally centered like the modal text prompts below.
    if let PanePromptState::Question(q) = prompt {
        let view = crate::presentation::request::project_question(q);
        frame.render_widget(Clear, area);
        frame.render_widget(
            coco_tui_ui::widgets::QuestionWidget::new(&view, styles),
            area,
        );
        return;
    }

    let box_area = center_horizontally(area, interaction_prompt_box_width(area.width));
    if let PanePromptState::Permission(p) = prompt {
        let (title, lines, border_color) = crate::presentation::request::permission_styled_content(
            p,
            state.session.permission_mode,
            styles,
        );
        let lines = compact_prompt_lines(lines, area.height.saturating_sub(2) as usize);
        frame.render_widget(Clear, box_area);
        frame.render_widget(
            Paragraph::new(lines).wrap(Wrap { trim: false }).block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(title)
                    .border_style(Style::default().fg(border_color)),
            ),
            box_area,
        );
        return;
    }

    let Some(text_surface) = crate::surface_content::prompt_text_surface(prompt) else {
        return;
    };
    let (title, body, border_color) =
        crate::surface_content::surface_content(text_surface, state, styles);
    let body = compact_prompt_body(&body, area.height.saturating_sub(2) as usize);
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

fn compact_prompt_lines(lines: Vec<Line<'static>>, max_lines: usize) -> Vec<Line<'static>> {
    compact_sequence(
        lines,
        max_lines,
        |line| line.spans.iter().all(|span| span.content.trim().is_empty()),
        || Line::from("..."),
    )
}

/// Shrink `items` to fit `max_lines`, shared by the styled (`Line`) and plain
/// (`&str`) prompt-compaction paths. Prefers keeping the whole trailing block
/// after the last blank separator (so a prompt's action rows always survive)
/// and otherwise keeps a head slice + an `ellipsis()` marker + a tail slice.
fn compact_sequence<T: Clone>(
    items: Vec<T>,
    max_lines: usize,
    is_blank: impl Fn(&T) -> bool,
    ellipsis: impl Fn() -> T,
) -> Vec<T> {
    if max_lines == 0 || items.len() <= max_lines {
        return items;
    }
    if max_lines == 1 {
        return items.into_iter().last().into_iter().collect();
    }
    if max_lines == 2 {
        let mut iter = items.into_iter();
        let first = iter.next();
        let last = iter.last().or_else(|| first.clone());
        return first.into_iter().chain(last).collect();
    }
    if let Some(blank_idx) = items.iter().rposition(&is_blank) {
        let tail_len = items.len().saturating_sub(blank_idx + 1);
        if tail_len > 0 {
            if tail_len < max_lines {
                let head_count = max_lines.saturating_sub(tail_len + 1);
                let mut compact = Vec::new();
                compact.extend(items.iter().take(head_count).cloned());
                compact.push(ellipsis());
                compact.extend(items.iter().skip(blank_idx + 1).cloned());
                return compact;
            }
            if tail_len == max_lines {
                return items.into_iter().skip(blank_idx + 1).collect();
            }
        }
    }
    let tail_count = (max_lines / 2).max(1);
    let head_count = max_lines.saturating_sub(tail_count + 1);
    let mut compact = Vec::new();
    compact.extend(items.iter().take(head_count).cloned());
    compact.push(ellipsis());
    compact.extend(
        items
            .iter()
            .skip(items.len().saturating_sub(tail_count))
            .cloned(),
    );
    compact
}

fn compact_prompt_body(body: &str, max_lines: usize) -> String {
    if max_lines == 0 || body.lines().count() <= max_lines {
        return body.to_string();
    }
    let lines = body.lines().map(str::to_owned).collect::<Vec<_>>();
    compact_sequence(
        lines,
        max_lines,
        |line| line.trim().is_empty(),
        || "...".to_string(),
    )
    .join("\n")
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
