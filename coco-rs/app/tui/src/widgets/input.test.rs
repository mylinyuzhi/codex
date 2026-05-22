use super::*;
use crate::state::InputState;

fn input(text: &str) -> InputState {
    let mut input = InputState::new();
    input.set_text(text);
    input
}

#[test]
fn input_render_model_strips_bash_prefix_for_display() {
    let input = input("! cargo test");

    let model = InputRenderModel::build(&input, false, false, None, false, None);

    assert_eq!(model.prompt_mode, PromptMode::Bash);
    assert_eq!(model.prefix_consumed, 2);
    assert_eq!(model.display_text, "cargo test");
    assert_eq!(model.title, " Bash Mode ");
    assert!(!model.is_placeholder);
}

#[test]
fn input_render_model_combines_plan_and_bash_titles() {
    let input = input("!pwd");

    let model = InputRenderModel::build(&input, false, true, None, false, None);

    assert_eq!(model.title, " Plan Mode • Bash Mode ");
}

#[test]
fn input_render_model_queued_placeholder_wins_over_suggestion() {
    let input = InputState::new();

    let model = InputRenderModel::build(&input, false, false, Some("Try this prompt"), true, None);

    assert_eq!(model.display_text, "Press ↑ to edit queued messages");
    assert!(model.is_placeholder);
}

#[test]
fn input_render_model_empty_default_has_no_placeholder_text() {
    let input = InputState::new();

    let model = InputRenderModel::build(&input, false, false, None, false, None);

    assert_eq!(model.display_text, "");
    assert!(!model.is_placeholder);
}

#[test]
fn input_render_model_command_palette_filter_wins_over_placeholder() {
    let input = InputState::new();

    let model =
        InputRenderModel::build(&input, false, false, Some("ignored"), false, Some("config"));

    assert_eq!(model.display_text, "/config");
    assert_eq!(model.command_palette_filter.as_deref(), Some("config"));
    assert!(!model.is_placeholder);
}

#[test]
fn input_render_model_streaming_forces_queue_title_and_normal_prompt() {
    let input = input("! cargo test");

    let model = InputRenderModel::build(&input, true, true, None, false, None);

    assert_eq!(model.prompt_mode, PromptMode::Normal);
    assert_eq!(model.prefix_consumed, 0);
    assert_eq!(model.display_text, "! cargo test");
    assert_eq!(model.title, " Queue Input ");
}
