//! Team member status panel widget.
//!
//! Displays active team members with their status, role, and recent
//! activity. Follows the SubagentPanel pattern.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Color;
use ratatui::style::Style;
use ratatui::style::Stylize;
use ratatui::widgets::Block;
use ratatui::widgets::Borders;
use ratatui::widgets::Widget;

use unicode_width::UnicodeWidthStr;

use crate::state::TeamMemberEntry;
use crate::state::TeamMemberStatus;
use crate::theme::Theme;

/// Team panel widget.
///
/// Displays a list of team members with their status and role.
pub struct TeamPanel<'a> {
    members: &'a [TeamMemberEntry],
    team_name: &'a str,
    theme: &'a Theme,
    max_display: i32,
}

impl<'a> TeamPanel<'a> {
    /// Create a new team panel.
    pub fn new(members: &'a [TeamMemberEntry], team_name: &'a str, theme: &'a Theme) -> Self {
        Self {
            members,
            team_name,
            theme,
            max_display: 8,
        }
    }

    /// Set the maximum number of members to display.
    pub fn max_display(mut self, max: i32) -> Self {
        self.max_display = max;
        self
    }
}

impl Widget for TeamPanel<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.height < 3 || area.width < 10 {
            return;
        }

        let title = format!(" Team: {} ", self.team_name);
        let block = Block::default()
            .title(title.bold())
            .borders(Borders::ALL)
            .border_style(Style::default().fg(self.theme.border));

        let inner = block.inner(area);
        block.render(area, buf);

        if inner.height < 1 || self.members.is_empty() {
            return;
        }

        let mut y = inner.y;
        for member in self.members.iter().take(self.max_display as usize) {
            if y >= inner.y + inner.height {
                break;
            }

            // Status icon
            let (icon, style) = match member.status {
                TeamMemberStatus::Active => {
                    ("\u{25cf}", Style::default().fg(self.theme.tool_running))
                } // ●
                TeamMemberStatus::Idle => ("\u{25cb}", Style::default().fg(self.theme.secondary)), // ○
                TeamMemberStatus::ShuttingDown => ("\u{25d4}", Style::default().fg(Color::Yellow)), // ◔
                TeamMemberStatus::Stopped => ("\u{25cf}", Style::default().fg(self.theme.text_dim)), // ● dim
            };

            // Render status icon
            buf.set_string(inner.x, y, icon, style);

            // Role indicator (leader gets a crown)
            let role_icon = if member.is_leader { "\u{2605} " } else { "" }; // ★
            let role_x = inner.x + 2;
            if !role_icon.is_empty() {
                buf.set_string(role_x, y, role_icon, Style::default().fg(Color::Yellow));
            }

            // Member name
            let name_x = role_x + UnicodeWidthStr::width(role_icon) as u16;
            let name = member.display_name();
            let name_width =
                UnicodeWidthStr::width(name.as_str()).min((inner.width as usize).saturating_sub(4));
            buf.set_string(
                name_x,
                y,
                &name[..name.floor_char_boundary(name_width)],
                style.bold(),
            );

            // Agent type (dimmed)
            let type_x = name_x + name_width as u16 + 1;
            if type_x < inner.x + inner.width - 1 {
                let agent_type = member.agent_type.as_deref().unwrap_or("general");
                let available = (inner.x + inner.width - type_x) as usize;
                let type_str = if UnicodeWidthStr::width(agent_type) > available {
                    format!(
                        "{}...",
                        &agent_type[..agent_type.floor_char_boundary(available.saturating_sub(3))]
                    )
                } else {
                    agent_type.to_string()
                };
                buf.set_string(
                    type_x,
                    y,
                    format!("({type_str})"),
                    Style::default().fg(self.theme.text_dim),
                );
            }

            y += 1;
        }

        // Show count if more
        if self.members.len() > self.max_display as usize && y < inner.y + inner.height {
            let remaining = self.members.len() - self.max_display as usize;
            let text = format!("  +{remaining} more");
            buf.set_string(inner.x, y, text, Style::default().fg(self.theme.text_dim));
        }
    }
}

#[cfg(test)]
#[path = "team_panel.test.rs"]
mod tests;
