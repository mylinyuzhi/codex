use crate::tools::read::ReadTool;
use coco_tool::Tool;
use coco_tool::ToolUseContext;
use serde_json::json;
use std::io::Write;

#[tokio::test]
async fn test_read_basic_file() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("hello.txt");
    std::fs::write(&file, "line one\nline two\nline three\n").unwrap();

    let ctx = ToolUseContext::test_default();
    let result = ReadTool
        .execute(json!({"file_path": file.to_str().unwrap()}), &ctx)
        .await
        .unwrap();

    let text = result.data.as_str().unwrap();
    assert!(text.contains("1\tline one"));
    assert!(text.contains("2\tline two"));
    assert!(text.contains("3\tline three"));
}

#[tokio::test]
async fn test_read_with_offset_limit() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("lines.txt");
    let content: String = (1..=100).map(|i| format!("line {i}\n")).collect();
    std::fs::write(&file, &content).unwrap();

    let ctx = ToolUseContext::test_default();
    let result = ReadTool
        .execute(
            json!({"file_path": file.to_str().unwrap(), "offset": 10, "limit": 5}),
            &ctx,
        )
        .await
        .unwrap();

    let text = result.data.as_str().unwrap();
    assert!(text.contains("11\tline 11"));
    assert!(text.contains("15\tline 15"));
    assert!(!text.contains("16\tline 16"));
    assert!(text.contains("more lines not shown"));
}

#[tokio::test]
async fn test_read_nonexistent_file() {
    let ctx = ToolUseContext::test_default();
    let result = ReadTool
        .execute(json!({"file_path": "/nonexistent/file.txt"}), &ctx)
        .await;

    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.to_string().contains("not found"));
}

#[tokio::test]
async fn test_read_empty_file() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("empty.txt");
    std::fs::File::create(&file).unwrap();

    let ctx = ToolUseContext::test_default();
    let result = ReadTool
        .execute(json!({"file_path": file.to_str().unwrap()}), &ctx)
        .await
        .unwrap();

    let text = result.data.as_str().unwrap();
    assert!(text.contains("empty"));
}

#[tokio::test]
async fn test_read_image_file() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("photo.png");
    let mut f = std::fs::File::create(&file).unwrap();
    f.write_all(b"\x89PNG\r\n").unwrap();

    let ctx = ToolUseContext::test_default();
    let result = ReadTool
        .execute(json!({"file_path": file.to_str().unwrap()}), &ctx)
        .await
        .unwrap();

    let text = result.data.as_str().unwrap();
    assert!(text.contains("image"));
}

#[tokio::test]
async fn test_read_directory_error() {
    let dir = tempfile::tempdir().unwrap();
    let ctx = ToolUseContext::test_default();
    let result = ReadTool
        .execute(json!({"file_path": dir.path().to_str().unwrap()}), &ctx)
        .await;

    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.to_string().contains("directory"));
}

#[tokio::test]
async fn test_read_binary_file() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("data.sqlite");
    std::fs::write(&file, b"\x00\x01\x02\x03").unwrap();

    let ctx = ToolUseContext::test_default();
    let result = ReadTool
        .execute(json!({"file_path": file.to_str().unwrap()}), &ctx)
        .await
        .unwrap();

    let text = result.data.as_str().unwrap();
    assert!(text.contains("binary"));
}
