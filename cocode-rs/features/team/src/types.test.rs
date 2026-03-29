use std::collections::BTreeMap;

use crate::types::*;

#[test]
fn message_type_serde_round_trip() {
    let types = [
        MessageType::Message,
        MessageType::Broadcast,
        MessageType::ShutdownRequest,
        MessageType::ShutdownResponse,
        MessageType::PlanApprovalRequest,
        MessageType::PlanApprovalResponse,
        MessageType::IdleNotification,
        MessageType::SandboxPermissionRequest,
        MessageType::SandboxPermissionResponse,
    ];
    for mt in types {
        let json = serde_json::to_string(&mt).unwrap();
        let parsed: MessageType = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, mt);
    }
}

#[test]
fn message_type_as_str() {
    assert_eq!(MessageType::Message.as_str(), "message");
    assert_eq!(MessageType::ShutdownRequest.as_str(), "shutdown_request");
}

#[test]
fn member_status_display() {
    assert_eq!(MemberStatus::Active.to_string(), "active");
    assert_eq!(MemberStatus::ShuttingDown.to_string(), "shutting_down");
}

#[test]
fn agent_message_new() {
    let msg = AgentMessage::new("agent-a", "agent-b", "hello", MessageType::Message);
    assert!(!msg.id.is_empty());
    assert_eq!(msg.from, "agent-a");
    assert_eq!(msg.to, "agent-b");
    assert_eq!(msg.content, "hello");
    assert!(!msg.read);
    assert!(msg.team_name.is_none());
}

#[test]
fn agent_message_with_team() {
    let msg = AgentMessage::new("a", "b", "hi", MessageType::Broadcast).with_team("my-team");
    assert_eq!(msg.team_name.as_deref(), Some("my-team"));
}

#[test]
fn team_has_member_by_id() {
    let team = Team {
        name: "test".into(),
        description: None,
        agent_type: None,
        leader_agent_id: None,
        members: vec![TeamMember {
            agent_id: "a1".into(),
            name: Some("alice".into()),
            agent_type: None,
            model: None,
            joined_at: 0,
            cwd: None,
            status: MemberStatus::Active,
            background: false,
        }],
        created_at: 0,
    };
    assert!(team.has_member("a1"));
    assert!(team.has_member("alice"));
    assert!(!team.has_member("bob"));
}

#[test]
fn team_active_non_leader_members() {
    let team = Team {
        name: "test".into(),
        description: None,
        agent_type: None,
        leader_agent_id: Some("lead".into()),
        members: vec![
            TeamMember {
                agent_id: "lead".into(),
                name: None,
                agent_type: None,
                model: None,
                joined_at: 0,
                cwd: None,
                status: MemberStatus::Active,
                background: false,
            },
            TeamMember {
                agent_id: "worker".into(),
                name: None,
                agent_type: None,
                model: None,
                joined_at: 0,
                cwd: None,
                status: MemberStatus::Active,
                background: false,
            },
            TeamMember {
                agent_id: "stopped".into(),
                name: None,
                agent_type: None,
                model: None,
                joined_at: 0,
                cwd: None,
                status: MemberStatus::Stopped,
                background: false,
            },
        ],
        created_at: 0,
    };
    let active = team.active_non_leader_members();
    assert_eq!(active.len(), 1);
    assert_eq!(active[0].agent_id, "worker");
}

#[test]
fn format_team_summary_empty() {
    let teams = BTreeMap::new();
    assert_eq!(format_team_summary(&teams), "No teams.");
}

#[test]
fn format_team_summary_with_team() {
    let mut teams = BTreeMap::new();
    teams.insert(
        "t1".into(),
        Team {
            name: "t1".into(),
            description: Some("Test team".into()),
            agent_type: None,
            leader_agent_id: None,
            members: vec![TeamMember {
                agent_id: "a1".into(),
                name: Some("alice".into()),
                agent_type: None,
                model: None,
                joined_at: 0,
                cwd: None,
                status: MemberStatus::Active,
                background: false,
            }],
            created_at: 0,
        },
    );
    let summary = format_team_summary(&teams);
    assert!(summary.contains("Team: t1"));
    assert!(summary.contains("alice"));
    assert!(summary.contains("[active]"));
}

