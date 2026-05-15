//! Unit tests for [`PromptMode`] prefix detection and [`InputState`] mode
//! derivation. TS parity reference: `components/PromptInput/inputModes.ts`.

use super::InputState;
use super::PromptMode;
use pretty_assertions::assert_eq;

#[test]
fn prompt_mode_from_empty_is_normal() {
    assert_eq!(PromptMode::from_text(""), PromptMode::Normal);
}

#[test]
fn prompt_mode_bang_prefix_is_bash() {
    assert_eq!(PromptMode::from_text("!ls -la"), PromptMode::Bash);
    assert_eq!(PromptMode::from_text("!"), PromptMode::Bash);
    assert_eq!(PromptMode::from_text("! echo hi"), PromptMode::Bash);
}

#[test]
fn prompt_mode_hash_prefix_is_memory() {
    assert_eq!(PromptMode::from_text("#note this"), PromptMode::Memory);
    assert_eq!(PromptMode::from_text("# note"), PromptMode::Memory);
}

#[test]
fn prompt_mode_leading_space_kills_prefix() {
    // TS getModeFromInput uses startsWith — leading whitespace defeats it.
    assert_eq!(PromptMode::from_text(" !ls"), PromptMode::Normal);
    assert_eq!(PromptMode::from_text("\t#x"), PromptMode::Normal);
}

#[test]
fn prompt_mode_text_passthrough_for_other_chars() {
    assert_eq!(PromptMode::from_text("hello"), PromptMode::Normal);
    assert_eq!(PromptMode::from_text("/help"), PromptMode::Normal);
    assert_eq!(PromptMode::from_text("@file.rs"), PromptMode::Normal);
}

#[test]
fn strip_prefix_normal_passes_text_through() {
    assert_eq!(PromptMode::Normal.strip_prefix("hello"), "hello");
    assert_eq!(PromptMode::Normal.strip_prefix(""), "");
}

#[test]
fn strip_prefix_bash_drops_bang_and_one_space() {
    assert_eq!(PromptMode::Bash.strip_prefix("!ls"), "ls");
    assert_eq!(PromptMode::Bash.strip_prefix("! ls"), "ls");
    // Multiple leading spaces: only one consumed (matches TS `slice(1)`).
    assert_eq!(PromptMode::Bash.strip_prefix("!  ls"), " ls");
    assert_eq!(PromptMode::Bash.strip_prefix("!"), "");
}

#[test]
fn strip_prefix_memory_drops_hash_and_one_space() {
    assert_eq!(PromptMode::Memory.strip_prefix("#note"), "note");
    assert_eq!(PromptMode::Memory.strip_prefix("# note"), "note");
    assert_eq!(PromptMode::Memory.strip_prefix("#"), "");
}

#[test]
fn input_state_prompt_mode_tracks_text() {
    let mut state = InputState::new();
    assert_eq!(state.prompt_mode(), PromptMode::Normal);

    state.textarea.insert_str("!");
    assert_eq!(state.prompt_mode(), PromptMode::Bash);

    state.textarea.insert_str("ls");
    assert_eq!(state.prompt_mode(), PromptMode::Bash);

    // Delete the prefix (move home, forward-delete) — back to Normal.
    state.textarea.set_cursor(0);
    state.textarea.delete_forward(1);
    assert_eq!(state.prompt_mode(), PromptMode::Normal);
    assert_eq!(state.text(), "ls");
}

#[test]
fn input_state_prompt_mode_hash_then_swap_to_bang() {
    let mut state = InputState::new();
    state.textarea.insert_str("#");
    assert_eq!(state.prompt_mode(), PromptMode::Memory);

    state.textarea.set_cursor(0);
    state.textarea.delete_forward(1);
    state.textarea.set_cursor(0);
    state.textarea.insert_str("!");
    assert_eq!(state.prompt_mode(), PromptMode::Bash);
}

#[test]
fn title_i18n_keys_match_yaml_layout() {
    // The render layer looks these up via `t!(...)` so they must
    // exist in locales/*.yaml. Asserting the literal here catches
    // refactors that rename keys without updating both files.
    assert_eq!(PromptMode::Normal.title_i18n_key(), "input.title");
    assert_eq!(PromptMode::Bash.title_i18n_key(), "input.title_bash_mode");
    assert_eq!(
        PromptMode::Memory.title_i18n_key(),
        "input.title_memory_mode"
    );
}
