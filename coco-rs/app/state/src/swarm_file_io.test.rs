use super::*;
use crate::swarm::TeamMember;

fn make_test_team_file() -> TeamFile {
    TeamFile {
        name: "test-team".to_string(),
        description: Some("A test team".to_string()),
        created_at: 1000,
        lead_agent_id: "leader@test-team".to_string(),
        lead_session_id: Some("session-1".to_string()),
        hidden_pane_ids: Vec::new(),
        team_allowed_paths: Vec::new(),
        members: vec![TeamMember {
            agent_id: "worker-1@test-team".to_string(),
            name: "worker-1".to_string(),
            agent_type: None,
            model: None,
            prompt: Some("Do research".to_string()),
            color: Some("blue".to_string()),
            plan_mode_required: false,
            joined_at: 1000,
            tmux_pane_id: String::new(),
            cwd: "/tmp".to_string(),
            worktree_path: None,
            session_id: None,
            subscriptions: Vec::new(),
            backend_type: None,
            is_active: true,
            mode: None,
        }],
    }
}

#[test]
fn test_team_dir_path() {
    let dir = get_team_dir("My Team");
    let path_str = dir.to_string_lossy();
    assert!(path_str.contains("teams"));
    assert!(path_str.contains("my-team"));
}

#[test]
fn test_team_file_path() {
    let path = get_team_file_path("test");
    assert!(path.to_string_lossy().ends_with("config.json"));
}

#[test]
fn test_write_and_read_team_file() {
    let dir = tempfile::tempdir().unwrap();
    let team_dir = dir.path().join("test-team");
    std::fs::create_dir_all(&team_dir).unwrap();

    let tf = make_test_team_file();
    let path = team_dir.join("config.json");
    let content = serde_json::to_string_pretty(&tf).unwrap();
    std::fs::write(&path, &content).unwrap();

    let parsed: TeamFile = serde_json::from_str(&content).unwrap();
    assert_eq!(parsed.name, "test-team");
    assert_eq!(parsed.members.len(), 1);
    assert_eq!(parsed.members[0].name, "worker-1");
    assert_eq!(parsed.members[0].prompt.as_deref(), Some("Do research"));
    assert!(parsed.members[0].is_active);
}

#[test]
fn test_team_file_serde_new_fields() {
    let tf = TeamFile {
        name: "test".to_string(),
        description: None,
        created_at: 1000,
        lead_agent_id: "lead@test".to_string(),
        lead_session_id: None,
        hidden_pane_ids: vec!["pane-1".to_string()],
        team_allowed_paths: vec![super::super::swarm::TeamAllowedPath {
            path: "/src".to_string(),
            tool_name: "Edit".to_string(),
            added_by: "leader".to_string(),
            added_at: 2000,
        }],
        members: Vec::new(),
    };

    let json = serde_json::to_string(&tf).unwrap();
    assert!(json.contains("hidden_pane_ids"));
    assert!(json.contains("team_allowed_paths"));
    assert!(json.contains("pane-1"));

    let parsed: TeamFile = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.hidden_pane_ids, vec!["pane-1"]);
    assert_eq!(parsed.team_allowed_paths.len(), 1);
    assert_eq!(parsed.team_allowed_paths[0].tool_name, "Edit");
}

#[test]
fn test_list_team_names_empty() {
    // Won't find any teams in a temp dir, but validates no panic
    let names = list_team_names();
    // May or may not be empty depending on whether ~/.claude/teams/ exists
    let _ = names;
}

#[test]
fn test_sanitize_name_for_dir() {
    assert_eq!(super::super::swarm::sanitize_name("My Team!"), "my-team-");
    assert_eq!(super::super::swarm::sanitize_name("test-1"), "test-1");
}
