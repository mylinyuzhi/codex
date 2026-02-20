use super::*;
use std::io::Write as IoWrite;
use std::path::PathBuf;
use tempfile::NamedTempFile;

fn make_context() -> ToolContext {
    ToolContext::new("call-1", "session-1", PathBuf::from("/tmp"))
}

#[tokio::test]
async fn test_edit_file() {
    let mut file = NamedTempFile::new().unwrap();
    write!(file, "Hello World").unwrap();
    let path = file.path().to_str().unwrap().to_string();

    let tool = EditTool::new();
    let mut ctx = make_context();
    ctx.record_file_read(file.path()).await;

    let input = serde_json::json!({
        "file_path": path,
        "old_string": "World",
        "new_string": "Rust"
    });

    let result = tool.execute(input, &mut ctx).await.unwrap();
    assert!(!result.is_error);

    let content = std::fs::read_to_string(file.path()).unwrap();
    assert_eq!(content, "Hello Rust");
}

#[tokio::test]
async fn test_edit_requires_read_first() {
    let mut file = NamedTempFile::new().unwrap();
    write!(file, "Hello World").unwrap();
    let path = file.path().to_str().unwrap().to_string();

    let tool = EditTool::new();
    let mut ctx = make_context();
    // Don't read the file first

    let input = serde_json::json!({
        "file_path": path,
        "old_string": "World",
        "new_string": "Rust"
    });

    let result = tool.execute(input, &mut ctx).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_edit_non_unique_string() {
    let mut file = NamedTempFile::new().unwrap();
    write!(file, "foo bar foo").unwrap();
    let path = file.path().to_str().unwrap().to_string();

    let tool = EditTool::new();
    let mut ctx = make_context();
    ctx.record_file_read(file.path()).await;

    let input = serde_json::json!({
        "file_path": path,
        "old_string": "foo",
        "new_string": "baz"
    });

    let result = tool.execute(input, &mut ctx).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_edit_replace_all() {
    let mut file = NamedTempFile::new().unwrap();
    write!(file, "foo bar foo").unwrap();
    let path = file.path().to_str().unwrap().to_string();

    let tool = EditTool::new();
    let mut ctx = make_context();
    ctx.record_file_read(file.path()).await;

    let input = serde_json::json!({
        "file_path": path,
        "old_string": "foo",
        "new_string": "baz",
        "replace_all": true
    });

    let result = tool.execute(input, &mut ctx).await.unwrap();
    assert!(!result.is_error);

    let content = std::fs::read_to_string(file.path()).unwrap();
    assert_eq!(content, "baz bar baz");
}

#[test]
fn test_tool_properties() {
    let tool = EditTool::new();
    assert_eq!(tool.name(), "Edit");
    assert!(!tool.is_concurrent_safe());
}

#[tokio::test]
async fn test_plan_mode_blocks_non_plan_file() {
    let mut file = NamedTempFile::new().unwrap();
    write!(file, "Hello World").unwrap();
    let path = file.path().to_str().unwrap().to_string();
    let plan_file = PathBuf::from("/tmp/plan.md");

    let tool = EditTool::new();
    let mut ctx = make_context().with_plan_mode(true, Some(plan_file));
    ctx.record_file_read(file.path()).await;

    let input = serde_json::json!({
        "file_path": path,
        "old_string": "World",
        "new_string": "Rust"
    });

    let result = tool.execute(input, &mut ctx).await;
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.to_string().contains("Plan mode"));
}

#[tokio::test]
async fn test_plan_mode_allows_plan_file() {
    use tempfile::TempDir;
    let dir = TempDir::new().unwrap();
    let plan_file = dir.path().join("plan.md");
    std::fs::write(&plan_file, "# Plan\n\nold content").unwrap();

    let tool = EditTool::new();
    let mut ctx = make_context().with_plan_mode(true, Some(plan_file.clone()));
    ctx.record_file_read(&plan_file).await;

    let input = serde_json::json!({
        "file_path": plan_file.to_str().unwrap(),
        "old_string": "old content",
        "new_string": "new content"
    });

    let result = tool.execute(input, &mut ctx).await.unwrap();
    assert!(!result.is_error);

    let content = std::fs::read_to_string(&plan_file).unwrap();
    assert!(content.contains("new content"));
}

#[tokio::test]
async fn test_non_plan_mode_allows_any_file() {
    let mut file = NamedTempFile::new().unwrap();
    write!(file, "Hello World").unwrap();
    let path = file.path().to_str().unwrap().to_string();

    let tool = EditTool::new();
    let mut ctx = make_context();
    ctx.record_file_read(file.path()).await;

    let input = serde_json::json!({
        "file_path": path,
        "old_string": "World",
        "new_string": "Rust"
    });

    let result = tool.execute(input, &mut ctx).await.unwrap();
    assert!(!result.is_error);
}

#[tokio::test]
async fn test_edit_preserves_crlf_line_endings() {
    use tempfile::TempDir;
    let dir = TempDir::new().unwrap();
    let file_path = dir.path().join("crlf.txt");

    std::fs::write(&file_path, "line1\r\nline2\r\nline3\r\n").unwrap();

    let tool = EditTool::new();
    let mut ctx = make_context();
    ctx.record_file_read(&file_path).await;

    let input = serde_json::json!({
        "file_path": file_path.to_str().unwrap(),
        "old_string": "line2",
        "new_string": "modified"
    });

    let result = tool.execute(input, &mut ctx).await.unwrap();
    assert!(!result.is_error);

    let bytes = std::fs::read(&file_path).unwrap();
    let content = String::from_utf8(bytes).unwrap();
    assert!(content.contains("\r\n"), "CRLF should be preserved");
    assert!(content.contains("modified"), "Edit should be applied");
}

#[tokio::test]
async fn test_edit_preserves_lf_line_endings() {
    use tempfile::TempDir;
    let dir = TempDir::new().unwrap();
    let file_path = dir.path().join("lf.txt");

    std::fs::write(&file_path, "line1\nline2\nline3\n").unwrap();

    let tool = EditTool::new();
    let mut ctx = make_context();
    ctx.record_file_read(&file_path).await;

    let input = serde_json::json!({
        "file_path": file_path.to_str().unwrap(),
        "old_string": "line2",
        "new_string": "modified"
    });

    let result = tool.execute(input, &mut ctx).await.unwrap();
    assert!(!result.is_error);

    let bytes = std::fs::read(&file_path).unwrap();
    let content = String::from_utf8(bytes).unwrap();
    assert!(!content.contains("\r\n"), "LF should be preserved, no CRLF");
    assert!(content.contains("modified"), "Edit should be applied");
}

#[tokio::test]
async fn test_edit_rejects_ipynb_files() {
    use tempfile::TempDir;
    let dir = TempDir::new().unwrap();
    let file_path = dir.path().join("test.ipynb");

    std::fs::write(
        &file_path,
        r#"{"cells": [], "metadata": {}, "nbformat": 4}"#,
    )
    .unwrap();

    let tool = EditTool::new();
    let mut ctx = make_context();
    ctx.record_file_read(&file_path).await;

    let input = serde_json::json!({
        "file_path": file_path.to_str().unwrap(),
        "old_string": "cells",
        "new_string": "modified"
    });

    let result = tool.execute(input, &mut ctx).await;
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.to_string().contains("NotebookEdit"));
}

