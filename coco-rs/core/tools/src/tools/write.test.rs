use crate::tools::write::WriteTool;
use coco_tool::Tool;
use coco_tool::ToolUseContext;
use serde_json::json;

#[tokio::test]
async fn test_write_new_file() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("new.txt");

    let ctx = ToolUseContext::test_default();
    let result = WriteTool
        .execute(
            json!({"file_path": file.to_str().unwrap(), "content": "hello\nworld\n"}),
            &ctx,
        )
        .await
        .unwrap();

    let text = result.data.as_str().unwrap();
    assert!(text.contains("created"));
    assert!(text.contains("2 lines"));
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "hello\nworld\n");
}

#[tokio::test]
async fn test_write_overwrite_existing() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("existing.txt");
    std::fs::write(&file, "old content").unwrap();

    let ctx = ToolUseContext::test_default();
    let result = WriteTool
        .execute(
            json!({"file_path": file.to_str().unwrap(), "content": "new content"}),
            &ctx,
        )
        .await
        .unwrap();

    let text = result.data.as_str().unwrap();
    assert!(text.contains("updated"));
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "new content");
}

#[tokio::test]
async fn test_write_creates_parent_dirs() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("a").join("b").join("c.txt");

    let ctx = ToolUseContext::test_default();
    let result = WriteTool
        .execute(
            json!({"file_path": file.to_str().unwrap(), "content": "deep"}),
            &ctx,
        )
        .await;

    assert!(result.is_ok());
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "deep");
}

#[tokio::test]
async fn test_write_missing_content() {
    let ctx = ToolUseContext::test_default();
    let result = WriteTool
        .execute(json!({"file_path": "/tmp/test.txt"}), &ctx)
        .await;

    assert!(result.is_err());
}
