use crate::tools::grep::GrepTool;
use coco_tool::Tool;
use coco_tool::ToolUseContext;
use serde_json::json;

#[tokio::test]
async fn test_grep_files_with_matches() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.rs"), "fn hello() {}\nfn world() {}").unwrap();
    std::fs::write(dir.path().join("b.rs"), "fn goodbye() {}").unwrap();
    std::fs::write(dir.path().join("c.txt"), "no match here").unwrap();

    let ctx = ToolUseContext::test_default();
    let result = GrepTool
        .execute(
            json!({
                "pattern": "hello",
                "path": dir.path().to_str().unwrap(),
                "output_mode": "files_with_matches"
            }),
            &ctx,
        )
        .await
        .unwrap();

    let text = result.data.as_str().unwrap();
    assert!(text.contains("a.rs"));
    assert!(!text.contains("b.rs"));
}

#[tokio::test]
async fn test_grep_content_mode() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("test.rs"), "line1\nfn hello()\nline3").unwrap();

    let ctx = ToolUseContext::test_default();
    let result = GrepTool
        .execute(
            json!({
                "pattern": "hello",
                "path": dir.path().to_str().unwrap(),
                "output_mode": "content"
            }),
            &ctx,
        )
        .await
        .unwrap();

    let text = result.data.as_str().unwrap();
    assert!(text.contains("fn hello()"));
    assert!(text.contains(":2:"));
}

#[tokio::test]
async fn test_grep_count_mode() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("multi.rs"), "fn a()\nfn b()\nfn c()").unwrap();

    let ctx = ToolUseContext::test_default();
    let result = GrepTool
        .execute(
            json!({
                "pattern": "fn",
                "path": dir.path().to_str().unwrap(),
                "output_mode": "count"
            }),
            &ctx,
        )
        .await
        .unwrap();

    let text = result.data.as_str().unwrap();
    assert!(text.contains(":3"));
}

#[tokio::test]
async fn test_grep_case_insensitive() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("mixed.txt"), "Hello\nhello\nHELLO").unwrap();

    let ctx = ToolUseContext::test_default();
    let result = GrepTool
        .execute(
            json!({
                "pattern": "hello",
                "path": dir.path().to_str().unwrap(),
                "output_mode": "count",
                "-i": true
            }),
            &ctx,
        )
        .await
        .unwrap();

    let text = result.data.as_str().unwrap();
    assert!(text.contains(":3"));
}

#[tokio::test]
async fn test_grep_no_matches() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("empty.txt"), "nothing here").unwrap();

    let ctx = ToolUseContext::test_default();
    let result = GrepTool
        .execute(
            json!({
                "pattern": "zzzzz",
                "path": dir.path().to_str().unwrap()
            }),
            &ctx,
        )
        .await
        .unwrap();

    let text = result.data.as_str().unwrap();
    assert!(text.contains("No matches"));
}

#[tokio::test]
async fn test_grep_context_lines() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("ctx.txt"),
        "line1\nline2\nMATCH\nline4\nline5",
    )
    .unwrap();

    let ctx = ToolUseContext::test_default();
    let result = GrepTool
        .execute(
            json!({
                "pattern": "MATCH",
                "path": dir.path().to_str().unwrap(),
                "output_mode": "content",
                "-C": 1
            }),
            &ctx,
        )
        .await
        .unwrap();

    let text = result.data.as_str().unwrap();
    assert!(text.contains("line2"));
    assert!(text.contains("MATCH"));
    assert!(text.contains("line4"));
}

#[tokio::test]
async fn test_grep_single_file() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("single.txt");
    std::fs::write(&file, "alpha\nbeta\ngamma").unwrap();

    let ctx = ToolUseContext::test_default();
    let result = GrepTool
        .execute(
            json!({
                "pattern": "beta",
                "path": file.to_str().unwrap(),
                "output_mode": "content"
            }),
            &ctx,
        )
        .await
        .unwrap();

    let text = result.data.as_str().unwrap();
    assert!(text.contains("beta"));
}
