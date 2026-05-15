//! TUI rendering — the View in TEA.
//!
//! Pure function: takes immutable state, renders to frame.
//! No side effects except pixel drawing.

use ratatui::prelude::*;
use ratatui::widgets::Block;
use ratatui::widgets::Borders;
use ratatui::widgets::Clear;
use ratatui::widgets::Paragraph;
use ratatui::widgets::Wrap;

use coco_types::ModelRole;

use crate::constants;
use crate::i18n::t;
use crate::render_overlays;
use crate::state::AppState;
use crate::state::FocusTarget;
use crate::state::PromptMode;
use crate::state::Toast;
use crate::state::ToastSeverity;
use crate::theme::Theme;
use crate::widgets::SuggestionPopup;

/// Crate version surfaced in the header bar.
const COCO_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Total height of the header band (logo + info rows).
const HEADER_HEIGHT: u16 = 3;

/// Logo gutter width (9 logo cells + 2-space padding) — matches the
/// `Clawd` mascot's column count from Claude Code's `CondensedLogo`.
const HEADER_LOGO_WIDTH: u16 = 11;

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
    if verification_nudge_rows > 0 {
        frame.render_widget(
            crate::widgets::VerificationNudgeBanner::new(theme),
            verification_nudge,
        );
    }
    render_main_area(frame, main, state, theme);

    // Overlays on top
    if let Some(ref overlay) = state.ui.overlay {
        render_overlays::render_overlay(frame, area, overlay, state, theme);
    }

    // Toasts at top-right
    if state.ui.has_toasts() {
        render_toasts(frame, area, &state.ui.toasts, theme);
    }
}

/// Header band: 3-row COCO mascot + 3 info rows.
///
/// Mirrors Claude Code's `CondensedLogo` (`components/LogoV2/CondensedLogo.tsx`):
/// a 3-row block-glyph mascot on the left, with stacked info on the
/// right — row 1 brand + version, row 2 model id with the live
/// `thinking_effort` dial and ⚡ fast-mode flag, row 3 cwd + git branch
/// + worktree (each suppressed when absent).
fn render_header_bar(frame: &mut Frame, area: Rect, state: &AppState, theme: &Theme) {
    if area.height == 0 {
        return;
    }

    // Split horizontally: mascot gutter (11 cols) | info column.
    let logo_w = HEADER_LOGO_WIDTH.min(area.width);
    let [logo_area, info_area] = area.layout(&Layout::horizontal([
        Constraint::Length(logo_w),
        Constraint::Min(0),
    ]));

    // ── Mascot: two "boxed CO" eyes ──
    // Each ╭─╮│●│╰─╯ cell renders one eye — the rounded box outline
    // echoes the C of "coco" while the inner ● is the O pupil, giving
    // a "CO CO" pair that reads as two laughing big eyes. Keeps the
    // 9-cell-wide × 3-row footprint of the previous Claude mascot so
    // the info column on the right stays aligned.
    let logo_color = Style::default().fg(theme.primary);
    let logo_lines = vec![
        Line::from(Span::styled(" ╭─╮ ╭─╮  ", logo_color)),
        Line::from(Span::styled(" │●│ │●│  ", logo_color)),
        Line::from(Span::styled(" ╰─╯ ╰─╯  ", logo_color)),
    ];
    frame.render_widget(Paragraph::new(logo_lines), logo_area);

    // ── Row 1: COCO + version ──
    let mut row1: Vec<Span> = Vec::new();
    row1.push(Span::styled("COCO", Style::default().fg(theme.text).bold()));
    row1.push(Span::raw(" "));
    row1.push(Span::styled(
        format!("v{COCO_VERSION}"),
        Style::default().fg(theme.text_dim),
    ));

    // ── Row 2: model_id  *  thinking_effort  ⚡ ──
    // Effort comes from the live `session.thinking_effort` dial (Ctrl+T
    // cycles it) — always rendered so the user can see the current
    // level at a glance. Model id pulls from the `ModelRole::Main`
    // binding first to honor in-session `/model` switches.
    let mut row2: Vec<Span> = Vec::new();
    let (provider, model_id) = state
        .session
        .model_by_role
        .get(&ModelRole::Main)
        .map(|b| (b.provider.clone(), b.model_id.clone()))
        .unwrap_or_else(|| (state.session.provider.clone(), state.session.model.clone()));
    if model_id.is_empty() {
        row2.push(Span::styled(
            t!("status.no_model").to_string(),
            Style::default().fg(theme.text_dim).italic(),
        ));
    } else {
        let head = if provider.is_empty() {
            model_id
        } else {
            format!("{provider}/{model_id}")
        };
        row2.push(Span::styled(
            head,
            Style::default().fg(theme.primary).bold(),
        ));
        row2.push(Span::styled("  *  ", Style::default().fg(theme.border)));
        row2.push(Span::styled(
            state.session.thinking_effort.to_string(),
            Style::default().fg(theme.accent),
        ));
        if state.session.fast_mode {
            row2.push(Span::raw("  "));
            row2.push(Span::styled("⚡", Style::default().fg(theme.warning)));
        }
    }

    // ── Row 3: cwd  branch  worktree ──
    let mut row3: Vec<Span> = Vec::new();
    if let Some(ref dir) = state.session.working_dir {
        let display = tildify_path(dir);
        let max_w = info_area.width.saturating_sub(2) as usize;
        let cwd = truncate_path_for_width(&display, max_w);
        row3.push(Span::styled(cwd, Style::default().fg(theme.text_dim)));
    }
    if let Some(ref branch) = state.session.git_branch {
        if !row3.is_empty() {
            row3.push(Span::raw(" "));
        }
        row3.push(Span::styled(
            format!(" {branch}"),
            Style::default().fg(theme.text_dim),
        ));
    }
    if let Some(ref wt) = state.session.worktree_path {
        let short = wt.rsplit('/').next().unwrap_or(wt);
        if !row3.is_empty() {
            row3.push(Span::raw(" "));
        }
        row3.push(Span::styled(
            format!("🌿 {short}"),
            Style::default().fg(theme.success),
        ));
    }

    let info_lines = vec![Line::from(row1), Line::from(row2), Line::from(row3)];
    frame.render_widget(Paragraph::new(info_lines), info_area);
}

