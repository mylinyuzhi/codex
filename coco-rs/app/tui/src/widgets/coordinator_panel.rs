//! Coordinator agent status panel — steerable list of background agents.
//!
//! TS: components/CoordinatorAgentStatus.tsx
//!
//! Shows background agent tasks with elapsed time, token count, queued
//! message count, and task description.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::text::Span;
use ratatui::widgets::Block;
use ratatui::widgets::Borders;
use ratatui::widgets::Paragraph;
use ratatui::widgets::Widget;

use crate::i18n::t;
use crate::presentation::styles::UiStyles;

/// A background agent task for the coordinator panel.
#[derive(Debug, Clone)]
pub struct CoordinatorTask {
    pub task_id: String,
    pub description: String,
    pub is_running: bool,
    pub elapsed_ms: i64,
    pub token_count: i64,
    pub queued_messages: i32,
}

impl CoordinatorTask {
    /// Convert from a session-state `SubagentInstance`. Token count
    /// and elapsed_ms aren't tracked on the per-instance struct yet,
    /// so they default to 0 — the panel still renders meaningfully,
    /// and when richer telemetry lands the field can populate without
    /// changing the call site (P2 minimum-viable rendering).
    pub fn from_subagent(s: &crate::state::SubagentInstance) -> Self {
        use crate::state::SubagentStatus;
        Self {
            task_id: s.agent_id.clone(),
            description: s.description.clone(),
            is_running: matches!(s.status, SubagentStatus::Running),
            elapsed_ms: 0,
            token_count: 0,
            queued_messages: 0,
        }
    }
}

/// Coordinator agent status panel widget.
pub struct CoordinatorPanel<'a> {
    tasks: &'a [CoordinatorTask],
    selected_index: Option<i32>,
    styles: UiStyles<'a>,
}

impl<'a> CoordinatorPanel<'a> {
    pub fn new(tasks: &'a [CoordinatorTask], styles: UiStyles<'a>) -> Self {
        Self {
            tasks,
            selected_index: None,
            styles,
        }
    }

    pub fn selected_index(mut self, index: Option<i32>) -> Self {
        self.selected_index = index;
        self
    }
}

impl Widget for CoordinatorPanel<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let mut lines: Vec<Line> = Vec::new();

        for (i, task) in self.tasks.iter().enumerate() {
            let is_selected = self.selected_index == Some(i as i32);
            let selector = if is_selected { "▸ " } else { "  " };

            let status_icon = if task.is_running { "▶" } else { "⏸" };
            let status_color = if task.is_running {
                self.styles.tool_running()
            } else {
                self.styles.dim()
            };

            let elapsed = format_elapsed(task.elapsed_ms);
            let tokens = format_tokens(task.token_count);

            let mut spans = vec![
                Span::raw(selector),
                Span::raw(format!("{status_icon} ")).fg(status_color),
            ];

            // Description (truncated)
            let max_desc = (area.width as usize).saturating_sub(30);
            let desc = if task.description.len() > max_desc {
                format!("{}…", &task.description[..max_desc.saturating_sub(1)])
            } else {
                task.description.clone()
            };
            spans.push(Span::raw(desc).fg(self.styles.text()));

            // Stats
            spans.push(Span::raw(format!(" {elapsed}")).dim());
            if task.token_count > 0 {
                spans.push(Span::raw(format!(" ↕{tokens}")).dim());
            }
            if task.queued_messages > 0 {
                spans.push(
                    Span::raw(format!(" 📨{}", task.queued_messages)).fg(self.styles.accent()),
                );
            }

            lines.push(Line::from(spans));
        }

        if lines.is_empty() {
            lines.push(Line::from(
                Span::raw(format!("  {}", t!("coordinator.none"))).dim(),
            ));
        }

        let panel = Paragraph::new(lines).block(
            Block::default()
                .borders(Borders::TOP)
                .title(format!(" {} ", t!("coordinator.title")))
                .border_style(ratatui::style::Style::default().fg(self.styles.border())),
        );
        panel.render(area, buf);
    }
}

fn format_elapsed(ms: i64) -> String {
    let secs = ms / 1000;
    if secs < 60 {
        format!("{secs}s")
    } else {
        let mins = secs / 60;
        let remaining_secs = secs % 60;
        format!("{mins}:{remaining_secs:02}")
    }
}

fn format_tokens(tokens: i64) -> String {
    if tokens >= 1000 {
        format!("{:.1}k", tokens as f64 / 1000.0)
    } else {
        format!("{tokens}")
    }
}

#[cfg(test)]
#[path = "coordinator_panel.test.rs"]
mod tests;
