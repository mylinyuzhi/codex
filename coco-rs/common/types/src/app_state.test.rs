use super::ToolAppState;
use crate::PermissionMode;

#[test]
fn default_is_all_zero() {
    // Sanity: a fresh session has no plan-mode latches, no counters, no
    // pending approval. Drivers rely on Default() matching the "empty
    // JSON object" behavior the old serde_json representation had.
    let s = ToolAppState::default();
    assert_eq!(s.permission_mode, None);
    assert_eq!(s.pre_plan_mode, None);
    assert!(s.stripped_dangerous_rules.is_none());
    assert!(!s.has_exited_plan_mode);
    assert!(!s.needs_plan_mode_exit_attachment);
    assert_eq!(s.plan_mode_attachment_count, 0);
    assert_eq!(s.plan_mode_turns_since_last_attachment, 0);
    assert_eq!(s.last_permission_mode, None);
    assert_eq!(s.plan_mode_entry_ms, None);
    assert!(!s.awaiting_plan_approval);
    assert_eq!(s.awaiting_plan_approval_request_id, None);
}

#[test]
fn struct_update_syntax_composes() {
    // Tests previously built ad-hoc snapshots with `json!({...})`.
    // The struct-update spread is the replacement idiom — verify it
    // produces the expected field values so migrating tests is trivial.
    let s = ToolAppState {
        awaiting_plan_approval: true,
        awaiting_plan_approval_request_id: Some("plan_approval-alice-a-1".into()),
        last_permission_mode: Some(PermissionMode::AcceptEdits),
        ..Default::default()
    };
    assert!(s.awaiting_plan_approval);
    assert_eq!(
        s.awaiting_plan_approval_request_id.as_deref(),
        Some("plan_approval-alice-a-1")
    );
    assert_eq!(s.last_permission_mode, Some(PermissionMode::AcceptEdits));
    // Unfilled fields stay default.
    assert_eq!(s.plan_mode_attachment_count, 0);
}
