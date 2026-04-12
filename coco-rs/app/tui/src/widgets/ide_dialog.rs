//! IDE integration dialogs — connection status and onboarding.
//!
//! TS: src/components/IdeOnboardingDialog.tsx, IdeAutoConnectDialog.tsx

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::text::Span;
use ratatui::widgets::Block;
use ratatui::widgets::Borders;
use ratatui::widgets::Paragraph;
use ratatui::widgets::Widget;
use ratatui::widgets::Wrap;

use crate::theme::Theme;

/// IDE connection state.
#[derive(Debug, Clone)]
pub struct IdeConnectionState {
    pub ide_name: String,
    pub ide_type: IdeType,
    pub status: IdeConnectionStatus,
    pub port: Option<i32>,
    pub tools_available: Vec<String>,
}

/// Supported IDE type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IdeType {
    VsCode,
    Cursor,
    Windsurf,
    JetBrains,
    Other,
}

/// IDE connection lifecycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IdeConnectionStatus {
    Disconnected,
    Connecting,
    Connected,
    Error,
}

/// IDE connection dialog widget.
pub struct IdeDialogWidget<'a> {
    state: &'a IdeConnectionState,
    theme: &'a Theme,
}

impl<'a> IdeDialogWidget<'a> {
    pub fn new(state: &'a IdeConnectionState, theme: &'a Theme) -> Self {
        Self { state, theme }
    }
}

impl Widget for IdeDialogWidget<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let mut lines: Vec<Line> = Vec::new();

        // Connection status
        let (icon, color, label) = match self.state.status {
            IdeConnectionStatus::Disconnected => ("○", self.theme.text_dim, "Disconnected"),
            IdeConnectionStatus::Connecting => ("◌", self.theme.warning, "Connecting..."),
            IdeConnectionStatus::Connected => ("●", self.theme.success, "Connected"),
            IdeConnectionStatus::Error => ("✗", self.theme.error, "Error"),
        };

        lines.push(Line::from(vec![
            Span::raw(format!("  {icon} ")).fg(color),
            Span::raw(&self.state.ide_name)
                .fg(self.theme.primary)
                .bold(),
            Span::raw(format!(" — {label}")).fg(color),
        ]));

        if let Some(port) = self.state.port {
            lines.push(Line::from(
                Span::raw(format!("  Port: {port}")).fg(self.theme.text_dim),
            ));
        }

        lines.push(Line::default());

        // Available tools
        if !self.state.tools_available.is_empty() {
            lines.push(Line::from(
                Span::raw("  Available IDE tools:").fg(self.theme.text_dim),
            ));
            for tool in &self.state.tools_available {
                lines.push(Line::from(
                    Span::raw(format!("    • {tool}")).fg(self.theme.text),
                ));
            }
        }

        // IDE type info
        let type_desc = match self.state.ide_type {
            IdeType::VsCode => "VS Code extension detected",
            IdeType::Cursor => "Cursor editor detected",
            IdeType::Windsurf => "Windsurf editor detected",
            IdeType::JetBrains => "JetBrains plugin detected",
            IdeType::Other => "IDE extension detected",
        };
        lines.push(Line::default());
        lines.push(Line::from(
            Span::raw(format!("  {type_desc}"))
                .fg(self.theme.text_dim)
                .italic(),
        ));

        lines.push(Line::default());
        lines.push(Line::from(
            Span::raw("  [Esc] Close").fg(self.theme.text_dim),
        ));

        let block = Block::default()
            .borders(Borders::ALL)
            .title(" IDE Connection ")
            .border_style(ratatui::style::Style::default().fg(self.theme.border_focused));

        let paragraph = Paragraph::new(lines)
            .block(block)
            .wrap(Wrap { trim: false });
        paragraph.render(area, buf);
    }
}
