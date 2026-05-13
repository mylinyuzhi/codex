use super::*;
use crate::state::AppState;
use crate::state::Overlay;
use coco_types::ModelRole;

/// `cycle_model` opens the picker for the Main role, seeded with at
/// least the builtin Anthropic / OpenAI / Google / DeepSeek entries
/// (provider-grouped because the seeder sorts on provider_display).
#[test]
fn cycle_model_seeds_builtin_registry() {
    let mut state = AppState::new();
    state.session.model = "claude-sonnet-4-6".to_string();
    state.session.provider = "anthropic".to_string();
    cycle_model(&mut state);
    let m = match &state.ui.overlay {
        Some(Overlay::ModelPicker(m)) => m.clone(),
        other => panic!("expected ModelPicker, got {other:?}"),
    };
    assert_eq!(m.role, ModelRole::Main);
    assert!(!m.entries.is_empty(), "picker should have builtin entries");
    let providers: Vec<&str> = m.entries.iter().map(|e| e.provider.as_str()).collect();
    assert!(providers.contains(&"anthropic"));
    assert!(providers.contains(&"openai"));
    // The current Main model is marked.
    let current = m.entries.iter().find(|e| e.is_current_for_role).unwrap();
    assert_eq!(current.model_id, "claude-sonnet-4-6");
}

/// Role cycle wraps via Tab/Shift+Tab over the canonical 9-role order
/// and rebuilds the entries each time (clearing the filter).
#[test]
fn cycle_model_role_wraps_and_resets_filter() {
    let mut state = AppState::new();
    state.session.model = "claude-sonnet-4-6".to_string();
    cycle_model(&mut state);
    if let Some(Overlay::ModelPicker(m)) = &mut state.ui.overlay {
        m.filter = "ignored".to_string();
    }
    cycle_model_role(&mut state, 1);
    let m = match &state.ui.overlay {
        Some(Overlay::ModelPicker(m)) => m,
        _ => unreachable!(),
    };
    assert_eq!(m.role, ModelRole::Fast);
    assert!(m.filter.is_empty(), "Tab should reset filter");

    // Wrap-around: from Main, Shift+Tab (delta = -1) goes to Subagent.
    cycle_model_role(&mut state, -1); // Fast → Main
    cycle_model_role(&mut state, -1); // Main → Subagent (wrap)
    let m = match &state.ui.overlay {
        Some(Overlay::ModelPicker(m)) => m,
        _ => unreachable!(),
    };
    assert_eq!(m.role, ModelRole::Subagent);
}

/// `next_role` cycles deterministically over the canonical order and
/// stays consistent with the renderer (see render_overlays/pickers.rs).
#[test]
fn next_role_cycles_canonical_order() {
    assert_eq!(next_role(ModelRole::Main, 1), ModelRole::Fast);
    assert_eq!(next_role(ModelRole::Subagent, 1), ModelRole::Main);
    assert_eq!(next_role(ModelRole::Main, -1), ModelRole::Subagent);
    assert_eq!(next_role(ModelRole::Plan, 2), ModelRole::Review);
}

/// Provider inference covers every builtin family. The `o`-prefix case
/// is broad on purpose — OpenAI's `o1`/`o3` reasoning models would
/// land here if they're added to the registry.
#[test]
fn infer_provider_covers_builtin_families() {
    assert_eq!(
        infer_provider("claude-sonnet-4-6"),
        ("anthropic", "Anthropic")
    );
    assert_eq!(infer_provider("gpt-5-4"), ("openai", "OpenAI"));
    assert_eq!(infer_provider("o1-preview"), ("openai", "OpenAI"));
    assert_eq!(infer_provider("gemini-2.5-pro"), ("google", "Google"));
    assert_eq!(infer_provider("deepseek-v4-pro"), ("deepseek", "DeepSeek"));
    assert_eq!(infer_provider("custom-model"), ("other", "Other"));
}
