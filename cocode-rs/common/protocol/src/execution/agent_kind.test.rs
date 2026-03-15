use super::*;

#[test]
fn test_main_kind() {
    let kind = AgentKind::main();
    assert!(kind.is_main());
    assert!(!kind.is_subagent());
    assert_eq!(kind.agent_type_str(), "main");
    assert_eq!(kind.to_string(), "main");
}

#[test]
fn test_subagent_kind() {
    let kind = AgentKind::subagent("session-123", "explore");
    assert!(kind.is_subagent());
    assert!(!kind.is_main());
    assert_eq!(kind.agent_type_str(), "explore");
    assert_eq!(kind.parent_session_id(), Some("session-123"));
    assert_eq!(kind.to_string(), "subagent:explore");
}

#[test]
fn test_extraction_kind() {
    let kind = AgentKind::extraction();
    assert!(kind.is_extraction());
    assert!(!kind.is_main());
    assert_eq!(kind.agent_type_str(), "extraction");
    assert_eq!(kind.to_string(), "extraction");
}

#[test]
fn test_compaction_kind() {
    let kind = AgentKind::compaction();
    assert!(kind.is_compaction());
    assert!(!kind.is_main());
    assert_eq!(kind.agent_type_str(), "compaction");
    assert_eq!(kind.to_string(), "compaction");
}

#[test]
fn test_default() {
    assert_eq!(AgentKind::default(), AgentKind::Main);
}

#[test]
fn test_serde_main() {
    let kind = AgentKind::Main;
    let json = serde_json::to_string(&kind).unwrap();
    let parsed: AgentKind = serde_json::from_str(&json).unwrap();
    assert_eq!(kind, parsed);
}

#[test]
fn test_serde_subagent() {
    let kind = AgentKind::subagent("session-123", "explore");
    let json = serde_json::to_string(&kind).unwrap();
    assert!(json.contains("Subagent"));
    assert!(json.contains("session-123"));
    assert!(json.contains("explore"));
    let parsed: AgentKind = serde_json::from_str(&json).unwrap();
    assert_eq!(kind, parsed);
}

#[test]
fn test_serde_extraction() {
    let kind = AgentKind::Extraction;
    let json = serde_json::to_string(&kind).unwrap();
    let parsed: AgentKind = serde_json::from_str(&json).unwrap();
    assert_eq!(kind, parsed);
}

#[test]
fn test_serde_compaction() {
    let kind = AgentKind::Compaction;
    let json = serde_json::to_string(&kind).unwrap();
    let parsed: AgentKind = serde_json::from_str(&json).unwrap();
    assert_eq!(kind, parsed);
}