#[tokio::test]
async fn test_edit_flexible_match_indentation() {
    use tempfile::TempDir;
    let dir = TempDir::new().unwrap();
    let file_path = dir.path().join("indent.rs");

    std::fs::write(
        &file_path,
        "fn main() {\n    let x = 1;\n    let y = 2;\n}\n",
    )
    .unwrap();

    let tool = EditTool::new();
    let mut ctx = make_context();
    ctx.record_file_read(&file_path).await;

    let input = serde_json::json!({
        "file_path": file_path.to_str().unwrap(),
        "old_string": "  let x = 1;\n  let y = 2;",
        "new_string": "  let x = 10;\n  let y = 20;"
    });

    let result = tool.execute(input, &mut ctx).await.unwrap();
    assert!(!result.is_error);

    let content = std::fs::read_to_string(&file_path).unwrap();
    assert!(content.contains("    let x = 10;"));
    assert!(content.contains("    let y = 20;"));
}

#[tokio::test]
async fn test_edit_flexible_match_trailing_spaces() {
    use tempfile::TempDir;
    let dir = TempDir::new().unwrap();
    let file_path = dir.path().join("trailing.txt");

    std::fs::write(&file_path, "hello world\ngoodbye world\n").unwrap();

    let tool = EditTool::new();
    let mut ctx = make_context();
    ctx.record_file_read(&file_path).await;

    let input = serde_json::json!({
        "file_path": file_path.to_str().unwrap(),
        "old_string": "hello world  ",
        "new_string": "hello rust"
    });

    let result = tool.execute(input, &mut ctx).await.unwrap();
    assert!(!result.is_error);

    let content = std::fs::read_to_string(&file_path).unwrap();
    assert!(content.contains("hello rust"));
}

