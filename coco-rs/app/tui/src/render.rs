//! TUI rendering — the View in TEA.
//!
//! Pure function: takes immutable state, renders to frame.
//! No side effects except pixel drawing.

use ratatui::prelude::*;
use ratatui::widgets::Clear;
use ratatui::widgets::Paragraph;
use ratatui::widgets::Wrap;

use crate::constants;
use crate::presentation::activity::TurnActivityView;
use crate::presentation::activity::inline_activity_height;
use crate::presentation::activity::turn_activity_view;
use crate::presentation::footer::FooterSpan;
use crate::presentation::footer::FooterTone;
use crate::presentation::footer::FooterView;
use crate::presentation::footer::footer_view;
use crate::presentation::header::HEADER_HEIGHT;
use crate::presentation::header::header_areas;
use crate::presentation::header::header_bar_view;
use crate::presentation::input::InlinePopupView;
use crate::presentation::input::inline_popup_view;
use crate::presentation::styles::UiStyles;
use crate::render_overlays;
use crate::state::AppState;
use crate::state::FocusTarget;
use crate::state::Toast;
use crate::state::ToastSeverity;
use crate::widgets::SuggestionPopup;

/// Layout slots produced by `render` for downstream consumers.
///
/// Today only the cursor decision (in `crate::cursor`) reads this — it
/// needs the `input` Rect to compute the cursor position after draw.
#[derive(Debug, Default, Clone, Copy)]
pub struct FrameLayout {
    /// Bordered input widget rect. `Rect::default()` when render did not
    /// reach the input (e.g. an overlay covers the full screen).
    pub input: Rect,
}

/// Render the full TUI layout.
///
/// Returns the [`FrameLayout`] for downstream consumers (cursor pin,
/// future desired-height calc). The cursor decision in `crate::cursor`
/// reads `layout.input` after this returns; ratatui's frame is fully
/// painted by the time we return so it's safe to do post-draw side
/// effects keyed off the layout.
pub fn render(frame: &mut Frame, state: &AppState) -> FrameLayout {
    let mut layout = FrameLayout::default();
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
    let verification_nudge_rows: u16 = if crate::widgets::VerificationNudgeBanner::should_display(
        state.session.verification_nudge_pending,
    ) {
        1
    } else {
        0
    };

    // ratatui 0.30: `Rect::layout()` returns a fixed-size array so we can
    // destructure directly — no runtime bounds check when reading each slot.
    // A 1-row gap sits between the banner stack and the main area so the
    // header doesn't crowd the input/chat when no banners are active.
    // The status bar lives inside `main` (rendered by `render_chat_and_input`
    // in the row right under the input). A slash-command popup, when
    // active, takes that same slot and grows downward — covering the
    // status bar and pushing the input upward when the popup needs more
    // rows than the filler at the bottom can give back. Matches
    // codex-rs/tui's bottom-pane layout where the composer sits above
    // the popup_rect and the popup replaces the footer.
    let [
        header,
        fallback,
        rate_limit,
        permission_mode,
        context_warning,
        stream_stall,
        interrupt,
        verification_nudge,
        _gap,
        main,
    ] = area.layout(&Layout::vertical([
        Constraint::Length(HEADER_HEIGHT),           // header (logo + info)
        Constraint::Length(fallback_rows),           // model fallback
        Constraint::Length(rate_limit_rows),         // rate limit
        Constraint::Length(permission_mode_rows),    // permission mode
        Constraint::Length(context_warning_rows),    // context warning
        Constraint::Length(stream_stall_rows),       // stream stall
        Constraint::Length(interrupt_rows),          // interrupt
        Constraint::Length(verification_nudge_rows), // verification nudge
        Constraint::Length(1),                       // breathing gap
        Constraint::Min(1),                          // main area (chat + input + status/popup)
    ]));

    let styles = UiStyles::new(theme);

    render_header_bar(frame, header, state, styles);
    if fallback_rows > 0
        && let Some(desc) = state.session.model_fallback_banner.as_deref()
    {
        frame.render_widget(
            crate::widgets::ModelFallbackBanner::new(desc, styles),
            fallback,
        );
    }
    if rate_limit_rows > 0
        && let Some(info) = state.session.rate_limit_info.as_ref()
    {
        frame.render_widget(
            crate::widgets::RateLimitPanel::new(info, styles),
            rate_limit,
        );
    }
    if permission_mode_rows > 0 {
        frame.render_widget(
            crate::widgets::PermissionModeBanner::new(state.session.permission_mode, styles),
            permission_mode,
        );
    }
    if context_warning_rows > 0
        && let Some(pct) = state.session.context_usage_percent
    {
        frame.render_widget(
            crate::widgets::ContextWarningBanner::new(pct, styles),
            context_warning,
        );
    }
    if stream_stall_rows > 0 {
        frame.render_widget(
            crate::widgets::StreamStallIndicator::new(styles),
            stream_stall,
        );
    }
    if interrupt_rows > 0 {
        frame.render_widget(crate::widgets::InterruptBanner::new(styles), interrupt);
    }
    if verification_nudge_rows > 0 {
        frame.render_widget(
            crate::widgets::VerificationNudgeBanner::new(styles),
            verification_nudge,
        );
    }
    render_main_area(frame, main, state, styles, &mut layout);

    // Overlays on top
    if let Some(ref overlay) = state.ui.overlay {
        render_overlays::render_overlay(frame, area, overlay, state, styles);
    }

    // Toasts at top-right
    if state.ui.has_toasts() {
        render_toasts(frame, area, &state.ui.toasts, styles);
    }

    layout
}

