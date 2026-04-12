use super::*;

#[test]
fn test_team_lead_name() {
    assert_eq!(TEAM_LEAD_NAME, "team-lead");
}

#[test]
fn test_env_var_names() {
    assert_eq!(TEAMMATE_COLOR_ENV_VAR, "CLAUDE_CODE_AGENT_COLOR");
    assert_eq!(PLAN_MODE_REQUIRED_ENV_VAR, "CLAUDE_CODE_PLAN_MODE_REQUIRED");
    assert_eq!(AGENT_TEAMS_ENV_VAR, "CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS");
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
