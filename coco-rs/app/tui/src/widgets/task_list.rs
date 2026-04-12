//! Task list widget — background task management overlay.
//!
//! TS: src/components/tasks/ (12 files, 4K LOC)
//! Shows running/completed/failed tasks with progress, output preview, and controls.

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

use crate::theme::Theme;

/// A task entry for the task list display.
#[derive(Debug, Clone)]
pub struct TaskEntry {
    pub id: String,
    pub name: String,
    pub status: TaskDisplayStatus,
    pub task_type: TaskDisplayType,
    pub progress: Option<String>,
    pub elapsed_ms: i64,
}

/// Task display status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskDisplayStatus {
    Running,
    Completed,
    Failed,
    Backgrounded,
}

/// Task display type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskDisplayType {
    Shell,
    Agent,
    Dream,
    Remote,
}

/// Task list widget for the overlay.
pub struct TaskListWidget<'a> {
    tasks: &'a [TaskEntry],
    selected: i32,
    theme: &'a Theme,
}

impl<'a> TaskListWidget<'a> {
    pub fn new(tasks: &'a [TaskEntry], theme: &'a Theme) -> Self {
        Self {
            tasks,
            selected: 0,
            theme,
        }
    }

    pub fn selected(mut self, index: i32) -> Self {
        self.selected = index;
        self
    }
}

impl Widget for TaskListWidget<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let mut lines: Vec<Line> = Vec::new();

        if self.tasks.is_empty() {
            lines.push(Line::from(
                Span::raw("  No background tasks").fg(self.theme.text_dim),
            ));
        }

        for (i, task) in self.tasks.iter().enumerate() {
            let is_selected = i as i32 == self.selected;
            let marker = if is_selected { "▸ " } else { "  " };

            let (icon, color) = match task.status {
                TaskDisplayStatus::Running => ("●", self.theme.tool_running),
                TaskDisplayStatus::Completed => ("✓", self.theme.tool_completed),
                TaskDisplayStatus::Failed => ("✗", self.theme.tool_error),
                TaskDisplayStatus::Backgrounded => ("◐", self.theme.text_dim),
            };

            let type_label = match task.task_type {
                TaskDisplayType::Shell => "shell",
                TaskDisplayType::Agent => "agent",
                TaskDisplayType::Dream => "dream",
                TaskDisplayType::Remote => "remote",
            };

            let elapsed = format_elapsed(task.elapsed_ms);

            let mut spans = vec![
                Span::raw(marker),
                Span::raw(format!("{icon} ")).fg(color),
                Span::raw(&task.name).fg(self.theme.text),
                Span::raw(format!(" [{type_label}]")).fg(self.theme.text_dim),
                Span::raw(format!(" ({elapsed})")).fg(self.theme.text_dim),
            ];

            if let Some(ref progress) = task.progress {
                spans.push(
                    Span::raw(format!(" — {progress}"))
                        .fg(self.theme.text_dim)
                        .italic(),
                );
            }

            lines.push(Line::from(spans));
        }

        lines.push(Line::default());
        lines.push(Line::from(vec![
            Span::raw("  [Enter] View  ").fg(self.theme.text_dim),
            Span::raw("[K] Kill  ").fg(self.theme.text_dim),
            Span::raw("[Esc] Close").fg(self.theme.text_dim),
        ]));

        let block = Block::default()
            .borders(Borders::ALL)
            .title(" Background Tasks ")
            .border_style(ratatui::style::Style::default().fg(self.theme.border_focused));

        let paragraph = Paragraph::new(lines)
            .block(block)
            .wrap(Wrap { trim: false });
        paragraph.render(area, buf);
    }
}

fn format_elapsed(ms: i64) -> String {
    if ms >= 60_000 {
        format!("{}m{}s", ms / 60_000, (ms % 60_000) / 1000)
    } else if ms >= 1000 {
        format!("{}s", ms / 1000)
    } else {
        format!("{ms}ms")
    }
}
