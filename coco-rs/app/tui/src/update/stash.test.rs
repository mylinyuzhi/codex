use super::swap_input_draft;
use crate::state::AppState;
use crate::state::ui::StashedInput;

fn fresh_stash() -> StashedInput {
    StashedInput {
        text: String::new(),
        cursor: 0,
        paste_entries: Vec::new(),
    }
}

#[test]
fn empty_input_with_no_stash_is_silent_noop() {
    let mut state = AppState::new();
    swap_input_draft(&mut state);
    assert!(state.ui.input.text.is_empty());
    assert!(state.ui.stashed_input.is_none());
    // TS-mirror: no toast on the silent no-op.
    assert!(state.ui.toasts.is_empty());
}

#[test]
fn non_empty_input_pushes_to_stash_and_clears_input() {
    let mut state = AppState::new();
    state.ui.input.text = "hello world".to_string();
    state.ui.input.cursor = 5;

    swap_input_draft(&mut state);

    assert_eq!(state.ui.input.text, "");
    assert_eq!(state.ui.input.cursor, 0);
    let stash = state.ui.stashed_input.as_ref().expect("stash present");
    assert_eq!(stash.text, "hello world");
    assert_eq!(stash.cursor, 5);
}

#[test]
fn empty_input_with_stash_pops_stash_into_input() {
    let mut state = AppState::new();
    state.ui.stashed_input = Some(StashedInput {
        text: "stashed".to_string(),
        cursor: 7,
        ..fresh_stash()
    });

    swap_input_draft(&mut state);

    assert_eq!(state.ui.input.text, "stashed");
    assert_eq!(state.ui.input.cursor, 7);
    assert!(state.ui.stashed_input.is_none());
}

#[test]
fn stash_round_trips_paste_entries() {
    let mut state = AppState::new();
    let pill = state.ui.paste_manager.add_text("first paste".into());
    state.ui.input.text = format!("hello {pill} world");
    state.ui.input.cursor = state.ui.input.text.chars().count() as i32;

    // Push: paste entries move into the stash slot.
    swap_input_draft(&mut state);
    assert!(state.ui.input.text.is_empty());
    assert!(state.ui.paste_manager.entries().is_empty());
    let stash = state.ui.stashed_input.as_ref().expect("pushed");
    assert_eq!(stash.paste_entries.len(), 1);
    assert!(stash.text.contains("[Pasted text #1]"));

    // Pop: paste entries restored alongside text + cursor so pills
    // still resolve.
    swap_input_draft(&mut state);
    assert!(state.ui.input.text.contains("[Pasted text #1]"));
    assert_eq!(state.ui.paste_manager.entries().len(), 1);
    let resolved = state.ui.paste_manager.resolve(&state.ui.input.text);
    assert!(resolved.contains("first paste"));
}

#[test]
fn non_empty_input_overwrites_existing_stash() {
    // TS-mirror behavior: pushing with a prior stash does NOT swap —
    // the prior stash is overwritten. There is no stash list.
    let mut state = AppState::new();
    state.ui.stashed_input = Some(StashedInput {
        text: "old".to_string(),
        cursor: 3,
        ..fresh_stash()
    });
    state.ui.input.text = "new".to_string();
    state.ui.input.cursor = 3;

    swap_input_draft(&mut state);

    let stash = state.ui.stashed_input.as_ref().expect("stash present");
    assert_eq!(
        stash.text, "new",
        "push overwrites the prior stash (TS-faithful)",
    );
    assert!(state.ui.input.text.is_empty());
}

#[test]
fn whitespace_only_input_is_treated_as_empty() {
    // TS uses `input.trim() === ''`, so all-whitespace input pops
    // the stash (or no-ops) rather than pushing.
    let mut state = AppState::new();
    state.ui.input.text = "   \n  ".to_string();
    state.ui.stashed_input = Some(StashedInput {
        text: "real draft".to_string(),
        cursor: 10,
        ..fresh_stash()
    });

    swap_input_draft(&mut state);

    assert_eq!(state.ui.input.text, "real draft");
    assert!(state.ui.stashed_input.is_none());
}
