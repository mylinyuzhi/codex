use crate::tools::edit::EditTool;
use coco_tool::Tool;
use coco_tool::ToolUseContext;
use serde_json::json;

#[tokio::test]
async fn test_edit_single_replacement() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("test.rs");
    std::fs::write(&file, "fn hello() {\n    println!(\"hi\");\n}\n").unwrap();

    let ctx = ToolUseContext::test_default();
    let result = EditTool
        .execute(
            json!({
                "file_path": file.to_str().unwrap(),
                "old_string": "println!(\"hi\")",
                "new_string": "println!(\"hello world\")"
            }),
            &ctx,
        )
        .await
        .unwrap();

    let text = result.data.as_str().unwrap();
    assert!(text.contains("updated successfully"));
    let content = std::fs::read_to_string(&file).unwrap();
    assert!(content.contains("hello world"));
    assert!(!content.contains("\"hi\""));
}

#[tokio::test]
async fn test_edit_replace_all() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("multi.txt");
    std::fs::write(&file, "foo bar foo baz foo").unwrap();

    let ctx = ToolUseContext::test_default();
    let result = EditTool
        .execute(
            json!({
                "file_path": file.to_str().unwrap(),
                "old_string": "foo",
                "new_string": "qux",
                "replace_all": true
            }),
            &ctx,
        )
        .await
        .unwrap();

    let text = result.data.as_str().unwrap();
    assert!(text.contains("3 replacement(s)"));
    let content = std::fs::read_to_string(&file).unwrap();
    assert_eq!(content, "qux bar qux baz qux");
}

#[tokio::test]
async fn test_edit_not_unique_error() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("dup.txt");
    std::fs::write(&file, "aaa bbb aaa").unwrap();

    let ctx = ToolUseContext::test_default();
    let result = EditTool
        .execute(
            json!({
                "file_path": file.to_str().unwrap(),
                "old_string": "aaa",
                "new_string": "ccc"
            }),
            &ctx,
        )
        .await;

    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.to_string().contains("2 times"));
}

#[tokio::test]
async fn test_edit_not_found_error() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("miss.txt");
    std::fs::write(&file, "hello world").unwrap();

    let ctx = ToolUseContext::test_default();
    let result = EditTool
        .execute(
            json!({
                "file_path": file.to_str().unwrap(),
                "old_string": "zzz",
                "new_string": "xxx"
            }),
            &ctx,
        )
        .await;

    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.to_string().contains("not found"));
}

#[tokio::test]
async fn test_edit_same_string_error() {
    let tool = EditTool;
    let ctx = coco_tool::ToolUseContext::test_default();
    let result = tool.validate_input(
        &json!({
            "file_path": "/tmp/x.txt",
            "old_string": "same",
            "new_string": "same"
        }),
        &ctx,
    );

    assert!(matches!(
        result,
        coco_tool::ValidationResult::Invalid { .. }
    ));
}

#[tokio::test]
async fn test_edit_file_not_found() {
    let ctx = ToolUseContext::test_default();
    let result = EditTool
        .execute(
            json!({
                "file_path": "/nonexistent/file.txt",
                "old_string": "a",
                "new_string": "b"
            }),
            &ctx,
        )
        .await;

    assert!(result.is_err());
}
