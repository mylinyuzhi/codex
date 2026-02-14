//! Status bar widget.
//!
//! Displays:
//! - Current model name
//! - Thinking level
//! - Plan mode indicator
//! - Context window usage gauge
//! - Token breakdown (input/output/cache)
//! - Cost estimate
//! - Working directory
//! - Thinking duration

use std::time::Duration;

use cocode_protocol::ReasoningEffort;
use cocode_protocol::ThinkingLevel;
use cocode_protocol::TokenUsage;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::text::Span;
use ratatui::widgets::Widget;

use crate::i18n::t;
use crate::state::PlanPhase;
use crate::theme::Theme;

/// Status bar widget showing model, thinking level, plan mode, and tokens.
pub struct StatusBar<'a> {
    model: &'a str,
    thinking_level: &'a ThinkingLevel,
    plan_mode: bool,
    token_usage: &'a TokenUsage,
    theme: &'a Theme,
    /// Whether currently streaming thinking content.
    is_thinking: bool,
    /// Whether thinking display is enabled.
    show_thinking_enabled: bool,
    /// Current or last thinking duration.
    thinking_duration: Option<Duration>,
    /// Current phase in plan mode.
    plan_phase: Option<PlanPhase>,
    /// Number of connected MCP servers.
    mcp_server_count: i32,
    /// Number of queued commands.
    queued_count: i32,
    /// Context window tokens used.
    context_window_used: i32,
    /// Context window total capacity.
    context_window_total: i32,
    /// Estimated cost in cents.
    estimated_cost_cents: i32,
    /// Working directory.
    working_dir: Option<&'a str>,
}

impl<'a> StatusBar<'a> {
    /// Create a new status bar.
    pub fn new(
        model: &'a str,
        thinking_level: &'a ThinkingLevel,
        plan_mode: bool,
        token_usage: &'a TokenUsage,
        theme: &'a Theme,
    ) -> Self {
        Self {
            model,
            thinking_level,
            plan_mode,
            token_usage,
            theme,
            is_thinking: false,
            show_thinking_enabled: false,
            thinking_duration: None,
            plan_phase: None,
            mcp_server_count: 0,
            queued_count: 0,
            context_window_used: 0,
            context_window_total: 0,
            estimated_cost_cents: 0,
            working_dir: None,
        }
    }

    /// Set whether the assistant is currently thinking.
    pub fn is_thinking(mut self, thinking: bool) -> Self {
        self.is_thinking = thinking;
        self
    }

    /// Set whether thinking display is enabled.
    pub fn show_thinking_enabled(mut self, enabled: bool) -> Self {
        self.show_thinking_enabled = enabled;
        self
    }

    /// Set the thinking duration (current or last completed).
    pub fn thinking_duration(mut self, duration: Option<Duration>) -> Self {
        self.thinking_duration = duration;
        self
    }

    /// Set the plan phase.
    pub fn plan_phase(mut self, phase: Option<PlanPhase>) -> Self {
        self.plan_phase = phase;
        self
    }

    /// Set the MCP server count.
    pub fn mcp_server_count(mut self, count: i32) -> Self {
        self.mcp_server_count = count;
        self
    }

    /// Set the queue count.
    pub fn queue_counts(mut self, queued: i32, _steering: i32) -> Self {
        self.queued_count = queued;
        self
    }

    /// Set the context window usage.
    pub fn context_window(mut self, used: i32, total: i32) -> Self {
        self.context_window_used = used;
        self.context_window_total = total;
        self
    }

    /// Set the estimated cost in cents.
    pub fn estimated_cost(mut self, cents: i32) -> Self {
        self.estimated_cost_cents = cents;
        self
    }

    /// Set the working directory.
    pub fn working_dir(mut self, dir: Option<&'a str>) -> Self {
        self.working_dir = dir;
        self
    }

