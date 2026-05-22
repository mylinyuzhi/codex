//! Subagent status panel widget.
//!
//! Displays active subagents with their status and progress.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Color;
use ratatui::style::Style;
use ratatui::style::Stylize;
use ratatui::widgets::Block;
use ratatui::widgets::Borders;
use ratatui::widgets::Widget;

use unicode_width::UnicodeWidthStr;

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
    focused_index: Option<i32>,
}

impl<'a> SubagentPanel<'a> {
    /// Create a new subagent panel.
    pub fn new(subagents: &'a [SubagentInstance], theme: &'a Theme) -> Self {
        Self {
            subagents,
            theme,
            max_display: 5,
            focused_index: None,
        }
    }

    /// Set the maximum number of subagents to display.
    pub fn max_display(mut self, max: i32) -> Self {
        self.max_display = max;
        self
    }

    /// Set the focused subagent index (for quick-switch highlighting).
    pub fn focused_index(mut self, index: Option<i32>) -> Self {
        self.focused_index = index;
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
        for (idx, subagent) in self
            .subagents
            .iter()
            .enumerate()
            .take(self.max_display as usize)
        {
            let is_focused = self.focused_index == Some(idx as i32);

            if y >= inner.y + inner.height {
                break;
            }

            // Status icon
            let (icon, style) = match subagent.status {
                SubagentStatus::Running => ("⚙", Style::default().fg(self.theme.tool_running)),
                SubagentStatus::Completed => ("✓", Style::default().fg(self.theme.tool_completed)),
                SubagentStatus::Failed => ("✗", Style::default().fg(self.theme.tool_error)),
                SubagentStatus::Backgrounded => ("◐", Style::default().fg(self.theme.secondary)),
                SubagentStatus::Killed => ("⊘", Style::default().fg(self.theme.tool_error)),
            };

            let focus_style = if is_focused {
                style.bold().underlined()
            } else {
                style
            };

            // Use agent color from definition if available
            let type_color = subagent
                .color
                .as_deref()
                .and_then(parse_agent_color)
                .unwrap_or(self.theme.text_dim);

            // Agent type icon based on type name
            let type_icon = match subagent.agent_type.as_str() {
                "explore" => "~ ",
                "plan" => "# ",
                "bash" => "> ",
                "code" | "code-simplifier" => "* ",
                _ => "",
            };

            // Format: "icon type_icon type: description (elapsed)"
            let type_str = &subagent.agent_type;
            let desc_str = &subagent.description;

            // Elapsed time for running agents
            let elapsed_str = if subagent.status == SubagentStatus::Running {
                let secs = subagent.started_at.elapsed().as_secs();
                if secs > 0 {
                    format!(" {secs}s")
                } else {
                    String::new()
                }
            } else {
                String::new()
            };

            // Render status icon
            buf.set_string(inner.x, y, icon, focus_style);

            // Render agent type icon (uses definition color if available)
            let icon_x = inner.x + 2;
            let type_icon_style = if is_focused {
                Style::default().fg(type_color).bold().underlined()
            } else {
                Style::default().fg(type_color)
            };
            buf.set_string(icon_x, y, type_icon, type_icon_style);

            // Render agent type
            let type_x = icon_x + UnicodeWidthStr::width(type_icon) as u16;
            let type_width = UnicodeWidthStr::width(type_str.as_str())
                .min((inner.width as usize).saturating_sub(3));
            buf.set_string(type_x, y, &type_str[..type_width], focus_style.bold());

            // Render colon
            let colon_x = type_x + type_width as u16;
            if colon_x < inner.x + inner.width - 1 {
                buf.set_string(colon_x, y, ": ", Style::default().fg(self.theme.text_dim));
            }

            // Render description (truncated if needed) + elapsed
            let desc_x = colon_x + 2;
            if desc_x < inner.x + inner.width - 1 {
                let elapsed_reserve = UnicodeWidthStr::width(elapsed_str.as_str());
                let available = (inner.x + inner.width - desc_x) as usize - elapsed_reserve;
                let desc = if UnicodeWidthStr::width(desc_str.as_str()) > available {
                    format!(
                        "{}...",
                        &desc_str[..desc_str.floor_char_boundary(available.saturating_sub(3))]
                    )
                } else {
                    desc_str.clone()
                };
                buf.set_string(desc_x, y, &desc, Style::default());
                if !elapsed_str.is_empty() {
                    let elapsed_x = desc_x + UnicodeWidthStr::width(desc.as_str()) as u16;
                    if elapsed_x < inner.x + inner.width {
                        buf.set_string(
                            elapsed_x,
                            y,
                            &elapsed_str,
                            Style::default().fg(self.theme.text_dim),
                        );
                    }
                }
            }

            y += 1;

            // Render output file path for backgrounded agents
            if subagent.status == SubagentStatus::Backgrounded
                && y < inner.y + inner.height
                && let Some(ref output_file) = subagent.output_file
            {
                let path_str = output_file.to_string_lossy();
                let truncated = if UnicodeWidthStr::width(path_str.as_ref()) > 40 {
                    format!("...{}", &path_str[path_str.len() - 37..])
                } else {
                    path_str.to_string()
                };
                let file_line = format!("  \u{2192} {truncated}");
                let available = inner.width as usize;
                let text = if UnicodeWidthStr::width(file_line.as_str()) > available {
                    format!(
                        "{}...",
                        &file_line[..file_line.floor_char_boundary(available.saturating_sub(3))]
                    )
                } else {
                    file_line
                };
                buf.set_string(inner.x, y, text, Style::default().fg(self.theme.text_dim));
                y += 1;
            }

            // Render result preview for completed agents
            if subagent.status == SubagentStatus::Completed
                && y < inner.y + inner.height
                && let Some(ref result) = subagent.result
            {
                let preview = if UnicodeWidthStr::width(result.as_str()) > 60 {
                    format!("{}...", &result[..result.floor_char_boundary(57)])
                } else {
                    result.clone()
                };
                let preview_line = format!("  {preview}");
                let available = inner.width as usize;
                let text = if UnicodeWidthStr::width(preview_line.as_str()) > available {
                    format!(
                        "{}...",
                        &preview_line
                            [..preview_line.floor_char_boundary(available.saturating_sub(3))]
                    )
                } else {
                    preview_line
                };
                buf.set_string(
                    inner.x,
                    y,
                    text,
                    Style::default().fg(self.theme.text_dim).italic(),
                );
                y += 1;
            }

            // Render progress on next line if available
            if let Some(ref progress) = subagent.progress
                && y < inner.y + inner.height
            {
                let progress_str = if let (Some(current), Some(total)) =
                    (progress.current_step, progress.total_steps)
                {
                    format!(
                        "  {}",
                        t!("subagent.step_progress", current = current, total = total)
                    )
                } else if let Some(ref msg) = progress.message {
                    format!("  {msg}")
                } else {
                    String::new()
                };

                if !progress_str.is_empty() {
                    let available = inner.width as usize;
                    let text = if UnicodeWidthStr::width(progress_str.as_str()) > available {
                        format!(
                            "{}...",
                            &progress_str
                                [..progress_str.floor_char_boundary(available.saturating_sub(3))]
                        )
                    } else {
                        progress_str
                    };
                    buf.set_string(inner.x, y, text, Style::default().fg(self.theme.text_dim));
                    y += 1;
                }
            }
        }

        // Show count if more items exist
        if self.subagents.len() > self.max_display as usize && y < inner.y + inner.height {
            let remaining = self.subagents.len() - self.max_display as usize;
            let text = format!("  {}", t!("subagent.more", count = remaining));
            buf.set_string(inner.x, y, text, Style::default().fg(self.theme.text_dim));
        }
    }
}

/// Parse an agent color name to a ratatui Color.
fn parse_agent_color(name: &str) -> Option<Color> {
    match name {
        "cyan" => Some(Color::Cyan),
        "blue" => Some(Color::Blue),
        "green" => Some(Color::Green),
        "yellow" => Some(Color::Yellow),
        "magenta" => Some(Color::Magenta),
        "red" => Some(Color::Red),
        "orange" => Some(Color::LightRed),
        _ => None,
    }
}

#[cfg(test)]
#[path = "subagent_panel.test.rs"]
mod tests;