#[tokio::test]
async fn test_edit_flexible_respects_replace_all() {
    use tempfile::TempDir;
    let dir = TempDir::new().unwrap();
    let file_path = dir.path().join("replace_all.txt");

    std::fs::write(&file_path, "    foo bar\n    baz\n    foo bar\n    baz\n").unwrap();

    let tool = EditTool::new();
    let mut ctx = make_context();
    ctx.record_file_read(&file_path).await;

    let input = serde_json::json!({
        "file_path": file_path.to_str().unwrap(),
        "old_string": "foo bar\nbaz",
        "new_string": "replaced\nline",
        "replace_all": true
    });

    let result = tool.execute(input, &mut ctx).await.unwrap();
    assert!(!result.is_error);

    let content = std::fs::read_to_string(&file_path).unwrap();
    assert_eq!(
        content.matches("replaced").count(),
        2,
        "Should replace both occurrences"
    );
}

#[tokio::test]
async fn test_edit_flexible_preserves_crlf() {
    use tempfile::TempDir;
    let dir = TempDir::new().unwrap();
    let file_path = dir.path().join("crlf_flex.txt");

    std::fs::write(&file_path, "    line1\r\n    line2\r\n    line3\r\n").unwrap();

    let tool = EditTool::new();
    let mut ctx = make_context();
    ctx.record_file_read(&file_path).await;

    let input = serde_json::json!({
        "file_path": file_path.to_str().unwrap(),
        "old_string": "line2",
        "new_string": "modified"
    });

    let result = tool.execute(input, &mut ctx).await.unwrap();
    assert!(!result.is_error);

    let bytes = std::fs::read(&file_path).unwrap();
    let content = String::from_utf8(bytes).unwrap();
    assert!(content.contains("\r\n"), "CRLF should be preserved");
    assert!(content.contains("modified"), "Edit should be applied");
}

#[tokio::test]
async fn test_edit_diff_stats_in_output() {
    use tempfile::TempDir;
    let dir = TempDir::new().unwrap();
    let file_path = dir.path().join("stats.txt");

    std::fs::write(&file_path, "line1\nline2\nline3\n").unwrap();

    let tool = EditTool::new();
    let mut ctx = make_context();
    ctx.record_file_read(&file_path).await;

    let input = serde_json::json!({
        "file_path": file_path.to_str().unwrap(),
        "old_string": "line2",
        "new_string": "modified\nextra"
    });

    let result = tool.execute(input, &mut ctx).await.unwrap();
    let text = match &result.content {
        cocode_protocol::ToolResultContent::Text(t) => t,
        _ => panic!("Expected text"),
    };
    assert!(text.contains("Successfully edited"));
    assert!(text.contains("(+"), "Should contain diff stats");
}

