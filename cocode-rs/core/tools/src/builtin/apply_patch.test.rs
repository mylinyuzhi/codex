use super::*;
use std::fs;
use std::path::PathBuf;
use tempfile::TempDir;

fn make_context(cwd: PathBuf) -> ToolContext {
    ToolContext::new("call-1", "session-1", cwd)
}

#[test]
fn test_tool_properties() {
    let tool = ApplyPatchTool::new();
    assert_eq!(tool.name(), "apply_patch");
    assert!(!tool.is_concurrent_safe());
    assert!(!tool.is_read_only());
}

#[test]
fn test_input_schema() {
    let tool = ApplyPatchTool::new();
    let schema = tool.input_schema();
    assert_eq!(schema["type"], "object");
    assert!(schema["properties"]["input"].is_object());
}

#[test]
fn test_function_definition() {
    let def = ApplyPatchTool::function_definition();
    assert_eq!(def.name, "apply_patch");
    assert_eq!(def.input_schema["type"], "object");
    assert!(def.input_schema["properties"]["input"].is_object());
}

#[test]
fn test_freeform_definition() {
    let def = ApplyPatchTool::freeform_definition();
    assert_eq!(def.name, "apply_patch");
    assert!(def.provider_options.is_some());
    let opts = def.provider_options.unwrap();
    let openai_opts = opts.get("openai").unwrap();
    let format = &openai_opts["custom_format"];
    assert_eq!(format["type"], "grammar");
    assert_eq!(format["syntax"], "lark");
    assert!(
        format["definition"]
            .as_str()
            .unwrap()
            .contains("Begin Patch")
    );
}

#[tokio::test]
async fn test_apply_patch_add_file() {
    let dir = TempDir::new().unwrap();
    let new_file = dir.path().join("hello.txt");

    let tool = ApplyPatchTool::new();
    let mut ctx = make_context(dir.path().to_path_buf());

    let patch = format!(
        "*** Begin Patch\n*** Add File: {}\n+Hello, world!\n*** End Patch",
        new_file.display()
    );

    let input = serde_json::json!({ "input": patch });
    let result = tool.execute(input, &mut ctx).await.unwrap();

    assert!(!result.is_error);
    assert!(new_file.exists());
    let content = fs::read_to_string(&new_file).unwrap();
    assert_eq!(content, "Hello, world!\n");
}

#[tokio::test]
async fn test_apply_patch_update_file() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("update.txt");
    fs::write(&file, "foo\nbar\n").unwrap();

    let tool = ApplyPatchTool::new();
    let mut ctx = make_context(dir.path().to_path_buf());

    let patch = format!(
        "*** Begin Patch\n*** Update File: {}\n@@\n foo\n-bar\n+baz\n*** End Patch",
        file.display()
    );

    let input = serde_json::json!({ "input": patch });
    let result = tool.execute(input, &mut ctx).await.unwrap();

    assert!(!result.is_error);
    let content = fs::read_to_string(&file).unwrap();
    assert_eq!(content, "foo\nbaz\n");
}

#[tokio::test]
async fn test_apply_patch_delete_file() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("delete.txt");
    fs::write(&file, "to be deleted").unwrap();

    let tool = ApplyPatchTool::new();
    let mut ctx = make_context(dir.path().to_path_buf());

    let patch = format!(
        "*** Begin Patch\n*** Delete File: {}\n*** End Patch",
        file.display()
    );

    let input = serde_json::json!({ "input": patch });
    let result = tool.execute(input, &mut ctx).await.unwrap();

    assert!(!result.is_error);
    assert!(!file.exists());
}

#[tokio::test]
async fn test_apply_patch_freeform_mode() {
    let dir = TempDir::new().unwrap();
    let new_file = dir.path().join("freeform.txt");

    let tool = ApplyPatchTool::new();
    let mut ctx = make_context(dir.path().to_path_buf());

    let patch = format!(
        "*** Begin Patch\n*** Add File: {}\n+Freeform content\n*** End Patch",
        new_file.display()
    );

    // Auto-detect: string input is treated as freeform
    let input = serde_json::Value::String(patch);
    let result = tool.execute(input, &mut ctx).await.unwrap();

    assert!(!result.is_error);
    assert!(new_file.exists());
    let content = fs::read_to_string(&new_file).unwrap();
    assert_eq!(content, "Freeform content\n");
}

#[tokio::test]
async fn test_plan_mode_blocks_non_plan_file() {
    let dir = TempDir::new().unwrap();
    let new_file = dir.path().join("blocked.txt");
    let plan_file = dir.path().join("plan.md");
    fs::write(&plan_file, "# Plan").unwrap();

    let tool = ApplyPatchTool::new();
    let mut ctx = make_context(dir.path().to_path_buf()).with_plan_mode(true, Some(plan_file));

    let patch = format!(
        "*** Begin Patch\n*** Add File: {}\n+Should be blocked\n*** End Patch",
        new_file.display()
    );

    let input = serde_json::json!({ "input": patch });
    let result = tool.execute(input, &mut ctx).await;

    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.to_string().contains("Plan mode"));
    assert!(!new_file.exists());
}

#[tokio::test]
async fn test_plan_mode_allows_plan_file() {
    let dir = TempDir::new().unwrap();
    let plan_file = dir.path().join("plan.md");
    fs::write(&plan_file, "# Plan\nold content\n").unwrap();

    let tool = ApplyPatchTool::new();
    let mut ctx =
        make_context(dir.path().to_path_buf()).with_plan_mode(true, Some(plan_file.clone()));

    let patch = format!(
        "*** Begin Patch\n*** Update File: {}\n@@\n # Plan\n-old content\n+new content\n*** End Patch",
        plan_file.display()
    );

    let input = serde_json::json!({ "input": patch });
    let result = tool.execute(input, &mut ctx).await.unwrap();

    assert!(!result.is_error);
    let content = fs::read_to_string(&plan_file).unwrap();
    assert!(content.contains("new content"));
}

