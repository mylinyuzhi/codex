use super::*;
use std::io::Write as IoWrite;
use std::path::PathBuf;
use tempfile::NamedTempFile;

fn make_context() -> ToolContext {
    let mut ctx = ToolContext::new("call-1", "session-1", PathBuf::from("/tmp"));
    // Enable SmartEdit feature for tests
    ctx.env.features.enable(Feature::SmartEdit);
    ctx
}

#[test]
fn test_tool_properties() {
    let tool = SmartEditTool::new();
    assert_eq!(tool.name(), "SmartEdit");
    assert!(!tool.is_read_only());
    assert_eq!(tool.concurrency_safety(), ConcurrencySafety::Unsafe);
    assert_eq!(tool.feature_gate(), Some(Feature::SmartEdit));
}

#[tokio::test]
async fn test_smart_edit_basic() {
    let mut file = NamedTempFile::new().unwrap();
    write!(file, "Hello World").unwrap();
    let path = file.path().to_str().unwrap().to_string();

    let tool = SmartEditTool::new();
    let mut ctx = make_context();
    ctx.record_file_read(file.path()).await;

    let input = serde_json::json!({
        "file_path": path,
        "old_string": "World",
        "new_string": "Rust",
        "instruction": "Replace World with Rust"
    });

    let result = tool.execute(input, &mut ctx).await.unwrap();
    assert!(!result.is_error);

    let content = std::fs::read_to_string(file.path()).unwrap();
    assert_eq!(content, "Hello Rust");
}

#[tokio::test]
async fn test_smart_edit_requires_read_first() {
    let mut file = NamedTempFile::new().unwrap();
    write!(file, "Hello World").unwrap();
    let path = file.path().to_str().unwrap().to_string();

    let tool = SmartEditTool::new();
    let mut ctx = make_context();

    let input = serde_json::json!({
        "file_path": path,
        "old_string": "World",
        "new_string": "Rust",
        "instruction": "Replace World with Rust"
    });

    let result = tool.execute(input, &mut ctx).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_smart_edit_no_model_call_fn_falls_back_to_error() {
    let mut file = NamedTempFile::new().unwrap();
    write!(file, "Hello World").unwrap();
    let path = file.path().to_str().unwrap().to_string();

    let tool = SmartEditTool::new();
    let mut ctx = make_context();
    ctx.record_file_read(file.path()).await;

    // old_string doesn't match anything in the file
    let input = serde_json::json!({
        "file_path": path,
        "old_string": "Nonexistent Content",
        "new_string": "Replacement",
        "instruction": "Replace nonexistent content"
    });

    let result = tool.execute(input, &mut ctx).await;
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(err.contains("not found in file"));
}

#[tokio::test]
async fn test_smart_edit_with_mock_model_call() {
    use crate::context::ModelCallResult;
    use std::sync::Arc;

    let mut file = NamedTempFile::new().unwrap();
    write!(file, "fn main() {{\n    let x = 1;\n}}").unwrap();
    let path = file.path().to_str().unwrap().to_string();

    let tool = SmartEditTool::new();
    let mut ctx = make_context();
    ctx.record_file_read(file.path()).await;

    // Set up mock model_call_fn that returns a corrected search/replace
    let mock_fn: crate::context::ModelCallFn = Arc::new(|_input| {
        Box::pin(async {
            let json = serde_json::json!({
                "search": "    let x = 1;",
                "replace": "    let x = 42;",
                "no_changes_required": false,
                "explanation": "Fixed variable name in search string"
            });
            Ok(ModelCallResult {
                response: cocode_inference::LanguageModelGenerateResult::text(
                    serde_json::to_string(&json).unwrap(),
                    cocode_inference::Usage::default(),
                ),
            })
        })
    });
    ctx.agent.model_call_fn = Some(mock_fn);

    // Use a completely wrong variable name that won't match any strategy
    let input = serde_json::json!({
        "file_path": path,
        "old_string": "    let y = 1;",  // wrong variable name (y vs x)
        "new_string": "    let x = 42;",
        "instruction": "Change x from 1 to 42"
    });

    let result = tool.execute(input, &mut ctx).await.unwrap();
    assert!(!result.is_error);

    let content = std::fs::read_to_string(file.path()).unwrap();
    assert!(content.contains("let x = 42;"));
    let text = match &result.content {
        cocode_protocol::ToolResultContent::Text(t) => t,
        _ => panic!("Expected text"),
    };
    assert!(text.contains("LLM correction"));
}

#[tokio::test]
async fn test_smart_edit_no_changes_required() {
    use crate::context::ModelCallResult;
    use std::sync::Arc;

    let mut file = NamedTempFile::new().unwrap();
    write!(file, "fn main() {{\n    let x = 42;\n}}").unwrap();
    let path = file.path().to_str().unwrap().to_string();

    let tool = SmartEditTool::new();
    let mut ctx = make_context();
    ctx.record_file_read(file.path()).await;

    let mock_fn: crate::context::ModelCallFn = Arc::new(|_input| {
        Box::pin(async {
            let json = serde_json::json!({
                "search": "",
                "replace": "",
                "no_changes_required": true,
                "explanation": "The edit is already applied"
            });
            Ok(ModelCallResult {
                response: cocode_inference::LanguageModelGenerateResult::text(
                    serde_json::to_string(&json).unwrap(),
                    cocode_inference::Usage::default(),
                ),
            })
        })
    });
    ctx.agent.model_call_fn = Some(mock_fn);

    let input = serde_json::json!({
        "file_path": path,
        "old_string": "let x = 1;",
        "new_string": "let x = 42;",
        "instruction": "Change x from 1 to 42"
    });

    let result = tool.execute(input, &mut ctx).await.unwrap();
    assert!(!result.is_error);
    let text = match &result.content {
        cocode_protocol::ToolResultContent::Text(t) => t,
        _ => panic!("Expected text"),
    };
    assert!(text.contains("no changes required"));
}

// ── Plan mode create_new_file test ──────────────────────────────

#[tokio::test]
async fn test_create_new_file_blocked_in_plan_mode() {
    let dir = tempfile::TempDir::new().unwrap();
    let new_file = dir.path().join("new_code.rs");
    let plan_file = dir.path().join("plan.md");

    let tool = SmartEditTool::new();
    let mut ctx = make_context().with_plan_mode(true, Some(plan_file));

    // empty old_string triggers create_new_file path
    let input = serde_json::json!({
        "file_path": new_file.to_str().unwrap(),
        "old_string": "",
        "new_string": "fn main() {}",
        "instruction": "Create new file"
    });

    let result = tool.execute(input, &mut ctx).await;
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.to_string().contains("Plan mode"));
    assert!(!new_file.exists());
}

