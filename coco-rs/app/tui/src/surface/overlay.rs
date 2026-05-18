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
use crate::presentation::layout::text_width;
use crate::presentation::styles::UiStyles;
use crate::state::AppState;
use crate::state::Overlay;
use crate::surface::compatibility::TerminalCompatibility;
use crate::surface::terminal::SurfaceFrame;
use crate::widgets::TranscriptLayoutIndex;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OverlaySurfacePlacement {
    ComposerInline,
    InlineDecision,
    AltScreen,
}

const INLINE_DECISION_RECENT_INTERACTION: Duration = Duration::from_secs(2);
const DECISION_OVERLAY_MIN_WIDTH: u16 = 54;
const DECISION_OVERLAY_MAX_WIDTH: u16 = 120;
const DECISION_OVERLAY_MIN_HEIGHT: u16 = 12;
const DECISION_OVERLAY_MAX_HEIGHT: u16 = 28;
const DEFAULT_OVERLAY_MIN_WIDTH: u16 = 40;
const DEFAULT_OVERLAY_MAX_WIDTH: u16 = 100;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct OverlayBoxPolicy {
    width_percent: u16,
    min_width: u16,
    max_width: u16,
    min_height: u16,
    max_height: u16,
}

const DECISION_OVERLAY_POLICY: OverlayBoxPolicy = OverlayBoxPolicy {
    width_percent: 84,
    min_width: DECISION_OVERLAY_MIN_WIDTH,
    max_width: DECISION_OVERLAY_MAX_WIDTH,
    min_height: DECISION_OVERLAY_MIN_HEIGHT,
    max_height: DECISION_OVERLAY_MAX_HEIGHT,
};

