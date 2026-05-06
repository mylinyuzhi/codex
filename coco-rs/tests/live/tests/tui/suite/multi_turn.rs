//! Three sequential prompts against a scripted assistant. Verifies
//! state accumulates correctly across turns — each turn appends a user
//! message, drives one engine round-trip, and finalizes a fresh
//! assistant `ChatMessage` without clobbering prior history.

use std::time::Duration;

use anyhow::Result;

use crate::tui::harness::TuiHarness;
use crate::tui::scripted_model::Reply;

pub async fn run() -> Result<()> {
    let mut harness = TuiHarness::builder()
        .with_replies([
            Reply::text("turn-1: counted 1"),
            Reply::text("turn-2: counted 2"),
            Reply::text("turn-3: counted 3"),
        ])
        .with_max_turns(4)
        .build()
        .await?;

    let prompts = ["count to 1", "now to 2", "now to 3"];
    let expected_replies = [
        "turn-1: counted 1",
        "turn-2: counted 2",
        "turn-3: counted 3",
    ];

    for (i, prompt) in prompts.iter().enumerate() {
        harness.submit(prompt).await;
        let ok = harness.pump_until_idle(Duration::from_secs(15)).await?;
        assert!(ok, "multi_turn: turn {} flagged is_error", i + 1);
    }

    // Three separate engine sessions = three model calls.
    assert_eq!(
        harness.model.call_count(),
        3,
        "multi_turn: expected 3 LLM calls (one per submit), got {}",
        harness.model.call_count()
    );

    // History order: U1, A1, U2, A2, U3, A3 — interleaving is what
    // makes the chat-panel render coherent.
    let messages = &harness.state.session.messages;
    let mut user_idxs = Vec::new();
    let mut assistant_idxs = Vec::new();
    for (i, m) in messages.iter().enumerate() {
        match m.role {
            coco_tui::state::session::ChatRole::User => user_idxs.push(i),
            coco_tui::state::session::ChatRole::Assistant => assistant_idxs.push(i),
            _ => {}
        }
    }
    assert_eq!(
        user_idxs.len(),
        3,
        "multi_turn: expected 3 user messages, got {} (all messages: {})",
        user_idxs.len(),
        messages.len()
    );
    assert_eq!(
        assistant_idxs.len(),
        3,
        "multi_turn: expected 3 assistant messages, got {}",
        assistant_idxs.len()
    );

    for (i, prompt) in prompts.iter().enumerate() {
        let m = &messages[user_idxs[i]];
        assert_eq!(
            m.text_content(),
            *prompt,
            "multi_turn: user message {i} text mismatch"
        );
    }
    for (i, expected) in expected_replies.iter().enumerate() {
        let m = &messages[assistant_idxs[i]];
        assert!(
            m.text_content().contains(expected),
            "multi_turn: assistant {i} text missing `{expected}`, got `{}`",
            m.text_content()
        );
    }

    // turn_count tracks the *current session's* turn number — each
    // `submit` opens a fresh `run_with_events`, so the counter resets
    // to 1 every time. Just confirm we observed at least one
    // TurnCompleted (set above by handle_core_event).
    assert!(
        harness.state.session.turn_count >= 1,
        "multi_turn: turn_count never advanced (={})",
        harness.state.session.turn_count
    );

    harness.shutdown().await;
    Ok(())
}
