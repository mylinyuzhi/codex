//! Picker-mechanics tests focused on the parts that aren't covered
//! by the broader `update` integration tests.
//!
//! Effort cycling is the most fragile bit: the focused entry's
//! `supported_efforts` list drives the wraparound, and missing
//! entries (e.g. a model without thinking capability) must no-op
//! rather than panic.

use super::*;
use crate::state::AppState;
use crate::state::ModelEntry;
use crate::state::ModelPickerOverlay;
use crate::state::Overlay;
use coco_types::ModelRole;
use coco_types::ReasoningEffort;

fn picker(entries: Vec<ModelEntry>, selected: i32, effort: Option<ReasoningEffort>) -> AppState {
    let mut s = AppState::new();
    s.ui.set_overlay(Overlay::ModelPicker(ModelPickerOverlay {
        role: ModelRole::Main,
        entries,
        filter: String::new(),
        selected,
        effort,
    }));
    s
}

fn entry(
    model_id: &str,
    efforts: &[ReasoningEffort],
    default: Option<ReasoningEffort>,
) -> ModelEntry {
    ModelEntry {
        provider: "test".into(),
        provider_display: "Test".into(),
        model_id: model_id.into(),
        display_name: model_id.into(),
        context_window: Some(200_000),
        supported_efforts: efforts.to_vec(),
        default_effort: default,
        is_current_for_role: false,
    }
}

#[test]
fn cycle_effort_advances_through_supported_levels() {
    let levels = [
        ReasoningEffort::Low,
        ReasoningEffort::Medium,
        ReasoningEffort::High,
    ];
    let mut s = picker(
        vec![entry("m", &levels, Some(ReasoningEffort::Low))],
        0,
        Some(ReasoningEffort::Low),
    );
    cycle_model_effort(&mut s, 1);
    let Some(Overlay::ModelPicker(m)) = &s.ui.overlay else {
        panic!()
    };
    assert_eq!(m.effort, Some(ReasoningEffort::Medium));
    cycle_model_effort(&mut s, 1);
    let Some(Overlay::ModelPicker(m)) = &s.ui.overlay else {
        panic!()
    };
    assert_eq!(m.effort, Some(ReasoningEffort::High));
}

#[test]
fn cycle_effort_wraps_around_at_endpoints() {
    let levels = [
        ReasoningEffort::Low,
        ReasoningEffort::Medium,
        ReasoningEffort::High,
    ];
    let mut s = picker(
        vec![entry("m", &levels, Some(ReasoningEffort::High))],
        0,
        Some(ReasoningEffort::High),
    );
    // Wrap forward from High → Low.
    cycle_model_effort(&mut s, 1);
    let Some(Overlay::ModelPicker(m)) = &s.ui.overlay else {
        panic!()
    };
    assert_eq!(m.effort, Some(ReasoningEffort::Low));
    // Wrap back from Low → High.
    cycle_model_effort(&mut s, -1);
    let Some(Overlay::ModelPicker(m)) = &s.ui.overlay else {
        panic!()
    };
    assert_eq!(m.effort, Some(ReasoningEffort::High));
}

#[test]
fn cycle_effort_noops_when_no_supported_levels() {
    let mut s = picker(vec![entry("m", &[], None)], 0, None);
    cycle_model_effort(&mut s, 1);
    let Some(Overlay::ModelPicker(m)) = &s.ui.overlay else {
        panic!()
    };
    assert!(m.effort.is_none());
}

#[test]
fn cycle_effort_noops_outside_picker() {
    let mut s = AppState::new();
    // No overlay → cycle_effort should silently no-op (no panic).
    cycle_model_effort(&mut s, 1);
    assert!(s.ui.overlay.is_none());
}

#[test]
fn filtered_models_matches_provider_display() {
    let entries = vec![
        ModelEntry {
            provider: "anthropic".into(),
            provider_display: "Anthropic".into(),
            model_id: "claude-haiku-4-5".into(),
            display_name: "Claude Haiku".into(),
            context_window: Some(200_000),
            supported_efforts: vec![],
            default_effort: None,
            is_current_for_role: false,
        },
        ModelEntry {
            provider: "openai".into(),
            provider_display: "OpenAI".into(),
            model_id: "gpt-5-4".into(),
            display_name: "GPT-5.4".into(),
            context_window: Some(272_000),
            supported_efforts: vec![],
            default_effort: None,
            is_current_for_role: false,
        },
    ];
    let m = ModelPickerOverlay {
        role: ModelRole::Main,
        entries,
        filter: "open".to_string(),
        selected: 0,
        effort: None,
    };
    let filtered = filtered_models(&m);
    assert_eq!(filtered.len(), 1);
    assert_eq!(filtered[0].provider, "openai");
}
