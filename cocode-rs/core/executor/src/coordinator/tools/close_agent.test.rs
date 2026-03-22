use super::*;

#[test]
fn test_close_agent_request_defaults() {
    let json = r#"{"agent_id":"agent-abc"}"#;
    let req: CloseAgentRequest = serde_json::from_str(json).expect("deserialize");
    assert_eq!(req.agent_id, "agent-abc");
    assert!(!req.force);
}

#[test]
fn test_close_agent_request_force() {
    let req = CloseAgentRequest {
        agent_id: "agent-xyz".to_string(),
        force: true,
    };
    let json = serde_json::to_string(&req).expect("serialize");
    let back: CloseAgentRequest = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(back.agent_id, "agent-xyz");
    assert!(back.force);
}
