//! Header bar widget — session context display.

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

/// Header bar showing session context.
pub struct HeaderBar<'a> {
    state: &'a AppState,
    theme: &'a Theme,
}

impl<'a> HeaderBar<'a> {
    pub fn new(state: &'a AppState, theme: &'a Theme) -> Self {
        Self { state, theme }
    }
}

impl Widget for HeaderBar<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let mut parts = Vec::new();

        // Working directory (short name)
        if let Some(ref dir) = self.state.session.working_dir {
            let short = dir.rsplit('/').next().unwrap_or(dir);
            parts.push(Span::raw(format!(" {short}")).fg(self.theme.primary));
        }

        // Model
        if !self.state.session.model.is_empty() {
            parts.push(Span::raw(" | ").fg(self.theme.border));
            parts.push(Span::raw(self.state.session.model.as_str()).fg(self.theme.text_dim));
        }

        // Plan mode
        if self.state.is_plan_mode() {
            parts.push(Span::raw(" | ").fg(self.theme.border));
            parts.push(
                Span::raw(t!("status.plan").to_string())
                    .fg(self.theme.plan_mode)
                    .bold(),
            );
        }

        // Compacting indicator. Prefer the phase-aware label so the user
        // can see which sub-phase is active (matches TS REPL.tsx:2502
        // `spinnerMessage` switch). Falls back to the generic label when
        // a phase event hasn't arrived yet.
        if self.state.session.is_compacting {
            use crate::state::session::CompactionPhaseLabel;
            let phase_label = match self.state.session.compaction_phase {
                Some(CompactionPhaseLabel::PreCompactHooks) => Some("status.compacting_pre_hooks"),
                Some(CompactionPhaseLabel::PostCompactHooks) => {
                    Some("status.compacting_post_hooks")
                }
                Some(CompactionPhaseLabel::SessionStartHooks) => {
                    Some("status.compacting_session_start_hooks")
                }
                Some(CompactionPhaseLabel::Summarizing) | None => None,
            };
            let label = phase_label
                .map(|k| t!(k).to_string())
                .unwrap_or_else(|| t!("status.compacting").to_string());
            parts.push(Span::raw(" | ").fg(self.theme.border));
            parts.push(Span::raw(label).fg(self.theme.warning).italic());
        }

        // Turn count
        if self.state.session.turn_count > 0 {
            parts.push(Span::raw(" | ").fg(self.theme.border));
            parts.push(
                Span::raw(t!("status.turn_short", n = self.state.session.turn_count).to_string())
                    .fg(self.theme.text_dim),
            );
        }

        let line = Line::from(parts);
        let header = Paragraph::new(line).style(Style::default().bg(self.theme.border));
        header.render(area, buf);
    }
}
