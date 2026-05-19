//! Cross-layer regression guard for `/clear` (D1 fix).
//!
//! Pre-fix: `SessionRuntime::clear_conversation` cleared engine
//! `MessageHistory` but emitted no `ServerNotification` event. The
//! TUI's `TranscriptView` kept stale cells from the cleared session;
//! SDK NDJSON observers never saw the clear. Visible bug: after
//! `/clear`, the cleared transcript stayed on screen and the next
//! turn appeared interleaved with it.
//!
//! Post-fix: full-scope `/clear` (Conversation / All) emits
//! `SessionResetForResume { session_id: new }`; lighter `/clear
//! history` emits `MessageTruncated { keep_count: 0 }`. Both events
//! drive the same TUI teardown path (`TranscriptView::on_session_reset`
//! resp. `on_message_truncated`); SDK observers see them on the wire.
//!
//! This test exercises the TUI-side reactions to both events — the
//! `tui_runner::run_clear_conversation` emit happens in the CLI
//! layer (not unit-testable from `app/tui`), but its product is one
//! of these two wire shapes, and both are JSON-roundtripped here to
//! prove they carry the right payload.

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

/// JSON-roundtrip a `Protocol(ServerNotification)` event.
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
        message_uuid: Some(uuid::Uuid::nil()),
    }
}

/// Full-scope `/clear` (rotates session_id) — emits
/// `SessionResetForResume`. TUI must wipe transcript + overlays and
/// rotate `conversation_id`.
#[tokio::test]
async fn clear_full_emits_session_reset_and_wipes_state() {
    let mut state = AppState::new();

    // Seed prior-session state.
    for i in 0..3 {
        let m = create_user_message(&format!("prior {i}"));
        handle_core_event(
            &mut state,
            protocol_evt(ServerNotification::MessageAppended { message: m }),
        );
    }
    state.session.tool_executions.push(fake_running_tool("c1"));
    state.ui.streaming = Some(StreamingState::default());
    state.session.conversation_id = Some("pre-clear-session".into());
    assert_eq!(state.session.transcript.len(), 3, "prior cells seeded");

    // tui_runner emits this after runtime.clear_conversation rotates
    // the session id and clears the engine history.
    let (tx, mut rx) = mpsc::channel::<CoreEvent>(4);
    tx.send(protocol_evt(ServerNotification::SessionResetForResume {
        session_id: "post-clear-session".into(),
    }))
    .await
    .expect("channel accepts the event");

    let observed = rx.recv().await.expect("SDK observer receives event");
    let CoreEvent::Protocol(ServerNotification::SessionResetForResume { session_id }) = &observed
    else {
        panic!("expected Protocol(SessionResetForResume), got {observed:?}");
    };
    assert_eq!(session_id, "post-clear-session");

    handle_core_event(&mut state, roundtrip(observed));

    assert!(
        state.session.transcript.is_empty(),
        "transcript wiped after SessionResetForResume"
    );
    assert!(
        state.session.tool_executions.is_empty(),
        "tool_executions wiped (no anchor survives a session reset)"
    );
    assert!(state.ui.streaming.is_none(), "streaming overlay wiped");
    assert_eq!(
        state.session.conversation_id.as_deref(),
        Some("post-clear-session"),
        "conversation_id rotates to the new session"
    );
}

/// `/clear history` (Rust-only lighter scope) — emits
/// `MessageTruncated { keep_count: 0 }`. Same TUI effect on cell list;
/// `conversation_id` is NOT rotated.
#[tokio::test]
async fn clear_history_scope_emits_truncate_to_zero() {
    let mut state = AppState::new();
    for i in 0..2 {
        let m = create_user_message(&format!("m{i}"));
        handle_core_event(
            &mut state,
            protocol_evt(ServerNotification::MessageAppended { message: m }),
        );
    }
    let pre_clear_conv_id = state.session.conversation_id.clone();
    assert_eq!(state.session.transcript.len(), 2);

    handle_core_event(
        &mut state,
        roundtrip(protocol_evt(ServerNotification::MessageTruncated {
            keep_count: 0,
        })),
    );

    assert!(
        state.session.transcript.is_empty(),
        "transcript wiped on truncate-to-zero"
    );
    assert_eq!(
        state.session.conversation_id, pre_clear_conv_id,
        "conversation_id NOT rotated for /clear history scope"
    );
}
