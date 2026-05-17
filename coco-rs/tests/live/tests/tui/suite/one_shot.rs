//! Single-prompt round trip with a scripted assistant. Verifies:
//! - The harness pumps the engine end-to-end (SessionStarted →
//!   TurnStarted → TextDelta(s) → TurnCompleted → SessionResult).
//! - `handle_core_event` folds `TurnCompleted`'s flushed streaming
//!   buffer into a `ChatMessage::assistant_text` entry.
//! - The rendered buffer surfaces both the user's prompt and the
//!   assistant's reply (proves the chat panel widget pulled them out
//!   of state.session.messages).

use std::time::Duration;

use anyhow::Result;

use crate::tui::harness::TuiHarness;
use crate::tui::scripted_model::Reply;

pub async fn run() -> Result<()> {
    let mut harness = TuiHarness::builder()
        .with_replies([Reply::text("ack from scripted model: hello back")])
        .build()
        .await?;

    harness.submit("hello").await;
    let ok = harness.pump_until_idle(Duration::from_secs(15)).await?;
    assert!(
        ok,
        "one_shot: SessionResult flagged is_error=true ({} events)",
        harness.events.len()
    );

    // Engine round-trip: exactly one model call (no tool follow-ups).
    assert_eq!(
        harness.model.call_count(),
        1,
        "one_shot: expected 1 LLM call, got {}",
        harness.model.call_count()
    );

    // State shape: user message + assistant reply landed in the chat.
    let messages = &harness.state.session.messages;
    assert!(
        messages.iter().any(
            |m| matches!(m.role, coco_tui::state::session::ChatRole::User)
                && m.text_content() == "hello"
        ),
        "one_shot: user message not folded into session.messages \
         (got {} messages)",
        messages.len()
    );
    assert!(
        messages.iter().any(
            |m| matches!(m.role, coco_tui::state::session::ChatRole::Assistant)
                && m.text_content().contains("ack from scripted model")
        ),
        "one_shot: assistant reply not folded into session.messages \
         (got {} messages: {:?})",
        messages.len(),
        messages
            .iter()
            .map(|m| (m.role, m.text_content().to_string()))
            .collect::<Vec<_>>()
    );

    // Render side: user-visible buffer surfaces both turns.
    let rendered = harness.render_to_string()?;
    assert!(
        rendered.contains("hello"),
        "one_shot: rendered buffer missing user prompt:\n{rendered}"
    );
    assert!(
        rendered.contains("ack from scripted model"),
        "one_shot: rendered buffer missing assistant reply:\n{rendered}"
    );
    assert_no_large_gap_between_last_reply_and_input(&rendered);

    harness.shutdown().await;
    Ok(())
}

fn assert_no_large_gap_between_last_reply_and_input(rendered: &str) {
    let lines: Vec<&str> = rendered.lines().collect();
    let assistant_idx = lines
        .iter()
        .position(|line| line.contains("ack from scripted model"))
        .unwrap_or_else(|| panic!("one_shot: assistant line missing:\n{rendered}"));
    let input_idx = lines
        .iter()
        .enumerate()
        .skip(assistant_idx + 1)
        .find_map(|(index, line)| line.trim_start().starts_with("❯").then_some(index))
        .unwrap_or_else(|| panic!("one_shot: input prompt after assistant missing:\n{rendered}"));
    let gap = input_idx.saturating_sub(assistant_idx + 1);
    assert!(
        gap <= 3,
        "one_shot: input viewport drifted {gap} rows below the final assistant reply:\n{rendered}"
    );
}
