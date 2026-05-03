use super::*;
use pretty_assertions::assert_eq;

#[test]
fn parses_object_with_selected_memories_array() {
    let resp = r#"{"selected_memories": ["a.md", "b.md"]}"#;
    assert_eq!(
        parse_selection_response(resp),
        vec!["a.md".to_string(), "b.md".to_string()]
    );
}

#[test]
fn falls_back_to_bare_array() {
    let resp = "[\"a.md\", \"b.md\"]";
    assert_eq!(
        parse_selection_response(resp),
        vec!["a.md".to_string(), "b.md".to_string()]
    );
}

#[test]
fn returns_empty_for_unparseable() {
    assert!(parse_selection_response("not json").is_empty());
}

#[test]
fn prefetch_tracks_surfaced_and_budget() {
    let state = PrefetchState::new();
    assert!(!state.is_surfaced("a.md"));
    state.mark_surfaced("a.md", 100);
    assert!(state.is_surfaced("a.md"));
    assert!(!state.is_budget_exhausted());
    state.mark_surfaced("b.md", MAX_SESSION_BYTES);
    assert!(state.is_budget_exhausted());
}
