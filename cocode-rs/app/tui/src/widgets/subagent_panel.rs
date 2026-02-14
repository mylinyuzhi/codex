//! Subagent status panel widget.
//!
//! Displays active subagents with their status and progress.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::style::Stylize;
use ratatui::widgets::Block;
use ratatui::widgets::Borders;
use ratatui::widgets::Widget;

use crate::i18n::t;
use crate::state::SubagentInstance;
use crate::state::SubagentStatus;
use crate::theme::Theme;

/// Subagent panel widget.
///
/// Displays a list of active subagents with their status and progress.
pub struct SubagentPanel<'a> {
    subagents: &'a [SubagentInstance],
    theme: &'a Theme,
    max_display: i32,
}

impl<'a> SubagentPanel<'a> {
    /// Create a new subagent panel.
    pub fn new(subagents: &'a [SubagentInstance], theme: &'a Theme) -> Self {
        Self {
            subagents,
            theme,
            max_display: 5,
        }
    }

    /// Set the maximum number of subagents to display.
    pub fn max_display(mut self, max: i32) -> Self {
        self.max_display = max;
        self
    }
}

impl Widget for SubagentPanel<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.height < 3 || area.width < 10 {
            return;
        }

        // Create border
        let block = Block::default()
            .title(format!(" {} ", t!("subagent.title")).bold())
            .borders(Borders::ALL)
            .border_style(Style::default().fg(self.theme.border));

        let inner = block.inner(area);
        block.render(area, buf);

        if inner.height < 1 || self.subagents.is_empty() {
            return;
        }

        // Render subagents
        let mut y = inner.y;
        for subagent in self.subagents.iter().take(self.max_display as usize) {
            if y >= inner.y + inner.height {
                break;
            }

            // Status icon
            let (icon, style) = match subagent.status {
                SubagentStatus::Running => ("⚙", Style::default().fg(self.theme.tool_running)),
                SubagentStatus::Completed => ("✓", Style::default().fg(self.theme.tool_completed)),
                SubagentStatus::Failed => ("✗", Style::default().fg(self.theme.tool_error)),
                SubagentStatus::Backgrounded => ("◐", Style::default().fg(self.theme.secondary)),
            };

            // Agent type icon based on type name
            let type_icon = match subagent.agent_type.to_lowercase().as_str() {
                "explore" => "~ ",
                "plan" => "# ",
                "bash" => "> ",
                "code" | "code-simplifier" => "* ",
                _ => "",
            };

            // Format: "icon type_icon type: description"
            let type_str = &subagent.agent_type;
            let desc_str = &subagent.description;

            // Render status icon
            buf.set_string(inner.x, y, icon, style);

            // Render agent type icon
            let icon_x = inner.x + 2;
            buf.set_string(
                icon_x,
                y,
                type_icon,
                Style::default().fg(self.theme.text_dim),
            );

            // Render agent type
            let type_x = icon_x + type_icon.len() as u16;
            let type_width = type_str.len().min((inner.width as usize).saturating_sub(3));
            buf.set_string(type_x, y, &type_str[..type_width], style.bold());

            // Render colon
            let colon_x = type_x + type_width as u16;
            if colon_x < inner.x + inner.width - 1 {
                buf.set_string(colon_x, y, ": ", Style::default().fg(self.theme.text_dim));
            }

            // Render description (truncated if needed)
            let desc_x = colon_x + 2;
            if desc_x < inner.x + inner.width - 1 {
                let available = (inner.x + inner.width - desc_x) as usize;
                let desc = if desc_str.len() > available {
                    format!("{}...", &desc_str[..available.saturating_sub(3)])
                } else {
                    desc_str.clone()
                };
                buf.set_string(desc_x, y, desc, Style::default());
            }

            y += 1;

            // Render progress on next line if available
            if let Some(ref progress) = subagent.progress {
                if y < inner.y + inner.height {
                    let progress_str = if let (Some(current), Some(total)) =
                        (progress.current_step, progress.total_steps)
                    {
                        format!(
                            "  {}",
                            t!("subagent.step_progress", current = current, total = total)
                        )
                    } else if let Some(ref msg) = progress.message {
                        format!("  {}", msg)
                    } else {
                        String::new()
                    };

                    if !progress_str.is_empty() {
                        let available = inner.width as usize;
                        let text = if progress_str.len() > available {
                            format!("{}...", &progress_str[..available.saturating_sub(3)])
                        } else {
                            progress_str
                        };
                        buf.set_string(inner.x, y, text, Style::default().fg(self.theme.text_dim));
                        y += 1;
                    }
                }
            }
        }

        // Show count if more items exist
        if self.subagents.len() > self.max_display as usize {
            if y < inner.y + inner.height {
                let remaining = self.subagents.len() - self.max_display as usize;
                let text = format!("  {}", t!("subagent.more", count = remaining));
                buf.set_string(inner.x, y, text, Style::default().fg(self.theme.text_dim));
            }
        }
    }
}

#[cfg(test)]
#[path = "subagent_panel.test.rs"]
mod tests;
