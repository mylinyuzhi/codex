use super::*;
use crate::state::AppState;
use crate::state::overlay::CommandPaletteOverlay;
use crate::state::overlay::CostWarningOverlay;
use crate::state::overlay::PermissionDetail;
use crate::state::overlay::PermissionOverlay;
use crate::state::overlay::TranscriptOverlay;
use crate::surface::compatibility::TerminalCompatibility;

fn permission_overlay() -> Overlay {
    permission_overlay_with_id("p1")
}

fn permission_overlay_with_id(request_id: &str) -> Overlay {
    Overlay::Permission(PermissionOverlay {
        request_id: request_id.to_string(),
        tool_name: "Bash".to_string(),
        description: "Run command".to_string(),
        detail: PermissionDetail::Generic {
            input_preview: "echo hi".to_string(),
        },
        risk_level: None,
        show_always_allow: false,
        classifier_checking: false,
        classifier_auto_approved: None,
        choices: None,
        selected_choice: 0,
        original_input: None,
    })
}

#[test]
fn command_palette_keeps_composer_inline_placement() {
    let overlay = Overlay::CommandPalette(CommandPaletteOverlay {
        commands: Vec::new(),
        filter: String::new(),
        selected: 0,
    });

    assert_eq!(
        overlay_surface_placement(Some(&overlay)),
        Some(OverlaySurfacePlacement::ComposerInline)
    );
    assert!(!history_emission_deferred(Some(&overlay)));
}

#[test]
fn permission_uses_inline_decision_placement_and_defers_history() {
    let overlay = permission_overlay();

    assert_eq!(
        overlay_surface_placement(Some(&overlay)),
        Some(OverlaySurfacePlacement::InlineDecision)
    );
    assert!(history_emission_deferred(Some(&overlay)));
}

#[test]
fn focused_recent_permission_uses_inline_decision_for_state() {
    let now = std::time::Instant::now();
    let mut state = AppState::new();
    state.ui.terminal_focused = true;
    state
        .ui
        .record_surface_interaction(now - std::time::Duration::from_millis(500));
    state.ui.set_overlay(permission_overlay());

    assert_eq!(
        overlay_surface_placement_for_state(&state, now),
        Some(OverlaySurfacePlacement::InlineDecision)
    );
    assert!(history_emission_deferred_for_state(&state, now));
}

#[test]
fn stale_permission_upgrades_to_alt_screen_for_attention() {
    let now = std::time::Instant::now();
    let mut state = AppState::new();
    state.ui.terminal_focused = true;
    state
        .ui
        .record_surface_interaction(now - std::time::Duration::from_secs(3));
    state.ui.set_overlay(permission_overlay());

    assert_eq!(
        overlay_surface_placement_for_state(&state, now),
        Some(OverlaySurfacePlacement::AltScreen)
    );
}

#[test]
fn unfocused_permission_upgrades_to_alt_screen_for_attention() {
    let now = std::time::Instant::now();
    let mut state = AppState::new();
    state.ui.terminal_focused = false;
    state.ui.record_surface_interaction(now);
    state.ui.set_overlay(permission_overlay());

    assert_eq!(
        overlay_surface_placement_for_state(&state, now),
        Some(OverlaySurfacePlacement::AltScreen)
    );
}

#[test]
fn stale_cost_warning_upgrades_to_alt_screen_for_attention() {
    let now = std::time::Instant::now();
    let mut state = AppState::new();
    state.ui.terminal_focused = true;
    state
        .ui
        .record_surface_interaction(now - std::time::Duration::from_secs(3));
    state
        .ui
        .set_overlay(Overlay::CostWarning(CostWarningOverlay {
            current_cost_cents: 1200,
            threshold_cents: 1000,
        }));

    assert_eq!(
        overlay_surface_placement_for_state(&state, now),
        Some(OverlaySurfacePlacement::AltScreen)
    );
}

#[test]
fn overlay_surface_state_latches_recent_inline_decision_until_overlay_changes() {
    let now = std::time::Instant::now();
    let mut state = AppState::new();
    state.ui.terminal_focused = true;
    state.ui.record_surface_interaction(now);
    state.ui.set_overlay(permission_overlay());
    let mut surface = OverlaySurfaceState::default();

    let first = surface.plan(&state, TerminalCompatibility::NativeScrollback, now);
    let second = surface.plan(
        &state,
        TerminalCompatibility::NativeScrollback,
        now + std::time::Duration::from_secs(10),
    );

    assert_eq!(
        first.overlay_placement,
        Some(OverlaySurfacePlacement::InlineDecision)
    );
    assert_eq!(
        second.overlay_placement,
        Some(OverlaySurfacePlacement::InlineDecision)
    );
    assert!(!second.attention_requested);
}

#[test]
fn overlay_surface_state_notifies_once_for_attention_alt_screen_latch() {
    let now = std::time::Instant::now();
    let mut state = AppState::new();
    state.ui.terminal_focused = false;
    state.ui.record_surface_interaction(now);
    state.ui.set_overlay(permission_overlay());
    let mut surface = OverlaySurfaceState::default();

    let first = surface.plan(&state, TerminalCompatibility::NativeScrollback, now);
    let second = surface.plan(
        &state,
        TerminalCompatibility::NativeScrollback,
        now + std::time::Duration::from_millis(100),
    );

    assert_eq!(
        first.overlay_placement,
        Some(OverlaySurfacePlacement::AltScreen)
    );
    assert!(first.attention_requested);
    assert_eq!(
        second.overlay_placement,
        Some(OverlaySurfacePlacement::AltScreen)
    );
    assert!(!second.attention_requested);
}

#[test]
fn turn_completion_timestamp_does_not_make_inline_decision_attention_safe() {
    let now = std::time::Instant::now();
    let mut state = AppState::new();
    state.ui.terminal_focused = true;
    state.session.last_user_interaction_at = now;
    state.ui.set_overlay(permission_overlay());

    assert_eq!(
        overlay_surface_placement_for_state(&state, now),
        Some(OverlaySurfacePlacement::AltScreen)
    );
}

#[test]
fn queued_same_kind_overlay_gets_new_attention_latch() {
    let now = std::time::Instant::now();
    let mut state = AppState::new();
    state.ui.terminal_focused = true;
    state.ui.record_surface_interaction(now);
    state.ui.set_overlay(permission_overlay_with_id("p1"));
    state.ui.set_overlay(permission_overlay_with_id("p2"));
    let mut surface = OverlaySurfaceState::default();

    let first = surface.plan(&state, TerminalCompatibility::NativeScrollback, now);
    state.ui.dismiss_overlay();
    state
        .ui
        .record_surface_interaction(now - std::time::Duration::from_secs(10));
    let second = surface.plan(
        &state,
        TerminalCompatibility::NativeScrollback,
        now + std::time::Duration::from_secs(10),
    );

    assert_eq!(
        first.overlay_placement,
        Some(OverlaySurfacePlacement::InlineDecision)
    );
    assert_eq!(
        second.overlay_placement,
        Some(OverlaySurfacePlacement::AltScreen)
    );
    assert!(second.attention_requested);
}

#[test]
fn transcript_uses_alt_screen_placement_and_defers_history() {
    let overlay = Overlay::Transcript(TranscriptOverlay::new());

    assert_eq!(
        overlay_surface_placement(Some(&overlay)),
        Some(OverlaySurfacePlacement::AltScreen)
    );
    assert!(history_emission_deferred(Some(&overlay)));
}
