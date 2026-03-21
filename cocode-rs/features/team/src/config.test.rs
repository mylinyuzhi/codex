use crate::config::TeamConfig;

#[test]
fn defaults_are_sensible() {
    let cfg = TeamConfig::default();
    assert_eq!(cfg.max_members_per_team, 10);
    assert_eq!(cfg.mailbox_poll_interval_ms, 500);
    assert_eq!(cfg.idle_timeout_secs, 300);
    assert_eq!(cfg.shutdown_timeout_secs, 60);
    assert!(cfg.persist_to_disk);
    assert_eq!(cfg.default_agent_type, "general-purpose");
}

#[test]
fn serde_round_trip() {
    let cfg = TeamConfig {
        max_members_per_team: 5,
        mailbox_poll_interval_ms: 1000,
        idle_timeout_secs: 120,
        shutdown_timeout_secs: 30,
        persist_to_disk: false,
        default_agent_type: "explore".to_string(),
    };
    let json = serde_json::to_string(&cfg).unwrap();
    let parsed: TeamConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.max_members_per_team, 5);
    assert!(!parsed.persist_to_disk);
}

#[test]
fn deserialize_with_defaults() {
    let json = r#"{"max_members_per_team": 20}"#;
    let cfg: TeamConfig = serde_json::from_str(json).unwrap();
    assert_eq!(cfg.max_members_per_team, 20);
    // Other fields get defaults
    assert_eq!(cfg.mailbox_poll_interval_ms, 500);
    assert!(cfg.persist_to_disk);
}
