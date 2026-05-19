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
use crate::presentation::styles::UiStyles;
use crate::state::AppState;

/// Bottom status bar.
pub struct StatusBar<'a> {
    state: &'a AppState,
    styles: UiStyles<'a>,
}

impl<'a> StatusBar<'a> {
    pub fn new(state: &'a AppState, styles: UiStyles<'a>) -> Self {
        Self { state, styles }
    }
}

impl Widget for StatusBar<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        // Exit-confirmation hint takes the whole bar when armed so
        // it's unmissable. TS:
        // `PromptInputFooterLeftSide.tsx:147-150` collapses the rest
        // of the footer onto a single "Press X again to exit" line.
        if let Some(key) = self.state.ui.pending_exit_hint() {
            let text = t!("status.exit_prompt", key = key.label()).to_string();
            tracing::info!(
                key = key.label(),
                prompt = %text,
                width = area.width,
                "status bar rendering exit prompt"
            );
            let line = Line::from(Span::raw(text).fg(self.styles.warning()).bold());
            let bar = Paragraph::new(line).style(Style::default().bg(self.styles.border()));
            bar.render(area, buf);
            return;
        }

        let mut parts = Vec::new();

        // Provider tag (e.g. `[anthropic]`). Suppressed when the
        // provider is unknown so mock/test sessions and pre-bootstrap
        // states render unchanged — see `SessionState::provider`.
        if !self.state.session.provider.is_empty() {
            parts.push(
                Span::raw(format!(" [{}]", self.state.session.provider)).fg(self.styles.dim()),
            );
        }

        // Model
        parts.push(
            Span::raw(format!(" {}", self.state.session.model))
                .fg(self.styles.primary())
                .bold(),
        );

        // Fast mode
        if self.state.session.fast_mode {
            parts.push(Span::raw(" ⚡").fg(self.styles.warning()));
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
                        coco_types::ReasoningEffort::Off
                    ) {
                        self.styles.dim()
                    } else {
                        self.styles.accent()
                    },
                ),
            );
        }

        // Sandbox shield (TS `SandboxPromptFooterHint`). Surfaces when the
        // engine has wrapped tool execution in a platform sandbox
        // (bwrap/Seatbelt) so the user knows shell writes are confined.
        if self.state.session.sandbox_active {
            parts.push(Span::raw(" 🛡").fg(self.styles.success()));
        }

        // Permission mode
        parts.push(Span::raw(" | ").fg(self.styles.border()));
        parts.push(
            Span::raw(format!("{:?}", self.state.session.permission_mode)).fg(self.styles.dim()),
        );

        // Token usage
        let tokens = &self.state.session.token_usage;
        let total = tokens.input_tokens + tokens.output_tokens;
        if total > 0 {
            parts.push(Span::raw(" | ").fg(self.styles.border()));
            if total >= 1000 {
                parts.push(
                    Span::raw(
                        t!(
                            "status.tokens_k",
                            value = format!("{:.1}", total as f64 / 1000.0)
                        )
                        .to_string(),
                    )
                    .fg(self.styles.dim()),
                );
            } else {
                parts.push(
                    Span::raw(t!("status.tokens_total", count = total).to_string())
                        .fg(self.styles.dim()),
                );
            }
        }

        // Cache read tokens
        if tokens.cache_read_tokens > 0 {
            parts.push(
                Span::raw(format!(" ({}cr)", tokens.cache_read_tokens)).fg(self.styles.dim()),
            );
        }

        // MCP servers
        let mcp_count = self.state.session.connected_mcp_count();
        if mcp_count > 0 {
            parts.push(Span::raw(" | ").fg(self.styles.border()));
            parts.push(
                Span::raw(t!("status.mcp", count = mcp_count).to_string()).fg(self.styles.dim()),
            );
        }

        // LSP indicator. Sticky-on for the session when the
        // `LspManagerAdapter` reported `is_connected = true` at startup
        // (Feature::Lsp on + prewarm spawned ≥ 1 server). Mirrors the
        // MCP badge pattern.
        if self.state.session.lsp_active {
            parts.push(Span::raw(" | ").fg(self.styles.border()));
            parts.push(Span::raw("LSP").fg(self.styles.accent()));
        }

        // Cost
        if self.state.session.estimated_cost_cents > 0 {
            parts.push(Span::raw(" | ").fg(self.styles.border()));
            let cost = self.state.session.estimated_cost_cents as f64 / 100.0;
            parts.push(Span::raw(format!("${cost:.2}")).fg(self.styles.dim()));
        }

        // Message count — count merged view so engine-pushed cells
        // (the bulk of the live transcript after Commit 2) participate.
        parts.push(Span::raw(" | ").fg(self.styles.border()));
        parts.push(
            Span::raw(t!("status.msgs", count = self.state.session.transcript.len()).to_string())
                .fg(self.styles.dim()),
        );

        let line = Line::from(parts);
        let bar = Paragraph::new(line).style(Style::default().bg(self.styles.border()));
        bar.render(area, buf);
    }
}
