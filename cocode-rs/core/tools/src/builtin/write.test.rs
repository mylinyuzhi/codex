use super::*;
use std::path::PathBuf;
use tempfile::TempDir;

fn make_context() -> ToolContext {
    ToolContext::new("call-1", "session-1", PathBuf::from("/tmp"))
}

#[tokio::test]
async fn test_write_new_file() {
    let dir = TempDir::new().unwrap();
    let file_path = dir.path().join("test.txt");

    let tool = WriteTool::new();
    let mut ctx = make_context();

    let input = serde_json::json!({
        "file_path": file_path.to_str().unwrap(),
        "content": "Hello World"
    });

    let result = tool.execute(input, &mut ctx).await.unwrap();
    assert!(!result.is_error);

    let content = std::fs::read_to_string(&file_path).unwrap();
    assert_eq!(content, "Hello World");
}

#[tokio::test]
async fn test_write_creates_parent_dirs() {
    let dir = TempDir::new().unwrap();
    let file_path = dir.path().join("sub").join("dir").join("test.txt");

    let tool = WriteTool::new();
    let mut ctx = make_context();

    let input = serde_json::json!({
        "file_path": file_path.to_str().unwrap(),
        "content": "nested content"
    });

    let result = tool.execute(input, &mut ctx).await.unwrap();
    assert!(!result.is_error);

    let content = std::fs::read_to_string(&file_path).unwrap();
    assert_eq!(content, "nested content");
}

#[tokio::test]
async fn test_write_existing_requires_read() {
    let dir = TempDir::new().unwrap();
    let file_path = dir.path().join("existing.txt");
    std::fs::write(&file_path, "original").unwrap();

    let tool = WriteTool::new();
    let mut ctx = make_context();
    // Don't read the file first

    let input = serde_json::json!({
        "file_path": file_path.to_str().unwrap(),
        "content": "overwritten"
    });

    let result = tool.execute(input, &mut ctx).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_write_existing_after_read() {
    let dir = TempDir::new().unwrap();
    let file_path = dir.path().join("existing.txt");
    std::fs::write(&file_path, "original").unwrap();

    let tool = WriteTool::new();
    let mut ctx = make_context();
    ctx.record_file_read(&file_path).await;

    let input = serde_json::json!({
        "file_path": file_path.to_str().unwrap(),
        "content": "overwritten"
    });

    let result = tool.execute(input, &mut ctx).await.unwrap();
    assert!(!result.is_error);

    let content = std::fs::read_to_string(&file_path).unwrap();
    assert_eq!(content, "overwritten");
}

#[test]
fn test_tool_properties() {
    let tool = WriteTool::new();
    assert_eq!(tool.name(), "Write");
    assert!(!tool.is_concurrent_safe());
}

#[tokio::test]
async fn test_write_new_file_says_created() {
    let dir = TempDir::new().unwrap();
    let file_path = dir.path().join("brand_new.txt");

    let tool = WriteTool::new();
    let mut ctx = make_context();

    let input = serde_json::json!({
        "file_path": file_path.to_str().unwrap(),
        "content": "Hello"
    });

    let result = tool.execute(input, &mut ctx).await.unwrap();
    let text = match &result.content {
        cocode_protocol::ToolResultContent::Text(t) => t,
        _ => panic!("Expected text"),
    };
    assert!(text.contains("Successfully created"));
}

#[tokio::test]
async fn test_write_existing_shows_diff_stats() {
    let dir = TempDir::new().unwrap();
    let file_path = dir.path().join("existing_stats.txt");
    std::fs::write(&file_path, "line1\nline2\n").unwrap();

    let tool = WriteTool::new();
    let mut ctx = make_context();
    ctx.record_file_read(&file_path).await;

    let input = serde_json::json!({
        "file_path": file_path.to_str().unwrap(),
        "content": "line1\nmodified\nextra\n"
    });

    let result = tool.execute(input, &mut ctx).await.unwrap();
    let text = match &result.content {
        cocode_protocol::ToolResultContent::Text(t) => t,
        _ => panic!("Expected text"),
    };
    assert!(text.contains("Successfully wrote to"));
    assert!(text.contains("(+"), "Should contain diff stats");
}

#[tokio::test]
async fn test_plan_mode_blocks_non_plan_file() {
    let dir = TempDir::new().unwrap();
    let file_path = dir.path().join("code.rs");
    let plan_file = dir.path().join("plan.md");

    let tool = WriteTool::new();
    let mut ctx = make_context().with_plan_mode(true, Some(plan_file));

    let input = serde_json::json!({
        "file_path": file_path.to_str().unwrap(),
        "content": "fn main() {}"
    });

    let result = tool.execute(input, &mut ctx).await;
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.to_string().contains("Plan mode"));
}

