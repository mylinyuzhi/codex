use super::*;

fn create_test_agents() -> Vec<AgentInfo> {
    vec![
        AgentInfo {
            agent_type: "explore".to_string(),
            name: "Explore".to_string(),
            description: "Read-only codebase exploration".to_string(),
        },
        AgentInfo {
            agent_type: "bash".to_string(),
            name: "Bash".to_string(),
            description: "Command execution specialist".to_string(),
        },
        AgentInfo {
            agent_type: "general-purpose".to_string(),
            name: "General Purpose".to_string(),
            description: "General-purpose agent".to_string(),
        },
        AgentInfo {
            agent_type: "plan".to_string(),
            name: "Plan".to_string(),
            description: "Software architect agent".to_string(),
        },
    ]
}

#[test]
fn test_search_agent_prefix_only() {
    let mut manager = AgentSearchManager::new();
    manager.load_agents(create_test_agents().into_iter());

    // "agent" or "agent-" should return all agents
    let results = manager.search("agent");
    assert_eq!(results.len(), 4);

    let results = manager.search("agent-");
    assert_eq!(results.len(), 4);
}

#[test]
fn test_search_exact_match() {
    let mut manager = AgentSearchManager::new();
    manager.load_agents(create_test_agents().into_iter());

    let results = manager.search("agent-explore");
    assert!(!results.is_empty());
    assert_eq!(results[0].agent_type, "explore");
}

#[test]
fn test_search_prefix_match() {
    let mut manager = AgentSearchManager::new();
    manager.load_agents(create_test_agents().into_iter());

    let results = manager.search("agent-exp");
    assert!(!results.is_empty());
    assert_eq!(results[0].agent_type, "explore");
}

#[test]
fn test_search_fuzzy_match() {
    let mut manager = AgentSearchManager::new();
    manager.load_agents(create_test_agents().into_iter());

    let results = manager.search("agent-expl");
    assert!(!results.is_empty());
    assert_eq!(results[0].agent_type, "explore");
}

#[test]
fn test_search_no_match() {
    let mut manager = AgentSearchManager::new();
    manager.load_agents(create_test_agents().into_iter());

    let results = manager.search("agent-xyz");
    assert!(results.is_empty());
}