#[test]
fn sandbox_permission_request_round_trip() {
    let req = SandboxPermissionRequest::new(
        "npm install",
        SandboxRestrictionKind::Network,
        "registry.npmjs.org",
    )
    .with_worker_name("builder-1")
    .with_worker_color("#00ff00");
    assert!(req.request_id.starts_with("sandbox-"));
    assert!(req.created_at > 0);
    assert_eq!(req.worker_name.as_deref(), Some("builder-1"));

    let msg = req.clone().into_message("worker-1", "leader").unwrap();
    assert_eq!(msg.message_type, MessageType::SandboxPermissionRequest);
    assert_eq!(msg.from, "worker-1");
    assert_eq!(msg.to, "leader");

    let parsed = SandboxPermissionRequest::from_message(&msg).unwrap();
    assert_eq!(parsed.request_id, req.request_id);
    assert_eq!(parsed.command, "npm install");
    assert_eq!(parsed.restriction_kind, SandboxRestrictionKind::Network);
    assert_eq!(parsed.detail, "registry.npmjs.org");
    assert_eq!(parsed.worker_name.as_deref(), Some("builder-1"));
    assert_eq!(parsed.worker_color.as_deref(), Some("#00ff00"));
    assert_eq!(parsed.created_at, req.created_at);
}

#[test]
fn sandbox_permission_request_optional_fields_default() {
    let req = SandboxPermissionRequest::new("ls", SandboxRestrictionKind::Filesystem, "/etc");
    assert!(req.worker_name.is_none());
    assert!(req.worker_color.is_none());

    let msg = req.into_message("w", "l").unwrap();
    let parsed = SandboxPermissionRequest::from_message(&msg).unwrap();
    assert!(parsed.worker_name.is_none());
    assert!(parsed.worker_color.is_none());
}

#[test]
fn sandbox_permission_response_round_trip() {
    let resp = SandboxPermissionResponse {
        request_id: "sandbox-123-abcd".to_string(),
        approved: true,
        detail: Some("registry.npmjs.org".to_string()),
        timestamp: 1711540800000,
    };

    let msg = resp.into_message("leader", "worker-1").unwrap();
    assert_eq!(msg.message_type, MessageType::SandboxPermissionResponse);

    let parsed = SandboxPermissionResponse::from_message(&msg).unwrap();
    assert_eq!(parsed.request_id, "sandbox-123-abcd");
    assert!(parsed.approved);
    assert_eq!(parsed.detail.as_deref(), Some("registry.npmjs.org"));
    assert_eq!(parsed.timestamp, 1711540800000);
}

#[test]
fn sandbox_restriction_kind_serde_round_trip() {
    let kinds = [
        SandboxRestrictionKind::Network,
        SandboxRestrictionKind::Filesystem,
        SandboxRestrictionKind::UnixSocket,
    ];
    for kind in kinds {
        let json = serde_json::to_string(&kind).unwrap();
        let parsed: SandboxRestrictionKind = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, kind);
    }
}

#[test]
fn sandbox_permission_from_wrong_type_returns_none() {
    let msg = AgentMessage::new("a", "b", "{}", MessageType::Message);
    assert!(SandboxPermissionRequest::from_message(&msg).is_none());
    assert!(SandboxPermissionResponse::from_message(&msg).is_none());
}

#[test]
fn team_serde_round_trip() {
    let team = Team {
        name: "test".into(),
        description: Some("A test team".into()),
        agent_type: Some("general".into()),
        leader_agent_id: Some("lead".into()),
        members: vec![TeamMember {
            agent_id: "a1".into(),
            name: Some("alice".into()),
            agent_type: Some("explore".into()),
            model: Some("haiku".into()),
            joined_at: 1000,
            cwd: Some("/tmp".into()),
            status: MemberStatus::Active,
            background: false,
        }],
        created_at: 999,
    };
    let json = serde_json::to_string(&team).unwrap();
    let parsed: Team = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.name, "test");
    assert_eq!(parsed.members.len(), 1);
    assert_eq!(parsed.members[0].agent_id, "a1");
}
