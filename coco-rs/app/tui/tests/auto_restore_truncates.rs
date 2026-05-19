//! Cross-layer regression guard for `MessageTruncated` (plan §6.3, §9).
//!
//! Auto-restore (and explicit rewind) converges on a single
//! `ServerNotification::MessageTruncated { keep_count }` so engine,
//! TUI, and SDK observers all derive their state from the same event.
//! This test:
//!
//!   1. Seeds the TUI transcript via wire `MessageAppended` events.
//!   2. Seeds the overlays the protocol handler is supposed to wipe
//!      (`tool_executions`, `ui.streaming`).
//!   3. Pushes the `MessageTruncated` event through a real
//!      `mpsc::channel<CoreEvent>` so SDK-style observers see it.
//!   4. Roundtrips the observed event through JSON to prove the wire
//!      shape carries no `serde_json::Value` payload anywhere.
//!   5. Feeds the roundtripped event into `coco_tui::handle_core_event`
//!      and asserts: transcript shrinks to `keep_count` cells, both
//!      overlays clear.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use coco_messages::create_user_message;
use coco_tui::AppState;
use coco_tui::handle_core_event;
use coco_tui::state::StreamingState;
use coco_tui::state::ToolExecution;
use coco_tui::state::ToolStatus;
use coco_types::CoreEvent;
use coco_types::ServerNotification;
use tokio::sync::mpsc;

fn protocol_evt(notif: ServerNotification) -> CoreEvent {
    CoreEvent::Protocol(notif)
}

/// JSON-roundtrip a `Protocol(ServerNotification)` event. Only the
/// inner `ServerNotification` is wire-serializable — `CoreEvent` is
/// the in-process 3-layer dispatch enum and carries no serde derives.
fn roundtrip(evt: CoreEvent) -> CoreEvent {
    let CoreEvent::Protocol(notif) = evt else {
        panic!("roundtrip helper only handles Protocol variants");
    };
    let json = serde_json::to_string(&notif).expect("ServerNotification serializes");
    let back: ServerNotification =
        serde_json::from_str(&json).expect("ServerNotification roundtrips through JSON");
    CoreEvent::Protocol(back)
}

fn fake_running_tool(call_id: &str) -> ToolExecution {
    ToolExecution {
        call_id: call_id.into(),
        name: "Read".into(),
        status: ToolStatus::Running,
        started_at: std::time::Instant::now(),
        completed_at: None,
        description: None,
        streaming_input: None,
    }
}

#[tokio::test]
async fn truncate_shrinks_transcript_and_clears_overlays() {
    let mut state = AppState::new();

    // ── Seed transcript with 3 user messages through the wire ──────
    for i in 0..3 {
        let m = create_user_message(&format!("msg {i}"));
        handle_core_event(
            &mut state,
            protocol_evt(ServerNotification::MessageAppended { message: m }),
        );
    }
    assert_eq!(state.session.transcript.len(), 3, "three cells seeded");

    // ── Seed overlays the truncate handler must wipe ───────────────
    state.session.tool_executions.push(fake_running_tool("c1"));
    state.session.tool_executions.push(fake_running_tool("c2"));
    state.ui.streaming = Some(StreamingState::default());

    // ── SDK-observer path: event flows through a real channel ──────
    let (tx, mut rx) = mpsc::channel::<CoreEvent>(4);
    tx.send(protocol_evt(ServerNotification::MessageTruncated {
        keep_count: 1,
    }))
    .await
    .expect("channel accepts the event");

    let observed = rx.recv().await.expect("SDK observer receives event");
    let CoreEvent::Protocol(ServerNotification::MessageTruncated { keep_count }) = &observed else {
        panic!("expected Protocol(MessageTruncated), got {observed:?}");
    };
    assert_eq!(*keep_count, 1, "wire payload preserves keep_count");

    // ── Feed through JSON + handle_core_event ──────────────────────
    handle_core_event(&mut state, roundtrip(observed));

    assert_eq!(
        state.session.transcript.len(),
        1,
        "transcript truncated to keep_count"
    );
    assert!(
        state.session.tool_executions.is_empty(),
        "tool_executions cleared on truncate (plan §6.3)"
    );
    assert!(
        state.ui.streaming.is_none(),
        "streaming overlay cleared on truncate (plan §6.3)"
    );
}

#[tokio::test]
async fn truncate_to_zero_empties_transcript() {
    let mut state = AppState::new();
    for i in 0..2 {
        let m = create_user_message(&format!("m{i}"));
        handle_core_event(
            &mut state,
            protocol_evt(ServerNotification::MessageAppended { message: m }),
        );
    }
    assert_eq!(state.session.transcript.len(), 2);

    handle_core_event(
        &mut state,
        roundtrip(protocol_evt(ServerNotification::MessageTruncated {
            keep_count: 0,
        })),
    );

    assert!(
        state.session.transcript.is_empty(),
        "keep_count=0 drops every cell"
    );
}

#[tokio::test]
async fn truncate_beyond_history_is_a_noop() {
    let mut state = AppState::new();
    let m = create_user_message("only");
    handle_core_event(
        &mut state,
        protocol_evt(ServerNotification::MessageAppended { message: m }),
    );

    // keep_count larger than current len — the view must not panic or
    // shrink below the actual cell count.
    handle_core_event(
        &mut state,
        roundtrip(protocol_evt(ServerNotification::MessageTruncated {
            keep_count: 99,
        })),
    );

    assert_eq!(
        state.session.transcript.len(),
        1,
        "keep_count larger than history is clamped (no-op)"
    );
}
