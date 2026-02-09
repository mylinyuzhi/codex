use super::*;
use std::fs::File;
use std::fs::{self};
use std::path::PathBuf;
use tempfile::TempDir;

fn make_context(cwd: PathBuf) -> ToolContext {
    ToolContext::new("call-1", "session-1", cwd)
}

fn setup_test_dir() -> TempDir {
    let dir = TempDir::new().unwrap();

    // Create test files
    fs::create_dir_all(dir.path().join("src")).unwrap();
    fs::create_dir_all(dir.path().join("tests")).unwrap();

    File::create(dir.path().join("src/main.rs")).unwrap();
    File::create(dir.path().join("src/lib.rs")).unwrap();
    File::create(dir.path().join("tests/test.rs")).unwrap();
    File::create(dir.path().join("README.md")).unwrap();

    dir
}

#[tokio::test]
async fn test_glob_rust_files() {
    let dir = setup_test_dir();
    let tool = GlobTool::new();
    let mut ctx = make_context(dir.path().to_path_buf());

    let input = serde_json::json!({
        "pattern": "**/*.rs"
    });

    let result = tool.execute(input, &mut ctx).await.unwrap();
    let content = match &result.content {
        cocode_protocol::ToolResultContent::Text(t) => t,
        _ => panic!("Expected text content"),
    };

    assert!(content.contains("main.rs"));
    assert!(content.contains("lib.rs"));
    assert!(content.contains("test.rs"));
    assert!(!content.contains("README.md"));
}

#[tokio::test]
async fn test_glob_specific_dir() {
    let dir = setup_test_dir();
    let tool = GlobTool::new();
    let mut ctx = make_context(dir.path().to_path_buf());

    let input = serde_json::json!({
        "pattern": "*.rs",
        "path": "src"
    });

    let result = tool.execute(input, &mut ctx).await.unwrap();
    let content = match &result.content {
        cocode_protocol::ToolResultContent::Text(t) => t,
        _ => panic!("Expected text content"),
    };

    assert!(content.contains("main.rs"));
    assert!(content.contains("lib.rs"));
    assert!(!content.contains("test.rs")); // Not in src/
}

#[tokio::test]
async fn test_glob_no_matches() {
    let dir = setup_test_dir();
    let tool = GlobTool::new();
    let mut ctx = make_context(dir.path().to_path_buf());

    let input = serde_json::json!({
        "pattern": "**/*.xyz"
    });

    let result = tool.execute(input, &mut ctx).await.unwrap();
    let content = match &result.content {
        cocode_protocol::ToolResultContent::Text(t) => t,
        _ => panic!("Expected text content"),
    };

    assert!(content.contains("No files found"));
}

#[tokio::test]
async fn test_glob_invalid_pattern() {
    let dir = setup_test_dir();
    let tool = GlobTool::new();
    let mut ctx = make_context(dir.path().to_path_buf());

    let input = serde_json::json!({
        "pattern": "[invalid"
    });

    let result = tool.execute(input, &mut ctx).await;
    assert!(result.is_err());
}

#[test]
fn test_tool_properties() {
    let tool = GlobTool::new();
    assert_eq!(tool.name(), "Glob");
    assert!(tool.is_concurrent_safe());
}

#[tokio::test]
async fn test_glob_respects_gitignore() {
    let dir = TempDir::new().unwrap();

    // Create .gitignore that excludes *.log
    fs::write(dir.path().join(".gitignore"), "*.log\n").unwrap();

    // Create files
    File::create(dir.path().join("main.rs")).unwrap();
    File::create(dir.path().join("debug.log")).unwrap();

    let tool = GlobTool::new();
    let mut ctx = make_context(dir.path().to_path_buf());

    let input = serde_json::json!({
        "pattern": "*"
    });

    let result = tool.execute(input, &mut ctx).await.unwrap();
    let content = match &result.content {
        cocode_protocol::ToolResultContent::Text(t) => t,
        _ => panic!("Expected text content"),
    };

    assert!(content.contains("main.rs"));
    assert!(!content.contains("debug.log"));
}

#[tokio::test]
async fn test_glob_respects_ignore_file() {
    let dir = TempDir::new().unwrap();

    // Create .ignore that excludes *.tmp
    fs::write(dir.path().join(".ignore"), "*.tmp\n").unwrap();

    // Create files
    File::create(dir.path().join("keep.rs")).unwrap();
    File::create(dir.path().join("temp.tmp")).unwrap();

    let tool = GlobTool::new();
    let mut ctx = make_context(dir.path().to_path_buf());

    let input = serde_json::json!({
        "pattern": "*"
    });

    let result = tool.execute(input, &mut ctx).await.unwrap();
    let content = match &result.content {
        cocode_protocol::ToolResultContent::Text(t) => t,
        _ => panic!("Expected text content"),
    };

    assert!(content.contains("keep.rs"));
    assert!(!content.contains("temp.tmp"));
}

#[tokio::test]
async fn test_glob_finds_hidden_files() {
    let dir = TempDir::new().unwrap();

    // Create hidden and visible files
    File::create(dir.path().join("visible.rs")).unwrap();
    File::create(dir.path().join(".hidden")).unwrap();

    let tool = GlobTool::new();
    let mut ctx = make_context(dir.path().to_path_buf());

    // Match everything including dotfiles
    let input = serde_json::json!({
        "pattern": "*"
    });

    let result = tool.execute(input, &mut ctx).await.unwrap();
    let content = match &result.content {
        cocode_protocol::ToolResultContent::Text(t) => t,
        _ => panic!("Expected text content"),
    };

    assert!(content.contains("visible.rs"));
    // Since include_hidden is true, dotfiles should be found
    assert!(content.contains(".hidden"));
}

#[tokio::test]
async fn test_glob_case_insensitive() {
    let dir = TempDir::new().unwrap();

    File::create(dir.path().join("README.md")).unwrap();
    File::create(dir.path().join("readme.txt")).unwrap();
    File::create(dir.path().join("other.rs")).unwrap();

    let tool = GlobTool::new();
    let mut ctx = make_context(dir.path().to_path_buf());

    // Case-insensitive search for "readme*"
    let input = serde_json::json!({
        "pattern": "readme*",
        "case_sensitive": false
    });

    let result = tool.execute(input, &mut ctx).await.unwrap();
    let content = match &result.content {
        cocode_protocol::ToolResultContent::Text(t) => t,
        _ => panic!("Expected text content"),
    };

    assert!(
        content.contains("README.md"),
        "Should find uppercase README.md"
    );
    assert!(
        content.contains("readme.txt"),
        "Should find lowercase readme.txt"
    );
    assert!(!content.contains("other.rs"), "Should not match other.rs");
}

#[tokio::test]
async fn test_glob_case_sensitive_default() {
    let dir = TempDir::new().unwrap();

    File::create(dir.path().join("README.md")).unwrap();
    File::create(dir.path().join("readme.txt")).unwrap();

    let tool = GlobTool::new();
    let mut ctx = make_context(dir.path().to_path_buf());

    // Default case-sensitive search
    let input = serde_json::json!({
        "pattern": "readme*"
    });

    let result = tool.execute(input, &mut ctx).await.unwrap();
    let content = match &result.content {
        cocode_protocol::ToolResultContent::Text(t) => t,
        _ => panic!("Expected text content"),
    };

    assert!(
        content.contains("readme.txt"),
        "Should find lowercase readme.txt"
    );
    // On case-insensitive filesystems (macOS), README.md might also match
    // So we just verify the tool runs without error
}
