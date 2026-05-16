//! Overlay placement and rendering for native-scrollback surfaces.

use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::widgets::Block;
use ratatui::widgets::Borders;
use ratatui::widgets::Clear;
use ratatui::widgets::Paragraph;
use ratatui::widgets::Wrap;

use crate::presentation::layout;
use crate::presentation::styles::UiStyles;
use crate::state::AppState;
use crate::state::Overlay;
use crate::surface::terminal::SurfaceFrame;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OverlaySurfacePlacement {
    ComposerInline,
    InlineDecision,
    AltScreen,
}

pub(crate) fn overlay_surface_placement(
    overlay: Option<&Overlay>,
) -> Option<OverlaySurfacePlacement> {
    let overlay = overlay?;
    Some(match overlay {
        Overlay::CommandPalette(_) => OverlaySurfacePlacement::ComposerInline,
        Overlay::Permission(_)
        | Overlay::Question(_)
        | Overlay::Elicitation(_)
        | Overlay::SandboxPermission(_)
        | Overlay::CostWarning(_)
        | Overlay::McpServerApproval(_)
        | Overlay::PlanEntry(_)
        | Overlay::PlanExit(_)
        | Overlay::PlanApproval(_)
        | Overlay::Feedback(_)
        | Overlay::IdleReturn(_)
        | Overlay::Trust(_)
        | Overlay::AutoModeOptIn(_)
        | Overlay::BypassPermissions(_)
        | Overlay::WorktreeExit(_)
        | Overlay::Bridge(_)
        | Overlay::InvalidConfig(_)
        | Overlay::Error(_) => OverlaySurfacePlacement::InlineDecision,
        Overlay::Help
        | Overlay::ModelPicker(_)
        | Overlay::SessionBrowser(_)
        | Overlay::GlobalSearch(_)
        | Overlay::QuickOpen(_)
        | Overlay::Export(_)
        | Overlay::DiffView(_)
        | Overlay::Doctor(_)
        | Overlay::TaskDetail(_)
        | Overlay::McpServerSelect(_)
        | Overlay::ContextVisualization
        | Overlay::Rewind(_)
        | Overlay::Settings(_)
        | Overlay::MemoryDialog(_)
        | Overlay::Transcript(_) => OverlaySurfacePlacement::AltScreen,
    })
}

pub(crate) fn history_emission_deferred(overlay: Option<&Overlay>) -> bool {
    overlay_surface_placement(overlay)
        .is_some_and(|placement| !matches!(placement, OverlaySurfacePlacement::ComposerInline))
}

pub(crate) fn render_surface_overlay(
    frame: &mut SurfaceFrame<'_>,
    area: Rect,
    overlay: &Overlay,
    state: &AppState,
    styles: UiStyles<'_>,
) {
    if matches!(
        overlay_surface_placement(Some(overlay)),
        Some(OverlaySurfacePlacement::ComposerInline)
    ) {
        return;
    }

    let (title, body, border_color) =
        crate::render_overlays::overlay_content(overlay, state, styles);
    let width = ((area.width as u32 * 70 / 100) as u16).clamp(40, 100);
    let height = body
        .lines()
        .count()
        .saturating_add(4)
        .min(u16::MAX as usize) as u16;
    let overlay_area = layout::centered_fixed_area(area, width, height);

    frame.render_widget(Clear, overlay_area);
    frame.render_widget(
        Paragraph::new(body).wrap(Wrap { trim: false }).block(
            Block::default()
                .borders(Borders::ALL)
                .title(title)
                .border_style(Style::default().fg(border_color)),
        ),
        overlay_area,
    );
}

#[cfg(test)]
#[path = "overlay.test.rs"]
mod tests;
