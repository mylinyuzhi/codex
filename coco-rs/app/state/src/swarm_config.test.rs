use super::*;

#[test]
fn test_teammate_mode_default() {
    assert_eq!(TeammateMode::default(), TeammateMode::Auto);
}

#[test]
fn test_teammate_mode_as_str() {
    assert_eq!(TeammateMode::Auto.as_str(), "auto");
    assert_eq!(TeammateMode::Tmux.as_str(), "tmux");
    assert_eq!(TeammateMode::InProcess.as_str(), "in-process");
    assert_eq!(TeammateMode::Iterm2.as_str(), "iterm2");
}

#[test]
fn test_teammate_mode_serde() {
    let json = serde_json::to_string(&TeammateMode::InProcess).unwrap();
    assert_eq!(json, "\"in-process\"");
    let parsed: TeammateMode = serde_json::from_str("\"tmux\"").unwrap();
    assert_eq!(parsed, TeammateMode::Tmux);
}

#[test]
fn test_team_config_default() {
    let config = TeamConfig::default();
    assert!(config.enabled);
    assert_eq!(config.teammate_mode, TeammateMode::Auto);
    assert!(config.teammate_default_model.is_none());
    assert!(config.show_spinner_tree);
    assert_eq!(config.max_agents, 8);
}

#[test]
fn test_feature_gate_disabled_config() {
    let config = TeamConfig {
        enabled: false,
        ..TeamConfig::default()
    };
    assert!(!is_agent_teams_enabled(&config, /*cli_flag*/ false));
    // CLI flag doesn't override config.enabled=false
    assert!(!is_agent_teams_enabled(&config, /*cli_flag*/ true));
}

#[test]
fn test_feature_gate_cli_flag() {
    let config = TeamConfig::default();
    assert!(is_agent_teams_enabled(&config, /*cli_flag*/ true));
}

#[test]
fn test_feature_gate_default_off() {
    let config = TeamConfig::default();
    // Without env var or CLI flag, default is off
    assert!(!is_agent_teams_enabled(&config, /*cli_flag*/ false));
}
