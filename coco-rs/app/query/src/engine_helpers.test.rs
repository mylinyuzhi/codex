//! Unit tests for `engine_helpers` free functions.
//!
//! Limited scope on purpose — heavier pipeline tests live in `engine.test.rs`.
//! Here we cover the small pure helpers that downstream code relies on.

use super::most_recent_assistant_exceeds;
use coco_messages::AssistantContent;
use coco_messages::Message;
use coco_messages::create_assistant_message;
use coco_messages::create_user_message;
use coco_types::TokenUsage;

fn assistant_with_total(total: i64) -> Message {
    // Distribute the total so the helper's sum of
    // (input + cache_read + cache_creation + output) equals `total`.
    // Concentrate on `input_tokens` for predictability.
    let usage = TokenUsage {
        input_tokens: total,
        ..TokenUsage::default()
    };
    create_assistant_message(vec![AssistantContent::text("(test)")], "test-model", usage)
}

#[test]
fn returns_false_on_empty_history() {
    // Cold start: no assistant turn yet — the swap should stay disabled.
    let empty: &[Message] = &[];
    assert!(!most_recent_assistant_exceeds(empty, 200_000));
}

#[test]
fn returns_false_when_most_recent_assistant_under_threshold() {
    let msgs = vec![assistant_with_total(150_000)];
    assert!(!most_recent_assistant_exceeds(&msgs, 200_000));
}

#[test]
fn returns_true_when_most_recent_assistant_over_threshold() {
    let msgs = vec![assistant_with_total(250_000)];
    assert!(most_recent_assistant_exceeds(&msgs, 200_000));
}

#[test]
fn looks_only_at_most_recent_assistant_turn() {
    // TS `findLast` semantics: an old over-threshold assistant must
    // NOT trigger fallback once a fresh under-threshold turn lands.
    let msgs = vec![
        assistant_with_total(500_000),
        create_user_message("interim"),
        assistant_with_total(50_000),
    ];
    assert!(
        !most_recent_assistant_exceeds(&msgs, 200_000),
        "stale large-context turns must not poison the bypass"
    );
}

#[test]
fn aggregates_input_cache_and_output_tokens() {
    let usage = TokenUsage {
        input_tokens: 100_000,
        output_tokens: 50_000,
        input_token_details: coco_types::InputTokenDetails {
            no_cache_tokens: 0,
            cache_read_tokens: 60_000,
            cache_write_tokens: 5_000,
        },
        ..TokenUsage::default()
    };
    let msgs = vec![create_assistant_message(
        vec![AssistantContent::text("(test)")],
        "test-model",
        usage,
    )];
    // 100k + 50k + 60k + 5k = 215k > 200k.
    assert!(most_recent_assistant_exceeds(&msgs, 200_000));
    // Threshold 220k → does not exceed.
    assert!(!most_recent_assistant_exceeds(&msgs, 220_000));
}
