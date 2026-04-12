use super::*;
use crate::swarm::TeammateIdentity;
use crate::swarm_constants::AgentColorName;

fn make_identity() -> TeammateIdentity {
    TeammateIdentity {
        agent_id: "worker@test".into(),
        agent_name: "worker".into(),
        team_name: "test".into(),
        color: Some(AgentColorName::Blue),
        plan_mode_required: false,
    }
}

#[test]
fn test_new_task_state() {
    let state =
        InProcessTeammateTaskState::new("task-1".into(), make_identity(), "Do research".into());
    assert_eq!(state.task_type, "in_process_teammate");
    assert_eq!(state.task_id, "task-1");
    assert_eq!(state.identity.agent_name, "worker");
    assert_eq!(state.prompt, "Do research");
    assert!(!state.is_idle);
    assert!(!state.shutdown_requested);
    assert_eq!(state.turn_count, 0);
    assert!(state.messages.is_empty());
}

#[test]
fn test_append_message_within_cap() {
    let mut state =
        InProcessTeammateTaskState::new("task-1".into(), make_identity(), "test".into());
    for i in 0..10 {
        state.append_message(TaskMessage {
            role: "assistant".into(),
            content: format!("msg {i}"),
            tool_name: None,
        });
    }
    assert_eq!(state.messages.len(), 10);
}

#[test]
fn test_append_message_caps_at_limit() {
    let mut state =
        InProcessTeammateTaskState::new("task-1".into(), make_identity(), "test".into());
    for i in 0..60 {
        state.append_message(TaskMessage {
            role: "assistant".into(),
            content: format!("msg {i}"),
            tool_name: None,
        });
    }
    assert_eq!(state.messages.len(), TEAMMATE_MESSAGES_UI_CAP);
    // First message should be msg 10 (oldest 10 were dropped)
    assert_eq!(state.messages[0].content, "msg 10");
}

#[test]
fn test_total_tokens() {
    let mut state =
        InProcessTeammateTaskState::new("task-1".into(), make_identity(), "test".into());
    state.input_tokens = 1000;
    state.output_tokens = 500;
    assert_eq!(state.total_tokens(), 1500);
}

#[test]
fn test_elapsed_ms() {
    let state = InProcessTeammateTaskState::new("task-1".into(), make_identity(), "test".into());
    // Should be very small since we just created it
    assert!(state.elapsed_ms() < 1000);
}

#[test]
fn test_task_state_serde() {
    let state = InProcessTeammateTaskState::new("task-1".into(), make_identity(), "test".into());
    let json = serde_json::to_string(&state).unwrap();
    let parsed: InProcessTeammateTaskState = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.task_id, "task-1");
    assert_eq!(parsed.identity.agent_name, "worker");
}

#[test]
fn test_teammate_messages_ui_cap() {
    assert_eq!(TEAMMATE_MESSAGES_UI_CAP, 50);
}
