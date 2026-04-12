use coco_types::PermissionMode;
use tokio::sync::mpsc;

use super::*;

// ── TeammateIdentity ──

#[test]
fn test_teammate_identity_serde_roundtrip() {
    let identity = TeammateIdentity {
        agent_id: "researcher@my-team".into(),
        agent_name: "researcher".into(),
        team_name: "my-team".into(),
        color: Some(AgentColorName::Blue),
        plan_mode_required: false,
    };
    let json = serde_json::to_string(&identity).expect("serialize");
    let parsed: TeammateIdentity = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(parsed.agent_id, "researcher@my-team");
    assert_eq!(parsed.color, Some(AgentColorName::Blue));
}

// ── AgentSpawnResult ──

#[test]
fn test_agent_spawn_result_success() {
    let result = AgentSpawnResult::success("id-1".into(), "worker-1".into());
    assert_eq!(result.status, SubAgentStatus::Running);
    assert!(result.error.is_none());
}

#[test]
fn test_agent_spawn_result_failure() {
    let result = AgentSpawnResult::failure("id-2".into(), "worker-2".into(), "timeout".into());
    assert_eq!(result.status, SubAgentStatus::Failed);
    assert_eq!(result.error.as_deref(), Some("timeout"));
}

// ── PermissionSyncBridge ──

#[tokio::test]
async fn test_permission_sync_bridge_request_resolve() {
    let (tx, mut rx) = mpsc::channel::<SwarmPermissionRequest>(16);
    let bridge = PermissionSyncBridge::new(tx);

    let request = SwarmPermissionRequest {
        id: "req-1".into(),
        worker_id: "worker-1".into(),
        worker_name: "researcher".into(),
        worker_color: None,
        team_name: "team-a".into(),
        tool_name: "Bash".into(),
        tool_use_id: "tu-1".into(),
        description: "run tests".into(),
        input: serde_json::json!({"command": "cargo test"}),
        status: PermissionRequestStatus::Pending,
        resolved_by: None,
        resolved_at: None,
        feedback: None,
        created_at: 1000,
    };

    let bridge_clone = Arc::new(bridge);
    let bridge_for_task = Arc::clone(&bridge_clone);

    // Spawn a task that requests permission
    let handle = tokio::spawn(async move { bridge_for_task.request_permission(request).await });

    // Leader side: receive and resolve
    let received = rx.recv().await.expect("should receive request");
    assert_eq!(received.id, "req-1");
    assert_eq!(received.tool_name, "Bash");

    let resolved = bridge_clone
        .resolve_permission(
            "req-1",
            PermissionResolution {
                decision: PermissionRequestStatus::Approved,
                resolved_by: PermissionResolver::Leader,
                feedback: None,
                updated_input: None,
            },
        )
        .await;
    assert!(resolved, "should find and resolve the pending request");

    let result = handle.await.expect("task should complete");
    let resolution = result.expect("should get resolution");
    assert_eq!(resolution.decision, PermissionRequestStatus::Approved);
}

#[tokio::test]
async fn test_permission_bridge_pending_count() {
    let (tx, _rx) = mpsc::channel::<SwarmPermissionRequest>(16);
    let bridge = PermissionSyncBridge::new(tx);
    assert_eq!(bridge.pending_count().await, 0);
}

#[tokio::test]
async fn test_permission_bridge_resolve_unknown_id() {
    let (tx, _rx) = mpsc::channel::<SwarmPermissionRequest>(16);
    let bridge = PermissionSyncBridge::new(tx);
    let result = bridge
        .resolve_permission(
            "nonexistent",
            PermissionResolution {
                decision: PermissionRequestStatus::Rejected,
                resolved_by: PermissionResolver::Leader,
                feedback: Some("denied".into()),
                updated_input: None,
            },
        )
        .await;
    assert!(!result, "resolving unknown ID should return false");
}

// ── TeamManager ──

