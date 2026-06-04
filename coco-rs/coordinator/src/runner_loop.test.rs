//! Smoke tests for `runner_loop` after the task-storage unification.
//!
//! The pre-refactor suite exercised the deleted `InProcessTeammateTaskState`
//! mirror — every assertion read `state.is_idle`, `state.messages`,
//! `state.current_work_cancel` from the parallel store the
//! task-storage refactor removed. Those tests are no longer meaningful
//! against the unified `TaskManager`-only model; their replacements
//! live in `tasks/running.test.rs` (canonical row + control-handle
//! sibling map) and `agent_handle/mod.test.rs` (teammate dispatch).
//!
//! This file keeps the no-arg helpers (`AgentQueryConfig::default`,
//! `WaitResult` shape checks) as compile-time tripwires so accidental
//! API changes there fail loudly.

use super::*;

#[test]
fn agent_query_config_default_is_constructible() {
    let cfg = AgentQueryConfig::default();
    assert!(cfg.system_prompt.is_empty());
    assert!(cfg.allowed_tools.is_empty());
    assert!(cfg.disallowed_tools.is_empty());
    assert!(cfg.fork_context_messages.is_empty());
    assert!(cfg.cancel.is_none());
}

#[test]
fn wait_result_aborted_is_constructible() {
    let r = WaitResult::Aborted;
    assert!(matches!(r, WaitResult::Aborted));
}

// ── select_mailbox_prompt: priority + filter (pure, no I/O) ──

fn msg(from: &str, text: &str, read: bool) -> mailbox::TeammateMessage {
    mailbox::TeammateMessage {
        from: from.to_string(),
        text: text.to_string(),
        timestamp: "2026-06-04T00:00:00Z".to_string(),
        read,
        color: None,
        summary: None,
    }
}

fn shutdown_text() -> String {
    serde_json::to_string(&mailbox::ProtocolMessage::ShutdownRequest {
        request_id: "shutdown-1".to_string(),
        from: TEAM_LEAD_NAME.to_string(),
        reason: None,
        timestamp: "2026-06-04T00:00:00Z".to_string(),
    })
    .unwrap()
}

fn mode_set_text() -> String {
    serde_json::to_string(&mailbox::ProtocolMessage::ModeSetRequest {
        mode: coco_types::PermissionMode::Plan,
        from: TEAM_LEAD_NAME.to_string(),
    })
    .unwrap()
}

fn plan_approval_response_text() -> String {
    serde_json::to_string(&mailbox::ProtocolMessage::PlanApprovalResponse {
        request_id: "plan-1".to_string(),
        approved: true,
        feedback: None,
        timestamp: String::new(),
        permission_mode: None,
    })
    .unwrap()
}

#[test]
fn select_mailbox_prompt_shutdown_outranks_text() {
    let messages = vec![
        msg(TEAM_LEAD_NAME, "do the thing", false),
        msg(TEAM_LEAD_NAME, &shutdown_text(), false),
        msg("researcher", "peer note", false),
    ];
    let (idx, result) = select_mailbox_prompt(&messages).expect("a prompt");
    assert_eq!(idx, 1);
    assert!(matches!(result, WaitResult::ShutdownRequest { .. }));
}

#[test]
fn select_mailbox_prompt_team_lead_outranks_peer_regardless_of_order() {
    // Peer message appears first, but the team-lead arm wins.
    let messages = vec![
        msg("researcher", "peer note", false),
        msg(TEAM_LEAD_NAME, "leader task", false),
    ];
    let (idx, result) = select_mailbox_prompt(&messages).expect("a prompt");
    assert_eq!(idx, 1);
    match result {
        WaitResult::NewMessage { message, from, .. } => {
            assert_eq!(from, TEAM_LEAD_NAME);
            assert_eq!(message, "leader task");
        }
        other => panic!("expected NewMessage, got {other:?}"),
    }
}

#[test]
fn select_mailbox_prompt_peer_is_fifo() {
    let messages = vec![msg("alice", "first", false), msg("bob", "second", false)];
    let (idx, result) = select_mailbox_prompt(&messages).expect("a prompt");
    assert_eq!(idx, 0);
    match result {
        WaitResult::NewMessage { from, message, .. } => {
            assert_eq!(from, "alice");
            assert_eq!(message, "first");
        }
        other => panic!("expected NewMessage, got {other:?}"),
    }
}

#[test]
fn select_mailbox_prompt_skips_structured_responses() {
    // A control message + a response message in the teammate's own inbox must
    // NOT be injected as prompts (gap-1 mis-injection guard). Neither is a
    // ShutdownRequest, so the scan yields nothing.
    let messages = vec![
        msg(TEAM_LEAD_NAME, &mode_set_text(), false),
        msg(TEAM_LEAD_NAME, &plan_approval_response_text(), false),
    ];
    assert!(select_mailbox_prompt(&messages).is_none());
}

#[test]
fn select_mailbox_prompt_skips_read_and_empty() {
    assert!(select_mailbox_prompt(&[]).is_none());
    let messages = vec![msg(TEAM_LEAD_NAME, "already handled", true)];
    assert!(select_mailbox_prompt(&messages).is_none());
}

#[test]
fn select_mailbox_prompt_plain_text_alongside_structured_picks_text() {
    // A real leader prompt arriving after a response message is still found.
    let messages = vec![
        msg(TEAM_LEAD_NAME, &plan_approval_response_text(), false),
        msg(TEAM_LEAD_NAME, "now do step 2", false),
    ];
    let (idx, result) = select_mailbox_prompt(&messages).expect("a prompt");
    assert_eq!(idx, 1);
    assert!(matches!(result, WaitResult::NewMessage { .. }));
}
