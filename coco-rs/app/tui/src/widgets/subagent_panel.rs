//! Subagent panel widget — displays spawned agent instances.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::text::Span;
use ratatui::widgets::Block;
use ratatui::widgets::Borders;
use ratatui::widgets::Paragraph;
use ratatui::widgets::Widget;

use crate::i18n::t;
use crate::presentation::styles::UiStyles;
use crate::state::session::SubagentInstance;
use crate::state::session::SubagentStatus;
use crate::state::transcript_view::RenderedCell;

/// Number of recent message lines per teammate when preview is on.
/// TS uses 3 (`getMessagePreview` in `TeammateSpinnerLine.tsx`); coco-rs
/// matches.
const PREVIEW_LINES_PER_TEAMMATE: usize = 3;

/// Side panel showing subagent status.
pub struct SubagentPanel<'a> {
    subagents: &'a [SubagentInstance],
    focused_index: Option<i32>,
    styles: UiStyles<'a>,
    /// When set + non-empty, the panel renders up to
    /// [`PREVIEW_LINES_PER_TEAMMATE`] recent message lines per agent.
    /// TS `showTeammateMessagePreview` (`TeammateSpinnerTree`).
    cells_for_preview: Option<&'a [RenderedCell]>,
}

impl<'a> SubagentPanel<'a> {
    pub fn new(subagents: &'a [SubagentInstance], styles: UiStyles<'a>) -> Self {
        Self {
            subagents,
            focused_index: None,
            styles,
            cells_for_preview: None,
        }
    }

    pub fn focused_index(mut self, index: Option<i32>) -> Self {
        self.focused_index = index;
        self
    }

    /// Enable per-teammate message preview lines (TS
    /// `showTeammateMessagePreview`). Pass the engine-authoritative
    /// transcript cells — the panel filters per teammate by reading
    /// `SystemMessage::Informational` rows with a
    /// `teammate:<agent_id>` title.
    pub fn message_preview(mut self, cells: &'a [RenderedCell]) -> Self {
        self.cells_for_preview = Some(cells);
        self
    }
}

/// Last `n` lines from `teammate_id`'s recent messages in this
/// session. Walks newest-first so the most recent activity wins, then
/// reverses so the rendered lines read top-to-bottom in chronological
/// order. Mirrors TS `getMessagePreview` (`TeammateSpinnerLine.tsx`).
///
/// Teammate messages arrive as `SystemMessage::Informational` cells
/// whose title is `teammate:<agent_id>` (the convention set by
/// `server_notification_handler::protocol::push_teammate_message`).
fn last_preview_lines(cells: &[RenderedCell], teammate_id: &str, n: usize) -> Vec<String> {
    let prefix = format!("teammate:{teammate_id}");
    let mut lines: Vec<String> = Vec::new();
    for cell in cells.iter().rev() {
        let coco_messages::Message::System(coco_messages::SystemMessage::Informational(info)) =
            cell.source.as_ref()
        else {
            continue;
        };
        if info.title != prefix {
            continue;
        }
        for line in info.message.lines().rev() {
            if lines.len() >= n {
                break;
            }
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            lines.push(trimmed.to_string());
        }
        if lines.len() >= n {
            break;
        }
    }
    lines.reverse();
    lines
}

/// Compact elapsed-time chip: `5s` / `1m 23s` / `1h 04m`. Optimised
/// for the narrow side-panel column; longer formats round to the
/// next-lower unit. Mirrors the spirit of TS `formatDistanceToNow`
/// without pulling in a time-formatting crate.
fn format_short_elapsed(ms: i64) -> String {
    let secs = (ms / 1000).max(0);
    if secs < 60 {
        format!("{secs}s")
    } else if secs < 3_600 {
        format!("{}m {:02}s", secs / 60, secs % 60)
    } else {
        format!("{}h {:02}m", secs / 3_600, (secs % 3_600) / 60)
    }
}

/// Compact token-count chip: `<1000` shown as-is, otherwise `1.2k`.
/// TS uses the same threshold in `CoordinatorAgentStatus.tsx`.
fn format_short_tokens(total: i64) -> String {
    if total < 1_000 {
        format!("{total}")
    } else {
        format!("{:.1}k", total as f64 / 1_000.0)
    }
}

impl Widget for SubagentPanel<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let mut lines: Vec<Line> = Vec::new();

        for (i, agent) in self.subagents.iter().enumerate() {
            let is_focused = self.focused_index == Some(i as i32);

            let (icon, color) = match agent.status {
                SubagentStatus::Running => ("●", self.styles.tool_running()),
                SubagentStatus::Completed => ("✓", self.styles.tool_completed()),
                SubagentStatus::Backgrounded => ("◐", self.styles.dim()),
                SubagentStatus::Failed => ("✗", self.styles.tool_error()),
            };

            // TS parity: BLACK_CIRCLE for the currently-viewed agent,
            // figures.circle (○) for siblings. The viewed marker is
            // distinct from the status icon — focus answers "which
            // agent am I looking at right now", status answers "what
            // is each agent doing".
            let focus_marker = if is_focused { "▸ " } else { "  " };

            let mut spans = vec![
                Span::raw(focus_marker.to_string()),
                Span::raw(format!("{icon} ")).fg(color),
                Span::raw(agent.description.clone()).fg(self.styles.text()),
                Span::raw(format!(" ({})", agent.agent_type)).fg(self.styles.dim()),
            ];

            // Elapsed time chip — TS uses `formatDistanceToNow` here;
            // coco-rs picks a compact `Xs` / `Xm Ys` shape to stay
            // inside the compact activity width budget.
            if let Some(started_ms) = agent.started_at_ms {
                let now_ms = crate::state::session::now_ms();
                let elapsed_ms = (now_ms - started_ms).max(0);
                spans.push(
                    Span::raw(format!(" {}", format_short_elapsed(elapsed_ms)))
                        .fg(self.styles.dim()),
                );
            }

            // Token chip — show `↕Nk` when total > 0. TS shows
            // `↑input ↓output` separately; coco-rs collapses to the
            // total because the compact activity surface is narrow and the
            // direction is less actionable than the magnitude.
            if let Some(tokens) = agent.token_usage.as_ref() {
                let total = tokens.input_tokens + tokens.output_tokens;
                if total > 0 {
                    spans.push(
                        Span::raw(format!(" ↕{}", format_short_tokens(total)))
                            .fg(self.styles.dim()),
                    );
                }
            }

            lines.push(Line::from(spans));

            // TS-parity: when `showTeammateMessagePreview` is on,
            // each spinner line is followed by up to N indented
            // recent-activity lines from this teammate's messages.
            if let Some(cells) = self.cells_for_preview {
                for preview in
                    last_preview_lines(cells, &agent.agent_id, PREVIEW_LINES_PER_TEAMMATE)
                {
                    lines.push(Line::from(vec![
                        Span::raw("    "),
                        Span::raw(preview).fg(self.styles.dim()),
                    ]));
                }
            }
        }

        if lines.is_empty() {
            lines.push(Line::from(
                Span::raw(format!("  {}", t!("subagent.no_agents"))).fg(self.styles.dim()),
            ));
        }

        let panel = Paragraph::new(lines).block(
            Block::default()
                .borders(Borders::LEFT | Borders::TOP)
                .title(format!(" {} ", t!("subagent.title")))
                .border_style(Style::default().fg(self.styles.border())),
        );
        panel.render(area, buf);
    }
}