#[tokio::test]
async fn test_team_manager_register_and_remove() {
    let team_file = TeamFile {
        name: "test-team".into(),
        description: None,
        created_at: 1000,
        lead_agent_id: "lead-1".into(),
        lead_session_id: None,
        hidden_pane_ids: Vec::new(),
        team_allowed_paths: Vec::new(),
        members: vec![TeamMember {
            agent_id: "worker-1".into(),
            name: "worker".into(),
            agent_type: Some("researcher".into()),
            model: None,
            prompt: None,
            color: None,
            plan_mode_required: false,
            joined_at: 1000,
            tmux_pane_id: String::new(),
            cwd: "/project".into(),
            worktree_path: None,
            session_id: None,
            subscriptions: Vec::new(),
            backend_type: None,
            is_active: true,
            mode: None,
        }],
    };

    let manager = TeamManager::new("test-team".into(), team_file);

    // Register a running agent
    manager
        .register_agent(SubAgentState {
            agent_id: "worker-1".into(),
            name: "worker".into(),
            status: SubAgentStatus::Running,
            turns: 0,
            model: None,
            working_dir: Some("/project".into()),
            last_message: None,
        })
        .await;

    let running = manager.running_agents().await;
    assert_eq!(running.len(), 1);

    // Remove the member
    assert!(manager.remove_member("worker-1").await);
    assert_eq!(manager.member_count().await, 0);
}

#[tokio::test]
async fn test_team_manager_set_member_mode() {
    let team_file = TeamFile {
        name: "mode-test".into(),
        description: None,
        created_at: 1000,
        lead_agent_id: "lead-1".into(),
        lead_session_id: None,
        hidden_pane_ids: Vec::new(),
        team_allowed_paths: Vec::new(),
        members: vec![TeamMember {
            agent_id: "w1".into(),
            name: "alice".into(),
            agent_type: None,
            model: None,
            prompt: None,
            color: None,
            plan_mode_required: false,
            joined_at: 1000,
            tmux_pane_id: String::new(),
            cwd: "/tmp".into(),
            worktree_path: None,
            session_id: None,
            subscriptions: Vec::new(),
            backend_type: None,
            is_active: true,
            mode: None,
        }],
    };

    let manager = TeamManager::new("mode-test".into(), team_file);

    assert!(
        manager
            .set_member_mode("alice", PermissionMode::AcceptEdits)
            .await
    );
    let tf = manager.team_file().await;
    assert_eq!(tf.members[0].mode, Some(PermissionMode::AcceptEdits));

    // Non-existent member
    assert!(!manager.set_member_mode("bob", PermissionMode::Plan).await);
}

#[tokio::test]
async fn test_team_manager_mailbox() {
    let team_file = TeamFile {
        name: "mail-test".into(),
        description: None,
        created_at: 1000,
        lead_agent_id: "lead-1".into(),
        lead_session_id: None,
        hidden_pane_ids: Vec::new(),
        team_allowed_paths: Vec::new(),
        members: vec![],
    };

    let manager = TeamManager::new("mail-test".into(), team_file);

    // Empty mailbox
    let msgs = manager.read_mailbox("alice").await;
    assert!(msgs.is_empty());

    // Send a message
    let msg = crate::AgentMessage {
        from_agent: "bob".into(),
        to_agent: "alice".into(),
        content: AgentMessageContent::Text {
            text: "hello".into(),
        },
        timestamp: 1000,
    };
    manager.send_message("alice", msg).await;

    // Read drains the mailbox
    let msgs = manager.read_mailbox("alice").await;
    assert_eq!(msgs.len(), 1);
    let msgs = manager.read_mailbox("alice").await;
    assert!(msgs.is_empty());
}

#[tokio::test]
async fn test_team_manager_is_leader() {
    let team_file = TeamFile {
        name: "leader-test".into(),
        description: None,
        created_at: 1000,
        lead_agent_id: "lead-xyz".into(),
        lead_session_id: None,
        hidden_pane_ids: Vec::new(),
        team_allowed_paths: Vec::new(),
        members: vec![],
    };
    let manager = TeamManager::new("leader-test".into(), team_file);
    assert!(manager.is_leader("lead-xyz").await);
    assert!(!manager.is_leader("worker-1").await);
}

// ── Utility functions ──

#[test]
fn test_generate_request_id_format() {
    let id = generate_request_id();
    assert!(
        id.starts_with("perm-"),
        "id should start with 'perm-': {id}"
    );
}

#[test]
fn test_generate_sandbox_request_id_format() {
    let id = generate_sandbox_request_id();
    assert!(
        id.starts_with("sandbox-"),
        "id should start with 'sandbox-': {id}"
    );
}

#[test]
fn test_sanitize_name() {
    assert_eq!(sanitize_name("My Team!"), "my-team-");
    assert_eq!(sanitize_name("test123"), "test123");
    assert_eq!(sanitize_name("a b.c"), "a-b-c");
}

#[test]
fn test_sanitize_agent_name() {
    assert_eq!(sanitize_agent_name("agent@team"), "agent-team");
    assert_eq!(sanitize_agent_name("no-at-sign"), "no-at-sign");
}
