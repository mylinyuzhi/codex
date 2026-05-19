//! Cross-layer regression guard for `SessionResetForResume` + bulk
//! `MessageAppended` replay (plan §6.3, §9).
//!
//! Simulates the resume burst: the engine emits
//! `SessionResetForResume` and then replays every loaded history
//! entry through `MessageAppended`. Verifies:
//!
//!   1. Reset clears `transcript`, `tool_executions`, and `ui.streaming`
//!      so prior-session overlays don't leak into the resumed view.
//!   2. `conversation_id` rotates to the new session id (prompt-cache
//!      collision guard).
//!   3. Each replayed `MessageAppended` re-derives a cell from the
//!      typed wire payload — i.e. the `Message` enum survives the
//!      JSON roundtrip with no `serde_json::Value` erasure.
//!   4. Typed `SystemMessage::UserInterruption` carries `for_tool_use`
//!      across the wire untouched.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use coco_messages::create_user_interruption_system_message;
use coco_messages::create_user_message;
use coco_tui::AppState;
use coco_tui::handle_core_event;
use coco_tui::state::CellKind;
use coco_tui::state::StreamingState;
use coco_tui::state::SystemCellKind;
use coco_tui::state::ToolExecution;
use coco_tui::state::ToolStatus;
use coco_types::CoreEvent;
use coco_types::ServerNotification;

/// JSON-roundtrip the wire payload, then wrap in `CoreEvent::Protocol`.
/// `CoreEvent` itself is in-process only (no serde derives); the
/// wire-visible type is `ServerNotification` and that's what the SDK
/// emits to NDJSON. Roundtripping here proves
/// `MessageAppended.message` carries a typed `Message` end-to-end —
/// if it ever regresses to `serde_json::Value`, deser would lose the
/// enum discriminant.
fn protocol_evt(notif: ServerNotification) -> CoreEvent {
    let json = serde_json::to_string(&notif).expect("ServerNotification serializes");
    let roundtripped: ServerNotification =
        serde_json::from_str(&json).expect("ServerNotification roundtrips through JSON");
    CoreEvent::Protocol(roundtripped)
}

fn fake_running_tool() -> ToolExecution {
    ToolExecution {
        call_id: "stale-call".into(),
        name: "Read".into(),
        status: ToolStatus::Running,
        started_at: std::time::Instant::now(),
        completed_at: None,
        description: None,
        streaming_input: None,
        // Stamped to an arbitrary UUID — SessionResetForResume wipes
        // every execution regardless of anchor, so the value is
        // immaterial for this test.
        message_uuid: Some(uuid::Uuid::nil()),
    }
}

#[test]
fn resume_clears_prior_overlays_then_rebuilds_transcript() {
    let mut state = AppState::new();

    // ── Seed prior-session state ────────────────────────────────────
    let stale_msg = create_user_message("stale prior-session prompt");
    handle_core_event(
        &mut state,
        protocol_evt(ServerNotification::MessageAppended { message: stale_msg }),
    );
    state.session.tool_executions.push(fake_running_tool());
    state.ui.streaming = Some(StreamingState::default());
    state.session.conversation_id = Some("prior-session".into());

    assert_eq!(state.session.transcript.len(), 1, "stale cell seeded");
    assert!(!state.session.tool_executions.is_empty());
    assert!(state.ui.streaming.is_some());

    // ── Emit SessionResetForResume ──────────────────────────────────
    handle_core_event(
        &mut state,
        protocol_evt(ServerNotification::SessionResetForResume {
            session_id: "resumed-001".into(),
        }),
    );

    assert!(
        state.session.transcript.is_empty(),
        "reset wipes derived transcript"
    );
    assert!(
        state.session.tool_executions.is_empty(),
        "reset wipes tool overlays"
    );
    assert!(
        state.ui.streaming.is_none(),
        "reset wipes streaming overlay"
    );
    assert_eq!(
        state.session.conversation_id.as_deref(),
        Some("resumed-001"),
        "conversation_id rotates to the new session"
    );

    // ── Replay history through MessageAppended ──────────────────────
    let m1 = create_user_message("first resumed prompt");
    let m1_uuid = *m1.uuid().expect("user message carries uuid");
    let m2 = create_user_interruption_system_message(true);
    let m2_uuid = *m2.uuid().expect("system interruption carries uuid");

    handle_core_event(
        &mut state,
        protocol_evt(ServerNotification::MessageAppended { message: m1 }),
    );
    handle_core_event(
        &mut state,
        protocol_evt(ServerNotification::MessageAppended { message: m2 }),
    );

    let cells = state.session.transcript.cells();
    assert_eq!(cells.len(), 2, "two replayed messages → two cells");
    assert_eq!(cells[0].message_uuid, m1_uuid);
    assert!(
        matches!(cells[0].kind, CellKind::UserText { .. }),
        "first replay is user text, got {:?}",
        cells[0].kind
    );

    assert_eq!(cells[1].message_uuid, m2_uuid);
    let CellKind::System(SystemCellKind::UserInterruption { for_tool_use }) = cells[1].kind else {
        panic!(
            "second replay must be System(UserInterruption), got {:?}",
            cells[1].kind
        );
    };
    assert!(
        for_tool_use,
        "for_tool_use must survive the JSON roundtrip untouched"
    );
}

#[test]
fn resume_with_no_prior_state_still_sets_conversation_id() {
    let mut state = AppState::new();
    assert!(state.session.transcript.is_empty());

    handle_core_event(
        &mut state,
        protocol_evt(ServerNotification::SessionResetForResume {
            session_id: "fresh-resume".into(),
        }),
    );

    assert!(state.session.transcript.is_empty());
    assert_eq!(
        state.session.conversation_id.as_deref(),
        Some("fresh-resume"),
        "id rotation fires even when there is nothing to clear"
    );
}
