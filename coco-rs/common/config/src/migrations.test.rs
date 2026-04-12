use super::*;

#[test]
fn test_run_migrations_from_zero() {
    let mut config = serde_json::json!({"apiKey": "sk-test"});
    let mut state = MigrationState::default();
    let applied = run_migrations(&mut config, &mut state).unwrap();
    assert_eq!(applied, 3);
    assert_eq!(state.current_version, LATEST_VERSION);
    assert!(config.get("api_key_helper").is_some());
    assert!(config.get("permissions").is_some());
}

#[test]
fn test_no_migrations_needed() {
    let mut config = serde_json::json!({});
    let mut state = MigrationState {
        current_version: LATEST_VERSION,
        last_migrated_at: None,
    };
    let applied = run_migrations(&mut config, &mut state).unwrap();
    assert_eq!(applied, 0);
}