    /// Format the model name for display.
    fn format_model(&self) -> Span<'static> {
        // Shorten long model names
        let name = if self.model.len() > 24 {
            let parts: Vec<&str> = self.model.split('-').collect();
            if parts.len() >= 2 {
                format!("{}-{}", parts[0], parts.last().unwrap_or(&""))
            } else {
                self.model[..24].to_string()
            }
        } else {
            self.model.to_string()
        };
        Span::raw(format!(" {name} ")).fg(self.theme.primary)
    }

    /// Format the thinking level for display.
    fn format_thinking(&self) -> Span<'static> {
        let (label, color) = match self.thinking_level.effort {
            ReasoningEffort::None => (t!("status.think_off").to_string(), self.theme.text_dim),
            ReasoningEffort::Minimal => (t!("status.think_min").to_string(), self.theme.text_dim),
            ReasoningEffort::Low => (t!("status.think_low").to_string(), self.theme.success),
            ReasoningEffort::Medium => (t!("status.think_med").to_string(), self.theme.warning),
            ReasoningEffort::High => (t!("status.think_high").to_string(), self.theme.thinking),
            ReasoningEffort::XHigh => (t!("status.think_max").to_string(), self.theme.error),
        };

        Span::raw(format!(" {} ", t!("status.think_label", level = label))).fg(color)
    }

    /// Format the plan mode indicator.
    fn format_plan_mode(&self) -> Option<Span<'static>> {
        if self.plan_mode {
            if let Some(phase) = self.plan_phase {
                let phase_text = format!(" {} {} ", phase.emoji(), phase.display_name());
                Some(Span::styled(
                    phase_text,
                    Style::default().bg(self.theme.plan_mode).bold(),
                ))
            } else {
                let plan_text = format!(" {} ", t!("status.plan"));
                Some(Span::styled(
                    plan_text,
                    Style::default().bg(self.theme.plan_mode).bold(),
                ))
            }
        } else {
            None
        }
    }

    /// Format the context window usage gauge.
    fn format_context_gauge(&self) -> Option<Span<'static>> {
        if self.context_window_total <= 0 {
            return None;
        }

        let percent =
            (self.context_window_used as f64 / self.context_window_total as f64 * 100.0) as i32;
        let percent = percent.clamp(0, 100);

        // Build gauge: [████░░] 62%
        let filled = (percent * 6 / 100) as usize;
        let empty = 6 - filled;
        let gauge = format!("[{}{}]{}%", "█".repeat(filled), "░".repeat(empty), percent);

        let color = if percent < 60 {
            self.theme.success
        } else if percent < 80 {
            self.theme.warning
        } else {
            self.theme.error
        };

        Some(Span::raw(format!(" {gauge} ")).fg(color))
    }

    /// Format the token breakdown.
    fn format_tokens(&self) -> Span<'static> {
        let input = self.token_usage.input_tokens;
        let output = self.token_usage.output_tokens;
        let cache = self.token_usage.cache_read_tokens.unwrap_or(0);

        let format_count = |count: i64| -> String {
            if count >= 1_000_000 {
                format!("{:.1}M", count as f64 / 1_000_000.0)
            } else if count >= 1_000 {
                format!("{:.1}k", count as f64 / 1_000.0)
            } else {
                format!("{count}")
            }
        };

        let mut text = format!(" ↑{} ↓{}", format_count(input), format_count(output));
        if cache > 0 {
            text.push_str(&format!(" ♻{}", format_count(cache)));
        }
        text.push(' ');

        Span::raw(text).fg(self.theme.text_dim)
    }

    /// Format the MCP server indicator.
    fn format_mcp_status(&self) -> Option<Span<'static>> {
        if self.mcp_server_count > 0 {
            Some(
                Span::raw(format!(
                    " {} ",
                    t!("status.mcp", count = self.mcp_server_count)
                ))
                .fg(self.theme.success),
            )
        } else {
            None
        }
    }

    /// Format the queue status indicator.
    fn format_queue_status(&self) -> Option<Span<'static>> {
        if self.queued_count == 0 {
            return None;
        }

        let text = format!(" {} ", t!("status.queued", count = self.queued_count));
        Some(Span::raw(text).fg(self.theme.warning))
    }

    /// Format the cost estimate.
    fn format_cost(&self) -> Option<Span<'static>> {
        if self.estimated_cost_cents <= 0 {
            return None;
        }

        let dollars = self.estimated_cost_cents as f64 / 100.0;
        Some(Span::raw(format!(" ${dollars:.2} ")).fg(self.theme.text_dim))
    }

    /// Format the working directory.
    fn format_working_dir(&self) -> Option<Span<'static>> {
        self.working_dir.map(|dir| {
            let home = std::env::var("HOME").unwrap_or_default();
            let shortened = if !home.is_empty() && dir.starts_with(&home) {
                format!("~{}", &dir[home.len()..])
            } else {
                dir.to_string()
            };
            // Truncate if too long
            let display = if shortened.len() > 25 {
                let parts: Vec<&str> = shortened.split('/').collect();
                if parts.len() > 3 {
                    format!("{}/.../{}", parts[..2].join("/"), parts[parts.len() - 1])
                } else {
                    shortened
                }
            } else {
                shortened
            };
            Span::raw(format!(" {display} ")).fg(self.theme.text_dim)
        })
    }

    /// Format keyboard hints.
    fn format_hints(&self) -> Span<'static> {
        Span::raw(format!(" {} ", t!("status.hints"))).fg(self.theme.text_dim)
    }

    /// Format the thinking status indicator.
    fn format_thinking_status(&self) -> Option<Span<'static>> {
        if self.is_thinking {
            let duration_text = if let Some(duration) = self.thinking_duration {
                let secs = duration.as_secs();
                if secs > 0 {
                    format!(" {} ", t!("status.thinking_with_duration", duration = secs))
                } else {
                    format!(" {} ", t!("status.thinking"))
                }
            } else {
                format!(" {} ", t!("status.thinking"))
            };
            Some(
                Span::raw(format!("🤔{duration_text}"))
                    .fg(self.theme.thinking)
                    .italic(),
            )
        } else if let Some(duration) = self.thinking_duration {
            let secs = duration.as_secs();
            if secs > 0 {
                Some(
                    Span::raw(format!(" {} ", t!("status.thought_for", duration = secs)))
                        .fg(self.theme.text_dim),
                )
            } else {
                None
            }
        } else {
            None
        }
    }

    /// Format the thinking display toggle indicator.
    fn format_thinking_toggle(&self) -> Option<Span<'static>> {
        if self.show_thinking_enabled {
            Some(Span::raw(" 💭 ").fg(self.theme.text_dim))
        } else {
            None
        }
    }
}