#[tokio::test]
async fn test_invalid_patch_returns_error() {
    let dir = TempDir::new().unwrap();

    let tool = ApplyPatchTool::new();
    let mut ctx = make_context(dir.path().to_path_buf());

    let input = serde_json::json!({ "input": "not a valid patch" });
    let result = tool.execute(input, &mut ctx).await;

    assert!(result.is_err());
}

// ── Plan mode check_permission tests ────────────────────────────

#[tokio::test]
async fn test_check_permission_plan_file_auto_allowed() {
    let dir = TempDir::new().unwrap();
    let plan_file = dir.path().join("plan.md");
    fs::write(&plan_file, "# Plan\nold content\n").unwrap();

    let tool = ApplyPatchTool::new();
    let ctx = make_context(dir.path().to_path_buf()).with_plan_mode(true, Some(plan_file.clone()));

    let patch = format!(
        "*** Begin Patch\n*** Update File: {}\n@@\n # Plan\n-old content\n+new content\n*** End Patch",
        plan_file.display()
    );

    let input = serde_json::json!({ "input": patch });
    let result = tool.check_permission(&input, &ctx).await;
    assert!(
        matches!(result, cocode_protocol::PermissionResult::Allowed),
        "Plan file patch should be auto-allowed, got: {result:?}"
    );
}

#[tokio::test]
async fn test_check_permission_non_plan_file_denied_in_plan_mode() {
    let dir = TempDir::new().unwrap();
    let plan_file = dir.path().join("plan.md");
    let other_file = dir.path().join("code.rs");

    let tool = ApplyPatchTool::new();
    let ctx = make_context(dir.path().to_path_buf()).with_plan_mode(true, Some(plan_file));

    let patch = format!(
        "*** Begin Patch\n*** Add File: {}\n+fn main() {{}}\n*** End Patch",
        other_file.display()
    );

    let input = serde_json::json!({ "input": patch });
    let result = tool.check_permission(&input, &ctx).await;
    assert!(
        matches!(result, cocode_protocol::PermissionResult::Denied { .. }),
        "Non-plan file patch in plan mode should be denied, got: {result:?}"
    );
}

#[tokio::test]
async fn test_check_permission_multi_file_mixed_targets_denied() {
    let dir = TempDir::new().unwrap();
    let plan_file = dir.path().join("plan.md");
    let other_file = dir.path().join("code.rs");
    fs::write(&plan_file, "# Plan\nold content\n").unwrap();

    let tool = ApplyPatchTool::new();
    let ctx = make_context(dir.path().to_path_buf()).with_plan_mode(true, Some(plan_file.clone()));

    // Patch targets both plan file AND another file → should be denied
    let patch = format!(
        "*** Begin Patch\n*** Update File: {}\n@@\n # Plan\n-old content\n+new content\n*** Add File: {}\n+fn main() {{}}\n*** End Patch",
        plan_file.display(),
        other_file.display()
    );

    let input = serde_json::json!({ "input": patch });
    let result = tool.check_permission(&input, &ctx).await;
    assert!(
        matches!(result, cocode_protocol::PermissionResult::Denied { .. }),
        "Multi-file patch with non-plan targets should be denied, got: {result:?}"
    );
}

#[tokio::test]
async fn test_check_permission_unparseable_patch_passthrough() {
    let dir = TempDir::new().unwrap();
    let plan_file = dir.path().join("plan.md");

    let tool = ApplyPatchTool::new();
    let ctx = make_context(dir.path().to_path_buf()).with_plan_mode(true, Some(plan_file));

    let input = serde_json::json!({ "input": "not a valid patch" });
    let result = tool.check_permission(&input, &ctx).await;
    assert!(
        matches!(result, cocode_protocol::PermissionResult::Passthrough),
        "Unparseable patch should passthrough, got: {result:?}"
    );
}

#[tokio::test]
async fn test_check_permission_not_plan_mode_passthrough() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("code.rs");

    let tool = ApplyPatchTool::new();
    let ctx = make_context(dir.path().to_path_buf()); // not in plan mode

    let patch = format!(
        "*** Begin Patch\n*** Add File: {}\n+fn main() {{}}\n*** End Patch",
        file.display()
    );

    let input = serde_json::json!({ "input": patch });
    let result = tool.check_permission(&input, &ctx).await;
    assert!(
        matches!(result, cocode_protocol::PermissionResult::Passthrough),
        "Not in plan mode should passthrough, got: {result:?}"
    );
}

#[tokio::test]
async fn test_context_modifiers_added() {
    let dir = TempDir::new().unwrap();
    let file = dir.path().join("modified.txt");

    let tool = ApplyPatchTool::new();
    let mut ctx = make_context(dir.path().to_path_buf());

    let patch = format!(
        "*** Begin Patch\n*** Add File: {}\n+content\n*** End Patch",
        file.display()
    );

    let input = serde_json::json!({ "input": patch });
    let result = tool.execute(input, &mut ctx).await.unwrap();

    // Should have a FileRead context modifier
    assert!(!result.modifiers.is_empty());
    let has_file_read = result
        .modifiers
        .iter()
        .any(|m| matches!(m, ContextModifier::FileRead { path, .. } if *path == file));
    assert!(has_file_read);
}