// ── File creation tests ─────────────────────────────────────────

#[tokio::test]
async fn test_edit_create_new_file() {
    use tempfile::TempDir;
    let dir = TempDir::new().unwrap();
    let file_path = dir.path().join("new_file.txt");

    let tool = EditTool::new();
    let mut ctx = make_context();

    let input = serde_json::json!({
        "file_path": file_path.to_str().unwrap(),
        "old_string": "",
        "new_string": "hello world"
    });

    let result = tool.execute(input, &mut ctx).await.unwrap();
    assert!(!result.is_error);
    let text = match &result.content {
        cocode_protocol::ToolResultContent::Text(t) => t,
        _ => panic!("Expected text"),
    };
    assert!(text.contains("Created new file"));

    let content = std::fs::read_to_string(&file_path).unwrap();
    assert_eq!(content, "hello world");
}

#[tokio::test]
async fn test_edit_create_existing_file_error() {
    use tempfile::TempDir;
    let dir = TempDir::new().unwrap();
    let file_path = dir.path().join("existing.txt");
    std::fs::write(&file_path, "already here").unwrap();

    let tool = EditTool::new();
    let mut ctx = make_context();

    let input = serde_json::json!({
        "file_path": file_path.to_str().unwrap(),
        "old_string": "",
        "new_string": "overwrite attempt"
    });

    let result = tool.execute(input, &mut ctx).await;
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.to_string().contains("already exists"));
}

#[tokio::test]
async fn test_edit_create_with_parent_dirs() {
    use tempfile::TempDir;
    let dir = TempDir::new().unwrap();
    let file_path = dir.path().join("deep").join("nested").join("file.txt");

    let tool = EditTool::new();
    let mut ctx = make_context();

    let input = serde_json::json!({
        "file_path": file_path.to_str().unwrap(),
        "old_string": "",
        "new_string": "nested content"
    });

    let result = tool.execute(input, &mut ctx).await.unwrap();
    assert!(!result.is_error);

    let content = std::fs::read_to_string(&file_path).unwrap();
    assert_eq!(content, "nested content");
}

// ── SHA256 staleness test ───────────────────────────────────────

#[tokio::test]
async fn test_edit_sha256_detects_external_modification() {
    use tempfile::TempDir;
    let dir = TempDir::new().unwrap();
    let file_path = dir.path().join("sha_test.txt");
    std::fs::write(&file_path, "original content").unwrap();

    let tool = EditTool::new();
    let mut ctx = make_context();

    // Record read state with hash
    let content = "original content".to_string();
    let mtime = std::fs::metadata(&file_path)
        .ok()
        .and_then(|m| m.modified().ok());
    ctx.record_file_read_with_state(&file_path, FileReadState::complete(content, mtime))
        .await;

    // Externally modify the file
    std::fs::write(&file_path, "externally modified").unwrap();

    let input = serde_json::json!({
        "file_path": file_path.to_str().unwrap(),
        "old_string": "original",
        "new_string": "updated"
    });

    let result = tool.execute(input, &mut ctx).await;
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.to_string().contains("modified externally"));
}

// ── Regex fallback test ─────────────────────────────────────────

