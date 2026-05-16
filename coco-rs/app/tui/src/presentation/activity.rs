//! View model for the live activity surface in the main console workspace.

use coco_types::ExpandedView;
use coco_types::TaskListStatus;
use unicode_width::UnicodeWidthStr;

use crate::constants;
use crate::i18n::t;
use crate::state::AppState;
use crate::state::MessageContent;
use crate::state::SubagentStatus;
use crate::state::TokenUsage;
use crate::state::session::TaskEntryStatus;
use crate::state::session::ToolStatus;

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
    pub(crate) text: String,
    pub(crate) tone: ActivityTone,
    pub(crate) bold: bool,
}

impl ActivitySpan {
    fn raw(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            tone: ActivityTone::Text,
            bold: false,
        }
    }

    fn tone(text: impl Into<String>, tone: ActivityTone) -> Self {
        Self {
            text: text.into(),
            tone,
            bold: false,
        }
    }

    fn bold(text: impl Into<String>, tone: ActivityTone) -> Self {
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
    fn blank() -> Self {
        Self { spans: Vec::new() }
    }

    fn section(title: String) -> Self {
        Self {
            spans: vec![ActivitySpan::bold(title, ActivityTone::Accent)],
        }
    }

    fn text(text: impl Into<String>, tone: ActivityTone) -> Self {
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
    let has_plan_activity =
        !state.session.plan_tasks.is_empty() || !state.session.todos_by_agent.is_empty();
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
    } else if has_tool_activity || state.session.stream_stall || state.session.was_interrupted {
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
    if state.session.was_interrupted {
        lines.push(ActivityLine {
            spans: vec![
                ActivitySpan::tone("  ! ", ActivityTone::Warning),
                ActivitySpan::tone(t!("toast.interrupted").to_string(), ActivityTone::Warning),
            ],
        });
    }
    if !lines.is_empty() {
        lines.push(ActivityLine::blank());
    }
    lines
}

fn append_plan_lines(state: &AppState, lines: &mut Vec<ActivityLine>) {
    if !state.session.plan_tasks.is_empty() {
        lines.push(ActivityLine::section(
            t!("plan_panel.section_tasks").to_string(),
        ));
        for task in &state.session.plan_tasks {
            let (icon, tone) = match task.status {
                TaskListStatus::Pending => ("○", ActivityTone::Dim),
                TaskListStatus::InProgress => ("◑", ActivityTone::Running),
                TaskListStatus::Completed => ("●", ActivityTone::Completed),
            };
            let owner = task
                .owner
                .as_deref()
                .map(|o| format!(" ({o})"))
                .unwrap_or_default();
            let blocked = if task.blocked_by.is_empty() {
                String::new()
            } else {
                format!(" [blocked by {}]", task.blocked_by.join(", "))
            };
            lines.push(ActivityLine {
                spans: vec![
                    ActivitySpan::raw("  "),
                    ActivitySpan::tone(format!("{icon} "), tone),
                    ActivitySpan::tone(format!("#{} ", task.id), ActivityTone::Dim),
                    ActivitySpan::raw(task.subject.clone()),
                    ActivitySpan::tone(owner, ActivityTone::Dim),
                    ActivitySpan::tone(blocked, ActivityTone::Warning),
                ],
            });
        }
        lines.push(ActivityLine::blank());
    }

    if !state.session.todos_by_agent.is_empty() {
        lines.push(ActivityLine::section(
            t!("plan_panel.section_todos").to_string(),
        ));
        let mut keys: Vec<&String> = state.session.todos_by_agent.keys().collect();
        keys.sort();
        for key in keys {
            let items = &state.session.todos_by_agent[key];
            if items.is_empty() {
                continue;
            }
            lines.push(ActivityLine::text(format!("  [{key}]"), ActivityTone::Dim));
            for item in items {
                let (icon, tone) = match item.status.as_str() {
                    "pending" => ("○", ActivityTone::Dim),
                    "in_progress" => ("◑", ActivityTone::Running),
                    "completed" => ("●", ActivityTone::Completed),
                    _ => ("?", ActivityTone::Dim),
                };
                lines.push(ActivityLine {
                    spans: vec![
                        ActivitySpan::raw("  "),
                        ActivitySpan::tone(format!("{icon} "), tone),
                        ActivitySpan::raw(item.content.clone()),
                    ],
                });
            }
        }
        lines.push(ActivityLine::blank());
    }

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
    for (i, agent) in state.session.subagents.iter().enumerate() {
        let is_focused = state.session.focused_subagent_index == Some(i as i32);
        let (icon, tone) = match agent.status {
            SubagentStatus::Running => ("●", ActivityTone::Running),
            SubagentStatus::Completed => ("✓", ActivityTone::Completed),
            SubagentStatus::Backgrounded => ("◐", ActivityTone::Dim),
            SubagentStatus::Failed => ("✗", ActivityTone::Error),
        };
        let focus_marker = if is_focused { "▸ " } else { "  " };
        let mut spans = vec![
            ActivitySpan::raw(focus_marker),
            ActivitySpan::tone(format!("{icon} "), tone),
            ActivitySpan::raw(agent.description.clone()),
            ActivitySpan::tone(format!(" ({})", agent.agent_type), ActivityTone::Dim),
        ];
        if let Some(started_ms) = agent.started_at_ms {
            let elapsed_ms = (crate::state::session::now_ms() - started_ms).max(0);
            spans.push(ActivitySpan::tone(
                format!(" {}", format_short_elapsed(elapsed_ms)),
                ActivityTone::Dim,
            ));
        }
        if let Some(tokens) = agent.token_usage.as_ref() {
            let total = tokens.input_tokens + tokens.output_tokens;
            if total > 0 {
                spans.push(ActivitySpan::tone(
                    format!(" ↕{}", format_short_tokens(total)),
                    ActivityTone::Dim,
                ));
            }
        }
        lines.push(ActivityLine { spans });

        if state.ui.show_teammate_message_preview {
            for preview in last_preview_lines(&state.session.messages, &agent.agent_id, 3) {
                lines.push(ActivityLine {
                    spans: vec![
                        ActivitySpan::raw("    "),
                        ActivitySpan::tone(preview.to_string(), ActivityTone::Dim),
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
        if let Some(tokens) = agent.token_usage.as_ref() {
            let total = total_tokens(tokens);
            if total > 0 {
                spans.push(ActivitySpan::tone(
                    format!(" ↕{}", format_short_tokens(total)),
                    ActivityTone::Dim,
                ));
            }
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

fn last_preview_lines<'a>(
    messages: &'a [crate::state::ChatMessage],
    teammate_id: &str,
    n: usize,
) -> Vec<&'a str> {
    let mut lines: Vec<&str> = Vec::new();
    for msg in messages.iter().rev() {
        let MessageContent::TeammateMessage { teammate, content } = &msg.content else {
            continue;
        };
        if teammate != teammate_id {
            continue;
        }
        for line in content.lines().rev() {
            if lines.len() >= n {
                break;
            }
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            lines.push(trimmed);
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

fn total_tokens(tokens: &TokenUsage) -> i64 {
    tokens.input_tokens + tokens.output_tokens
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
