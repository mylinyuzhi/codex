use super::*;

#[test]
fn test_current_agent_outside_scope() {
    // Outside any scope, current_agent should return None
    assert!(current_agent().is_none());
}

#[tokio::test]
async fn test_current_agent_inside_scope() {
    let identity = AgentIdentity {
        agent_id: "agent-1".to_string(),
        agent_type: "general-purpose".to_string(),
        parent_agent_id: None,
        depth: 0,
        name: None,
        team_name: None,
        color: None,
        plan_mode_required: false,
    };

    let result = CURRENT_AGENT
        .scope(identity, async {
            let current = current_agent();
            assert!(current.is_some());
            let current = current.unwrap();
            assert_eq!(current.agent_id, "agent-1");
            assert_eq!(current.agent_type, "general-purpose");
            assert!(current.parent_agent_id.is_none());
            assert_eq!(current.depth, 0);
            assert!(current.name.is_none());
            assert!(current.team_name.is_none());
            assert!(current.color.is_none());
            assert!(!current.plan_mode_required);
            42
        })
        .await;

    assert_eq!(result, 42);
}

#[tokio::test]
async fn test_nested_scope() {
    let parent = AgentIdentity {
        agent_id: "parent".to_string(),
        agent_type: "general-purpose".to_string(),
        parent_agent_id: None,
        depth: 0,
        name: None,
        team_name: None,
        color: None,
        plan_mode_required: false,
    };

    CURRENT_AGENT
        .scope(parent, async {
            let outer = current_agent().unwrap();
            assert_eq!(outer.agent_id, "parent");
            assert_eq!(outer.depth, 0);

            let child = AgentIdentity {
                agent_id: "child".to_string(),
                agent_type: "Explore".to_string(),
                parent_agent_id: Some("parent".to_string()),
                depth: 1,
                name: Some("explorer-1".to_string()),
                team_name: None,
                color: Some("cyan".to_string()),
                plan_mode_required: false,
            };

            CURRENT_AGENT
                .scope(child, async {
                    let inner = current_agent().unwrap();
                    assert_eq!(inner.agent_id, "child");
                    assert_eq!(inner.agent_type, "Explore");
                    assert_eq!(inner.parent_agent_id.as_deref(), Some("parent"));
                    assert_eq!(inner.depth, 1);
                    assert_eq!(inner.name.as_deref(), Some("explorer-1"));
                    assert_eq!(inner.color.as_deref(), Some("cyan"));
                })
                .await;

            // After inner scope, parent is restored
            let restored = current_agent().unwrap();
            assert_eq!(restored.agent_id, "parent");
        })
        .await;
}

#[tokio::test]
async fn test_scope_does_not_leak_across_tasks() {
    let identity = AgentIdentity {
        agent_id: "scoped".to_string(),
        agent_type: "test".to_string(),
        parent_agent_id: None,
        depth: 0,
        name: None,
        team_name: None,
        color: None,
        plan_mode_required: false,
    };

    CURRENT_AGENT
        .scope(identity, async {
            // Spawn a new task — task_local does NOT propagate
            let handle = tokio::spawn(async { current_agent() });
            let result = handle.await.unwrap();
            assert!(result.is_none());
        })
        .await;
}

#[tokio::test]
async fn test_identity_with_all_fields() {
    let identity = AgentIdentity {
        agent_id: "full-agent".to_string(),
        agent_type: "general-purpose".to_string(),
        parent_agent_id: Some("root".to_string()),
        depth: 2,
        name: Some("my-agent".to_string()),
        team_name: Some("team-alpha".to_string()),
        color: Some("blue".to_string()),
        plan_mode_required: true,
    };

    CURRENT_AGENT
        .scope(identity, async {
            let current = current_agent().unwrap();
            assert_eq!(current.agent_id, "full-agent");
            assert_eq!(current.agent_type, "general-purpose");
            assert_eq!(current.parent_agent_id.as_deref(), Some("root"));
            assert_eq!(current.depth, 2);
            assert_eq!(current.name.as_deref(), Some("my-agent"));
            assert_eq!(current.team_name.as_deref(), Some("team-alpha"));
            assert_eq!(current.color.as_deref(), Some("blue"));
            assert!(current.plan_mode_required);
        })
        .await;
}
