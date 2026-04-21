//! Unified task/todo panel.
//!
//! Renders three sections sourced from `SessionState`:
//! 1. **Plan items** — persistent V2 tasks (from `TaskListHandle`)
//!    projected into the panel by `ToolAppState.plan_tasks`.
//! 2. **Todos** — per-agent V1 checklists (from `TodoListHandle`) in
//!    `ToolAppState.todos_by_agent`. One sub-section per agent/session.
//! 3. **Running tasks** — background shell/agent tasks
//!    (`SessionState::active_tasks`), already tracked separately.
//!
//! TS parity: `components/tasks/BackgroundTasksDialog.tsx` for the
//! running list, plus the V1 todo renderer in `components/todos/`.
//! We collapse all three into a single panel that the user toggles
//! open — auto-expanded when `expanded_view == Tasks`.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::text::Span;
use ratatui::widgets::Block;
use ratatui::widgets::Borders;
use ratatui::widgets::Paragraph;
use ratatui::widgets::Widget;
use ratatui::widgets::Wrap;

use crate::i18n::t;
use crate::theme::Theme;
use coco_types::TaskListStatus;
use coco_types::TaskRecord;
use coco_types::TodoRecord;
use std::collections::HashMap;

use super::task_list::TaskEntry;

pub struct PlanPanel<'a> {
    plan_tasks: &'a [TaskRecord],
    todos: &'a HashMap<String, Vec<TodoRecord>>,
    running: &'a [TaskEntry],
    theme: &'a Theme,
}

impl<'a> PlanPanel<'a> {
    pub fn new(
        plan_tasks: &'a [TaskRecord],
        todos: &'a HashMap<String, Vec<TodoRecord>>,
        running: &'a [TaskEntry],
        theme: &'a Theme,
    ) -> Self {
        Self {
            plan_tasks,
            todos,
            running,
            theme,
        }
    }

    /// Whether the panel has anything to show — useful for the caller
    /// to decide if the layout slot should be allocated at all.
    pub fn has_content(&self) -> bool {
        !self.plan_tasks.is_empty() || !self.todos.is_empty() || !self.running.is_empty()
    }
}

fn status_icon_and_color<'a>(
    status: TaskListStatus,
    theme: &'a Theme,
) -> (&'static str, ratatui::style::Color) {
    match status {
        TaskListStatus::Pending => ("○", theme.text_dim),
        TaskListStatus::InProgress => ("◑", theme.tool_running),
        TaskListStatus::Completed => ("●", theme.tool_completed),
    }
}

fn todo_icon_and_color<'a>(
    status: &str,
    theme: &'a Theme,
) -> (&'static str, ratatui::style::Color) {
    match status {
        "pending" => ("○", theme.text_dim),
        "in_progress" => ("◑", theme.tool_running),
        "completed" => ("●", theme.tool_completed),
        _ => ("?", theme.text_dim),
    }
}

impl Widget for PlanPanel<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let mut lines: Vec<Line> = Vec::new();

        // ── Plan items (V2) ───────────────────────────────────
        if !self.plan_tasks.is_empty() {
            lines.push(Line::from(
                Span::raw(t!("plan_panel.section_tasks").to_string())
                    .fg(self.theme.accent)
                    .bold(),
            ));
            for task in self.plan_tasks {
                let (icon, color) = status_icon_and_color(task.status, self.theme);
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
                lines.push(Line::from(vec![
                    Span::raw("  "),
                    Span::raw(format!("{icon} ")).fg(color),
                    Span::raw(format!("#{} ", task.id)).fg(self.theme.text_dim),
                    Span::raw(task.subject.clone()).fg(self.theme.text),
                    Span::raw(owner).fg(self.theme.text_dim),
                    Span::raw(blocked).fg(self.theme.warning),
                ]));
            }
            lines.push(Line::default());
        }

        // ── Todos (V1) ────────────────────────────────────────
        if !self.todos.is_empty() {
            lines.push(Line::from(
                Span::raw(t!("plan_panel.section_todos").to_string())
                    .fg(self.theme.accent)
                    .bold(),
            ));
            // Stable iteration order (keys sorted) so snapshots don't flake.
            let mut keys: Vec<&String> = self.todos.keys().collect();
            keys.sort();
            for key in keys {
                let items = &self.todos[key];
                if items.is_empty() {
                    continue;
                }
                lines.push(Line::from(
                    Span::raw(format!("  [{key}]")).fg(self.theme.text_dim),
                ));
                for item in items {
                    let (icon, color) = todo_icon_and_color(&item.status, self.theme);
                    lines.push(Line::from(vec![
                        Span::raw("  "),
                        Span::raw(format!("{icon} ")).fg(color),
                        Span::raw(item.content.clone()).fg(self.theme.text),
                    ]));
                }
            }
            lines.push(Line::default());
        }

        // ── Running tasks ─────────────────────────────────────
        if !self.running.is_empty() {
            lines.push(Line::from(
                Span::raw(t!("plan_panel.section_running").to_string())
                    .fg(self.theme.accent)
                    .bold(),
            ));
            for task in self.running {
                let (icon, color) = match task.status {
                    super::task_list::TaskDisplayStatus::Running => ("●", self.theme.tool_running),
                    super::task_list::TaskDisplayStatus::Completed => {
                        ("✓", self.theme.tool_completed)
                    }
                    super::task_list::TaskDisplayStatus::Failed => ("✗", self.theme.tool_error),
                    super::task_list::TaskDisplayStatus::Backgrounded => ("◐", self.theme.text_dim),
                };
                lines.push(Line::from(vec![
                    Span::raw("  "),
                    Span::raw(format!("{icon} ")).fg(color),
                    Span::raw(task.name.clone()).fg(self.theme.text),
                ]));
            }
            lines.push(Line::default());
        }

        if lines.is_empty() {
            lines.push(Line::from(
                Span::raw(format!("  {}", t!("plan_panel.empty"))).fg(self.theme.text_dim),
            ));
        }

        let block = Block::default()
            .borders(Borders::ALL)
            .title(t!("plan_panel.title").to_string())
            .border_style(ratatui::style::Style::default().fg(self.theme.border_focused));
        let paragraph = Paragraph::new(lines)
            .block(block)
            .wrap(Wrap { trim: false });
        paragraph.render(area, buf);
    }
}

#[cfg(test)]
#[path = "plan_panel.test.rs"]
mod tests;