const DEFAULT_OVERLAY_POLICY: OverlayBoxPolicy = OverlayBoxPolicy {
    width_percent: 70,
    min_width: DEFAULT_OVERLAY_MIN_WIDTH,
    max_width: DEFAULT_OVERLAY_MAX_WIDTH,
    min_height: 1,
    max_height: u16::MAX,
};

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
    #[cfg(test)]
    pub(crate) fn plan(
        &mut self,
        state: &AppState,
        compatibility: TerminalCompatibility,
        now: Instant,
    ) -> SurfaceFramePlan {
        self.plan_inner(state, compatibility, now, None)
    }

    pub(crate) fn plan_for_native_viewport(
        &mut self,
        state: &AppState,
        compatibility: TerminalCompatibility,
        now: Instant,
        width: u16,
        max_height: u16,
    ) -> SurfaceFramePlan {
        self.plan_inner(state, compatibility, now, Some((width, max_height)))
    }

    fn plan_inner(
        &mut self,
        state: &AppState,
        compatibility: TerminalCompatibility,
        now: Instant,
        native_viewport_bounds: Option<(u16, u16)>,
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
        let needs_alt_screen_for_size = static_placement == OverlaySurfacePlacement::InlineDecision
            && native_viewport_bounds.is_some_and(|(width, max_height)| {
                inline_decision_needs_alt_screen_for_size(overlay, state, width, max_height)
            });
        let attention_requested = static_placement == OverlaySurfacePlacement::InlineDecision
            && inline_decision_needs_attention(overlay)
            && !inline_decision_is_attention_safe(state, now);
        let placement = if attention_requested || needs_alt_screen_for_size {
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

fn inline_decision_needs_alt_screen_for_size(
    overlay: &Overlay,
    state: &AppState,
    width: u16,
    max_height: u16,
) -> bool {
    if width == 0 || max_height == 0 {
        return false;
    }
    let styles = UiStyles::new(&state.ui.theme);
    let overlay_height = required_overlay_height(overlay, state, styles, width, max_height);
    let protected_bottom_rows =
        crate::surface::viewport::inline_decision_protected_bottom_rows(state, width, max_height);
    overlay_height.saturating_add(protected_bottom_rows) > max_height
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
    input_area: Option<Rect>,
    overlay: &Overlay,
    state: &AppState,
    transcript_layout: &mut TranscriptLayoutIndex,
    styles: UiStyles<'_>,
) {
    if matches!(
        overlay_surface_placement(Some(overlay)),
        Some(OverlaySurfacePlacement::ComposerInline)
    ) {
        return;
    }

    if let Overlay::Transcript(transcript) = overlay {
        frame.render_widget(Clear, area);
        frame.render_widget(
            crate::widgets::TranscriptOverlayWidget::new(
                state,
                transcript,
                transcript_layout,
                styles,
            ),
            area,
        );
        return;
    }

    let Some(text_overlay) = crate::overlay_content::text_overlay(overlay) else {
        return;
    };
    let (title, body, border_color) =
        crate::overlay_content::overlay_content(text_overlay, state, styles);
    let policy = overlay_box_policy(overlay);
    let placement_area = overlay_placement_area(area, input_area, overlay);
    if placement_area.height == 0 {
        return;
    }
    let (width, height) = overlay_box_size(placement_area, &body, policy);
    let overlay_area = layout::centered_fixed_area(placement_area, width, height);

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

fn overlay_placement_area(area: Rect, input_area: Option<Rect>, overlay: &Overlay) -> Rect {
    if !matches!(
        overlay_surface_placement(Some(overlay)),
        Some(OverlaySurfacePlacement::InlineDecision)
    ) {
        return area;
    }
    let Some(input_area) = input_area else {
        return area;
    };
    let top = area.y;
    let bottom = input_area.y.clamp(area.y, area.bottom());
    let height = bottom.saturating_sub(top);
    Rect::new(area.x, top, area.width, height)
}

pub(crate) fn required_overlay_height(
    overlay: &Overlay,
    state: &AppState,
    styles: UiStyles<'_>,
    width: u16,
    max_height: u16,
) -> u16 {
    if width == 0 || max_height == 0 {
        return 0;
    }
    if matches!(overlay, Overlay::Transcript(_)) {
        return max_height;
    }
    let Some(text_overlay) = crate::overlay_content::text_overlay(overlay) else {
        return 0;
    };
    let (_, body, _) = crate::overlay_content::overlay_content(text_overlay, state, styles);
    let area = Rect::new(0, 0, width, max_height);
    overlay_box_size(area, &body, overlay_box_policy(overlay)).1
}

fn overlay_box_policy(overlay: &Overlay) -> OverlayBoxPolicy {
    match overlay_surface_placement(Some(overlay)) {
        Some(OverlaySurfacePlacement::InlineDecision) => DECISION_OVERLAY_POLICY,
        Some(OverlaySurfacePlacement::AltScreen | OverlaySurfacePlacement::ComposerInline)
        | None => DEFAULT_OVERLAY_POLICY,
    }
}

fn overlay_box_size(area: Rect, body: &str, policy: OverlayBoxPolicy) -> (u16, u16) {
    let available_width = area.width.saturating_sub(2).max(1);
    let width = ((area.width as u32 * u32::from(policy.width_percent) / 100) as u16)
        .clamp(policy.min_width.min(available_width), policy.max_width)
        .min(available_width);
    let inner_width = width.saturating_sub(2).max(1) as usize;
    let wrapped_body_rows = body
        .lines()
        .map(|line| {
            let line_width = text_width(line);
            line_width.saturating_add(inner_width - 1) / inner_width
        })
        .map(|rows| rows.max(1))
        .sum::<usize>();
    let content_height = wrapped_body_rows.saturating_add(4).min(u16::MAX as usize) as u16;
    let available_height = area.height.saturating_sub(2).max(1);
    let height = content_height
        .clamp(
            policy.min_height.min(available_height),
            policy.max_height.min(available_height),
        )
        .min(available_height);
    (width, height)
}

#[cfg(test)]
#[path = "overlay.test.rs"]
mod tests;
