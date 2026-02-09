use super::*;
use std::fs;
use std::fs::File;
use std::path::PathBuf;
use tempfile::TempDir;

fn make_context(cwd: PathBuf) -> ToolContext {
    ToolContext::new("call-1", "session-1", cwd)
}

fn setup_test_dir() -> TempDir {
    let dir = TempDir::new().unwrap();

    fs::create_dir_all(dir.path().join("src")).unwrap();
    fs::create_dir_all(dir.path().join("tests")).unwrap();

    File::create(dir.path().join("src/main.rs")).unwrap();
    File::create(dir.path().join("src/lib.rs")).unwrap();
    File::create(dir.path().join("tests/test.rs")).unwrap();
    File::create(dir.path().join("README.md")).unwrap();

    dir
}

#[tokio::test]
async fn test_ls_basic() {
    let dir = setup_test_dir();
    let tool = LsTool::new();
    let mut ctx = make_context(dir.path().to_path_buf());

    // Default depth=1: only immediate children
    let input = serde_json::json!({
        "path": dir.path().to_str().unwrap()
    });

    let result = tool.execute(input, &mut ctx).await.unwrap();
    let content = match &result.content {
        cocode_protocol::ToolResultContent::Text(t) => t,
        _ => panic!("Expected text content"),
    };

    assert!(content.contains("src/"));
    assert!(content.contains("tests/"));
    assert!(content.contains("README.md"));
    assert!(content.contains("Absolute path:"));
    // depth=1 should NOT show files inside subdirectories
    assert!(!content.contains("main.rs"));
    assert!(!content.contains("lib.rs"));
    assert!(!content.contains("test.rs"));
}

#[tokio::test]
async fn test_ls_basic_with_depth2() {
    let dir = setup_test_dir();
    let tool = LsTool::new();
    let mut ctx = make_context(dir.path().to_path_buf());

    // Explicit depth=2: immediate children + their children
    let input = serde_json::json!({
        "path": dir.path().to_str().unwrap(),
        "depth": 2
    });

    let result = tool.execute(input, &mut ctx).await.unwrap();
    let content = match &result.content {
        cocode_protocol::ToolResultContent::Text(t) => t,
        _ => panic!("Expected text content"),
    };

    assert!(content.contains("src/"));
    assert!(content.contains("tests/"));
    assert!(content.contains("main.rs"));
    assert!(content.contains("lib.rs"));
    assert!(content.contains("test.rs"));
    assert!(content.contains("README.md"));
    assert!(content.contains("Absolute path:"));
}

#[tokio::test]
async fn test_ls_depth() {
    let dir = TempDir::new().unwrap();

    fs::create_dir_all(dir.path().join("a/b/c")).unwrap();
    fs::write(dir.path().join("root.txt"), "root").unwrap();
    fs::write(dir.path().join("a/level1.txt"), "level1").unwrap();
    fs::write(dir.path().join("a/b/level2.txt"), "level2").unwrap();
    fs::write(dir.path().join("a/b/c/level3.txt"), "level3").unwrap();

    let tool = LsTool::new();
    let mut ctx = make_context(dir.path().to_path_buf());

    // depth=1: only immediate children
    let input = serde_json::json!({
        "path": dir.path().to_str().unwrap(),
        "depth": 1
    });
    let result = tool.execute(input, &mut ctx).await.unwrap();
    let content = match &result.content {
        cocode_protocol::ToolResultContent::Text(t) => t,
        _ => panic!("Expected text content"),
    };
    assert!(content.contains("root.txt"));
    assert!(content.contains("a/"));
    assert!(!content.contains("level1.txt"));

    // depth=2: children + grandchildren
    let input = serde_json::json!({
        "path": dir.path().to_str().unwrap(),
        "depth": 2
    });
    let result = tool.execute(input, &mut ctx).await.unwrap();
    let content = match &result.content {
        cocode_protocol::ToolResultContent::Text(t) => t,
        _ => panic!("Expected text content"),
    };
    assert!(content.contains("root.txt"));
    assert!(content.contains("a/"));
    assert!(content.contains("level1.txt"));
    assert!(content.contains("b/"));
    assert!(!content.contains("level2.txt"));
}

