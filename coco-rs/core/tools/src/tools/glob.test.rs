use crate::tools::glob::GlobTool;
use coco_tool::Tool;
use coco_tool::ToolUseContext;
use serde_json::json;

#[tokio::test]
async fn test_glob_finds_rust_files() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("main.rs"), "fn main() {}").unwrap();
    std::fs::write(dir.path().join("lib.rs"), "pub mod foo;").unwrap();
    std::fs::write(dir.path().join("readme.md"), "# Hello").unwrap();

    let ctx = ToolUseContext::test_default();
    let result = GlobTool
        .execute(
            json!({"pattern": "*.rs", "path": dir.path().to_str().unwrap()}),
            &ctx,
        )
        .await
        .unwrap();

    let text = result.data.as_str().unwrap();
    assert!(text.contains("main.rs"));
    assert!(text.contains("lib.rs"));
    assert!(!text.contains("readme.md"));
}

#[tokio::test]
async fn test_glob_recursive_pattern() {
    let dir = tempfile::tempdir().unwrap();
    let sub = dir.path().join("src");
    std::fs::create_dir_all(&sub).unwrap();
    std::fs::write(sub.join("foo.rs"), "// foo").unwrap();
    std::fs::write(dir.path().join("top.rs"), "// top").unwrap();

    let ctx = ToolUseContext::test_default();
    let result = GlobTool
        .execute(
            json!({"pattern": "**/*.rs", "path": dir.path().to_str().unwrap()}),
            &ctx,
        )
        .await
        .unwrap();

    let text = result.data.as_str().unwrap();
    assert!(text.contains("foo.rs"));
    assert!(text.contains("top.rs"));
}

#[tokio::test]
async fn test_glob_no_matches() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("hello.txt"), "hi").unwrap();

    let ctx = ToolUseContext::test_default();
    let result = GlobTool
        .execute(
            json!({"pattern": "*.xyz", "path": dir.path().to_str().unwrap()}),
            &ctx,
        )
        .await
        .unwrap();

    let text = result.data.as_str().unwrap();
    assert!(text.contains("No files matched"));
}

#[tokio::test]
async fn test_glob_invalid_pattern() {
    let ctx = ToolUseContext::test_default();
    let result = GlobTool
        .execute(json!({"pattern": "[invalid", "path": "/tmp"}), &ctx)
        .await;

    assert!(result.is_err());
}
