use std::collections::BTreeMap;

use crate::types::*;

#[test]
fn message_type_serde_round_trip() {
    let types = [
        MessageType::Message,
        MessageType::Broadcast,
        MessageType::ShutdownRequest,
        MessageType::ShutdownResponse,
        MessageType::PlanApprovalResponse,
        MessageType::IdleNotification,
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