#[tokio::test]
async fn test_ls_dirs_first_sorting() {
    let dir = TempDir::new().unwrap();

    fs::create_dir(dir.path().join("zebra_dir")).unwrap();
    fs::write(dir.path().join("alpha.txt"), "alpha").unwrap();
    fs::create_dir(dir.path().join("alpha_dir")).unwrap();
    fs::write(dir.path().join("zebra.txt"), "zebra").unwrap();

    let tool = LsTool::new();
    let mut ctx = make_context(dir.path().to_path_buf());

    let input = serde_json::json!({
        "path": dir.path().to_str().unwrap(),
        "depth": 1
    });
    let result = tool.execute(input, &mut ctx).await.unwrap();
    let content = match &result.content {
        cocode_protocol::ToolResultContent::Text(t) => t,
        _ => panic!("Expected text content"),
    };

    // Directories should appear before files
    let lines: Vec<&str> = content.lines().collect();
    let mut found_file = false;
    for line in &lines[2..] {
        // skip header lines
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with("More") {
            continue;
        }
        if !trimmed.ends_with('/') && !trimmed.ends_with('@') && !trimmed.ends_with('?') {
            found_file = true;
        } else if trimmed.ends_with('/') && found_file {
            panic!("Directory found after file — sorting is wrong: {content}");
        }
    }
}

#[tokio::test]
async fn test_ls_pagination() {
    let dir = TempDir::new().unwrap();

    // Create 10 files
    for i in 0..10 {
        fs::write(dir.path().join(format!("file_{i:02}.txt")), "content").unwrap();
    }

    let tool = LsTool::new();
    let mut ctx = make_context(dir.path().to_path_buf());

    // Get first 3
    let input = serde_json::json!({
        "path": dir.path().to_str().unwrap(),
        "depth": 1,
        "offset": 1,
        "limit": 3
    });
    let result = tool.execute(input, &mut ctx).await.unwrap();
    let content = match &result.content {
        cocode_protocol::ToolResultContent::Text(t) => t,
        _ => panic!("Expected text content"),
    };
    assert!(content.contains("[3 of 10 entries shown]"));
    assert!(content.contains("More entries available"));

    // Get from offset 8
    let input = serde_json::json!({
        "path": dir.path().to_str().unwrap(),
        "depth": 1,
        "offset": 8,
        "limit": 5
    });
    let result = tool.execute(input, &mut ctx).await.unwrap();
    let content = match &result.content {
        cocode_protocol::ToolResultContent::Text(t) => t,
        _ => panic!("Expected text content"),
    };
    // 10 - 7 = 3 remaining
    assert!(content.contains("[3 of 10 entries shown]"));
    assert!(!content.contains("More entries available"));
}

