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
//! ## V1 / V2 mutual exclusion (`Feature::TaskV2`)
//!
//! `Feature::TaskV2` (key `task_v2`, default-on) is the runtime gate
//! mirroring TS `isTodoV2Enabled()` (`utils/tasks.ts:133-139`). It
//! decides which task-tool family the model sees:
//!
//! - **TaskV2 ON**: `TaskCreate/TaskGet/TaskUpdate/TaskList` are
//!   registered; `TodoWrite` is filtered out. The server emits
//!   `TaskPanelChanged` → `plan_tasks` populated; `todos_by_agent`
//!   stays empty.
//! - **TaskV2 OFF**: `TodoWrite` is the only task tool; the V2
//!   quartet is filtered out. The server emits per-agent todo updates
//!   → `todos_by_agent` populated; `plan_tasks` stays empty.
//!
//! The two are **never both populated** in normal operation, so the
//! `is_empty()` gates below collapse to "render whichever has data".
//! No explicit `Feature` lookup is needed — data presence is the gate.
//!
//! ## Divergence from TS (intentional, coco-rs extension)
//!
//! TS does **not** render the V1 TodoWrite list anywhere — TodoWrite
//! is a silent in-memory mutation on `appState.todos[sessionId]` with
//! no UI panel. Only `TaskListV2.tsx` renders, and it short-circuits
//! to `null` when `!isTodoV2Enabled()` — so TS users in interactive
//! REPL mode (V2 default-off in TS) see no task panel at all.
//!
//! coco-rs surfaces V1 in this panel as a deliberate extension so the
//! "Tasks" expanded view always has content when there's something to
//! show, regardless of which task-tool family is active. The default
//! in coco-rs is `Feature::TaskV2` default-on (V2 by default), so V1
//! rendering only fires for users who opted out via `settings.json`
//! `features.task_v2 = false` or a session that forces V1.
//!
//! TS parity reference: `components/tasks/BackgroundTasksDialog.tsx`
//! for the running list. V1 rendering has no TS counterpart.

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
use crate::presentation::styles::UiStyles;
use coco_types::TaskListStatus;
use coco_types::TaskRecord;
use coco_types::TodoRecord;
use std::collections::HashMap;

use super::task_list::TaskEntry;

pub struct PlanPanel<'a> {
    plan_tasks: &'a [TaskRecord],
    todos: &'a HashMap<String, Vec<TodoRecord>>,
    running: &'a [TaskEntry],
    styles: UiStyles<'a>,
}

impl<'a> PlanPanel<'a> {
    pub fn new(
        plan_tasks: &'a [TaskRecord],
        todos: &'a HashMap<String, Vec<TodoRecord>>,
        running: &'a [TaskEntry],
        styles: UiStyles<'a>,
    ) -> Self {
        Self {
            plan_tasks,
            todos,
            running,
            styles,
        }
    }

    /// Whether the panel has anything to show — useful for the caller
    /// to decide if the layout slot should be allocated at all.
    pub fn has_content(&self) -> bool {
        !self.plan_tasks.is_empty() || !self.todos.is_empty() || !self.running.is_empty()
    }
}

fn status_icon_and_color(
    status: TaskListStatus,
    styles: UiStyles<'_>,
) -> (&'static str, ratatui::style::Color) {
    match status {
        TaskListStatus::Pending => ("○", styles.dim()),
        TaskListStatus::InProgress => ("◑", styles.tool_running()),
        TaskListStatus::Completed => ("●", styles.tool_completed()),
    }
}

fn todo_icon_and_color(
    status: &str,
    styles: UiStyles<'_>,
) -> (&'static str, ratatui::style::Color) {
    match status {
        "pending" => ("○", styles.dim()),
        "in_progress" => ("◑", styles.tool_running()),
        "completed" => ("●", styles.tool_completed()),
        _ => ("?", styles.dim()),
    }
}

impl Widget for PlanPanel<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let mut lines: Vec<Line> = Vec::new();

        // ── Plan items (V2) ───────────────────────────────────
        if !self.plan_tasks.is_empty() {
            lines.push(Line::from(
                Span::raw(t!("plan_panel.section_tasks").to_string())
                    .fg(self.styles.accent())
                    .bold(),
            ));
            for task in self.plan_tasks {
                let (icon, color) = status_icon_and_color(task.status, self.styles);
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
                    Span::raw(format!("#{} ", task.id)).fg(self.styles.dim()),
                    Span::raw(task.subject.clone()).fg(self.styles.text()),
                    Span::raw(owner).fg(self.styles.dim()),
                    Span::raw(blocked).fg(self.styles.warning()),
                ]));
            }
            lines.push(Line::default());
        }

        // ── Todos (V1) ────────────────────────────────────────
        if !self.todos.is_empty() {
            lines.push(Line::from(
                Span::raw(t!("plan_panel.section_todos").to_string())
                    .fg(self.styles.accent())
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
                // TS parity (`TodoWriteTool`): when every entry under
                // an agent reaches `completed`, collapse the whole
                // group to a single dim summary line so the panel keeps
                // the per-agent header visible without flooding with
                // checked-off items the user already finished. Reading
                // back through the transcript expands them in full.
                let all_completed = items.iter().all(|it| it.status == "completed");
                if all_completed {
                    lines.push(Line::from(
                        Span::raw(format!("  [{key}] ({} completed)", items.len()))
                            .fg(self.styles.dim()),
                    ));
                    continue;
                }
                lines.push(Line::from(
                    Span::raw(format!("  [{key}]")).fg(self.styles.dim()),
                ));
                for item in items {
                    let (icon, color) = todo_icon_and_color(&item.status, self.styles);
                    lines.push(Line::from(vec![
                        Span::raw("  "),
                        Span::raw(format!("{icon} ")).fg(color),
                        Span::raw(item.content.clone()).fg(self.styles.text()),
                    ]));
                }
            }
            lines.push(Line::default());
        }

        // ── Running tasks ─────────────────────────────────────
        if !self.running.is_empty() {
            lines.push(Line::from(
                Span::raw(t!("plan_panel.section_running").to_string())
                    .fg(self.styles.accent())
                    .bold(),
            ));
            for task in self.running {
                let (icon, color) = match task.status {
                    super::task_list::TaskDisplayStatus::Running => {
                        ("●", self.styles.tool_running())
                    }
                    super::task_list::TaskDisplayStatus::Completed => {
                        ("✓", self.styles.tool_completed())
                    }
                    super::task_list::TaskDisplayStatus::Failed => ("✗", self.styles.tool_error()),
                    super::task_list::TaskDisplayStatus::Backgrounded => ("◐", self.styles.dim()),
                };
                lines.push(Line::from(vec![
                    Span::raw("  "),
                    Span::raw(format!("{icon} ")).fg(color),
                    Span::raw(task.name.clone()).fg(self.styles.text()),
                ]));
            }
            lines.push(Line::default());
        }

        if lines.is_empty() {
            lines.push(Line::from(
                Span::raw(format!("  {}", t!("plan_panel.empty"))).fg(self.styles.dim()),
            ));
        }

        let block = Block::default()
            .borders(Borders::ALL)
            .title(t!("plan_panel.title").to_string())
            .border_style(ratatui::style::Style::default().fg(self.styles.focused_border()));
        let paragraph = Paragraph::new(lines)
            .block(block)
            .wrap(Wrap { trim: false });
        paragraph.render(area, buf);
    }
}

#[cfg(test)]
#[path = "plan_panel.test.rs"]
mod tests;