fn render_header_bar(frame: &mut Frame, area: Rect, state: &AppState, styles: UiStyles<'_>) {
    if area.height == 0 {
        return;
    }

    let [logo_area, info_area] = header_areas(area);
    let view = header_bar_view(state, styles, info_area.width);
    frame.render_widget(Paragraph::new(view.logo_lines), logo_area);
    frame.render_widget(Paragraph::new(view.info_lines), info_area);
}

/// Main area: chat + live activity + bottom composer.
fn render_main_area(
    frame: &mut Frame,
    area: Rect,
    state: &AppState,
    styles: UiStyles<'_>,
    layout: &mut FrameLayout,
) {
    render_chat_and_input(frame, area, state, styles, layout);
}

/// Chat area + input area (vertical split). When the user has
/// focused a specific subagent (Tab/Shift-Tab cycle through
/// `subagents`), a one-line `TeammateViewHeader` is drawn above the
/// chat to make the focus visible and remind the user that Esc
/// returns to the main view. TS parity:
/// `components/TeammateViewHeader.tsx`.
fn render_chat_and_input(
    frame: &mut Frame,
    area: Rect,
    state: &AppState,
    styles: UiStyles<'_>,
    layout: &mut FrameLayout,
) {
    let input_height = 3.min(constants::MAX_INPUT_HEIGHT as u16);
    let focused_subagent = state
        .session
        .focused_subagent_index
        .and_then(|i| state.session.subagents.get(i as usize));
    let header_height: u16 = if focused_subagent.is_some() { 1 } else { 0 };
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

    // Vertical stack (content-flow from the top; matches codex-rs/tui's
    // bottom-pane composition where the composer sits above the popup
    // slot and the popup replaces the footer when active):
    //
    //   teammate header         (0 when no focused subagent)
    //   chat                    (fits content; shrinks when popup is tall)
    //   live activity           (0 unless active tools/agents/plan/status)
    //   queued-commands strip   (0 unless queue non-empty)
    //   input                   (3 rows; bordered top + bottom)
    //   stash notice            (0 when no stash)
    //   bottom slot             (1 row status bar, or popup_height when a
    //                            slash popup is active — same slot, so the
    //                            popup visually covers the status bar)
    //   filler                  (Min(0); empty space at the bottom when
    //                            chat + popup don't fill the screen)
    //
    // When the popup needs more rows than `filler` has to give back, the
    // chat area shrinks so the input rises with it — that is the
    // "push the input up when there's no room below" behaviour the user
    // expects from TS / codex-rs.
    let chat = build_chat_widget(state, styles, area.width);
    let chat_lines = chat.build_lines_owned();
    let chat_content_height = chat_lines.len() as u16;
    let inline_popup = inline_popup_view(state);
    let popup_items = inline_popup
        .as_ref()
        .map(InlinePopupView::item_count)
        .unwrap_or(0);
    let popup_active = popup_items > 0;
    let status_height: u16 = 1;
    let other_fixed_rows = header_height + activity_rows + queue_rows + input_height + stash_rows;
    let avail_below_input = area.height.saturating_sub(other_fixed_rows);
    let bottom_height: u16 = if popup_active {
        (popup_items as u16)
            .min(SuggestionPopup::DEFAULT_MAX_VISIBLE)
            .min(avail_below_input)
            .max(status_height)
    } else {
        status_height.min(avail_below_input)
    };
    let avail_for_chat = avail_below_input.saturating_sub(bottom_height);
    let chat_height = chat_content_height.min(avail_for_chat);

    let [
        header,
        chat_area,
        activity_area,
        queue,
        input,
        stash,
        bottom,
        _filler,
    ] = area.layout(&Layout::vertical([
        Constraint::Length(header_height),
        Constraint::Length(chat_height),
        Constraint::Length(activity_rows),
        Constraint::Length(queue_rows),
        Constraint::Length(input_height),
        Constraint::Length(stash_rows),
        Constraint::Length(bottom_height),
        Constraint::Min(0),
    ]));

    // Hand the input Rect to the cursor decision (consumed post-draw
    // in `Tui::draw` via `cursor::compute_cursor`). Cursor is no longer
    // set inside `render_input` itself.
    layout.input = input;

    if let Some(agent) = focused_subagent {
        let header_widget = crate::widgets::TeammateViewHeader::new(&agent.agent_type, styles)
            .agent_color(agent.color.as_deref())
            .description(Some(&agent.description));
        frame.render_widget(header_widget, header);
    }

    render_conversation_lines(
        frame,
        chat_area,
        chat_lines,
        chat_content_height,
        avail_for_chat,
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

    // Bottom slot: status bar by default, or the slash popup
    // (autocomplete / command palette) when one is active. The popup
    // grows downward from the row that normally holds the status bar
    // and covers it — same model as codex-rs/tui where the command
    // popup replaces the footer in the bottom pane's lower slot.
    if popup_active {
        // Anchor at the lower edge of `bottom` so the widget's
        // "walk up `popup_height` rows from `area.y`" lands inside the
        // slot. `max_visible` is pinned to the slot so a long list
        // never overflows into the input / chat above.
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

/// Build a fully-configured `ChatWidget` for the current session state.
/// Renders all committed messages plus any in-flight streaming buffer
/// and running-tool spinner. Mirrors TS Ink behavior: the viewport is
/// the single source of truth for what the user sees; nothing is pushed
/// to terminal native scrollback.
fn build_chat_widget<'a>(
    state: &'a AppState,
    styles: UiStyles<'a>,
    width: u16,
) -> crate::widgets::ChatWidget<'a> {
    let mut chat = crate::widgets::ChatWidget::new(&state.session.messages, styles)
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
    chat
}

/// Render pre-built chat lines as a wrapping paragraph. Auto-scrolls to
/// the bottom when content overflows and the user hasn't manually
/// scrolled (so the latest message + the input stay glued together).
fn render_conversation_lines(
    frame: &mut Frame,
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

/// Render the input area with mode indicator and streaming awareness.
fn render_input(frame: &mut Frame, state: &AppState, area: Rect, styles: UiStyles<'_>) {
    let is_focused = state.ui.focus == FocusTarget::Input;
    let command_palette_filter: Option<&str> = match state.ui.overlay.as_ref() {
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
    // Cursor placement is no longer set here. The single decision point
    // lives in `crate::cursor::compute_cursor`, consumed post-draw by
    // `Tui::draw` via `queue!(stdout, SetCursorStyle, MoveTo, Show/Hide)`.
    // See `docs/coco-rs/ui/rendering-hardening-and-rollback.md`.
}

/// Render the status bar.
fn render_status_bar(frame: &mut Frame, area: Rect, state: &AppState, styles: UiStyles<'_>) {
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

/// Render toast notifications at top-right corner.
fn render_toasts(
    frame: &mut Frame,
    area: Rect,
    toasts: &std::collections::VecDeque<Toast>,
    styles: UiStyles<'_>,
) {
    let toast_width: u16 = 40;
    let mut y = 1_u16;

    for toast in toasts.iter() {
        if y >= area.height - 2 {
            break;
        }

        let (icon, color) = match toast.severity {
            ToastSeverity::Info => ("ℹ", styles.dim()),
            ToastSeverity::Success => ("✓", styles.success()),
            ToastSeverity::Warning => ("⚠", styles.warning()),
            ToastSeverity::Error => ("✗", styles.error()),
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
