use super::*;

#[test]
fn test_input_state_insert() {
    let mut input = InputState::default();
    input.insert_char('H');
    input.insert_char('i');
    assert_eq!(input.text(), "Hi");
    assert_eq!(input.cursor, 2);
}

#[test]
fn test_input_state_delete() {
    let mut input = InputState::default();
    input.set_text("Hello");
    input.cursor = 3; // After "Hel"

    input.delete_backward();
    assert_eq!(input.text(), "Helo");
    assert_eq!(input.cursor, 2);

    input.delete_forward();
    assert_eq!(input.text(), "Heo");
}

#[test]
fn test_input_state_navigation() {
    let mut input = InputState::default();
    input.set_text("Hello");

    input.move_home();
    assert_eq!(input.cursor, 0);

    input.move_right();
    assert_eq!(input.cursor, 1);

    input.move_end();
    assert_eq!(input.cursor, 5);

    input.move_left();
    assert_eq!(input.cursor, 4);
}

#[test]
fn test_input_state_take() {
    let mut input = InputState::default();
    input.set_text("Hello");

    let text = input.take();
    assert_eq!(text, "Hello");
    assert!(input.is_empty());
    assert_eq!(input.cursor, 0);
}

#[test]
fn test_streaming_state() {
    let mut ui = UiState::default();

    ui.start_streaming("turn-1".to_string());
    assert!(ui.streaming.is_some());

    ui.append_streaming("Hello ");
    ui.append_streaming("World");
    assert_eq!(
        ui.streaming.as_ref().map(|s| s.content.as_str()),
        Some("Hello World")
    );

    ui.stop_streaming();
    assert!(ui.streaming.is_none());
}

#[test]
fn test_focus_target_default() {
    assert_eq!(FocusTarget::default(), FocusTarget::Input);
}

#[test]
fn test_current_at_token_simple() {
    let mut input = InputState::default();
    input.set_text("@src/main");

    let result = input.current_at_token();
    assert_eq!(result, Some((0, "src/main".to_string())));
}

#[test]
fn test_current_at_token_mid_text() {
    let mut input = InputState::default();
    input.set_text("read @src/lib.rs please");
    input.cursor = 16; // After "@src/lib.rs"

    let result = input.current_at_token();
    assert_eq!(result, Some((5, "src/lib.rs".to_string())));
}

#[test]
fn test_current_at_token_no_mention() {
    let mut input = InputState::default();
    input.set_text("no mention here");

    let result = input.current_at_token();
    assert_eq!(result, None);
}

#[test]
fn test_current_at_token_after_space() {
    let mut input = InputState::default();
    input.set_text("@file completed ");
    input.cursor = 16; // After space

    let result = input.current_at_token();
    assert_eq!(result, None); // Space breaks the mention
}

#[test]
fn test_insert_selected_path() {
    let mut input = InputState::default();
    input.set_text("read @src/ please");
    input.cursor = 10; // After "@src/"

    input.insert_selected_path(5, "src/main.rs");

    assert_eq!(input.text(), "read @src/main.rs  please");
    assert_eq!(input.cursor, 18); // After "@src/main.rs "
}

#[test]
fn test_file_suggestion_state() {
    let mut state = FileSuggestionState::new("src/".to_string(), 5);

    assert!(state.loading);
    assert!(state.suggestions.is_empty());
    assert_eq!(state.selected, 0);

    // Add suggestions
    state.update_suggestions(vec![
        FileSuggestionItem {
            path: "src/main.rs".to_string(),
            display_text: "src/main.rs".to_string(),
            score: 100,
            match_indices: vec![],
            is_directory: false,
        },
        FileSuggestionItem {
            path: "src/lib.rs".to_string(),
            display_text: "src/lib.rs".to_string(),
            score: 90,
            match_indices: vec![],
            is_directory: false,
        },
    ]);

    assert!(!state.loading);
    assert_eq!(state.suggestions.len(), 2);

    // Navigate
    state.move_down();
    assert_eq!(state.selected, 1);

    state.move_down(); // Should not go past last
    assert_eq!(state.selected, 1);

    state.move_up();
    assert_eq!(state.selected, 0);

    state.move_up(); // Should not go negative
    assert_eq!(state.selected, 0);
}

