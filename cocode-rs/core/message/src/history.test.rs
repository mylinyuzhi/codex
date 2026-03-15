use super::*;

fn make_turn(number: i32) -> Turn {
    let user_msg = TrackedMessage::user(format!("Message {number}"), format!("turn-{number}"));
    let mut turn = Turn::new(number, user_msg);
    turn.set_assistant_message(TrackedMessage::assistant(
        format!("Response {number}"),
        format!("turn-{number}"),
        None,
    ));
    turn.update_usage(TokenUsage::new(10, 5));
    turn
}

#[test]
fn test_empty_history() {
    let history = MessageHistory::new();
    assert_eq!(history.turn_count(), 0);
    assert!(history.current_turn().is_none());
}

#[test]
fn test_add_turns() {
    let mut history = MessageHistory::new();

    history.add_turn(make_turn(1));
    assert_eq!(history.turn_count(), 1);

    history.add_turn(make_turn(2));
    assert_eq!(history.turn_count(), 2);
}

#[test]
fn test_system_message() {
    let mut history = MessageHistory::new();
    history.set_system_message(TrackedMessage::system("You are helpful", "system"));

    let messages = history.messages_for_api();
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].role, hyper_sdk::Role::System);
}

#[test]
fn test_messages_for_api() {
    let mut history = MessageHistory::new();
    history.set_system_message(TrackedMessage::system("You are helpful", "system"));
    history.add_turn(make_turn(1));

    let messages = history.messages_for_api();
    // System + user + assistant
    assert_eq!(messages.len(), 3);
}

#[test]
fn test_total_usage() {
    let mut history = MessageHistory::new();
    history.add_turn(make_turn(1));
    history.add_turn(make_turn(2));

    let usage = history.total_usage();
    assert_eq!(usage.input_tokens, 20);
    assert_eq!(usage.output_tokens, 10);
}

#[test]
fn test_compaction() {
    let mut history = MessageHistory::new();
    for i in 1..=10 {
        history.add_turn(make_turn(i));
    }

    assert_eq!(history.turn_count(), 10);

    history.apply_compaction("Summary of turns 1-8".to_string(), 2, "turn-10", 5000);
    assert_eq!(history.turn_count(), 2);
    assert!(history.compacted_summary().is_some());

    // Verify compaction boundary
    let boundary = history.compaction_boundary().unwrap();
    assert_eq!(boundary.turn_id, "turn-10");
    assert_eq!(boundary.turn_number, 10);
    assert_eq!(boundary.turns_compacted, 8);
    assert_eq!(boundary.tokens_saved, 5000);
    assert!(boundary.timestamp_ms > 0);
}

#[test]
fn test_builder() {
    let history = HistoryBuilder::new()
        .context_window(64000)
        .compaction_threshold(0.7)
        .max_turns(50)
        .system_message("You are helpful")
        .build();

    assert_eq!(history.config.context_window, 64000);
    assert_eq!(history.config.compaction_threshold, 0.7);
    assert_eq!(history.config.max_turns, 50);
    assert!(history.system_message.is_some());
}

#[test]
fn test_needs_compaction_by_turns() {
    let config = HistoryConfig {
        max_turns: 5,
        auto_compact: true,
        ..Default::default()
    };
    let mut history = MessageHistory::with_config(config);

    for i in 1..=6 {
        history.add_turn(make_turn(i));
    }

    assert!(history.needs_compaction());
}

#[test]
fn test_clear() {
    let mut history = MessageHistory::new();
    history.add_turn(make_turn(1));
    history.apply_compaction("Summary".to_string(), 1, "turn-1", 100);

    // Verify compaction was applied
    assert!(history.compacted_summary().is_some());
    assert!(history.compaction_boundary().is_some());

    history.clear();
    assert_eq!(history.turn_count(), 0);
    assert!(history.compacted_summary().is_none());
    assert!(history.compaction_boundary().is_none());
}
