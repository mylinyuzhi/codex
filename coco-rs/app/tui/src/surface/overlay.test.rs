use super::*;
use crate::state::overlay::CommandPaletteOverlay;
use crate::state::overlay::PermissionDetail;
use crate::state::overlay::PermissionOverlay;
use crate::state::overlay::TranscriptOverlay;

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
    let overlay = Overlay::Permission(PermissionOverlay {
        request_id: "p1".to_string(),
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
    });

    assert_eq!(
        overlay_surface_placement(Some(&overlay)),
        Some(OverlaySurfacePlacement::InlineDecision)
    );
    assert!(history_emission_deferred(Some(&overlay)));
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
