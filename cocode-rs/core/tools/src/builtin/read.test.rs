use super::*;
use std::io::Write;
use std::path::PathBuf;
use tempfile::NamedTempFile;

fn make_context() -> ToolContext {
    ToolContext::new("call-1", "session-1", PathBuf::from("/tmp"))
}

#[tokio::test]
async fn test_read_file() {
    let mut file = NamedTempFile::new().unwrap();
    writeln!(file, "Line 1").unwrap();
    writeln!(file, "Line 2").unwrap();
    writeln!(file, "Line 3").unwrap();

    let tool = ReadTool::new();
    let mut ctx = make_context();

    let input = serde_json::json!({
        "file_path": file.path().to_str().unwrap()
    });

    let result = tool.execute(input, &mut ctx).await.unwrap();
    let content = match &result.content {
        cocode_protocol::ToolResultContent::Text(t) => t,
        _ => panic!("Expected text content"),
    };

    assert!(content.contains("Line 1"));
    assert!(content.contains("Line 2"));
    assert!(content.contains("Line 3"));
}

#[tokio::test]
async fn test_read_with_offset_and_limit() {
    let mut file = NamedTempFile::new().unwrap();
    for i in 1..=10 {
        writeln!(file, "Line {i}").unwrap();
    }

    let tool = ReadTool::new();
    let mut ctx = make_context();

    let input = serde_json::json!({
        "file_path": file.path().to_str().unwrap(),
        "offset": 3,
        "limit": 2
    });

    let result = tool.execute(input, &mut ctx).await.unwrap();
    let content = match &result.content {
        cocode_protocol::ToolResultContent::Text(t) => t,
        _ => panic!("Expected text content"),
    };

    assert!(content.contains("Line 4"));
    assert!(content.contains("Line 5"));
    assert!(!content.contains("Line 3"));
    assert!(!content.contains("Line 6"));
}

#[tokio::test]
async fn test_read_nonexistent_file() {
    let tool = ReadTool::new();
    let mut ctx = make_context();

    let input = serde_json::json!({
        "file_path": "/nonexistent/file.txt"
    });

    let result = tool.execute(input, &mut ctx).await;
    assert!(result.is_err());
}

#[test]
fn test_tool_properties() {
    let tool = ReadTool::new();
    assert_eq!(tool.name(), "Read");
    assert!(tool.is_concurrent_safe());
}
