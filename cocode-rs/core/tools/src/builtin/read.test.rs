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
    assert_eq!(tool.name(), cocode_protocol::ToolName::Read.as_str());
    assert!(tool.is_concurrent_safe());
}

#[test]
fn test_parse_page_span() {
    assert_eq!(parse_page_span("1-5"), Some(5));
    assert_eq!(parse_page_span("3-15"), Some(13));
    assert_eq!(parse_page_span("1-20"), Some(20));
    assert_eq!(parse_page_span("1-25"), Some(25));
    assert_eq!(parse_page_span("7"), Some(1));
    assert_eq!(parse_page_span("abc"), None);
    assert_eq!(parse_page_span("1-2-3"), None);
}

#[tokio::test]
async fn test_pdf_over_20_pages_requested_returns_error() {
    // Create a fake PDF file (just needs .pdf extension for the code path)
    let dir = tempfile::tempdir().unwrap();
    let pdf_path = dir.path().join("test.pdf");
    std::fs::write(&pdf_path, b"%PDF-1.4 fake").unwrap();

    let tool = ReadTool::new();
    let mut ctx = make_context();

    let input = serde_json::json!({
        "file_path": pdf_path.to_str().unwrap(),
        "pages": "1-25"
    });

    let result = tool.execute(input, &mut ctx).await;
    assert!(result.is_err());
    let err_msg = format!("{}", result.unwrap_err());
    assert!(
        err_msg.contains("20"),
        "Error should mention 20-page limit: {err_msg}"
    );
}
