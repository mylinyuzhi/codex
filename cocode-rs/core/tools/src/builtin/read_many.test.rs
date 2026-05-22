use super::*;
use std::path::PathBuf;
use tempfile::TempDir;

fn make_context(cwd: PathBuf) -> ToolContext {
    ToolContext::new("call-1", "session-1", cwd)
}

#[tokio::test]
async fn test_read_many_basic() {
    let dir = TempDir::new().unwrap();
    std::fs::write(dir.path().join("a.txt"), "hello\n").unwrap();
    std::fs::write(dir.path().join("b.txt"), "world\n").unwrap();

    let tool = ReadManyFilesTool::new();
    let mut ctx = make_context(dir.path().to_path_buf());

    let input = serde_json::json!({
        "paths": [
            dir.path().join("a.txt").to_str().unwrap(),
            dir.path().join("b.txt").to_str().unwrap()
        ]
    });

    let result = tool.execute(input, &mut ctx).await.unwrap();
    let content = match &result.content {
        cocode_protocol::ToolResultContent::Text(t) => t,
        _ => panic!("Expected text"),
    };

    assert!(content.contains("a.txt"));
    assert!(content.contains("hello"));
    assert!(content.contains("b.txt"));
    assert!(content.contains("world"));
    assert_eq!(result.modifiers.len(), 2);
}

#[tokio::test]
async fn test_read_many_missing_file() {
    let dir = TempDir::new().unwrap();
    std::fs::write(dir.path().join("exists.txt"), "content\n").unwrap();

    let tool = ReadManyFilesTool::new();
    let mut ctx = make_context(dir.path().to_path_buf());

    let input = serde_json::json!({
        "paths": [
            dir.path().join("exists.txt").to_str().unwrap(),
            dir.path().join("missing.txt").to_str().unwrap()
        ]
    });

    let result = tool.execute(input, &mut ctx).await.unwrap();
    let content = match &result.content {
        cocode_protocol::ToolResultContent::Text(t) => t,
        _ => panic!("Expected text"),
    };

    assert!(content.contains("content"));
    assert!(content.contains("[NOT FOUND]"));
}

#[tokio::test]
async fn test_read_many_empty_paths() {
    let dir = TempDir::new().unwrap();
    let tool = ReadManyFilesTool::new();
    let mut ctx = make_context(dir.path().to_path_buf());

    let input = serde_json::json!({
        "paths": []
    });

    let result = tool.execute(input, &mut ctx).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_read_many_tracks_file_reads() {
    let dir = TempDir::new().unwrap();
    let file_path = dir.path().join("tracked.txt");
    std::fs::write(&file_path, "tracked content\n").unwrap();

    let tool = ReadManyFilesTool::new();
    let mut ctx = make_context(dir.path().to_path_buf());

    let input = serde_json::json!({
        "paths": [file_path.to_str().unwrap()]
    });

    tool.execute(input, &mut ctx).await.unwrap();

    // File should be recorded as read
    assert!(ctx.was_file_read(&file_path).await);
}

#[test]
fn test_tool_properties() {
    let tool = ReadManyFilesTool::new();
    assert_eq!(tool.name(), "ReadManyFiles");
    assert!(tool.is_concurrent_safe());
    assert!(tool.is_read_only()); // default is true
}
