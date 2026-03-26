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

use unicode_width::UnicodeWidthStr;

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
    /// Number of queued dialog overlays waiting behind the active one.
    queued_dialogs: i32,
    /// Context window tokens used.
    context_window_used: i32,
    /// Context window total capacity.
    context_window_total: i32,
    /// Estimated cost in cents.
    estimated_cost_cents: i32,
    /// Working directory.
    working_dir: Option<&'a str>,
    /// Active output style name.
    output_style: Option<&'a str>,
    /// Dynamic spinner text (tool name, "Compacting...", etc.).
    spinner_text: Option<&'a str>,
    /// Spinner frame string (time-based).
    spinner_frame: &'a str,
    /// Whether fast mode is active.
    fast_mode: bool,
    /// Whether sandbox mode is active.
    sandbox_active: bool,
    /// Recent sandbox violation count.
    sandbox_violation_count: i32,
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
            queued_dialogs: 0,
            context_window_used: 0,
            context_window_total: 0,
            estimated_cost_cents: 0,
            working_dir: None,
            output_style: None,
            spinner_text: None,
            spinner_frame: "⠋",
            fast_mode: false,
            sandbox_active: false,
            sandbox_violation_count: 0,
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

    /// Set the queue counts.
    pub fn queue_counts(mut self, queued: i32, queued_dialogs: i32) -> Self {
        self.queued_count = queued;
        self.queued_dialogs = queued_dialogs;
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

    /// Set the active output style name.
    pub fn output_style(mut self, style: Option<&'a str>) -> Self {
        self.output_style = style;
        self
    }

    /// Set the dynamic spinner text.
    pub fn spinner_text(mut self, text: Option<&'a str>) -> Self {
        self.spinner_text = text;
        self
    }

    /// Set the spinner frame string.
    pub fn spinner_frame(mut self, frame: &'a str) -> Self {
        self.spinner_frame = frame;
        self
    }

    /// Set whether fast mode is active.
    pub fn fast_mode(mut self, active: bool) -> Self {
        self.fast_mode = active;
        self
    }

    /// Set whether sandbox mode is active.
    pub fn sandbox_active(mut self, active: bool) -> Self {
        self.sandbox_active = active;
        self
    }

    /// Set the sandbox violation count.
    pub fn sandbox_violation_count(mut self, count: i32) -> Self {
        self.sandbox_violation_count = count;
        self
    }

    /// Format the model name for display.
    fn format_model(&self) -> Span<'static> {
        // Shorten long model names
        let max_len = crate::constants::MODEL_NAME_MAX_LEN as usize;
        let name = if UnicodeWidthStr::width(self.model) > max_len {
            let parts: Vec<&str> = self.model.split('-').collect();
            if parts.len() >= 2 {
                format!(
                    "{}-{}",
                    parts.first().unwrap_or(&self.model),
                    parts.last().unwrap_or(&"")
                )
            } else {
                self.model[..max_len].to_string()
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
        let bar_count = crate::constants::CONTEXT_GAUGE_BAR_COUNT as usize;
        let filled = (percent as usize * bar_count / 100).min(bar_count);
        let empty = bar_count - filled;
        let gauge = format!("[{}{}]{}%", "█".repeat(filled), "░".repeat(empty), percent);

        let color = if percent < crate::constants::CONTEXT_WARNING_THRESHOLD {
            self.theme.success
        } else if percent < crate::constants::CONTEXT_ERROR_THRESHOLD {
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
            if count >= crate::constants::TOKEN_FORMAT_MILLIONS {
                format!(
                    "{:.1}M",
                    count as f64 / crate::constants::TOKEN_FORMAT_MILLIONS as f64
                )
            } else if count >= crate::constants::TOKEN_FORMAT_THOUSANDS {
                format!(
                    "{:.1}k",
                    count as f64 / crate::constants::TOKEN_FORMAT_THOUSANDS as f64
                )
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
        match (self.queued_count, self.queued_dialogs) {
            (0, 0) => None,
            (q, 0) => {
                let text = format!(" {} ", t!("status.queued", count = q));
                Some(Span::raw(text).fg(self.theme.warning))
            }
            (0, d) => {
                let text = format!(" {} ", t!("status.dialogs_waiting", count = d));
                Some(Span::raw(text).fg(self.theme.warning))
            }
            (q, d) => {
                let text = format!(
                    " {} +{} ",
                    t!("status.queued", count = q),
                    t!("status.dialogs_waiting", count = d)
                );
                Some(Span::raw(text).fg(self.theme.warning))
            }
        }
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
            let display =
                crate::path_display::shorten_path(dir, crate::constants::WORKING_DIR_MAX_LEN);
            Span::raw(format!(" {display} ")).fg(self.theme.text_dim)
        })
    }

    /// Format the output style indicator.
    fn format_output_style(&self) -> Option<Span<'static>> {
        self.output_style.map(|name| {
            Span::raw(format!(" {} ", t!("status.output_style", name = name)))
                .fg(self.theme.success)
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

    /// Format the fast mode indicator.
    fn format_fast_mode(&self) -> Option<Span<'static>> {
        if self.fast_mode {
            Some(
                Span::raw(format!(" {} ", t!("status.fast_mode")))
                    .fg(self.theme.warning)
                    .bold(),
            )
        } else {
            None
        }
    }

    /// Format the sandbox mode indicator with optional violation count.
    fn format_sandbox(&self) -> Option<Vec<Span<'static>>> {
        if !self.sandbox_active {
            return None;
        }

        let mut spans = vec![
            Span::raw(format!(" {} ", t!("status.sandbox")))
                .fg(self.theme.success)
                .bold(),
        ];

        if self.sandbox_violation_count > 0 {
            spans.push(
                Span::raw(format!("!{} ", self.sandbox_violation_count))
                    .fg(self.theme.warning)
                    .bold(),
            );
        }

        Some(spans)
    }
}

impl Widget for StatusBar<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.height < 1 {
            return;
        }

        let sep = || Span::raw("│").fg(self.theme.text_dim);
        let max_width = area.width as usize;

        // Core segments (always shown): model + thinking level
        let mut spans: Vec<Span> = vec![self.format_model(), sep(), self.format_thinking(), sep()];

        // Optional segments ordered by drop priority (last dropped first).
        // Each is (spans_to_add, display_width).
        let mut optional: Vec<Vec<Span>> = Vec::new();

        // Priority 1 (dropped last): fast mode, sandbox, plan mode, context gauge, spinner
        if let Some(fast_span) = self.format_fast_mode() {
            optional.push(vec![fast_span, sep()]);
        }
        if let Some(mut sandbox_spans) = self.format_sandbox() {
            sandbox_spans.push(sep());
            optional.push(sandbox_spans);
        }
        if let Some(plan_span) = self.format_plan_mode() {
            optional.push(vec![plan_span, sep()]);
        }
        if let Some(gauge_span) = self.format_context_gauge() {
            optional.push(vec![gauge_span, sep()]);
        }
        if let Some(text) = self.spinner_text {
            let shimmer_text = format!(" {} {text} ", self.spinner_frame);
            let mut shimmer = crate::shimmer::shimmer_spans(&shimmer_text);
            shimmer.push(sep());
            optional.push(shimmer);
        }

        // Priority 2: thinking status, output style, queue
        if let Some(thinking_span) = self.format_thinking_status() {
            optional.push(vec![thinking_span, sep()]);
        }
        if let Some(style_span) = self.format_output_style() {
            optional.push(vec![style_span, sep()]);
        }
        if let Some(queue_span) = self.format_queue_status() {
            optional.push(vec![queue_span, sep()]);
        }

        // Priority 3: MCP, tokens
        if let Some(mcp_span) = self.format_mcp_status() {
            optional.push(vec![mcp_span, sep()]);
        }
        optional.push(vec![self.format_tokens()]);

        // Priority 4 (dropped first): thinking toggle, cost, working dir
        if let Some(toggle_span) = self.format_thinking_toggle() {
            optional.push(vec![toggle_span, sep()]);
        }
        if let Some(cost_span) = self.format_cost() {
            optional.push(vec![sep(), cost_span]);
        }
        if let Some(dir_span) = self.format_working_dir() {
            optional.push(vec![sep(), dir_span]);
        }

        // Greedy fit: add optional segments while they fit
        let core_width: usize = spans.iter().map(ratatui::prelude::Span::width).sum();
        let mut used = core_width;

        for segment in &optional {
            let seg_width: usize = segment.iter().map(ratatui::prelude::Span::width).sum();
            if used + seg_width <= max_width {
                spans.extend(segment.iter().cloned());
                used += seg_width;
            }
        }

        // Add hints if there's room
        let hints = self.format_hints();
        let hints_width = hints.width();
        if used + hints_width + 2 <= max_width {
            let spacer_width = max_width - used - hints_width;
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
