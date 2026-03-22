use super::*;

#[test]
fn test_plan_config_default() {
    let config = PlanModeConfig::default();
    assert_eq!(config.agent_count, DEFAULT_PLAN_AGENT_COUNT);
    assert_eq!(config.explore_agent_count, DEFAULT_PLAN_EXPLORE_AGENT_COUNT);
}

#[test]
fn test_plan_config_serde() {
    let json = r#"{"agent_count": 3, "explore_agent_count": 4}"#;
    let config: PlanModeConfig = serde_json::from_str(json).unwrap();
    assert_eq!(config.agent_count, 3);
    assert_eq!(config.explore_agent_count, 4);
}

#[test]
fn test_plan_config_serde_defaults() {
    let json = r#"{}"#;
    let config: PlanModeConfig = serde_json::from_str(json).unwrap();
    assert_eq!(config.agent_count, DEFAULT_PLAN_AGENT_COUNT);
    assert_eq!(config.explore_agent_count, DEFAULT_PLAN_EXPLORE_AGENT_COUNT);
}

#[test]
fn test_validate_valid_config() {
    let config = PlanModeConfig::default();
    assert!(config.validate().is_ok());

    let config = PlanModeConfig {
        agent_count: 5,
        explore_agent_count: 5,
    };
    assert!(config.validate().is_ok());
}

#[test]
fn test_validate_invalid_agent_count() {
    let config = PlanModeConfig {
        agent_count: 0,
        explore_agent_count: 3,
    };
    assert!(config.validate().is_err());

    let config = PlanModeConfig {
        agent_count: 10,
        explore_agent_count: 3,
    };
    assert!(config.validate().is_err());
}

#[test]
fn test_validate_invalid_explore_agent_count() {
    let config = PlanModeConfig {
        agent_count: 3,
        explore_agent_count: 0,
    };
    assert!(config.validate().is_err());

    let config = PlanModeConfig {
        agent_count: 3,
        explore_agent_count: 6,
    };
    assert!(config.validate().is_err());
}

#[test]
fn test_clamp() {
    let mut config = PlanModeConfig {
        agent_count: 10,
        explore_agent_count: -5,
    };
    config.clamp_all();
    assert_eq!(config.agent_count, MAX_AGENT_COUNT);
    assert_eq!(config.explore_agent_count, MIN_AGENT_COUNT);
}