/// Replace the user's home prefix with `~` so the cwd row fits on a
/// single header line for paths that live under `$HOME`.
fn tildify_path(path: &str) -> String {
    if let Some(home) = dirs::home_dir()
        && let Some(home_str) = home.to_str()
        && let Some(rest) = path.strip_prefix(home_str)
    {
        return if rest.is_empty() {
            "~".to_string()
        } else if rest.starts_with('/') {
            format!("~{rest}")
        } else {
            format!("~/{rest}")
        };
    }
    path.to_string()
}

/// Truncate a path with a leading horizontal-ellipsis when it would
/// overflow the available width, preserving the deepest segments. Matches
/// the truncation style TS uses in `logoV2Utils.truncatePath`.
fn truncate_path_for_width(path: &str, max_width: usize) -> String {
    if max_width == 0 || path.chars().count() <= max_width {
        return path.to_string();
    }
    let suffix_chars: String = path
        .chars()
        .rev()
        .take(max_width.saturating_sub(1))
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    format!("…{suffix_chars}")
}

/// Main area: chat + input, optionally with side panel.
fn render_main_area(frame: &mut Frame, area: Rect, state: &AppState, theme: &Theme) {
    let has_tools =
        !state.session.tool_executions.is_empty() || !state.session.subagents.is_empty();
    let wide_enough = area.width >= constants::SIDE_PANEL_MIN_WIDTH as u16;

    // Task panel takes precedence over the tool side panel when the
    // user / a tool just auto-expanded it. Mirrors TS
    // `AppState.expandedView == 'tasks'` driving the right-rail layout.
    let show_plan_panel = matches!(state.session.expanded_view, coco_types::ExpandedView::Tasks)
        && wide_enough
        && (!state.session.plan_tasks.is_empty() || !state.session.todos_by_agent.is_empty());
    // Same for `'teammates'` — `app:toggleTodos` cycles into this when
    // there are running subagents.
    let show_teammates_panel = matches!(
        state.session.expanded_view,
        coco_types::ExpandedView::Teammates
    ) && wide_enough
        && !state.session.subagents.is_empty();

    if show_plan_panel {
        let [main, side] = area.layout(&Layout::horizontal([
            Constraint::Percentage(constants::NORMAL_TERMINAL_MAIN_PCT as u16),
            Constraint::Percentage(constants::NORMAL_TERMINAL_SIDE_PCT as u16),
        ]));
        render_chat_and_input(frame, main, state, theme);
        let running_entries = running_tasks_as_entries(&state.session.active_tasks);
        let panel = crate::widgets::PlanPanel::new(
            &state.session.plan_tasks,
            &state.session.todos_by_agent,
            &running_entries,
            theme,
        );
        frame.render_widget(panel, side);
    } else if show_teammates_panel {
        let [main, side] = area.layout(&Layout::horizontal([
            Constraint::Percentage(constants::NORMAL_TERMINAL_MAIN_PCT as u16),
            Constraint::Percentage(constants::NORMAL_TERMINAL_SIDE_PCT as u16),
        ]));
        render_chat_and_input(frame, main, state, theme);
        // ExpandedView::Teammates → subagent panel takes the entire
        // right rail (no tool panel above it). Honors
        // `state.ui.show_teammate_message_preview` for per-agent
        // recent-message preview lines.
        render_subagent_panel(frame, side, state, theme);
    } else if has_tools && wide_enough {
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

/// Convert `SessionState::active_tasks` (server-notification-driven
/// running-task entries) to the shape the `PlanPanel` widget expects.
/// Keeps widget code type-stable and the session state shape the same.
fn running_tasks_as_entries(
    entries: &[crate::state::session::TaskEntry],
) -> Vec<crate::widgets::task_list::TaskEntry> {
    use crate::state::session::TaskEntryStatus;
    use crate::widgets::task_list::TaskDisplayStatus;
    use crate::widgets::task_list::TaskDisplayType;
    use crate::widgets::task_list::TaskEntry as WidgetEntry;
    entries
        .iter()
        .map(|e| WidgetEntry {
            id: e.task_id.clone(),
            name: e.description.clone(),
            status: match e.status {
                TaskEntryStatus::Running => TaskDisplayStatus::Running,
                TaskEntryStatus::Completed => TaskDisplayStatus::Completed,
                TaskEntryStatus::Failed => TaskDisplayStatus::Failed,
                TaskEntryStatus::Stopped => TaskDisplayStatus::Backgrounded,
            },
            task_type: TaskDisplayType::Agent,
            progress: None,
            elapsed_ms: 0,
        })
        .collect()
}

/// Chat area + input area (vertical split). When the user has
/// focused a specific subagent (Tab/Shift-Tab cycle through
/// `subagents`), a one-line `TeammateViewHeader` is drawn above the
/// chat to make the focus visible and remind the user that Esc
/// returns to the main view. TS parity:
/// `components/TeammateViewHeader.tsx`.
fn render_chat_and_input(frame: &mut Frame, area: Rect, state: &AppState, theme: &Theme) {
    let input_height = 3.min(constants::MAX_INPUT_HEIGHT as u16);
    let focused_subagent = state
        .session
        .focused_subagent_index
        .and_then(|i| state.session.subagents.get(i as usize));
    let header_height: u16 = if focused_subagent.is_some() { 1 } else { 0 };
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
    let chat = build_chat_widget(state, theme, area.width);
    let chat_lines = chat.build_lines_owned();
    let chat_content_height = chat_lines.len() as u16;
    let popup_items = inline_popup_item_count(state);
    let popup_active = popup_items > 0;
    let status_height: u16 = 1;
    let other_fixed_rows = header_height + queue_rows + input_height + stash_rows;
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

    let [header, chat_area, queue, input, stash, bottom, _filler] =
        area.layout(&Layout::vertical([
            Constraint::Length(header_height),
            Constraint::Length(chat_height),
            Constraint::Length(queue_rows),
            Constraint::Length(input_height),
            Constraint::Length(stash_rows),
            Constraint::Length(bottom_height),
            Constraint::Min(0),
        ]));

    if let Some(agent) = focused_subagent {
        let header_widget = crate::widgets::TeammateViewHeader::new(&agent.agent_type, theme)
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
    if queue_rows > 0 {
        frame.render_widget(
            crate::widgets::QueueStatusWidget::new(&state.session.queued_commands, theme),
            queue,
        );
    }
    render_input(frame, state, input, theme);
    if stash_rows > 0
        && let Some(s) = state.ui.stashed_input.as_ref()
    {
        frame.render_widget(crate::widgets::StashNotice::new(s, theme), stash);
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
        if let Some(ref sug) = state.ui.active_suggestions {
            let popup = crate::widgets::SuggestionPopup::new(&sug.items, theme)
                .selected(sug.selected)
                .max_visible(bottom_height as usize);
            frame.render_widget(popup, anchor);
        } else if let Some(crate::state::Overlay::CommandPalette(cp)) = state.ui.overlay.as_ref() {
            let items = command_palette_suggestion_items(cp);
            if !items.is_empty() {
                let popup = crate::widgets::SuggestionPopup::new(&items, theme)
                    .selected(cp.selected.max(0) as usize)
                    .max_visible(bottom_height as usize);
                frame.render_widget(popup, anchor);
            }
        }
    } else if bottom_height > 0 {
        render_status_bar(frame, bottom, state, theme);
    }
}

/// Total rows the inline slash popup (autocomplete or command palette)
/// wants to claim above the input. Returned in items, not rows — the
/// caller caps it against the widget's `max_visible` and the layout's
/// available space.
fn inline_popup_item_count(state: &AppState) -> usize {
    let from_suggestions = state
        .ui
        .active_suggestions
        .as_ref()
        .map(|s| s.items.len())
        .unwrap_or(0);
    if from_suggestions > 0 {
        return from_suggestions;
    }
    if let Some(crate::state::Overlay::CommandPalette(cp)) = state.ui.overlay.as_ref() {
        let filter_lower = cp.filter.to_lowercase();
        return cp
            .commands
            .iter()
            .filter(|cmd| {
                filter_lower.is_empty() || cmd.name.to_lowercase().contains(&filter_lower)
            })
            .count();
    }
    0
}

/// Convert a `CommandPaletteOverlay` snapshot into the borderless
/// suggestion-popup row model, applying the same case-insensitive
/// substring filter the centered modal used. Lives next to the
/// renderer because it is the only consumer.
fn command_palette_suggestion_items(
    cp: &crate::state::CommandPaletteOverlay,
) -> Vec<crate::widgets::suggestion_popup::SuggestionItem> {
    let filter_lower = cp.filter.to_lowercase();
    cp.commands
        .iter()
        .filter(|cmd| filter_lower.is_empty() || cmd.name.to_lowercase().contains(&filter_lower))
        .map(|cmd| crate::widgets::suggestion_popup::SuggestionItem {
            label: format!("/{}", cmd.name),
            description: cmd.description.clone(),
            metadata: None,
        })
        .collect()
}

/// Build a fully-configured `ChatWidget` for the current session state.
/// Extracted so the same widget can be reused for both height
/// computation (via `build_lines_owned`) and final rendering.
fn build_chat_widget<'a>(
    state: &'a AppState,
    theme: &'a Theme,
    width: u16,
) -> crate::widgets::ChatWidget<'a> {
    let mut chat = crate::widgets::ChatWidget::new(&state.session.messages, theme)
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
fn render_input(frame: &mut Frame, state: &AppState, area: Rect, theme: &Theme) {
    let is_focused = state.ui.focus == FocusTarget::Input;
    let is_streaming = state.is_streaming();
    // Streaming suppresses prompt-mode rendering — bash/memory submissions
    // can't run mid-turn, so the input is always a queue entry while a turn
    // is in flight. TS parity: `PromptInputModeIndicator` checks
    // `isLoading` (== streaming) before applying the bash colour.
    let prompt_mode = if is_streaming {
        PromptMode::Normal
    } else {
        state.ui.input.prompt_mode()
    };
    let border_color = if is_focused {
        theme.border_focused
    } else {
        theme.border
    };

    // The indicator owns the leading prefix character when a prompt mode
    // is active — `! ` for bash, `# ` for memory — and the body renders
    // the stripped text. This matches TS where the `!` is part of the
    // `PromptInputModeIndicator` component rather than the input value.
    // Position 0 of `InputState.text` is the prefix; visual cursor is
    // offset by one in those modes so typing at "the start" really means
    // immediately after the indicator.
    let indicator = match (is_streaming, prompt_mode) {
        (true, _) => Span::styled("~ ", Style::default().fg(theme.warning)),
        (false, PromptMode::Bash) => Span::styled("! ", Style::default().fg(theme.accent)).bold(),
        (false, PromptMode::Memory) => {
            Span::styled("# ", Style::default().fg(theme.success)).bold()
        }
        // `❯` matches TS `figures.pointer` on macOS/Linux. Width is one
        // column in standard fonts; the trailing space keeps the gutter
        // at the same 2-col budget as the other indicators.
        (false, PromptMode::Normal) => Span::styled("❯ ", Style::default().fg(theme.primary)),
    };

    // P5 / A4: when the input is empty AND a post-turn prompt suggestion
    // is available, render the suggestion as the dim placeholder. Prefer
    // the freshest suggestion (last entry) so sequential turns each get
    // their own. The user accepts it by pressing Tab/Right at the empty
    // input — bound in `keybinding_bridge.rs`.
    let suggestion = state.session.prompt_suggestions.last();
    let is_empty = state.ui.input.is_empty();

    // When the command palette is open (Ctrl+P), typed characters route
    // to `cp.filter` rather than `input.text` — but the user still needs
    // to see what they typed. Mirror the filter into the input bar as
    // `/<filter>` so it reads identically to the TS slash-autocomplete
    // flow where keystrokes appear in the prompt as the popup filters.
    let command_palette_filter: Option<&str> = match state.ui.overlay.as_ref() {
        Some(crate::state::Overlay::CommandPalette(cp)) => Some(cp.filter.as_str()),
        _ => None,
    };

    // Match the submit-time prefix-stripping rule (`PromptMode::strip_prefix`):
    // drop the leading mode character plus one optional space so the body
    // shown here equals exactly what the engine will receive. Without this
    // alignment a typed `! ls` would render with double-spacing (`! ` from
    // the indicator plus a kept leading space in the body).
    let prefix_consumed: usize = if is_empty || prompt_mode == PromptMode::Normal {
        0
    } else {
        let body = &state.ui.input.text()[1..];
        1 + if body.starts_with(' ') { 1 } else { 0 }
    };
    // Placeholder priority (TS `usePromptInputPlaceholder`):
    //   1. command-palette mirror — wins over placeholders so the user
    //      sees their filter typed into the prompt.
    //   2. queued-command hint — only when the queue is non-empty so
    //      the user notices there's something to recall with ↑.
    //   3. prompt suggestion from the last post-turn fork.
    //   4. static default placeholder.
    let has_editable_queue = !state.session.queued_commands.is_empty();
    let display_text = if let Some(filter) = command_palette_filter {
        format!("/{filter}")
    } else if is_empty {
        if has_editable_queue {
            t!("input.placeholder_queued").to_string()
        } else if let Some(s) = suggestion {
            s.clone()
        } else {
            t!("input.placeholder").to_string()
        }
    } else {
        state.ui.input.text()[prefix_consumed..].to_string()
    };

    let text_style = if command_palette_filter.is_some() || !is_empty {
        Style::default().fg(theme.text)
    } else {
        Style::default().fg(theme.text_dim)
    };

    // Layered title:
    //   streaming         →  "Queue Input" (wins outright — modes can't fire mid-turn)
    //   plan + prompt     →  "Plan • Bash Mode" / "Plan • Memory Mode"
    //   prompt mode only  →  "Bash Mode" / "Memory Mode"
    //   plan only         →  "Plan Mode"
    //   default           →  "Input"
    // Plan mode is a session-wide permission flag, so when it's active
    // we surface it alongside the prompt mode rather than letting the
    // prompt mode mask it — otherwise the user would lose track of
    // being in plan after one bash sortie.
    let title = if is_streaming {
        format!(" {} ", t!("input.title_queue"))
    } else if prompt_mode != PromptMode::Normal && state.is_plan_mode() {
        format!(
            " {} • {} ",
            t!("input.title_plan_mode"),
            t!(prompt_mode.title_i18n_key())
        )
    } else if prompt_mode != PromptMode::Normal {
        format!(" {} ", t!(prompt_mode.title_i18n_key()))
    } else if state.is_plan_mode() {
        format!(" {} ", t!("input.title_plan_mode"))
    } else {
        // Default state: no title. The `❯` indicator already names the
        // widget; a permanent "Input" label is chrome that adds nothing.
        // Mode-specific titles above (Plan / Bash / Memory / Queue)
        // still fire because they carry actual state.
        String::new()
    };

    let input_line = Line::from(vec![indicator, Span::styled(display_text, text_style)]);
    let input = Paragraph::new(input_line).block(
        Block::default()
            .borders(Borders::TOP | Borders::BOTTOM)
            .title(title)
            .border_style(Style::default().fg(border_color)),
    );

    frame.render_widget(input, area);

    // Cursor position: indicator owns 2 columns. The visual cursor sits
    // `prefix_consumed` chars left of the raw cursor (the indicator owns
    // those chars) and clamps to 0 when the raw cursor lands inside the
    // consumed prefix (so backspace at that point deletes the `!` / `#`).
    // When the command palette is open, the cursor follows the mirrored
    // `/<filter>` so it stays at the end of what the user typed.
    let should_show_cursor = is_focused && (command_palette_filter.is_some() || !is_empty);
    if should_show_cursor {
        let indicator_width = 2_u16;
        let raw_cursor = if let Some(filter) = command_palette_filter {
            // 1 column for the leading `/` + visible filter width.
            1 + unicode_width::UnicodeWidthStr::width(filter) as i32
        } else {
            // Display column = width of the visible text up to the byte
            // offset of the cursor. Fixes CJK / wide-char cursor placement:
            // "你好" with cursor at end → column 4 (not 2).
            let visible_text = &state.ui.input.text()[prefix_consumed..];
            let cursor_byte = state
                .ui
                .input
                .textarea
                .cursor()
                .saturating_sub(prefix_consumed);
            let cursor_byte = cursor_byte.min(visible_text.len());
            unicode_width::UnicodeWidthStr::width(&visible_text[..cursor_byte]) as i32
        };
        let max_cursor = area.width.saturating_sub(indicator_width + 1) as i32;
        let cursor_x = area.x + indicator_width + raw_cursor.min(max_cursor) as u16;
        let cursor_y = area.y + 1;
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

/// Render subagent instances using SubagentPanel widget. Switches to
/// the richer [`CoordinatorPanel`] when the session is operating in
/// coordinator mode (`COCO_COORDINATOR_MODE=1` + `Feature::AgentTeams`).
/// The coordinator panel surfaces queued-message counts and elapsed
/// time per worker; the regular subagent panel is the default
/// per-spawn view.
fn render_subagent_panel(frame: &mut Frame, area: Rect, state: &AppState, theme: &Theme) {
    if is_coordinator_mode_active() {
        let tasks: Vec<crate::widgets::CoordinatorTask> = state
            .session
            .subagents
            .iter()
            .map(crate::widgets::CoordinatorTask::from_subagent)
            .collect();
        let panel = crate::widgets::CoordinatorPanel::new(&tasks, theme)
            .selected_index(state.session.focused_subagent_index);
        frame.render_widget(panel, area);
    } else {
        let mut panel = crate::widgets::SubagentPanel::new(&state.session.subagents, theme)
            .focused_index(state.session.focused_subagent_index);
        if state.ui.show_teammate_message_preview {
            panel = panel.message_preview(&state.session.messages);
        }
        frame.render_widget(panel, area);
    }
}

/// Returns true when both `Feature::AgentTeams` and the
/// `COCO_COORDINATOR_MODE` env gate are on. Reads via the central
/// helper rather than `std::env::var` directly per project rule.
fn is_coordinator_mode_active() -> bool {
    coco_config::env::is_env_truthy(coco_config::EnvKey::CocoCoordinatorMode)
}

/// Render the status bar.
fn render_status_bar(frame: &mut Frame, area: Rect, state: &AppState, theme: &Theme) {
    // Exit-confirmation hint takes the whole bar when armed so it's
    // visible in the same bottom-footer position as TS
    // `PromptInputFooterLeftSide.tsx:147-150`.
    if let Some(key) = state.ui.pending_exit_hint() {
        let text = t!("status.exit_prompt", key = key.label()).to_string();
        tracing::info!(
            key = key.label(),
            prompt = %text,
            width = area.width,
            "status bar rendering exit prompt"
        );
        let line = Line::from(Span::styled(
            text,
            Style::default().fg(theme.warning).bold(),
        ));
        frame.render_widget(Paragraph::new(line), area);
        return;
    }

    let mut parts = Vec::new();

    // Model: `provider/model_id` so it reads identically to the header
    // row. Falls back to bare `state.session.model` only when neither
    // `ModelRole::Main` binding nor `session.provider` carries a value
    // — keeping legacy "no provider set" callers working.
    let (provider, model_id) = state
        .session
        .model_by_role
        .get(&ModelRole::Main)
        .map(|b| (b.provider.clone(), b.model_id.clone()))
        .unwrap_or_else(|| (state.session.provider.clone(), state.session.model.clone()));
    let model_display = if !provider.is_empty() && !model_id.is_empty() {
        format!("{provider}/{model_id}")
    } else if !model_id.is_empty() {
        model_id
    } else {
        provider
    };
    let has_model = !model_display.is_empty();
    if has_model {
        parts.push(Span::styled(
            format!(" {model_display}"),
            Style::default().fg(theme.primary).bold(),
        ));
        // Fast mode flag, sits immediately after the model id so it
        // reads as a model attribute.
        if state.session.fast_mode {
            parts.push(Span::styled(" ⚡", Style::default().fg(theme.warning)));
        }
    }

    // Thinking effort, joined with ` * ` so it reads as one
    // model-config glance: `provider/model * effort`. Shown for every
    // value, even `Auto`, so the user always sees the dial position.
    // Falls back to a leading space when the model id is unavailable
    // (pre-bootstrap or `no model selected` test states) so the
    // permission indicator doesn't read as an orphan bullet.
    let join = if has_model { " * " } else { " " };
    parts.push(Span::styled(join, Style::default().fg(theme.text_dim)));
    parts.push(Span::styled(
        state.session.thinking_effort.to_string(),
        Style::default().fg(theme.text_dim),
    ));

    // Permission mode — TS-style label
    // (`PromptInputFooterLeftSide.tsx:348-355`). Only rendered for
    // active modes; default mode shows nothing so the footer matches
    // TS where `hasActiveMode = !isDefaultMode(currentMode)`. Color
    // also mirrors TS (`PermissionMode.ts:42-91`).
    if let Some((mode_label, mode_color)) =
        permission_mode_status_label(state.session.permission_mode, theme)
    {
        parts.push(Span::styled(", ", Style::default().fg(theme.text_dim)));
        parts.push(Span::styled(mode_label, Style::default().fg(mode_color)));
    }

    // Pending chord prefix (e.g. "ctrl+x …") — shown only while a
    // chord is in flight so users see the resolver waiting for the
    // next combo. Mirrors TS chord-status indicator from
    // `KeybindingProviderSetup.tsx`.
    if let Some(hint) = state.ui.kb_handle.pending_display() {
        parts.push(Span::styled(" | ", Style::default().fg(theme.border)));
        parts.push(Span::styled(
            hint,
            Style::default().fg(theme.warning).bold(),
        ));
    }

    // Token usage — always rendered so the bar layout is stable.
    // `↑` = prompt-side tokens, `↓` = completion-side. `cache <pct>%`
    // is the cache-hit share of the input (`cache_read / input`), shown
    // as `0%` when input is still 0 rather than suppressed — keeps the
    // segment in place from the first frame.
    let tokens = &state.session.token_usage;
    parts.push(Span::styled(" | ", Style::default().fg(theme.border)));
    parts.push(Span::styled(
        format!(
            "↑{} ↓{}",
            format_token_count(tokens.input_tokens),
            format_token_count(tokens.output_tokens)
        ),
        Style::default().fg(theme.text_dim),
    ));
    let cache_pct = if tokens.input_tokens > 0 {
        (tokens.cache_read_tokens * 100 / tokens.input_tokens).clamp(0, 100)
    } else {
        0
    };
    parts.push(Span::styled(
        format!(" · cache {cache_pct}%"),
        Style::default().fg(theme.text_dim),
    ));

    // Context window usage — always rendered. `0%` when no provider
    // window has been reported yet, so the position is reserved and
    // doesn't shift around as soon as the first usage frame arrives.
    let ctx_pct = if state.session.context_window_total > 0 {
        let used = state.session.context_window_used as i64;
        let total = state.session.context_window_total as i64;
        (used * 100 / total.max(1)).clamp(0, 100)
    } else {
        0
    };
    parts.push(Span::styled(" | ", Style::default().fg(theme.border)));
    let style = if ctx_pct > 90 {
        Style::default().fg(theme.error).bold()
    } else if ctx_pct > 70 {
        Style::default().fg(theme.warning)
    } else {
        Style::default().fg(theme.text_dim)
    };
    parts.push(Span::styled(format!("ctx {ctx_pct}%"), style));

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

    let line = Line::from(parts);
    frame.render_widget(Paragraph::new(line), area);
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

/// Status-bar label + color for a permission mode. Returns `None`
/// for `Default` — TS hides the mode indicator entirely in the
/// resting state (`PromptInputFooterLeftSide.tsx:321,348`: gated on
/// `hasActiveMode = !isDefaultMode(currentMode)`).
///
/// TS parity:
/// - Wording: `PromptInputFooterLeftSide.tsx:348-355`
///   (`permissionModeTitle(mode).toLowerCase() + ' on'`)
/// - Color:   `PermissionMode.ts:42-91` (`color` field per mode)
fn permission_mode_status_label(
    mode: coco_types::PermissionMode,
    theme: &Theme,
) -> Option<(String, ratatui::style::Color)> {
    let (key, color) = match mode {
        coco_types::PermissionMode::Default => return None,
        coco_types::PermissionMode::AcceptEdits => {
            ("permission_mode.status.accept_edits", theme.accent)
        }
        coco_types::PermissionMode::Plan => ("permission_mode.status.plan", theme.plan_mode),
        coco_types::PermissionMode::BypassPermissions => {
            ("permission_mode.status.bypass", theme.error)
        }
        coco_types::PermissionMode::DontAsk => ("permission_mode.status.dont_ask", theme.error),
        coco_types::PermissionMode::Auto => ("permission_mode.status.auto", theme.warning),
        coco_types::PermissionMode::Bubble => ("permission_mode.status.bubble", theme.text_dim),
    };
    Some((t!(key).to_string(), color))
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
