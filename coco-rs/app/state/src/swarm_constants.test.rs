use super::*;

#[test]
fn test_team_lead_name() {
    assert_eq!(TEAM_LEAD_NAME, "team-lead");
}

#[test]
fn test_env_var_names() {
    // coco-rs env namespace — no CLAUDE_ prefix (those belong to the
    // Anthropic provider SDKs, which live in vercel-ai-*).
    assert_eq!(TEAMMATE_COMMAND_ENV_VAR, "COCO_TEAMMATE_COMMAND");
    assert_eq!(TEAMMATE_COLOR_ENV_VAR, "COCO_AGENT_COLOR");
    assert_eq!(PLAN_MODE_REQUIRED_ENV_VAR, "COCO_PLAN_MODE_REQUIRED");
    assert_eq!(AGENT_TEAMS_ENV_VAR, "COCO_EXPERIMENTAL_AGENT_TEAMS");
    assert_eq!(AGENT_ID_ENV_VAR, "COCO_AGENT_ID");
    assert_eq!(AGENT_NAME_ENV_VAR, "COCO_AGENT_NAME");
    assert_eq!(TEAM_NAME_ENV_VAR, "COCO_TEAM_NAME");
    assert_eq!(PARENT_SESSION_ID_ENV_VAR, "COCO_PARENT_SESSION_ID");
    assert_eq!(VERIFY_PLAN_ENV_VAR, "COCO_VERIFY_PLAN");
}

#[test]
fn test_swarm_socket_name_format() {
    let name = swarm_socket_name();
    assert!(name.starts_with("claude-swarm-"));
}

#[test]
fn test_agent_color_name_as_str() {
    assert_eq!(AgentColorName::Red.as_str(), "red");
    assert_eq!(AgentColorName::Cyan.as_str(), "cyan");
}

#[test]
fn test_agent_color_name_display() {
    assert_eq!(format!("{}", AgentColorName::Purple), "purple");
}

#[test]
fn test_agent_color_name_serde() {
    let json = serde_json::to_string(&AgentColorName::Blue).unwrap();
    assert_eq!(json, "\"blue\"");
    let parsed: AgentColorName = serde_json::from_str("\"green\"").unwrap();
    assert_eq!(parsed, AgentColorName::Green);
}
