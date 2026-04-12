//! Teammate spinner tree — displays active teammates in tree layout.
//!
//! TS: components/Spinner/TeammateSpinnerTree.tsx, TeammateSpinnerLine.tsx
//!
//! Shows team lead as root with teammates as children, color-coded by
//! AgentColorName, with status/idle tracking, token/tool stats.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::text::Span;
use ratatui::widgets::Block;
use ratatui::widgets::Borders;
use ratatui::widgets::Paragraph;
use ratatui::widgets::Widget;

use crate::theme::Theme;

/// A teammate entry for spinner display.
#[derive(Debug, Clone)]
pub struct TeammateSpinnerEntry {
    pub name: String,
    pub color: Option<String>,
    pub is_idle: bool,
    pub shutdown_requested: bool,
    pub awaiting_plan_approval: bool,
    pub spinner_verb: Option<String>,
    pub past_tense_verb: Option<String>,
    pub tool_count: i32,
    pub token_count: i64,
    pub elapsed_ms: i64,
}

/// Teammate spinner tree widget.
pub struct TeammateSpinnerTree<'a> {
    teammates: &'a [TeammateSpinnerEntry],
    leader_name: &'a str,
    all_idle: bool,
    theme: &'a Theme,
}

impl<'a> TeammateSpinnerTree<'a> {
    pub fn new(
        teammates: &'a [TeammateSpinnerEntry],
        leader_name: &'a str,
        theme: &'a Theme,
    ) -> Self {
        let all_idle = teammates.iter().all(|t| t.is_idle);
        Self {
            teammates,
            leader_name,
            all_idle,
            theme,
        }
    }
}

impl Widget for TeammateSpinnerTree<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let mut lines: Vec<Line> = Vec::new();

        // Leader line (root)
        lines.push(Line::from(vec![
            Span::raw("● ").fg(self.theme.primary),
            Span::raw(self.leader_name).bold().fg(self.theme.text),
            " (lead)".dim(),
        ]));

        // Teammate lines (children)
        let count = self.teammates.len();
        for (i, teammate) in self.teammates.iter().enumerate() {
            let is_last = i == count - 1;
            let prefix = if is_last { "└─ " } else { "├─ " };

            let mut spans: Vec<Span> = Vec::new();
            spans.push(Span::raw(prefix).dim());

            // Status icon
            let (icon, icon_color) = if teammate.shutdown_requested {
                ("■", self.theme.tool_error)
            } else if teammate.awaiting_plan_approval {
                ("⏸", self.theme.warning)
            } else if teammate.is_idle {
                ("○", self.theme.text_dim)
            } else {
                ("●", self.theme.tool_running)
            };
            spans.push(Span::raw(format!("{icon} ")).fg(icon_color));

            // Name (with color hint)
            let name_color = teammate
                .color
                .as_deref()
                .map(agent_color_to_ratatui)
                .unwrap_or(self.theme.text);
            spans.push(Span::raw(&teammate.name).fg(name_color));

            // Status text
            if teammate.shutdown_requested {
                spans.push(" [stopping]".dim());
            } else if teammate.awaiting_plan_approval {
                spans.push(Span::raw(" [awaiting approval]").fg(self.theme.warning));
            } else if teammate.is_idle {
                let idle_text = format_idle_duration(teammate.elapsed_ms);
                if self.all_idle {
                    if let Some(verb) = &teammate.past_tense_verb {
                        spans.push(Span::raw(format!(" {verb}")).dim());
                    }
                } else {
                    spans.push(Span::raw(format!(" Idle {idle_text}")).dim());
                }
            } else if let Some(verb) = &teammate.spinner_verb {
                spans.push(Span::raw(format!(" {verb}…")).fg(self.theme.text_dim));
            }

            // Stats
            if teammate.tool_count > 0 || teammate.token_count > 0 {
                let stats = format!(
                    " ({} tools, {} tokens)",
                    teammate.tool_count,
                    format_token_count(teammate.token_count),
                );
                spans.push(Span::raw(stats).dim());
            }

            lines.push(Line::from(spans));
        }

        if lines.is_empty() {
            lines.push(Line::from(Span::raw("  No teammates").dim()));
        }

        let panel = Paragraph::new(lines).block(
            Block::default()
                .borders(Borders::TOP)
                .title(" Team ")
                .border_style(ratatui::style::Style::default().fg(self.theme.border)),
        );
        panel.render(area, buf);
    }
}

/// Format idle duration for display.
fn format_idle_duration(elapsed_ms: i64) -> String {
    let secs = elapsed_ms / 1000;
    if secs < 60 {
        format!("{secs}s")
    } else {
        let mins = secs / 60;
        format!("{mins}m")
    }
}

/// Format token count for compact display.
fn format_token_count(tokens: i64) -> String {
    if tokens >= 1000 {
        format!("{:.1}k", tokens as f64 / 1000.0)
    } else {
        format!("{tokens}")
    }
}

/// Map agent color name string to ratatui Color.
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

#[cfg(test)]
#[path = "teammate_spinner.test.rs"]
mod tests;