#[tokio::test]
async fn test_plan_mode_allows_plan_file() {
    let dir = TempDir::new().unwrap();
    let plan_file = dir.path().join("plan.md");

    let tool = WriteTool::new();
    let mut ctx = make_context().with_plan_mode(true, Some(plan_file.clone()));

    let input = serde_json::json!({
        "file_path": plan_file.to_str().unwrap(),
        "content": "# My Plan\n\n- Step 1\n- Step 2"
    });

    let result = tool.execute(input, &mut ctx).await.unwrap();
    assert!(!result.is_error);

    let content = std::fs::read_to_string(&plan_file).unwrap();
    assert!(content.contains("# My Plan"));
}

#[tokio::test]
async fn test_non_plan_mode_allows_any_file() {
    let dir = TempDir::new().unwrap();
    let file_path = dir.path().join("code.rs");

    let tool = WriteTool::new();
    // is_plan_mode = false (default)
    let mut ctx = make_context();

    let input = serde_json::json!({
        "file_path": file_path.to_str().unwrap(),
        "content": "fn main() {}"
    });

    let result = tool.execute(input, &mut ctx).await.unwrap();
    assert!(!result.is_error);
}

#[tokio::test]
async fn test_write_preserves_crlf_line_endings() {
    let dir = TempDir::new().unwrap();
    let file_path = dir.path().join("crlf.txt");

    // Create file with CRLF line endings
    std::fs::write(&file_path, "line1\r\nline2\r\n").unwrap();

    let tool = WriteTool::new();
    let mut ctx = make_context();
    ctx.record_file_read(&file_path).await;

    let input = serde_json::json!({
        "file_path": file_path.to_str().unwrap(),
        "content": "new line1\nnew line2\n"
    });

    let result = tool.execute(input, &mut ctx).await.unwrap();
    assert!(!result.is_error);

    // Verify CRLF was preserved
    let bytes = std::fs::read(&file_path).unwrap();
    assert!(bytes.windows(2).any(|w| w == b"\r\n"));
    assert!(!bytes.contains(&b'\n') || bytes.windows(2).filter(|w| *w == b"\r\n").count() > 0);
}

#[tokio::test]
async fn test_write_new_file_uses_lf() {
    let dir = TempDir::new().unwrap();
    let file_path = dir.path().join("new.txt");

    let tool = WriteTool::new();
    let mut ctx = make_context();

    let input = serde_json::json!({
        "file_path": file_path.to_str().unwrap(),
        "content": "line1\nline2\n"
    });

    let result = tool.execute(input, &mut ctx).await.unwrap();
    assert!(!result.is_error);

    // Verify LF is used for new files
    let bytes = std::fs::read(&file_path).unwrap();
    assert_eq!(bytes, b"line1\nline2\n");
}

// ── SHA256 staleness detection ────────────────────────────────

#[tokio::test]
async fn test_write_sha256_detects_external_modification() {
    let dir = TempDir::new().unwrap();
    let file_path = dir.path().join("sha_write_test.txt");
    std::fs::write(&file_path, "original content").unwrap();

    let tool = WriteTool::new();
    let mut ctx = make_context();

    // Record read state with hash
    let content = "original content".to_string();
    let mtime = std::fs::metadata(&file_path)
        .ok()
        .and_then(|m| m.modified().ok());
    use crate::context::FileReadState;
    ctx.record_file_read_with_state(&file_path, FileReadState::complete(content, mtime))
        .await;

    // Externally modify the file
    std::fs::write(&file_path, "externally modified").unwrap();

    let input = serde_json::json!({
        "file_path": file_path.to_str().unwrap(),
        "content": "new content"
    });

    let result = tool.execute(input, &mut ctx).await;
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.to_string().contains("modified externally"));
}
