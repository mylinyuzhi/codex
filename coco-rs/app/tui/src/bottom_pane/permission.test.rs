use pretty_assertions::assert_eq;
use serde_json::json;

use super::always_allow_updates;
use super::edit_path_allow_update;

fn rule_summaries(update: &coco_types::PermissionUpdate) -> Vec<(String, Option<String>)> {
    match update {
        coco_types::PermissionUpdate::AddRules { rules, .. } => rules
            .iter()
            .map(|r| (r.value.tool_pattern.clone(), r.value.rule_content.clone()))
            .collect(),
        other => panic!("expected AddRules, got {other:?}"),
    }
}

#[test]
fn test_always_allow_prefers_engine_suggestions() {
    let suggestions = vec![coco_types::PermissionUpdate::SetMode {
        mode: coco_types::PermissionMode::AcceptEdits,
    }];
    let updates = always_allow_updates(
        "apply_patch",
        Some(&json!({"patch": "*** Begin Patch\n*** End Patch\n"})),
        &suggestions,
    );
    // `PermissionUpdate` has no `PartialEq`; pin the shape instead.
    assert!(
        matches!(
            updates.as_slice(),
            [coco_types::PermissionUpdate::SetMode {
                mode: coco_types::PermissionMode::AcceptEdits,
            }]
        ),
        "engine suggestions must pass through verbatim: {updates:?}"
    );
}

#[test]
fn test_always_allow_write_tool_scopes_to_directory_not_tool_wide() {
    // "Don't ask again" on a write-capable tool must never produce a
    // tool-wide allow rule (writes-anywhere). With no engine suggestions,
    // the rule is scoped to the target file's directory.
    let updates = always_allow_updates(
        "Write",
        Some(&json!({"file_path": "/tmp/proj/notes.md"})),
        &[],
    );
    assert_eq!(updates.len(), 1);
    let rules = rule_summaries(&updates[0]);
    assert_eq!(
        rules,
        vec![("Edit".to_string(), Some("//tmp/proj/**".to_string()))]
    );
}

#[test]
fn test_always_allow_apply_patch_scopes_to_patch_target_dirs() {
    let patch = "*** Begin Patch\n\
                 *** Add File: /tmp/plans/calm-bouncing-biscuit.md\n\
                 +# plan\n\
                 *** Update File: /tmp/proj/src/main.rs\n\
                 @@\n\
                 -a\n\
                 +b\n\
                 *** End Patch\n";
    let updates = always_allow_updates("apply_patch", Some(&json!({ "patch": patch })), &[]);
    assert_eq!(updates.len(), 1);
    let rules = rule_summaries(&updates[0]);
    assert_eq!(
        rules,
        vec![
            ("Edit".to_string(), Some("//tmp/plans/**".to_string())),
            ("Edit".to_string(), Some("//tmp/proj/src/**".to_string())),
        ]
    );
}

#[test]
fn test_always_allow_non_write_tool_keeps_tool_wide_fallback() {
    let updates = always_allow_updates("WebSearch", Some(&json!({"query": "rust"})), &[]);
    assert_eq!(updates.len(), 1);
    let rules = rule_summaries(&updates[0]);
    assert_eq!(rules, vec![("WebSearch".to_string(), None)]);
}

#[test]
fn test_edit_path_allow_update_none_for_underivable_write_input() {
    // A write tool whose input carries no derivable target keeps the
    // legacy tool-wide fallback (still an explicit user action).
    assert!(edit_path_allow_update("apply_patch", Some(&json!({"patch": "garbage"}))).is_none());
    assert!(edit_path_allow_update("Write", Some(&json!({}))).is_none());
    assert!(edit_path_allow_update("Write", None).is_none());
}
