use super::*;
use std::fs::File;
use std::io::Write;
use tempfile::TempDir;

fn make_context(cwd: PathBuf) -> ToolContext {
    ToolContext::new("call-1", "session-1", cwd)
}

fn setup_test_dir() -> TempDir {
    let dir = TempDir::new().unwrap();

    // Create test files
    let mut file1 = File::create(dir.path().join("file1.rs")).unwrap();
    writeln!(file1, "fn main() {{").unwrap();
    writeln!(file1, "    println!(\"Hello, world!\");").unwrap();
    writeln!(file1, "}}").unwrap();

    let mut file2 = File::create(dir.path().join("file2.rs")).unwrap();
    writeln!(file2, "fn test_something() {{").unwrap();
    writeln!(file2, "    assert!(true);").unwrap();
    writeln!(file2, "}}").unwrap();

    let mut file3 = File::create(dir.path().join("other.txt")).unwrap();
    writeln!(file3, "This is a text file.").unwrap();
    writeln!(file3, "It has some content.").unwrap();

    dir
}

#[tokio::test]
async fn test_grep_basic() {
    let dir = setup_test_dir();
    let tool = GrepTool::new();
    let mut ctx = make_context(dir.path().to_path_buf());

    let input = serde_json::json!({
        "pattern": "fn "
    });

    let result = tool.execute(input, &mut ctx).await.unwrap();
    let content = match &result.content {
        cocode_protocol::ToolResultContent::Text(t) => t,
        _ => panic!("Expected text content"),
    };

    assert!(content.contains("file1.rs"));
    assert!(content.contains("file2.rs"));
}

#[tokio::test]
async fn test_grep_with_glob() {
    let dir = setup_test_dir();
    let tool = GrepTool::new();
    let mut ctx = make_context(dir.path().to_path_buf());

    let input = serde_json::json!({
        "pattern": "fn ",
        "glob": "*.rs"
    });

    let result = tool.execute(input, &mut ctx).await.unwrap();
    let content = match &result.content {
        cocode_protocol::ToolResultContent::Text(t) => t,
        _ => panic!("Expected text content"),
    };

    assert!(content.contains("file1.rs"));
    assert!(!content.contains("other.txt"));
}

#[tokio::test]
async fn test_grep_content_mode() {
    let dir = setup_test_dir();
    let tool = GrepTool::new();
    let mut ctx = make_context(dir.path().to_path_buf());

    let input = serde_json::json!({
        "pattern": "println",
        "output_mode": "content"
    });

    let result = tool.execute(input, &mut ctx).await.unwrap();
    let content = match &result.content {
        cocode_protocol::ToolResultContent::Text(t) => t,
        _ => panic!("Expected text content"),
    };

    assert!(content.contains("println"));
    assert!(content.contains("Hello, world!"));
}

#[tokio::test]
async fn test_grep_case_insensitive() {
    let dir = setup_test_dir();
    let tool = GrepTool::new();
    let mut ctx = make_context(dir.path().to_path_buf());

    let input = serde_json::json!({
        "pattern": "HELLO",
        "-i": true,
        "output_mode": "content"
    });

    let result = tool.execute(input, &mut ctx).await.unwrap();
    let content = match &result.content {
        cocode_protocol::ToolResultContent::Text(t) => t,
        _ => panic!("Expected text content"),
    };

    assert!(content.contains("Hello"));
}

#[tokio::test]
async fn test_grep_no_matches() {
    let dir = setup_test_dir();
    let tool = GrepTool::new();
    let mut ctx = make_context(dir.path().to_path_buf());

    let input = serde_json::json!({
        "pattern": "nonexistent_pattern_xyz"
    });

    let result = tool.execute(input, &mut ctx).await.unwrap();
    let content = match &result.content {
        cocode_protocol::ToolResultContent::Text(t) => t,
        _ => panic!("Expected text content"),
    };

    assert!(content.contains("No matches found"));
}

#[test]
fn test_tool_properties() {
    let tool = GrepTool::new();
    assert_eq!(tool.name(), "Grep");
    assert!(tool.is_concurrent_safe());
}

