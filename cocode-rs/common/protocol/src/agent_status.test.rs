use super::*;

#[test]
fn test_agent_status_default() {
    let status = AgentStatus::default();
    assert!(matches!(status, AgentStatus::Idle));
    assert!(!status.is_busy());
}

#[test]
fn test_agent_status_streaming() {
    let status = AgentStatus::streaming("turn-1");
    assert!(status.is_busy());
    assert!(status.is_streaming());
    assert!(!status.is_executing_tools());
}

#[test]
fn test_agent_status_executing_tools() {
    let status = AgentStatus::executing_tools(3, 1);
    assert!(status.is_busy());
    assert!(status.is_executing_tools());
    assert!(!status.is_streaming());

    if let AgentStatus::ExecutingTools { pending, completed } = status {
        assert_eq!(pending, 3);
        assert_eq!(completed, 1);
    } else {
        panic!("Expected ExecutingTools status");
    }
}

#[test]
fn test_agent_status_waiting_approval() {
    let status = AgentStatus::waiting_approval("req-123");
    assert!(status.is_busy());
    assert!(status.is_waiting_approval());
}

#[test]
fn test_agent_status_error() {
    let status = AgentStatus::error("Something went wrong");
    assert!(status.is_busy());
    assert!(status.is_error());
}

#[test]
fn test_agent_status_display() {
    assert_eq!(AgentStatus::Idle.to_string(), "Idle");
    assert!(
        AgentStatus::streaming("turn-1")
            .to_string()
            .contains("turn-1")
    );
    assert!(
        AgentStatus::executing_tools(3, 1)
            .to_string()
            .contains("1/4 done")
    );
    assert!(AgentStatus::Compacting.to_string().contains("Compacting"));
}

#[test]
fn test_agent_status_serde() {
    let status = AgentStatus::streaming("turn-1");
    let json = serde_json::to_string(&status).unwrap();
    assert!(json.contains("streaming"));
    assert!(json.contains("turn-1"));

    let parsed: AgentStatus = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, status);
}