#[test]
fn test_move_word_left() {
    let mut input = InputState::default();
    input.set_text("hello world test");

    // Cursor at end
    assert_eq!(input.cursor, 16);

    input.move_word_left();
    assert_eq!(input.cursor, 12); // Before "test"

    input.move_word_left();
    assert_eq!(input.cursor, 6); // Before "world"

    input.move_word_left();
    assert_eq!(input.cursor, 0); // At start

    input.move_word_left(); // Should stay at 0
    assert_eq!(input.cursor, 0);
}

#[test]
fn test_move_word_right() {
    let mut input = InputState::default();
    input.set_text("hello world test");
    input.cursor = 0;

    input.move_word_right();
    assert_eq!(input.cursor, 6); // After "hello "

    input.move_word_right();
    assert_eq!(input.cursor, 12); // After "world "

    input.move_word_right();
    assert_eq!(input.cursor, 16); // At end

    input.move_word_right(); // Should stay at end
    assert_eq!(input.cursor, 16);
}

#[test]
fn test_delete_word_backward() {
    let mut input = InputState::default();
    input.set_text("hello world test");

    input.delete_word_backward();
    assert_eq!(input.text(), "hello world ");
    assert_eq!(input.cursor, 12);

    input.delete_word_backward();
    assert_eq!(input.text(), "hello ");
    assert_eq!(input.cursor, 6);

    input.delete_word_backward();
    assert_eq!(input.text(), "");
    assert_eq!(input.cursor, 0);
}

#[test]
fn test_delete_word_forward() {
    let mut input = InputState::default();
    input.set_text("hello world test");
    input.cursor = 0;

    input.delete_word_forward();
    assert_eq!(input.text(), "world test");
    assert_eq!(input.cursor, 0);

    input.delete_word_forward();
    assert_eq!(input.text(), "test");
    assert_eq!(input.cursor, 0);

    input.delete_word_forward();
    assert_eq!(input.text(), "");
    assert_eq!(input.cursor, 0);
}

#[test]
fn test_toggle_thinking() {
    let mut ui = UiState::default();
    assert!(!ui.show_thinking);

    ui.toggle_thinking();
    assert!(ui.show_thinking);

    ui.toggle_thinking();
    assert!(!ui.show_thinking);
}

#[test]
fn test_user_scrolled() {
    let mut ui = UiState::default();
    assert!(!ui.user_scrolled);

    ui.mark_user_scrolled();
    assert!(ui.user_scrolled);

    ui.reset_user_scrolled();
    assert!(!ui.user_scrolled);
}

#[test]
fn test_current_slash_token_simple() {
    let mut input = InputState::default();
    input.set_text("/commit");

    let result = input.current_slash_token();
    assert_eq!(result, Some((0, "commit".to_string())));
}

#[test]
fn test_current_slash_token_mid_text() {
    let mut input = InputState::default();
    input.set_text("run /review file.rs");
    input.cursor = 11; // After "/review"

    let result = input.current_slash_token();
    assert_eq!(result, Some((4, "review".to_string())));
}

#[test]
fn test_current_slash_token_no_command() {
    let mut input = InputState::default();
    input.set_text("no command here");

    let result = input.current_slash_token();
    assert_eq!(result, None);
}

#[test]
fn test_current_slash_token_after_space() {
    let mut input = InputState::default();
    input.set_text("/commit completed ");
    input.cursor = 18; // After space

    let result = input.current_slash_token();
    assert_eq!(result, None); // Space breaks the command
}

#[test]
fn test_insert_selected_skill() {
    let mut input = InputState::default();
    input.set_text("run /com please");
    input.cursor = 8; // After "/com"

    input.insert_selected_skill(4, "commit");

    assert_eq!(input.text(), "run /commit  please");
    assert_eq!(input.cursor, 12); // After "/commit "
}

