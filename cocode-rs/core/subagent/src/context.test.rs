use super::*;

#[test]
fn test_child_context_serde_roundtrip() {
    let ctx = ChildToolUseContext {
        parent_session_id: "parent-123".to_string(),
        child_session_id: "child-456".to_string(),
        forked_from_turn: 7,
    };
    let json = serde_json::to_string(&ctx).expect("serialize");
    let back: ChildToolUseContext = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(back.parent_session_id, "parent-123");
    assert_eq!(back.child_session_id, "child-456");
    assert_eq!(back.forked_from_turn, 7);
}