#[tokio::test]
async fn test_grep_respects_gitignore() {
    let dir = TempDir::new().unwrap();

    // Create .gitignore that excludes *.log
    std::fs::write(dir.path().join(".gitignore"), "*.log\n").unwrap();

    // Create files
    let mut rs_file = File::create(dir.path().join("main.rs")).unwrap();
    writeln!(rs_file, "fn hello() {{}}").unwrap();

    let mut log_file = File::create(dir.path().join("debug.log")).unwrap();
    writeln!(log_file, "fn should_be_ignored() {{}}").unwrap();

    let tool = GrepTool::new();
    let mut ctx = make_context(dir.path().to_path_buf());

    let input = serde_json::json!({
        "pattern": "fn "
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
async fn test_grep_skips_binary_files() {
    let dir = TempDir::new().unwrap();

    // Create a text file with a match
    let mut text_file = File::create(dir.path().join("text.rs")).unwrap();
    writeln!(text_file, "fn search_me() {{}}").unwrap();

    // Create a binary file with null bytes
    let mut binary_file = File::create(dir.path().join("binary.bin")).unwrap();
    binary_file
        .write_all(b"fn search_me() {}\x00\x00\x00binary data")
        .unwrap();

    let tool = GrepTool::new();
    let mut ctx = make_context(dir.path().to_path_buf());

    let input = serde_json::json!({
        "pattern": "search_me"
    });

    let result = tool.execute(input, &mut ctx).await.unwrap();
    let content = match &result.content {
        cocode_protocol::ToolResultContent::Text(t) => t,
        _ => panic!("Expected text content"),
    };

    assert!(content.contains("text.rs"));
    // Binary file should be skipped by grep-searcher
    assert!(!content.contains("binary.bin"));
}

#[tokio::test]
async fn test_grep_context_lines_with_sink() {
    let dir = TempDir::new().unwrap();

    let mut file = File::create(dir.path().join("ctx.txt")).unwrap();
    writeln!(file, "line 1").unwrap();
    writeln!(file, "line 2 match").unwrap();
    writeln!(file, "line 3").unwrap();
    writeln!(file, "line 4").unwrap();
    writeln!(file, "line 5 match").unwrap();
    writeln!(file, "line 6").unwrap();

    let tool = GrepTool::new();
    let mut ctx = make_context(dir.path().to_path_buf());

    let input = serde_json::json!({
        "pattern": "match",
        "output_mode": "content",
        "-B": 1,
        "-A": 1
    });

    let result = tool.execute(input, &mut ctx).await.unwrap();
    let content = match &result.content {
        cocode_protocol::ToolResultContent::Text(t) => t,
        _ => panic!("Expected text content"),
    };

    // Should contain match lines (with :)
    assert!(content.contains("line 2 match"));
    assert!(content.contains("line 5 match"));
    // Should contain context lines (with -)
    assert!(content.contains("line 1"));
    assert!(content.contains("line 3"));
    assert!(content.contains("line 4"));
    assert!(content.contains("line 6"));
}

#[tokio::test]
async fn test_grep_count_mode() {
    let dir = setup_test_dir();
    let tool = GrepTool::new();
    let mut ctx = make_context(dir.path().to_path_buf());

    let input = serde_json::json!({
        "pattern": "fn ",
        "output_mode": "count"
    });

    let result = tool.execute(input, &mut ctx).await.unwrap();
    let content = match &result.content {
        cocode_protocol::ToolResultContent::Text(t) => t,
        _ => panic!("Expected text content"),
    };

    // Each .rs file has one "fn " match
    assert!(content.contains(":1"));
}

#[tokio::test]
async fn test_grep_multiline_cross_line() {
    let dir = TempDir::new().unwrap();

    let mut file = File::create(dir.path().join("multi.txt")).unwrap();
    writeln!(file, "fn hello() {{").unwrap();
    writeln!(file, "    world").unwrap();
    writeln!(file, "}}").unwrap();

    let tool = GrepTool::new();
    let mut ctx = make_context(dir.path().to_path_buf());

    let input = serde_json::json!({
        "pattern": "hello.*world",
        "multiline": true,
        "output_mode": "content"
    });

    let result = tool.execute(input, &mut ctx).await.unwrap();
    let content = match &result.content {
        cocode_protocol::ToolResultContent::Text(t) => t,
        _ => panic!("Expected text content"),
    };

    // Should match across lines when multiline is enabled
    assert!(content.contains("hello"));
    assert!(content.contains("world"));
}

#[tokio::test]
async fn test_grep_context_break_separators() {
    let dir = TempDir::new().unwrap();

    // Create a file with two matches far apart so context groups are disjoint
    let mut file = File::create(dir.path().join("breaks.txt")).unwrap();
    writeln!(file, "line 1 match").unwrap();
    writeln!(file, "line 2").unwrap();
    writeln!(file, "line 3").unwrap();
    writeln!(file, "line 4").unwrap();
    writeln!(file, "line 5").unwrap();
    writeln!(file, "line 6").unwrap();
    writeln!(file, "line 7").unwrap();
    writeln!(file, "line 8 match").unwrap();

    let tool = GrepTool::new();
    let mut ctx = make_context(dir.path().to_path_buf());

    let input = serde_json::json!({
        "pattern": "match",
        "output_mode": "content",
        "-A": 1
    });

    let result = tool.execute(input, &mut ctx).await.unwrap();
    let content = match &result.content {
        cocode_protocol::ToolResultContent::Text(t) => t,
        _ => panic!("Expected text content"),
    };

    // Should have a -- separator between disjoint context groups
    assert!(content.contains("line 1 match"));
    assert!(content.contains("line 8 match"));
    assert!(content.contains("  --"));
}

#[tokio::test]
async fn test_grep_content_grouped_by_file() {
    let dir = setup_test_dir();
    let tool = GrepTool::new();
    let mut ctx = make_context(dir.path().to_path_buf());

    let input = serde_json::json!({
        "pattern": "fn ",
        "output_mode": "content"
    });

    let result = tool.execute(input, &mut ctx).await.unwrap();
    let content = match &result.content {
        cocode_protocol::ToolResultContent::Text(t) => t,
        _ => panic!("Expected text content"),
    };

    // Output should have file headers (ending with :) and indented match lines
    assert!(content.contains("file1.rs:") || content.contains("file2.rs:"));
    // Lines should be indented with 2 spaces
    assert!(content.contains("  "));
}
