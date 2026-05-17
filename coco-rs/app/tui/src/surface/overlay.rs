//! Overlay placement and rendering for native-scrollback surfaces.

use std::time::Duration;
use std::time::Instant;

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
use crate::surface::compatibility::TerminalCompatibility;
use crate::surface::terminal::SurfaceFrame;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OverlaySurfacePlacement {
    ComposerInline,
    InlineDecision,
    AltScreen,
}

const INLINE_DECISION_RECENT_INTERACTION: Duration = Duration::from_secs(2);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HistorySurfaceMode {
    NativeScrollback,
    Viewport,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SurfaceFramePlan {
    pub(crate) overlay_placement: Option<OverlaySurfacePlacement>,
    pub(crate) history_surface: HistorySurfaceMode,
    pub(crate) attention_requested: bool,
}

impl SurfaceFramePlan {
    pub(crate) fn for_compatibility(compatibility: TerminalCompatibility) -> Self {
        Self {
            overlay_placement: None,
            history_surface: history_surface_mode(compatibility),
            attention_requested: false,
        }
    }

    pub(crate) fn native_history_enabled(self) -> bool {
        self.history_surface == HistorySurfaceMode::NativeScrollback
            && !self.history_emission_deferred()
    }

    pub(crate) fn finalized_history_in_viewport(self) -> bool {
        self.history_surface == HistorySurfaceMode::Viewport
    }

    pub(crate) fn history_emission_deferred(self) -> bool {
        self.overlay_placement
            .is_some_and(|placement| !matches!(placement, OverlaySurfacePlacement::ComposerInline))
    }
}

#[derive(Debug, Default, Clone)]
pub(crate) struct OverlaySurfaceState {
    latched: Option<LatchedOverlaySurface>,
}

#[derive(Debug, Clone)]
struct LatchedOverlaySurface {
    generation: u64,
    placement: OverlaySurfacePlacement,
}

impl OverlaySurfaceState {
    pub(crate) fn plan(
        &mut self,
        state: &AppState,
        compatibility: TerminalCompatibility,
        now: Instant,
    ) -> SurfaceFramePlan {
        let Some(overlay) = state.ui.active_overlay() else {
            self.latched = None;
            return SurfaceFramePlan::for_compatibility(compatibility);
        };

        let generation = state.ui.overlay_generation();
        let previous = self
            .latched
            .as_ref()
            .filter(|latched| latched.generation == generation);
        if let Some(latched) = previous {
            return SurfaceFramePlan {
                overlay_placement: Some(latched.placement),
                history_surface: history_surface_mode(compatibility),
                attention_requested: false,
            };
        }

        let Some(static_placement) = overlay_surface_placement(Some(overlay)) else {
            self.latched = None;
            return SurfaceFramePlan::for_compatibility(compatibility);
        };
        let attention_requested = static_placement == OverlaySurfacePlacement::InlineDecision
            && inline_decision_needs_attention(overlay)
            && !inline_decision_is_attention_safe(state, now);
        let placement = if attention_requested {
            OverlaySurfacePlacement::AltScreen
        } else {
            static_placement
        };
        self.latched = Some(LatchedOverlaySurface {
            generation,
            placement,
        });

        SurfaceFramePlan {
            overlay_placement: Some(placement),
            history_surface: history_surface_mode(compatibility),
            attention_requested,
        }
    }
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

#[cfg(test)]
pub(crate) fn overlay_surface_placement_for_state(
    state: &AppState,
    now: Instant,
) -> Option<OverlaySurfacePlacement> {
    let mut overlay_state = OverlaySurfaceState::default();
    overlay_state
        .plan(state, TerminalCompatibility::NativeScrollback, now)
        .overlay_placement
}

fn history_surface_mode(compatibility: TerminalCompatibility) -> HistorySurfaceMode {
    if compatibility.native_scrollback_enabled() {
        HistorySurfaceMode::NativeScrollback
    } else {
        HistorySurfaceMode::Viewport
    }
}

fn inline_decision_needs_attention(overlay: &Overlay) -> bool {
    matches!(
        overlay_surface_placement(Some(overlay)),
        Some(OverlaySurfacePlacement::InlineDecision)
    )
}

fn inline_decision_is_attention_safe(state: &AppState, now: Instant) -> bool {
    state.ui.terminal_focused
        && state
            .ui
            .surface_visibility_known_at
            .is_some_and(|known_at| {
                now.saturating_duration_since(known_at) <= INLINE_DECISION_RECENT_INTERACTION
            })
}

#[cfg(test)]
pub(crate) fn history_emission_deferred(overlay: Option<&Overlay>) -> bool {
    overlay_surface_placement(overlay)
        .is_some_and(|placement| !matches!(placement, OverlaySurfacePlacement::ComposerInline))
}

#[cfg(test)]
pub(crate) fn history_emission_deferred_for_state(state: &AppState, now: Instant) -> bool {
    overlay_surface_placement_for_state(state, now)
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
        crate::overlay_content::overlay_content(overlay, state, styles);
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