#[tokio::test]
async fn test_create_new_file_allowed_for_plan_file() {
    let dir = tempfile::TempDir::new().unwrap();
    let plan_file = dir.path().join("plan.md");

    let tool = SmartEditTool::new();
    let mut ctx = make_context().with_plan_mode(true, Some(plan_file.clone()));

    // empty old_string triggers create_new_file path — plan file should be allowed
    let input = serde_json::json!({
        "file_path": plan_file.to_str().unwrap(),
        "old_string": "",
        "new_string": "# My Plan",
        "instruction": "Create plan file"
    });

    let result = tool.execute(input, &mut ctx).await.unwrap();
    assert!(!result.is_error);

    let content = std::fs::read_to_string(&plan_file).unwrap();
    assert_eq!(content, "# My Plan");
}

// ── Plan mode check_permission tests ────────────────────────────

#[tokio::test]
async fn test_check_permission_plan_file_auto_allowed() {
    let dir = tempfile::TempDir::new().unwrap();
    let plan_file = dir.path().join("plan.md");
    std::fs::write(&plan_file, "# Plan").unwrap();

    let tool = SmartEditTool::new();
    let ctx = make_context().with_plan_mode(true, Some(plan_file.clone()));

    let input = serde_json::json!({
        "file_path": plan_file.to_str().unwrap(),
        "old_string": "# Plan",
        "new_string": "# Updated Plan",
        "instruction": "Update plan title"
    });

    let result = tool.check_permission(&input, &ctx).await;
    assert!(
        matches!(result, cocode_protocol::PermissionResult::Allowed),
        "Plan file edit should be auto-allowed, got: {result:?}"
    );
}

#[tokio::test]
async fn test_check_permission_non_plan_file_denied_in_plan_mode() {
    let plan_file = PathBuf::from("/tmp/plan.md");
    let other_file = PathBuf::from("/tmp/code.rs");

    let tool = SmartEditTool::new();
    let ctx = make_context().with_plan_mode(true, Some(plan_file));

    let input = serde_json::json!({
        "file_path": other_file.to_str().unwrap(),
        "old_string": "old",
        "new_string": "new",
        "instruction": "Change old to new"
    });

    let result = tool.check_permission(&input, &ctx).await;
    assert!(
        matches!(result, cocode_protocol::PermissionResult::Denied { .. }),
        "Non-plan file edit in plan mode should be denied, got: {result:?}"
    );
}

#[tokio::test]
async fn test_smart_edit_requires_instruction() {
    let tool = SmartEditTool::new();
    let mut ctx = make_context();

    let input = serde_json::json!({
        "file_path": "/tmp/test.txt",
        "old_string": "foo",
        "new_string": "bar"
        // missing instruction
    });

    let result = tool.execute(input, &mut ctx).await;
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(err.contains("instruction"));
}
