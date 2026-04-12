use super::*;

#[test]
fn test_compute_initial_team_context_none() {
    assert!(compute_initial_team_context(None, None).is_none());
}

#[test]
fn test_compute_initial_team_context_nonexistent_team() {
    let ctx = compute_initial_team_context(Some("nonexistent-xyz-123"), Some("worker"));
    assert!(ctx.is_none());
}

#[test]
fn test_extract_team_metadata_found() {
    let messages = vec![
        serde_json::json!({"role": "user", "content": "hello"}),
        serde_json::json!({"role": "assistant", "team_name": "my-team", "agent_name": "researcher"}),
    ];
    let result = extract_team_metadata(&messages);
    assert_eq!(
        result,
        Some(("my-team".to_string(), "researcher".to_string()))
    );
}

#[test]
fn test_extract_team_metadata_not_found() {
    let messages = vec![serde_json::json!({"role": "user", "content": "hello"})];
    assert!(extract_team_metadata(&messages).is_none());
}

#[test]
fn test_initialize_from_session_nonexistent() {
    let ctx = initialize_from_session("nonexistent-xyz-123", "worker");
    assert!(ctx.is_none());
}