#[tokio::test]
async fn test_edit_regex_fallback() {
    use tempfile::TempDir;
    let dir = TempDir::new().unwrap();
    let file_path = dir.path().join("regex_test.rs");

    // File has collapsed whitespace
    std::fs::write(&file_path, "function test(){body}\n").unwrap();

    let tool = EditTool::new();
    let mut ctx = make_context();
    ctx.record_file_read(&file_path).await;

    // Model provides with spaces around delimiters
    let input = serde_json::json!({
        "file_path": file_path.to_str().unwrap(),
        "old_string": "function test ( ) { body }",
        "new_string": "function test(){updated}"
    });

    let result = tool.execute(input, &mut ctx).await.unwrap();
    assert!(!result.is_error);

    let content = std::fs::read_to_string(&file_path).unwrap();
    assert!(content.contains("updated"));
    let text = match &result.content {
        cocode_protocol::ToolResultContent::Text(t) => t,
        _ => panic!("Expected text"),
    };
    assert!(text.contains("regex"));
}

// ── Pre-correction unescape test ────────────────────────────────

#[tokio::test]
async fn test_edit_pre_correction_unescape() {
    use tempfile::TempDir;
    let dir = TempDir::new().unwrap();
    let file_path = dir.path().join("unescape_test.txt");

    std::fs::write(&file_path, "line1\nline2\n").unwrap();

    let tool = EditTool::new();
    let mut ctx = make_context();
    ctx.record_file_read(&file_path).await;

    // Model over-escapes: \\n instead of real newline
    let input = serde_json::json!({
        "file_path": file_path.to_str().unwrap(),
        "old_string": "line1\\nline2",
        "new_string": "line1\\nupdated"
    });

    let result = tool.execute(input, &mut ctx).await.unwrap();
    assert!(!result.is_error);

    let content = std::fs::read_to_string(&file_path).unwrap();
    assert!(content.contains("updated"));
}

// ── SHA256 edge case: no hash (legacy read) skips check ───────

#[tokio::test]
async fn test_edit_sha256_no_hash_skips_check() {
    use tempfile::TempDir;
    let dir = TempDir::new().unwrap();
    let file_path = dir.path().join("no_hash.txt");
    std::fs::write(&file_path, "original content").unwrap();

    let tool = EditTool::new();
    let mut ctx = make_context();

    // record_file_read (simple) does NOT store a hash
    ctx.record_file_read(&file_path).await;

    // Externally modify the file — staleness check should be skipped
    std::fs::write(&file_path, "externally modified original content").unwrap();

    let input = serde_json::json!({
        "file_path": file_path.to_str().unwrap(),
        "old_string": "externally modified ",
        "new_string": ""
    });

    // Should succeed because no hash → no staleness check
    let result = tool.execute(input, &mut ctx).await.unwrap();
    assert!(!result.is_error);
}

// ── Sequential edits update hash correctly ────────────────────

#[tokio::test]
async fn test_edit_sequential_edits_update_hash() {
    use tempfile::TempDir;
    let dir = TempDir::new().unwrap();
    let file_path = dir.path().join("seq_edit.txt");
    std::fs::write(&file_path, "aaa bbb ccc").unwrap();

    let tool = EditTool::new();
    let mut ctx = make_context();

    // First read with full state
    let content = "aaa bbb ccc".to_string();
    let mtime = std::fs::metadata(&file_path)
        .ok()
        .and_then(|m| m.modified().ok());
    ctx.record_file_read_with_state(&file_path, FileReadState::complete(content, mtime))
        .await;

    // First edit
    let input1 = serde_json::json!({
        "file_path": file_path.to_str().unwrap(),
        "old_string": "aaa",
        "new_string": "xxx"
    });
    let result1 = tool.execute(input1, &mut ctx).await.unwrap();
    assert!(!result1.is_error);
    assert_eq!(std::fs::read_to_string(&file_path).unwrap(), "xxx bbb ccc");

    // Second edit — should use the updated hash from first edit
    let input2 = serde_json::json!({
        "file_path": file_path.to_str().unwrap(),
        "old_string": "bbb",
        "new_string": "yyy"
    });
    let result2 = tool.execute(input2, &mut ctx).await.unwrap();
    assert!(!result2.is_error);
    assert_eq!(std::fs::read_to_string(&file_path).unwrap(), "xxx yyy ccc");
}
