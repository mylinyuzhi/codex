//! Status bar widget — model, tokens, mode, MCP info.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::text::Span;
use ratatui::widgets::Paragraph;
use ratatui::widgets::Widget;

use crate::i18n::t;
use crate::state::AppState;
use crate::theme::Theme;

/// Bottom status bar.
pub struct StatusBar<'a> {
    state: &'a AppState,
    theme: &'a Theme,
}

impl<'a> StatusBar<'a> {
    pub fn new(state: &'a AppState, theme: &'a Theme) -> Self {
        Self { state, theme }
    }
}

impl Widget for StatusBar<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let mut parts = Vec::new();

        // Provider tag (e.g. `[anthropic]`). Suppressed when the
        // provider is unknown so mock/test sessions and pre-bootstrap
        // states render unchanged — see `SessionState::provider`.
        if !self.state.session.provider.is_empty() {
            parts.push(
                Span::raw(format!(" [{}]", self.state.session.provider)).fg(self.theme.text_dim),
            );
        }

        // Model
        parts.push(
            Span::raw(format!(" {}", self.state.session.model))
                .fg(self.theme.primary)
                .bold(),
        );

        // Fast mode
        if self.state.session.fast_mode {
            parts.push(Span::raw(" ⚡").fg(self.theme.warning));
        }

        // Thinking effort. `Auto` is the resting state — we suppress
        // the badge so the bar stays quiet when the user hasn't taken
        // explicit control. Any other state earns a brain glyph plus
        // the effort name so Ctrl+T's effect is visible.
        if !matches!(
            self.state.session.thinking_effort,
            coco_types::ReasoningEffort::Auto
        ) {
            parts.push(
                Span::raw(format!(" 🧠 {}", self.state.session.thinking_effort)).fg(
                    if matches!(
                        self.state.session.thinking_effort,
                        coco_types::ReasoningEffort::Disable
                    ) {
                        self.theme.text_dim
                    } else {
                        self.theme.accent
                    },
                ),
            );
        }

        // Sandbox shield (TS `SandboxPromptFooterHint`). Surfaces when the
        // engine has wrapped tool execution in a platform sandbox
        // (bwrap/Seatbelt) so the user knows shell writes are confined.
        if self.state.session.sandbox_active {
            parts.push(Span::raw(" 🛡").fg(self.theme.success));
        }

        // Permission mode
        parts.push(Span::raw(" | ").fg(self.theme.border));
        parts.push(
            Span::raw(format!("{:?}", self.state.session.permission_mode)).fg(self.theme.text_dim),
        );

        // Token usage
        let tokens = &self.state.session.token_usage;
        let total = tokens.input_tokens + tokens.output_tokens;
        if total > 0 {
            parts.push(Span::raw(" | ").fg(self.theme.border));
            if total >= 1000 {
                parts.push(
                    Span::raw(
                        t!(
                            "status.tokens_k",
                            value = format!("{:.1}", total as f64 / 1000.0)
                        )
                        .to_string(),
                    )
                    .fg(self.theme.text_dim),
                );
            } else {
                parts.push(
                    Span::raw(t!("status.tokens_total", count = total).to_string())
                        .fg(self.theme.text_dim),
                );
            }
        }

        // Cache read tokens
        if tokens.cache_read_tokens > 0 {
            parts.push(
                Span::raw(format!(" ({}cr)", tokens.cache_read_tokens)).fg(self.theme.text_dim),
            );
        }

        // MCP servers
        let mcp_count = self.state.session.connected_mcp_count();
        if mcp_count > 0 {
            parts.push(Span::raw(" | ").fg(self.theme.border));
            parts.push(
                Span::raw(t!("status.mcp", count = mcp_count).to_string()).fg(self.theme.text_dim),
            );
        }

        // LSP indicator. Sticky-on for the session when the
        // `LspManagerAdapter` reported `is_connected = true` at startup
        // (Feature::Lsp on + prewarm spawned ≥ 1 server). Mirrors the
        // MCP badge pattern.
        if self.state.session.lsp_active {
            parts.push(Span::raw(" | ").fg(self.theme.border));
            parts.push(Span::raw("LSP").fg(self.theme.accent));
        }

        // Cost
        if self.state.session.estimated_cost_cents > 0 {
            parts.push(Span::raw(" | ").fg(self.theme.border));
            let cost = self.state.session.estimated_cost_cents as f64 / 100.0;
            parts.push(Span::raw(format!("${cost:.2}")).fg(self.theme.text_dim));
        }

        // Message count
        parts.push(Span::raw(" | ").fg(self.theme.border));
        parts.push(
            Span::raw(t!("status.msgs", count = self.state.session.messages.len()).to_string())
                .fg(self.theme.text_dim),
        );

        let line = Line::from(parts);
        let bar = Paragraph::new(line).style(Style::default().bg(self.theme.border));
        bar.render(area, buf);
    }
}
