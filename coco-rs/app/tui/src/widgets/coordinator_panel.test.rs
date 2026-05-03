use super::*;

#[test]
fn test_format_elapsed() {
    assert_eq!(format_elapsed(5_000), "5s");
    assert_eq!(format_elapsed(65_000), "1:05");
    assert_eq!(format_elapsed(3_600_000), "60:00");
}

#[test]
fn test_format_tokens() {
    assert_eq!(format_tokens(500), "500");
    assert_eq!(format_tokens(2500), "2.5k");
}

#[test]
fn test_coordinator_task_from_subagent_running() {
    use crate::state::{SubagentInstance, SubagentStatus};
    let instance = SubagentInstance {
        agent_id: "agent-7af2".into(),
        agent_type: "Explore".into(),
        description: "Find auth code".into(),
        status: SubagentStatus::Running,
        color: Some("blue".into()),
    };
    let task = CoordinatorTask::from_subagent(&instance);
    assert_eq!(task.task_id, "agent-7af2");
    assert_eq!(task.description, "Find auth code");
    assert!(task.is_running, "Running maps to is_running=true");
    assert_eq!(task.elapsed_ms, 0);
    assert_eq!(task.token_count, 0);
    assert_eq!(task.queued_messages, 0);
}

#[test]
fn test_coordinator_task_from_subagent_completed_not_running() {
    use crate::state::{SubagentInstance, SubagentStatus};
    let instance = SubagentInstance {
        agent_id: "agent-1".into(),
        agent_type: "Plan".into(),
        description: "Plan refactor".into(),
        status: SubagentStatus::Completed,
        color: None,
    };
    assert!(
        !CoordinatorTask::from_subagent(&instance).is_running,
        "Completed must not register as is_running"
    );
}

#[test]
fn test_coordinator_task_from_subagent_failed_or_backgrounded_not_running() {
    use crate::state::{SubagentInstance, SubagentStatus};
    for status in [SubagentStatus::Failed, SubagentStatus::Backgrounded] {
        let instance = SubagentInstance {
            agent_id: "agent-x".into(),
            agent_type: "general-purpose".into(),
            description: "doing stuff".into(),
            status,
            color: None,
        };
        assert!(
            !CoordinatorTask::from_subagent(&instance).is_running,
            "{status:?} must not register as is_running"
        );
    }
}
