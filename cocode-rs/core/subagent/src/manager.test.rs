use super::*;

fn test_definition(name: &str) -> AgentDefinition {
    AgentDefinition {
        name: name.to_string(),
        description: format!("{name} agent"),
        agent_type: name.to_string(),
        tools: vec![],
        disallowed_tools: vec![],
        identity: None,
        max_turns: None,
        permission_mode: None,
    }
}

#[test]
fn test_new_manager() {
    let mgr = SubagentManager::new();
    assert!(mgr.agents.is_empty());
    assert!(mgr.definitions.is_empty());
}

#[test]
fn test_register_agent_type() {
    let mut mgr = SubagentManager::new();
    mgr.register_agent_type(test_definition("bash"));
    assert_eq!(mgr.definitions.len(), 1);
    assert_eq!(mgr.definitions[0].agent_type, "bash");
}

#[tokio::test]
async fn test_spawn_agent() {
    let mut mgr = SubagentManager::new();
    mgr.register_agent_type(test_definition("bash"));

    let id = mgr.spawn("bash", "run ls").await.expect("spawn");
    assert!(!id.is_empty());
    // Without an execute_fn, the stub completes immediately
    assert_eq!(mgr.get_status(&id), Some(AgentStatus::Completed));
}

#[tokio::test]
async fn test_spawn_unknown_type() {
    let mut mgr = SubagentManager::new();
    let result = mgr.spawn("nonexistent", "test").await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_spawn_full_with_stub() {
    let mut mgr = SubagentManager::new();
    mgr.register_agent_type(test_definition("bash"));

    let input = SpawnInput {
        agent_type: "bash".to_string(),
        prompt: "test".to_string(),
        identity: None,
        max_turns: None,
        run_in_background: false,
        allowed_tools: None,
        resume_from: None,
    };

    let result = mgr.spawn_full(input).await.expect("spawn_full");
    assert!(!result.agent_id.is_empty());
    assert!(result.output.is_some()); // Stub returns output
    assert!(result.background.is_none());
}

#[tokio::test]
async fn test_spawn_full_background() {
    let mut mgr = SubagentManager::new();
    mgr.register_agent_type(test_definition("bash"));

    let input = SpawnInput {
        agent_type: "bash".to_string(),
        prompt: "test".to_string(),
        identity: None,
        max_turns: None,
        run_in_background: true,
        allowed_tools: None,
        resume_from: None,
    };

    let result = mgr.spawn_full(input).await.expect("spawn_full");
    assert!(!result.agent_id.is_empty());
    assert!(result.output.is_none()); // Background has no immediate output
    assert!(result.background.is_some());
    assert_eq!(
        mgr.get_status(&result.agent_id),
        Some(AgentStatus::Backgrounded)
    );
}

#[tokio::test]
async fn test_resume_non_backgrounded() {
    let mut mgr = SubagentManager::new();
    mgr.register_agent_type(test_definition("bash"));
    let id = mgr.spawn("bash", "test").await.expect("spawn");
    let result = mgr.resume(&id).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_resume_backgrounded() {
    let mut mgr = SubagentManager::new();
    mgr.register_agent_type(test_definition("bash"));
    let id = mgr.spawn("bash", "test").await.expect("spawn");

    // Manually set to backgrounded for test.
    mgr.agents.get_mut(&id).expect("agent").status = AgentStatus::Backgrounded;

    let resumed_id = mgr.resume(&id).await.expect("resume");
    assert_eq!(resumed_id, id);
    assert_eq!(mgr.get_status(&id), Some(AgentStatus::Running));
}

#[test]
fn test_get_status_missing() {
    let mgr = SubagentManager::new();
    assert!(mgr.get_status("nonexistent").is_none());
}
