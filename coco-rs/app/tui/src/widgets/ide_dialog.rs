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

use crate::i18n::t;
use crate::presentation::styles::UiStyles;

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
    styles: UiStyles<'a>,
}

impl<'a> IdeDialogWidget<'a> {
    pub fn new(state: &'a IdeConnectionState, styles: UiStyles<'a>) -> Self {
        Self { state, styles }
    }
}

impl Widget for IdeDialogWidget<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let mut lines: Vec<Line> = Vec::new();

        // Connection status
        let (icon, color, label) = match self.state.status {
            IdeConnectionStatus::Disconnected => ("○", self.styles.dim(), t!("ide.disconnected")),
            IdeConnectionStatus::Connecting => ("◌", self.styles.warning(), t!("ide.connecting")),
            IdeConnectionStatus::Connected => ("●", self.styles.success(), t!("ide.connected")),
            IdeConnectionStatus::Error => ("✗", self.styles.error(), t!("ide.error")),
        };

        lines.push(Line::from(vec![
            Span::raw(format!("  {icon} ")).fg(color),
            Span::raw(&self.state.ide_name)
                .fg(self.styles.primary())
                .bold(),
            Span::raw(format!(" — {label}")).fg(color),
        ]));

        if let Some(port) = self.state.port {
            lines.push(Line::from(
                Span::raw(format!("  {}", t!("ide.port_prefix", port = port)))
                    .fg(self.styles.dim()),
            ));
        }

        lines.push(Line::default());

        // Available tools
        if !self.state.tools_available.is_empty() {
            lines.push(Line::from(
                Span::raw(format!("  {}", t!("ide.available_tools"))).fg(self.styles.dim()),
            ));
            for tool in &self.state.tools_available {
                lines.push(Line::from(
                    Span::raw(format!("    • {tool}")).fg(self.styles.text()),
                ));
            }
        }

        // IDE type info
        let type_desc = match self.state.ide_type {
            IdeType::VsCode => t!("ide.vscode_detected"),
            IdeType::Cursor => t!("ide.cursor_detected"),
            IdeType::Windsurf => t!("ide.windsurf_detected"),
            IdeType::JetBrains => t!("ide.jetbrains_detected"),
            IdeType::Other => t!("ide.generic_detected"),
        };
        lines.push(Line::default());
        lines.push(Line::from(
            Span::raw(format!("  {type_desc}"))
                .fg(self.styles.dim())
                .italic(),
        ));

        lines.push(Line::default());
        lines.push(Line::from(
            Span::raw(format!("  {}", t!("ide.hint_close"))).fg(self.styles.dim()),
        ));

        let block = Block::default()
            .borders(Borders::ALL)
            .title(t!("ide.dialog_title").to_string())
            .border_style(ratatui::style::Style::default().fg(self.styles.focused_border()));

        let paragraph = Paragraph::new(lines)
            .block(block)
            .wrap(Wrap { trim: false });
        paragraph.render(area, buf);
    }
}