#[tokio::test]
async fn test_ls_respects_gitignore() {
    let dir = TempDir::new().unwrap();

    fs::write(dir.path().join(".gitignore"), "*.log\n").unwrap();
    File::create(dir.path().join("main.rs")).unwrap();
    File::create(dir.path().join("debug.log")).unwrap();

    let tool = LsTool::new();
    let mut ctx = make_context(dir.path().to_path_buf());

    let input = serde_json::json!({
        "path": dir.path().to_str().unwrap()
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
async fn test_ls_respects_ignore() {
    let dir = TempDir::new().unwrap();

    fs::write(dir.path().join(".ignore"), "*.env\n").unwrap();
    File::create(dir.path().join("keep.rs")).unwrap();
    File::create(dir.path().join("secret.env")).unwrap();

    let tool = LsTool::new();
    let mut ctx = make_context(dir.path().to_path_buf());

    let input = serde_json::json!({
        "path": dir.path().to_str().unwrap()
    });
    let result = tool.execute(input, &mut ctx).await.unwrap();
    let content = match &result.content {
        cocode_protocol::ToolResultContent::Text(t) => t,
        _ => panic!("Expected text content"),
    };

    assert!(content.contains("keep.rs"));
    assert!(!content.contains("secret.env"));
}

#[tokio::test]
async fn test_ls_shows_dotfiles() {
    let dir = TempDir::new().unwrap();

    File::create(dir.path().join("visible.rs")).unwrap();
    File::create(dir.path().join(".hidden")).unwrap();

    let tool = LsTool::new();
    let mut ctx = make_context(dir.path().to_path_buf());

    let input = serde_json::json!({
        "path": dir.path().to_str().unwrap()
    });
    let result = tool.execute(input, &mut ctx).await.unwrap();
    let content = match &result.content {
        cocode_protocol::ToolResultContent::Text(t) => t,
        _ => panic!("Expected text content"),
    };

    assert!(content.contains("visible.rs"));
    assert!(content.contains(".hidden"));
}

#[tokio::test]
async fn test_ls_empty_directory() {
    let dir = TempDir::new().unwrap();
    let tool = LsTool::new();
    let mut ctx = make_context(dir.path().to_path_buf());

    let input = serde_json::json!({
        "path": dir.path().to_str().unwrap()
    });
    let result = tool.execute(input, &mut ctx).await.unwrap();
    let content = match &result.content {
        cocode_protocol::ToolResultContent::Text(t) => t,
        _ => panic!("Expected text content"),
    };

    assert!(content.contains("[Empty directory]"));
}

#[cfg(unix)]
#[tokio::test]
async fn test_ls_symlink_annotation() {
    use std::os::unix::fs::symlink;

    let dir = TempDir::new().unwrap();
    fs::write(dir.path().join("target.txt"), "target").unwrap();
    symlink(dir.path().join("target.txt"), dir.path().join("link")).unwrap();

    let tool = LsTool::new();
    let mut ctx = make_context(dir.path().to_path_buf());

    let input = serde_json::json!({
        "path": dir.path().to_str().unwrap()
    });
    let result = tool.execute(input, &mut ctx).await.unwrap();
    let content = match &result.content {
        cocode_protocol::ToolResultContent::Text(t) => t,
        _ => panic!("Expected text content"),
    };

    assert!(content.contains("link@"));
}

#[tokio::test]
async fn test_ls_nonexistent_path() {
    let dir = TempDir::new().unwrap();
    let tool = LsTool::new();
    let mut ctx = make_context(dir.path().to_path_buf());

    let input = serde_json::json!({
        "path": dir.path().join("nonexistent").to_str().unwrap()
    });
    let result = tool.execute(input, &mut ctx).await;
    assert!(result.is_err());
}

#[test]
fn test_tool_properties() {
    let tool = LsTool::new();
    assert_eq!(tool.name(), "LS");
    assert!(tool.is_concurrent_safe());
    assert!(tool.is_read_only());
}

#[test]
fn test_collect_entries_truncation() {
    let dir = TempDir::new().unwrap();

    // Create more files than MAX_COLLECT
    for i in 0..MAX_COLLECT + 100 {
        fs::write(dir.path().join(format!("file_{i:05}.txt")), "content").unwrap();
    }

    let ignore_config = IgnoreConfig::default().with_hidden(true);
    let ignore_service = IgnoreService::new(ignore_config);

    let (entries, truncated) = collect_entries(dir.path(), 1, &ignore_service);
    assert!(truncated, "should be truncated when exceeding MAX_COLLECT");
    assert_eq!(entries.len(), MAX_COLLECT);
}

#[test]
fn test_collect_entries_no_truncation() {
    let dir = TempDir::new().unwrap();

    for i in 0..10 {
        fs::write(dir.path().join(format!("file_{i}.txt")), "content").unwrap();
    }

    let ignore_config = IgnoreConfig::default().with_hidden(true);
    let ignore_service = IgnoreService::new(ignore_config);

    let (entries, truncated) = collect_entries(dir.path(), 1, &ignore_service);
    assert!(!truncated);
    assert_eq!(entries.len(), 10);
}

#[tokio::test]
async fn test_ls_truncation_message() {
    let dir = TempDir::new().unwrap();

    // Create more files than MAX_COLLECT
    for i in 0..MAX_COLLECT + 100 {
        fs::write(dir.path().join(format!("file_{i:05}.txt")), "content").unwrap();
    }

    let tool = LsTool::new();
    let mut ctx = make_context(dir.path().to_path_buf());

    let input = serde_json::json!({
        "path": dir.path().to_str().unwrap()
    });

    let result = tool.execute(input, &mut ctx).await.unwrap();
    let content = match &result.content {
        cocode_protocol::ToolResultContent::Text(t) => t,
        _ => panic!("Expected text content"),
    };

    assert!(content.contains("Results truncated at"));
    assert!(content.contains("use a more specific path or reduce depth"));
}
