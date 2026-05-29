//! View model for the live activity surface in the main console workspace.

use std::borrow::Cow;

use coco_types::ExpandedView;
use unicode_width::UnicodeWidthStr;

use crate::i18n::t;
use crate::state::AppState;
use crate::state::SubagentStatus;
use crate::state::session::TaskEntryStatus;
use crate::state::session::ToolStatus;
use coco_tui_ui::constants;

const MAX_TOOL_ACTIVITY_DISPLAY: usize = 5;
const INLINE_ACTIVITY_ROWS_NARROW: u16 = 3;
const INLINE_ACTIVITY_ROWS_NORMAL: u16 = 5;
const INLINE_ACTIVITY_ROWS_WIDE: u16 = 7;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ActivityTitle {
    Activity,
    Agents,
    Coordinator,
    Tasks,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ActivityBorder {
    Plan,
    Agents,
    Coordinator,
    Activity,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ActivityTone {
    Text,
    Dim,
    Accent,
    Running,
    Completed,
    Error,
    Warning,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ActivitySpan {
    /// `Cow<'static, str>` so `&'static str` literals stay borrowed
    /// (zero alloc) while `String` values (`format!`, owned clones)
    /// flow in as `Cow::Owned` without an extra round-trip. The
    /// `'static` bound keeps the type free of lifetime parameters
    /// so `ActivityLine` / `ActivitySurfaceView` / `TurnActivityView`
    /// don't have to thread a lifetime through every renderer.
    pub(crate) text: Cow<'static, str>,
    pub(crate) tone: ActivityTone,
    pub(crate) bold: bool,
}

impl ActivitySpan {
    pub(crate) fn raw(text: impl Into<Cow<'static, str>>) -> Self {
        Self {
            text: text.into(),
            tone: ActivityTone::Text,
            bold: false,
        }
    }

    pub(crate) fn tone(text: impl Into<Cow<'static, str>>, tone: ActivityTone) -> Self {
        Self {
            text: text.into(),
            tone,
            bold: false,
        }
    }

    pub(crate) fn bold(text: impl Into<Cow<'static, str>>, tone: ActivityTone) -> Self {
        Self {
            text: text.into(),
            tone,
            bold: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ActivityLine {
    pub(crate) spans: Vec<ActivitySpan>,
}

impl ActivityLine {
    pub(crate) fn blank() -> Self {
        Self { spans: Vec::new() }
    }

    pub(crate) fn section(title: impl Into<Cow<'static, str>>) -> Self {
        Self {
            spans: vec![ActivitySpan::bold(title, ActivityTone::Accent)],
        }
    }

    pub(crate) fn text(text: impl Into<Cow<'static, str>>, tone: ActivityTone) -> Self {
        Self {
            spans: vec![ActivitySpan::tone(text, tone)],
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ActivitySurfaceView {
    pub(crate) title: ActivityTitle,
    pub(crate) border: ActivityBorder,
    pub(crate) lines: Vec<ActivityLine>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum TurnActivityView {
    None,
    Surface(ActivitySurfaceView),
}

pub(crate) fn turn_activity_view(state: &AppState, width: u16) -> TurnActivityView {
    let has_subagents = !state.session.subagents.is_empty();
    // Include `active_tasks` so the Tasks panel can open when only running
    // background tasks (bash / agent) exist, without any plan items or
    // per-agent todos. Before: opening ctrl+t after `run_in_background`
    // showed an empty pane because the gate ignored `active_tasks`.
    let has_plan_activity = !state.session.plan_tasks.is_empty()
        || !state.session.todos_by_agent.is_empty()
        || !state.session.active_tasks.is_empty();
    let has_tool_activity = state
        .session
        .tool_executions
        .iter()
        .any(|t| matches!(t.status, ToolStatus::Queued | ToolStatus::Running));

    if matches!(state.session.expanded_view, ExpandedView::Tasks) && has_plan_activity {
        return TurnActivityView::Surface(limit_surface_rows(plan_surface(state), width));
    }

    if matches!(state.session.expanded_view, ExpandedView::Teammates) && has_subagents {
        return TurnActivityView::Surface(limit_surface_rows(agent_surface(state), width));
    }

    if has_subagents {
        TurnActivityView::Surface(limit_surface_rows(agent_surface(state), width))
    } else if has_tool_activity || state.session.stream_stall {
        TurnActivityView::Surface(limit_surface_rows(activity_surface(state), width))
    } else {
        TurnActivityView::None
    }
}

pub(crate) fn inline_activity_height(
    view: &TurnActivityView,
    available_height: u16,
    width: u16,
) -> u16 {
    let TurnActivityView::Surface(surface) = view else {
        return 0;
    };
    if available_height == 0 || surface.lines.is_empty() {
        return 0;
    }
    let visible_rows = surface.lines.len().min(activity_row_budget(width) as usize) as u16;
    (visible_rows + 1).min(available_height)
}

fn limit_surface_rows(mut surface: ActivitySurfaceView, width: u16) -> ActivitySurfaceView {
    let budget = activity_row_budget(width) as usize;
    if surface.lines.len() > budget {
        surface.lines.truncate(budget.saturating_sub(1));
        surface.lines.push(ActivityLine::text(
            t!("activity.more").to_string(),
            ActivityTone::Dim,
        ));
    }
    surface
}

fn activity_row_budget(width: u16) -> u16 {
    if width <= 60 {
        INLINE_ACTIVITY_ROWS_NARROW
    } else if width <= 100 {
        INLINE_ACTIVITY_ROWS_NORMAL
    } else {
        INLINE_ACTIVITY_ROWS_WIDE
    }
}

fn plan_surface(state: &AppState) -> ActivitySurfaceView {
    let mut lines = status_activity_lines(state);
    append_plan_lines(state, &mut lines);
    append_tool_lines(state, &mut lines);
    trim_trailing_blank(&mut lines);
    if lines.is_empty() {
        lines.push(ActivityLine::text(
            format!("  {}", t!("plan_panel.empty")),
            ActivityTone::Dim,
        ));
    }
    ActivitySurfaceView {
        title: ActivityTitle::Tasks,
        border: ActivityBorder::Plan,
        lines,
    }
}

fn agent_surface(state: &AppState) -> ActivitySurfaceView {
    let mut lines = status_activity_lines(state);
    if state.ui.coordinator_mode_active {
        append_coordinator_lines(state, &mut lines);
    } else {
        append_subagent_lines(state, &mut lines);
    }
    append_tool_lines(state, &mut lines);
    trim_trailing_blank(&mut lines);
    if lines.is_empty() {
        lines.push(ActivityLine::text(
            format!("  {}", t!("subagent.no_agents")),
            ActivityTone::Dim,
        ));
    }
    ActivitySurfaceView {
        title: if state.ui.coordinator_mode_active {
            ActivityTitle::Coordinator
        } else {
            ActivityTitle::Agents
        },
        border: if state.ui.coordinator_mode_active {
            ActivityBorder::Coordinator
        } else {
            ActivityBorder::Agents
        },
        lines,
    }
}

fn activity_surface(state: &AppState) -> ActivitySurfaceView {
    let mut lines = status_activity_lines(state);
    append_tool_lines(state, &mut lines);
    trim_trailing_blank(&mut lines);
    ActivitySurfaceView {
        title: ActivityTitle::Activity,
        border: ActivityBorder::Activity,
        lines,
    }
}

fn status_activity_lines(state: &AppState) -> Vec<ActivityLine> {
    let mut lines = Vec::new();
    if state.session.stream_stall {
        lines.push(ActivityLine {
            spans: vec![
                ActivitySpan::tone("  ! ", ActivityTone::Warning),
                ActivitySpan::tone(
                    t!("toast.stream_stall_detected").to_string(),
                    ActivityTone::Warning,
                ),
            ],
        });
    }
    if !lines.is_empty() {
        lines.push(ActivityLine::blank());
    }
    lines
}

fn append_plan_lines(state: &AppState, lines: &mut Vec<ActivityLine>) {
    // V2 (plan_tasks) and V1 (todos_by_agent) are rendered through the
    // dedicated todo panel module so the V1/V2 mutual exclusion,
    // priority sort, and TS-aligned glyphs live in one place.
    crate::widgets::todo_panel::append_lines(state, lines);

    if !state.session.active_tasks.is_empty() {
        lines.push(ActivityLine::section(
            t!("plan_panel.section_running").to_string(),
        ));
        for task in &state.session.active_tasks {
            let (icon, tone) = match task.status {
                TaskEntryStatus::Running => ("●", ActivityTone::Running),
                TaskEntryStatus::Completed => ("✓", ActivityTone::Completed),
                TaskEntryStatus::Failed => ("✗", ActivityTone::Error),
                TaskEntryStatus::Stopped => ("◐", ActivityTone::Dim),
            };
            lines.push(ActivityLine {
                spans: vec![
                    ActivitySpan::raw("  "),
                    ActivitySpan::tone(format!("{icon} "), tone),
                    ActivitySpan::raw(task.description.clone()),
                ],
            });
        }
        lines.push(ActivityLine::blank());
    }
}

fn append_subagent_lines(state: &AppState, lines: &mut Vec<ActivityLine>) {
    // Tree mode: when the Teammates view is expanded we draw a leader
    // row at the top and prefix each subagent with ├─ / └─ tree
    // connectors (highlighted ╞═ / ╘═ on the focused row). TS parity
    // with `TeammateSpinnerLine.tsx:83`. Otherwise we keep the flat
    // 2-space indent used by the inline activity surface.
    let tree_mode = matches!(
        state.session.expanded_view,
        coco_types::ExpandedView::Teammates
    );
    if tree_mode && !state.session.subagents.is_empty() {
        lines.push(ActivityLine {
            spans: vec![
                ActivitySpan::tone("● ", ActivityTone::Accent),
                ActivitySpan::bold("Leader", ActivityTone::Text),
                ActivitySpan::tone(" (lead)", ActivityTone::Dim),
            ],
        });
    }
    let total = state.session.subagents.len();
    for (i, agent) in state.session.subagents.iter().enumerate() {
        let is_focused = state.session.focused_subagent_index == Some(i as i32);
        // Backgrounded is orthogonal to status — a Running agent flipped
        // to background renders with the dim half-circle so the user can
        // tell it's detached but still alive.
        let (icon, tone) = if agent.is_backgrounded {
            ("◐", ActivityTone::Dim)
        } else {
            match agent.status {
                SubagentStatus::Running => ("●", ActivityTone::Running),
                SubagentStatus::Completed => ("✓", ActivityTone::Completed),
                SubagentStatus::Failed => ("✗", ActivityTone::Error),
            }
        };
        // Tree prefix in Teammates view: ╞═ / ╘═ for the focused row
        // (TS highlight state), ├─ / └─ otherwise. Falls back to the
        // existing 2-space focus marker in flat mode. All branches
        // return string literals, so we keep this as `&'static str`
        // and let `ActivitySpan::tone` do the single `String::from`
        // at the boundary instead of allocating twice.
        let row_prefix: &'static str = if tree_mode {
            match (is_focused, i + 1 == total) {
                (true, true) => "╘═ ",
                (true, false) => "╞═ ",
                (false, true) => "└─ ",
                (false, false) => "├─ ",
            }
        } else if is_focused {
            "▸ "
        } else {
            "  "
        };
        // Kind badge: differentiate TS `InProcessTeammateTask` (persistent
        // team member, `@name` prefix) from `LocalAgentTask` (Agent-tool
        // worker, plain agent_type). Mirrors TS `TeammateSpinnerLine`'s
        // `@{agentName}` rendering vs `AgentProgressLine`'s plain label.
        let agent_type = &agent.agent_type;
        let label = match agent.kind {
            crate::state::SubagentKind::Teammate => match agent.team_name.as_deref() {
                Some(team) => format!("@{agent_type}@{team}"),
                None => format!("@{agent_type}"),
            },
            crate::state::SubagentKind::Subagent => agent_type.clone(),
        };
        let mut spans = vec![
            ActivitySpan::tone(row_prefix, ActivityTone::Dim),
            ActivitySpan::tone(format!("{icon} "), tone),
            ActivitySpan::raw(agent.description.clone()),
            ActivitySpan::tone(format!(" ({label})"), ActivityTone::Dim),
        ];
        if let Some(started_ms) = agent.started_at_ms {
            let elapsed_ms = (crate::state::session::now_ms() - started_ms).max(0);
            spans.push(ActivitySpan::tone(
                format!(" {}", format_short_elapsed(elapsed_ms)),
                ActivityTone::Dim,
            ));
        }
        if agent.total_tokens > 0 {
            spans.push(ActivitySpan::tone(
                format!(" ↕{}", format_short_tokens(agent.total_tokens)),
                ActivityTone::Dim,
            ));
        }
        lines.push(ActivityLine { spans });

        // TS `TeammateSpinnerLine.tsx:160-169` active-text builder:
        // `summarizeRecentActivities` collapses trailing search/read
        // tools into a single line; otherwise falls back to the most
        // recent activity description. We append the canonical `· N
        // tools` stats segment in the same line so the user gets both
        // the action and the count without expanding the transcript.
        let has_tool_subline = agent.tool_count > 0 || agent.last_tool_name.is_some();
        if has_tool_subline && matches!(agent.status, SubagentStatus::Running) {
            let active_text =
                crate::widgets::activity_summary::summarize_trailing(&agent.recent_activities)
                    .or_else(|| agent.last_tool_name.clone());
            let mut subline = vec![ActivitySpan::raw("      ")];
            if let Some(text) = active_text {
                subline.push(ActivitySpan::tone(text, ActivityTone::Dim));
                subline.push(ActivitySpan::tone(" · ", ActivityTone::Dim));
            }
            subline.push(ActivitySpan::tone(
                format!("{} tools", agent.tool_count),
                ActivityTone::Dim,
            ));
            lines.push(ActivityLine { spans: subline });
        }

        // Completion summary mirrors TS `Done (N tools · ... · duration)`
        // plus the final assistant message preview when one was captured.
        if matches!(
            agent.status,
            SubagentStatus::Completed | SubagentStatus::Failed
        ) {
            let elapsed_ms = agent
                .started_at_ms
                .map(|s| (crate::state::session::now_ms() - s).max(0));
            let duration = elapsed_ms
                .map(|ms| format!(" · {}", format_short_elapsed(ms)))
                .unwrap_or_default();
            let done_label = if matches!(agent.status, SubagentStatus::Failed) {
                "Failed"
            } else {
                "Done"
            };
            lines.push(ActivityLine {
                spans: vec![
                    ActivitySpan::raw("      "),
                    ActivitySpan::tone(
                        format!("{done_label} ({} tools{duration})", agent.tool_count),
                        ActivityTone::Dim,
                    ),
                ],
            });
            if let Some(msg) = &agent.final_message {
                lines.push(ActivityLine {
                    spans: vec![
                        ActivitySpan::raw("      "),
                        ActivitySpan::tone(format!("“{msg}”"), ActivityTone::Dim),
                    ],
                });
            }
        }

        // Backgrounded but still alive — hint the user how to bring it back.
        // After the underlying task terminates the flag stays set but the
        // status icon already conveys the outcome, so the hint stays useful
        // only while running.
        if agent.is_backgrounded && matches!(agent.status, SubagentStatus::Running) {
            lines.push(ActivityLine {
                spans: vec![
                    ActivitySpan::raw("      "),
                    ActivitySpan::tone("↓ manage · Ctrl+T → Subagents", ActivityTone::Dim),
                ],
            });
        }

        if state.ui.show_teammate_message_preview {
            // Engine-pushed teammate Informational entries (Commit 2
            // routes them via `UserCommand::PushSystemMessage` with
            // `title = "teammate:<agent_id>"`) land in the engine's
            // `MessageHistory` and surface as cells.
            let cells = state.session.transcript.cells();
            for preview in last_preview_lines(cells, &agent.agent_id, 3) {
                lines.push(ActivityLine {
                    spans: vec![
                        ActivitySpan::raw("    "),
                        ActivitySpan::tone(preview, ActivityTone::Dim),
                    ],
                });
            }
        }
    }
}

fn append_coordinator_lines(state: &AppState, lines: &mut Vec<ActivityLine>) {
    for (i, agent) in state.session.subagents.iter().enumerate() {
        let is_selected = state.session.focused_subagent_index == Some(i as i32);
        let selector = if is_selected { "▸ " } else { "  " };
        let is_running = matches!(agent.status, SubagentStatus::Running);
        let status_icon = if is_running { "▶" } else { "⏸" };
        let status_tone = if is_running {
            ActivityTone::Running
        } else {
            ActivityTone::Dim
        };
        let desc = truncate_chars(&agent.description, 48);
        let mut spans = vec![
            ActivitySpan::raw(selector),
            ActivitySpan::tone(format!("{status_icon} "), status_tone),
            ActivitySpan::raw(desc),
            ActivitySpan::tone(" 0s", ActivityTone::Dim),
        ];
        if agent.total_tokens > 0 {
            spans.push(ActivitySpan::tone(
                format!(" ↕{}", format_short_tokens(agent.total_tokens)),
                ActivityTone::Dim,
            ));
        }
        lines.push(ActivityLine { spans });
    }
}

fn append_tool_lines(state: &AppState, lines: &mut Vec<ActivityLine>) {
    let tools: Vec<_> = state
        .session
        .tool_executions
        .iter()
        .filter(|tool| matches!(tool.status, ToolStatus::Queued | ToolStatus::Running))
        .collect();
    if tools.is_empty() {
        return;
    }

    if !lines.is_empty() && !matches!(lines.last(), Some(line) if line.spans.is_empty()) {
        lines.push(ActivityLine::blank());
    }
    lines.push(ActivityLine::section(
        t!("activity.section_tools").to_string(),
    ));
    let display_count = tools.len().min(MAX_TOOL_ACTIVITY_DISPLAY);
    let start = tools.len().saturating_sub(display_count);
    for tool in &tools[start..] {
        let (icon, tone) = match tool.status {
            ToolStatus::Queued => ("◦", ActivityTone::Running),
            ToolStatus::Running => ("⏳", ActivityTone::Running),
            ToolStatus::Completed => ("✓", ActivityTone::Completed),
            ToolStatus::Failed => ("✗", ActivityTone::Error),
        };
        let desc = tool.description.as_deref().unwrap_or(&tool.name);
        let truncated = truncate_chars(desc, constants::TOOL_DESCRIPTION_MAX_CHARS as usize);
        let elapsed = tool.elapsed();
        let elapsed_str = if elapsed.as_secs() >= 60 {
            format!("{}m{}s", elapsed.as_secs() / 60, elapsed.as_secs() % 60)
        } else if elapsed.as_secs() > 0 {
            format!("{}s", elapsed.as_secs())
        } else {
            format!("{}ms", elapsed.as_millis())
        };
        lines.push(ActivityLine {
            spans: vec![
                ActivitySpan::tone(format!("{icon} "), tone),
                ActivitySpan::raw(truncated),
                ActivitySpan::tone(format!(" ({elapsed_str})"), ActivityTone::Dim),
            ],
        });
    }
    lines.push(ActivityLine::blank());
}

/// Walk the engine-authoritative cells in reverse for the latest
/// `n` non-blank teammate-attributed preview lines. Teammate messages
/// arrive as `SystemMessage::Informational` cells whose title is
/// `teammate:<agent_id>` — that convention is set in
/// `server_notification_handler::protocol::push_teammate_message`.
fn last_preview_lines(
    cells: &[crate::state::transcript_view::RenderedCell],
    teammate_id: &str,
    n: usize,
) -> Vec<String> {
    let prefix = format!("teammate:{teammate_id}");
    let mut lines: Vec<String> = Vec::new();
    for cell in cells.iter().rev() {
        let coco_messages::Message::System(coco_messages::SystemMessage::Informational(info)) =
            cell.source.as_ref()
        else {
            continue;
        };
        if info.title != prefix {
            continue;
        }
        for line in info.message.lines().rev() {
            if lines.len() >= n {
                break;
            }
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            lines.push(trimmed.to_string());
        }
        if lines.len() >= n {
            break;
        }
    }
    lines.reverse();
    lines
}

fn format_short_elapsed(ms: i64) -> String {
    let secs = (ms / 1000).max(0);
    if secs < 60 {
        format!("{secs}s")
    } else if secs < 3_600 {
        format!("{}m {:02}s", secs / 60, secs % 60)
    } else {
        format!("{}h {:02}m", secs / 3_600, (secs % 3_600) / 60)
    }
}

fn format_short_tokens(total: i64) -> String {
    if total < 1_000 {
        format!("{total}")
    } else {
        format!("{:.1}k", total as f64 / 1_000.0)
    }
}

fn truncate_chars(value: &str, max_chars: usize) -> String {
    if value.width() <= max_chars {
        value.to_string()
    } else if max_chars == 0 {
        String::new()
    } else {
        format!(
            "{}…",
            value
                .chars()
                .take(max_chars.saturating_sub(1))
                .collect::<String>()
        )
    }
}

fn trim_trailing_blank(lines: &mut Vec<ActivityLine>) {
    while matches!(lines.last(), Some(line) if line.spans.is_empty()) {
        lines.pop();
    }
}

#[cfg(test)]
#[path = "activity.test.rs"]
mod tests;
