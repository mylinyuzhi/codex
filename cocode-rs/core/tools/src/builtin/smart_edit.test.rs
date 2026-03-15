use super::*;
use std::io::Write as IoWrite;
use std::path::PathBuf;
use tempfile::NamedTempFile;

fn make_context() -> ToolContext {
    let mut ctx = ToolContext::new("call-1", "session-1", PathBuf::from("/tmp"));
    // Enable SmartEdit feature for tests
    ctx.features.enable(Feature::SmartEdit);
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
            Ok(ModelCallResult {
                response: hyper_sdk::ObjectResponse::new(
                    "test-id",
                    "test-model",
                    serde_json::json!({
                        "search": "    let x = 1;",
                        "replace": "    let x = 42;",
                        "no_changes_required": false,
                        "explanation": "Fixed variable name in search string"
                    }),
                ),
            })
        })
    });
    ctx.model_call_fn = Some(mock_fn);

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
            Ok(ModelCallResult {
                response: hyper_sdk::ObjectResponse::new(
                    "test-id",
                    "test-model",
                    serde_json::json!({
                        "search": "",
                        "replace": "",
                        "no_changes_required": true,
                        "explanation": "The edit is already applied"
                    }),
                ),
            })
        })
    });
    ctx.model_call_fn = Some(mock_fn);

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