impl Widget for StatusBar<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.height < 1 {
            return;
        }

        // Build the status line
        let mut spans: Vec<Span> = vec![];

        // Model
        spans.push(self.format_model());
        spans.push(Span::raw("│").fg(self.theme.text_dim));

        // Thinking level
        spans.push(self.format_thinking());
        spans.push(Span::raw("│").fg(self.theme.text_dim));

        // Plan mode (if active)
        if let Some(plan_span) = self.format_plan_mode() {
            spans.push(plan_span);
            spans.push(Span::raw("│").fg(self.theme.text_dim));
        }

        // Context window gauge (if data available)
        if let Some(gauge_span) = self.format_context_gauge() {
            spans.push(gauge_span);
            spans.push(Span::raw("│").fg(self.theme.text_dim));
        }

        // Thinking status (if currently thinking)
        if let Some(thinking_span) = self.format_thinking_status() {
            spans.push(thinking_span);
            spans.push(Span::raw("│").fg(self.theme.text_dim));
        }

        // Thinking toggle indicator (if enabled)
        if let Some(toggle_span) = self.format_thinking_toggle() {
            spans.push(toggle_span);
            spans.push(Span::raw("│").fg(self.theme.text_dim));
        }

        // MCP servers (if any connected)
        if let Some(mcp_span) = self.format_mcp_status() {
            spans.push(mcp_span);
            spans.push(Span::raw("│").fg(self.theme.text_dim));
        }

        // Queue status (if items pending)
        if let Some(queue_span) = self.format_queue_status() {
            spans.push(queue_span);
            spans.push(Span::raw("│").fg(self.theme.text_dim));
        }

        // Tokens (input/output breakdown)
        spans.push(self.format_tokens());

        // Cost estimate
        if let Some(cost_span) = self.format_cost() {
            spans.push(Span::raw("│").fg(self.theme.text_dim));
            spans.push(cost_span);
        }

        // Working directory
        if let Some(dir_span) = self.format_working_dir() {
            spans.push(Span::raw("│").fg(self.theme.text_dim));
            spans.push(dir_span);
        }

        // Calculate used width
        let used_width: usize = spans.iter().map(ratatui::prelude::Span::width).sum();

        // Add hints if there's room
        let hints = self.format_hints();
        let hints_width = hints.width();
        if used_width + hints_width + 2 <= area.width as usize {
            // Add spacer
            let spacer_width = area.width as usize - used_width - hints_width;
            spans.push(Span::raw(" ".repeat(spacer_width)));
            spans.push(hints);
        }

        let line = Line::from(spans);
        buf.set_line(area.x, area.y, &line, area.width);
    }
}

#[cfg(test)]
#[path = "status_bar.test.rs"]
mod tests;
