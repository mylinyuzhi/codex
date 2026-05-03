use super::*;
use coco_types::{PermissionMode, ToolAppState};

fn default_state() -> ToolAppState {
    ToolAppState::default()
}

#[test]
fn test_should_suggest_default_state_returns_true() {
    assert!(should_suggest(&default_state(), false, false));
}

#[test]
fn test_should_suggest_env_disable_skips() {
    assert!(!should_suggest(&default_state(), false, true));
}

#[test]
fn test_should_suggest_non_interactive_skips() {
    assert!(!should_suggest(&default_state(), true, false));
}

#[test]
fn test_should_suggest_plan_mode_skips() {
    let mut state = default_state();
    state.permission_mode = Some(PermissionMode::Plan);
    assert!(!should_suggest(&state, false, false));
}

#[test]
fn test_should_suggest_awaiting_plan_approval_skips() {
    let mut state = default_state();
    state.awaiting_plan_approval = true;
    assert!(!should_suggest(&state, false, false));
}

#[test]
fn test_validate_suggestion_rejects_empty() {
    assert!(validate_suggestion("").is_none());
    assert!(validate_suggestion("   ").is_none());
}

#[test]
fn test_validate_suggestion_rejects_none_token() {
    assert!(validate_suggestion("NONE").is_none());
    assert!(validate_suggestion("none").is_none());
    assert!(validate_suggestion("None").is_none());
}

#[test]
fn test_validate_suggestion_rejects_overly_long() {
    let too_long = "one two three four five six seven eight nine ten \
                    eleven twelve thirteen fourteen fifteen sixteen \
                    seventeen eighteen nineteen twenty twenty-one \
                    twenty-two twenty-three twenty-four twenty-five";
    assert!(
        validate_suggestion(too_long).is_none(),
        "suggestion past 24 words must be rejected"
    );
}

#[test]
fn test_validate_suggestion_keeps_normal_length() {
    let ok = "Run the tests in coco-tools";
    assert_eq!(validate_suggestion(ok).as_deref(), Some(ok));
}

#[test]
fn test_record_then_mark_accepted() {
    let mut state = default_state();
    record_suggestion(
        &mut state,
        "show the diff".into(),
        "p1".into(),
        "2026-05-01T00:00:00Z".into(),
        Some("turn-7".into()),
    );
    let s = state.prompt_suggestion.as_ref().unwrap();
    assert_eq!(s.text, "show the diff");
    assert_eq!(s.prompt_id, "p1");
    assert!(s.accepted_at.is_none());

    let did_mark = mark_accepted(&mut state, "2026-05-01T00:00:05Z".into());
    assert!(did_mark);
    let s = state.prompt_suggestion.as_ref().unwrap();
    assert_eq!(s.accepted_at.as_deref(), Some("2026-05-01T00:00:05Z"));
}

#[test]
fn test_mark_accepted_no_suggestion_returns_false() {
    let mut state = default_state();
    assert!(!mark_accepted(&mut state, "2026-05-01T00:00:00Z".into()));
}

#[test]
fn test_clear_drops_suggestion() {
    let mut state = default_state();
    record_suggestion(&mut state, "x".into(), "p1".into(), "t".into(), None);
    assert!(state.prompt_suggestion.is_some());
    clear_suggestion(&mut state);
    assert!(state.prompt_suggestion.is_none());
}

#[test]
fn test_system_prompt_mentions_word_count_constraint() {
    let prompt = build_suggestion_system_prompt();
    assert!(prompt.contains("3 to 12 words"));
    // Tests pin the cache-key-affecting parts of the prompt; full
    // byte-equality with TS would be over-specified.
    assert!(prompt.contains("NONE"));
}
