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
    assert_eq!(def.parameters["type"], "object");
    assert!(def.parameters["properties"]["input"].is_object());
}

#[test]
fn test_freeform_definition() {
    let def = ApplyPatchTool::freeform_definition();
    assert_eq!(def.name, "apply_patch");
    assert!(def.custom_format.is_some());
    let format = def.custom_format.unwrap();
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
