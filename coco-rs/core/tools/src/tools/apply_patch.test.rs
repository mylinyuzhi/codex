use super::*;
use coco_tool_runtime::DynTool;
use coco_tool_runtime::ToolUseContext;
use coco_types::Features;
use coco_types::PermissionBehavior;
use coco_types::PermissionMode;
use coco_types::PermissionRule;
use coco_types::PermissionRuleSource;
use coco_types::PermissionRuleValue;
use coco_types::ToolCheckResult;
use coco_types::ToolOverrides;
use std::sync::Arc;

#[test]
fn is_enabled_only_when_model_adds_apply_patch() {
    let tool: &dyn DynTool = &ApplyPatchTool;

    // Default overrides — model does NOT add apply_patch as extra.
    let mut ctx = ToolUseContext::test_default();
    ctx.features = Arc::new(Features::with_defaults());
    ctx.tool_overrides = Arc::new(ToolOverrides::none());
    assert!(
        !tool.is_enabled(&ctx),
        "apply_patch must be hidden when the active model didn't add it"
    );

    // gpt-5-style overrides — extra: apply_patch.
    ctx.tool_overrides =
        Arc::new(ToolOverrides::default().with_extra(ToolId::Builtin(ToolName::ApplyPatch)));
    assert!(tool.is_enabled(&ctx));
}

#[tokio::test]
async fn check_permissions_accept_edits_allows_cwd_patch() {
    let dir = tempfile::tempdir().unwrap();
    let mut ctx = ToolUseContext::test_default();
    ctx.cwd_override = Some(dir.path().to_path_buf());
    ctx.permission_context.mode = PermissionMode::AcceptEdits;
    let input = serde_json::json!({
        "patch": "*** Begin Patch\n*** Add File: notes.txt\n+hello\n*** End Patch\n"
    });

    let result =
        <ApplyPatchTool as DynTool>::check_permissions(&ApplyPatchTool, &input, &ctx).await;

    assert!(matches!(result, ToolCheckResult::Allow { .. }));
}

#[tokio::test]
async fn check_permissions_path_scoped_edit_rule_allows_patch() {
    let dir = tempfile::Builder::new()
        .prefix("apply-patch-perms-")
        .tempdir_in(std::env::current_dir().unwrap())
        .unwrap();
    let mut ctx = ToolUseContext::test_default();
    ctx.cwd_override = Some(dir.path().to_path_buf());
    ctx.permission_context.allow_rules.insert(
        PermissionRuleSource::Session,
        vec![PermissionRule {
            source: PermissionRuleSource::Session,
            behavior: PermissionBehavior::Allow,
            value: PermissionRuleValue {
                tool_pattern: "Edit".into(),
                rule_content: Some(format!("/{}/**", dir.path().to_string_lossy())),
            },
        }],
    );
    let input = serde_json::json!({
        "patch": "*** Begin Patch\n*** Add File: notes.txt\n+hello\n*** End Patch\n"
    });

    let result =
        <ApplyPatchTool as DynTool>::check_permissions(&ApplyPatchTool, &input, &ctx).await;

    assert!(matches!(result, ToolCheckResult::Allow { .. }));
}

#[tokio::test]
async fn check_permissions_suspicious_path_requires_approval() {
    let dir = tempfile::tempdir().unwrap();
    let mut ctx = ToolUseContext::test_default();
    ctx.cwd_override = Some(dir.path().to_path_buf());
    ctx.permission_context.mode = PermissionMode::AcceptEdits;
    let input = serde_json::json!({
        "patch": "*** Begin Patch\n*** Add File: GIT~1/config\n+hello\n*** End Patch\n"
    });

    let result =
        <ApplyPatchTool as DynTool>::check_permissions(&ApplyPatchTool, &input, &ctx).await;

    assert!(matches!(result, ToolCheckResult::Ask { .. }));
}

#[tokio::test]
async fn check_permissions_mixed_internal_and_unsafe_paths_requires_approval() {
    let dir = tempfile::tempdir().unwrap();
    let mut ctx = ToolUseContext::test_default();
    ctx.cwd_override = Some(dir.path().to_path_buf());
    ctx.permission_context.mode = PermissionMode::AcceptEdits;
    let input = serde_json::json!({
        "patch": "*** Begin Patch\n*** Add File: .claude/plans/plan.md\n+ok\n*** Add File: GIT~1/config\n+bad\n*** End Patch\n"
    });

    let result =
        <ApplyPatchTool as DynTool>::check_permissions(&ApplyPatchTool, &input, &ctx).await;

    assert!(matches!(result, ToolCheckResult::Ask { .. }));
}

#[tokio::test]
async fn check_permissions_default_ask_includes_write_suggestions() {
    let cwd = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let mut ctx = ToolUseContext::test_default();
    ctx.cwd_override = Some(cwd.path().to_path_buf());
    ctx.permission_context.mode = PermissionMode::Default;
    let target = outside.path().join("notes.txt");
    let input = serde_json::json!({
        "patch": format!(
            "*** Begin Patch\n*** Add File: {}\n+hello\n*** End Patch\n",
            target.display()
        )
    });

    let result =
        <ApplyPatchTool as DynTool>::check_permissions(&ApplyPatchTool, &input, &ctx).await;

    let ToolCheckResult::Ask { suggestions, .. } = result else {
        panic!("expected ask");
    };
    assert!(suggestions.iter().any(|update| {
        matches!(
            update,
            coco_types::PermissionUpdate::SetMode {
                mode: PermissionMode::AcceptEdits
            }
        )
    }));
    let outside = outside.path().to_string_lossy().to_string();
    assert!(suggestions.iter().any(|update| {
        matches!(
            update,
            coco_types::PermissionUpdate::AddDirectories { directories, .. }
                if directories.iter().any(|dir| dir == &outside)
        )
    }));
}
