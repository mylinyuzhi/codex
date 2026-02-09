use super::*;

#[test]
fn test_lifecycle_status_variants() {
    let statuses = vec![
        AgentLifecycleStatus::Initializing,
        AgentLifecycleStatus::Running,
        AgentLifecycleStatus::Waiting,
        AgentLifecycleStatus::Completed,
        AgentLifecycleStatus::Failed,
    ];
    for status in &statuses {
        let _debug = format!("{status:?}");
        let _clone = status.clone();
    }
}

#[test]
fn test_lifecycle_equality() {
    assert_eq!(AgentLifecycleStatus::Running, AgentLifecycleStatus::Running);
    assert_ne!(
        AgentLifecycleStatus::Running,
        AgentLifecycleStatus::Completed
    );
}

#[test]
fn test_lifecycle_serde_roundtrip() {
    let status = AgentLifecycleStatus::Waiting;
    let json = serde_json::to_string(&status).expect("serialize");
    let back: AgentLifecycleStatus = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(back, AgentLifecycleStatus::Waiting);
}

#[test]
fn test_thread_id_unique() {
    let id1 = ThreadId::new();
    let id2 = ThreadId::new();
    assert_ne!(id1.0, id2.0);
}

#[test]
fn test_thread_id_not_empty() {
    let id = ThreadId::new();
    assert!(!id.0.is_empty());
}

#[test]
fn test_thread_id_default() {
    let id = ThreadId::default();
    assert!(!id.0.is_empty());
}