#[test]
fn test_skill_suggestion_state() {
    let mut state = SkillSuggestionState::new("com".to_string(), 0);

    assert!(!state.loading);
    assert!(state.suggestions.is_empty());
    assert_eq!(state.selected, 0);

    // Add suggestions
    state.update_suggestions(vec![
        SkillSuggestionItem {
            name: "commit".to_string(),
            description: "Generate a commit message".to_string(),
            score: -100,
            match_indices: vec![0, 1, 2],
        },
        SkillSuggestionItem {
            name: "config".to_string(),
            description: "Configure settings".to_string(),
            score: -98,
            match_indices: vec![0, 1],
        },
    ]);

    assert!(!state.loading);
    assert_eq!(state.suggestions.len(), 2);

    // Navigate
    state.move_down();
    assert_eq!(state.selected, 1);

    state.move_down(); // Should not go past last
    assert_eq!(state.selected, 1);

    state.move_up();
    assert_eq!(state.selected, 0);

    state.move_up(); // Should not go negative
    assert_eq!(state.selected, 0);
}

#[test]
fn test_thinking_duration() {
    let mut ui = UiState::default();

    // Initially not thinking
    assert!(!ui.is_thinking());
    assert!(ui.thinking_duration().is_none());

    // Start thinking
    ui.start_thinking();
    assert!(ui.is_thinking());
    assert!(ui.thinking_duration().is_some());

    // Duration should be small (just started)
    let duration = ui.thinking_duration().unwrap();
    assert!(duration.as_millis() < 1000);

    // Stop thinking
    ui.stop_thinking();
    assert!(!ui.is_thinking());
    assert!(ui.last_thinking_duration.is_some());

    // Clear thinking duration
    ui.clear_thinking_duration();
    assert!(ui.thinking_duration().is_none());
}

#[test]
fn test_terminal_focused_default() {
    let ui = UiState::default();
    assert!(!ui.terminal_focused);
}

#[test]
fn test_insert_selected_agent_basic() {
    let mut input = InputState::default();
    input.set_text("use @agent-exp to search");
    input.cursor = 14; // After "@agent-exp"

    input.insert_selected_agent(4, "explore");

    assert_eq!(input.text(), "use @agent-explore  to search");
    assert_eq!(input.cursor, 19); // After "@agent-explore "
}

#[test]
fn test_insert_selected_agent_start_of_line() {
    let mut input = InputState::default();
    input.set_text("@agent");
    input.cursor = 6;

    input.insert_selected_agent(0, "bash");

    assert_eq!(input.text(), "@agent-bash ");
    assert_eq!(input.cursor, 12);
}

#[test]
fn test_agent_suggestion_state_navigation() {
    let mut state = AgentSuggestionState::new("exp".to_string(), 0);

    state.update_suggestions(vec![
        AgentSuggestionItem {
            agent_type: "explore".to_string(),
            name: "Explore".to_string(),
            description: "Search codebase".to_string(),
            score: -100,
            match_indices: vec![0, 1, 2],
        },
        AgentSuggestionItem {
            agent_type: "explain".to_string(),
            name: "Explain".to_string(),
            description: "Explain code".to_string(),
            score: -90,
            match_indices: vec![0, 1],
        },
    ]);

    assert_eq!(state.selected, 0);

    // Move down
    state.move_down();
    assert_eq!(state.selected, 1);

    // Should not go past last
    state.move_down();
    assert_eq!(state.selected, 1);

    // Move up
    state.move_up();
    assert_eq!(state.selected, 0);

    // Should not go negative
    state.move_up();
    assert_eq!(state.selected, 0);
}

#[test]
fn test_current_at_token_quoted_with_space() {
    let mut input = InputState::default();
    input.set_text("@\"my file");
    input.cursor = 9; // After @"my file

    let result = input.current_at_token();
    assert_eq!(result, Some((0, "my file".to_string())));
}

#[test]
fn test_current_at_token_quoted_complete() {
    let mut input = InputState::default();
    input.set_text("@\"my file\" rest");
    input.cursor = 15; // After closing quote + space + rest

    let result = input.current_at_token();
    assert_eq!(result, None); // Closing quote means mention is complete
}
