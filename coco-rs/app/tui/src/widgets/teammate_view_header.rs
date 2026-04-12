//! Teammate view header — shown when viewing a teammate's transcript.
//!
//! TS: components/TeammateViewHeader.tsx
//!
//! Displays: "Viewing @agentName · [esc to return]" with colored agent name.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::text::Span;
use ratatui::widgets::Paragraph;
use ratatui::widgets::Widget;

use crate::theme::Theme;

/// Teammate view header widget.
pub struct TeammateViewHeader<'a> {
    agent_name: &'a str,
    agent_color: Option<&'a str>,
    description: Option<&'a str>,
    theme: &'a Theme,
}

impl<'a> TeammateViewHeader<'a> {
    pub fn new(agent_name: &'a str, theme: &'a Theme) -> Self {
        Self {
            agent_name,
            agent_color: None,
            description: None,
            theme,
        }
    }

    pub fn agent_color(mut self, color: Option<&'a str>) -> Self {
        self.agent_color = color;
        self
    }

    pub fn description(mut self, desc: Option<&'a str>) -> Self {
        self.description = desc;
        self
    }
}

impl Widget for TeammateViewHeader<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let name_color = self
            .agent_color
            .map(agent_color_to_ratatui)
            .unwrap_or(self.theme.primary);

        let mut lines = vec![Line::from(vec![
            Span::raw("Viewing ").dim(),
            Span::raw(format!("@{}", self.agent_name))
                .fg(name_color)
                .bold(),
            Span::raw(" · ").dim(),
            Span::raw("[esc to return]").dim(),
        ])];

        if let Some(desc) = self.description {
            lines.push(Line::from(Span::raw(desc).dim()));
        }

        let widget = Paragraph::new(lines);
        widget.render(area, buf);
    }
}

/// Map agent color name to ratatui Color.
fn agent_color_to_ratatui(color_name: &str) -> ratatui::style::Color {
    match color_name {
        "red" => ratatui::style::Color::Red,
        "blue" => ratatui::style::Color::Blue,
        "green" => ratatui::style::Color::Green,
        "yellow" => ratatui::style::Color::Yellow,
        "purple" | "magenta" => ratatui::style::Color::Magenta,
        "orange" => ratatui::style::Color::LightRed,
        "pink" => ratatui::style::Color::LightMagenta,
        "cyan" => ratatui::style::Color::Cyan,
        _ => ratatui::style::Color::Reset,
    }
}
