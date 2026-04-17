use crate::tools::glob::GlobTool;
use coco_tool::Tool;
use coco_tool::ToolUseContext;
use serde_json::json;

fn text(result: &coco_types::ToolResult<serde_json::Value>) -> &str {
    result.data.as_str().unwrap()
}

// -----------------------------------------------------------------------
// Tool trait contract (safety / concurrency flags)
// -----------------------------------------------------------------------

#[test]
fn test_glob_is_read_only() {
    assert!(GlobTool.is_read_only(&serde_json::Value::Null));
}

#[test]
fn test_glob_is_concurrency_safe() {
    assert!(GlobTool.is_concurrency_safe(&serde_json::Value::Null));
}

#[test]
fn test_glob_is_not_destructive() {
    assert!(!GlobTool.is_destructive(&serde_json::Value::Null));
}

#[test]
fn test_glob_is_search_command() {
    let info = GlobTool
        .is_search_or_read_command(&serde_json::Value::Null)
        .expect("Glob should report as search command");
    assert!(info.is_search);
}

// -----------------------------------------------------------------------
// Basic matching
// -----------------------------------------------------------------------

#[tokio::test]
async fn test_glob_pattern_match() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.rs"), "fn main() {}").unwrap();
    std::fs::write(dir.path().join("b.rs"), "fn test() {}").unwrap();
    std::fs::write(dir.path().join("c.txt"), "text file").unwrap();

    let ctx = ToolUseContext::test_default();
    let result = GlobTool
        .execute(
            json!({
                "pattern": "*.rs",
                "path": dir.path().to_str().unwrap()
            }),
            &ctx,
        )
        .await
        .unwrap();

    let t = text(&result);
    assert!(t.contains("a.rs"), "should match a.rs: {t}");
    assert!(t.contains("b.rs"), "should match b.rs: {t}");
    assert!(!t.contains("c.txt"), "should not match c.txt: {t}");
}

#[tokio::test]
async fn test_glob_recursive_pattern() {
    let dir = tempfile::tempdir().unwrap();
    let sub = dir.path().join("src");
    std::fs::create_dir_all(&sub).unwrap();
    std::fs::write(dir.path().join("root.rs"), "root").unwrap();
    std::fs::write(sub.join("nested.rs"), "nested").unwrap();

    let ctx = ToolUseContext::test_default();
    let result = GlobTool
        .execute(
            json!({
                "pattern": "**/*.rs",
                "path": dir.path().to_str().unwrap()
            }),
            &ctx,
        )
        .await
        .unwrap();

    let t = text(&result);
    assert!(t.contains("root.rs"), "should find root file: {t}");
    assert!(t.contains("nested.rs"), "should find nested file: {t}");
}

#[tokio::test]
async fn test_glob_no_matches() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.txt"), "hello").unwrap();

    let ctx = ToolUseContext::test_default();
    let result = GlobTool
        .execute(
            json!({
                "pattern": "*.xyz",
                "path": dir.path().to_str().unwrap()
            }),
            &ctx,
        )
        .await
        .unwrap();

    let t = text(&result);
    assert_eq!(t, "No files found");
}

#[tokio::test]
async fn test_glob_invalid_pattern() {
    let dir = tempfile::tempdir().unwrap();

    let ctx = ToolUseContext::test_default();
    let result = GlobTool
        .execute(
            json!({
                "pattern": "[invalid",
                "path": dir.path().to_str().unwrap()
            }),
            &ctx,
        )
        .await;

    assert!(result.is_err(), "should error on invalid glob pattern");
}

// -----------------------------------------------------------------------
// TS behavioral alignment
// -----------------------------------------------------------------------

#[tokio::test]
async fn test_glob_hidden_files_included() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join(".hidden"), "secret").unwrap();
    std::fs::write(dir.path().join("visible"), "public").unwrap();

    let ctx = ToolUseContext::test_default();
    let result = GlobTool
        .execute(
            json!({
                "pattern": "*",
                "path": dir.path().to_str().unwrap()
            }),
            &ctx,
        )
        .await
        .unwrap();

    let t = text(&result);
    assert!(
        t.contains(".hidden"),
        "should include hidden files (TS: --hidden): {t}"
    );
}

#[tokio::test]
async fn test_glob_no_gitignore_by_default() {
    let dir = tempfile::tempdir().unwrap();

    // Init minimal git repo so .gitignore takes effect
    std::fs::create_dir_all(dir.path().join(".git")).unwrap();
    std::fs::write(dir.path().join(".gitignore"), "*.log\n").unwrap();
    std::fs::write(dir.path().join("keep.txt"), "keep").unwrap();
    std::fs::write(dir.path().join("debug.log"), "log").unwrap();

    let ctx = ToolUseContext::test_default();
    let result = GlobTool
        .execute(
            json!({
                "pattern": "*",
                "path": dir.path().to_str().unwrap()
            }),
            &ctx,
        )
        .await
        .unwrap();

    let t = text(&result);
    assert!(
        t.contains("debug.log"),
        "should NOT respect .gitignore (TS: --no-ignore): {t}"
    );
}

#[tokio::test]
async fn test_glob_mtime_sorting_matches_ts() {
    // TS GlobTool uses `rg --files --sort=modified` which sorts ASCENDING by
    // mtime (oldest first). This test verifies coco-rs matches that ordering.
    let dir = tempfile::tempdir().unwrap();

    std::fs::write(dir.path().join("old.txt"), "old").unwrap();
    std::thread::sleep(std::time::Duration::from_millis(50));
    std::fs::write(dir.path().join("new.txt"), "new").unwrap();

    let ctx = ToolUseContext::test_default();
    let result = GlobTool
        .execute(
            json!({
                "pattern": "*.txt",
                "path": dir.path().to_str().unwrap()
            }),
            &ctx,
        )
        .await
        .unwrap();

    let t = text(&result);
    let new_pos = t.find("new.txt").expect("should find new.txt");
    let old_pos = t.find("old.txt").expect("should find old.txt");
    assert!(
        old_pos < new_pos,
        "oldest file should appear first (TS --sort=modified behavior): {t}"
    );
}

#[tokio::test]
async fn test_glob_truncation_message() {
    let dir = tempfile::tempdir().unwrap();

    for i in 0..5 {
        std::fs::write(
            dir.path().join(format!("file{i:03}.txt")),
            format!("content {i}"),
        )
        .unwrap();
    }

    let mut ctx = ToolUseContext::test_default();
    ctx.glob_limits.max_results = Some(3);

    let result = GlobTool
        .execute(
            json!({
                "pattern": "*.txt",
                "path": dir.path().to_str().unwrap()
            }),
            &ctx,
        )
        .await
        .unwrap();

    let t = text(&result);
    assert!(
        t.contains("Results are truncated"),
        "should have TS truncation message: {t}"
    );
}

// -----------------------------------------------------------------------
// max_result_size_chars
// -----------------------------------------------------------------------

#[tokio::test]
async fn test_glob_max_result_size_chars() {
    assert_eq!(GlobTool.max_result_size_chars(), 100_000);
}

// -----------------------------------------------------------------------
// Reads glob_limits from context
// -----------------------------------------------------------------------

#[tokio::test]
async fn test_glob_reads_glob_limits() {
    let dir = tempfile::tempdir().unwrap();
    for i in 0..10 {
        std::fs::write(
            dir.path().join(format!("f{i}.rs")),
            format!("fn f{i}() {{}}"),
        )
        .unwrap();
    }

    let mut ctx = ToolUseContext::test_default();
    ctx.glob_limits.max_results = Some(5);

    let result = GlobTool
        .execute(
            json!({
                "pattern": "*.rs",
                "path": dir.path().to_str().unwrap()
            }),
            &ctx,
        )
        .await
        .unwrap();

    let t = text(&result);
    let file_count = t.lines().filter(|l| l.ends_with(".rs")).count();
    assert_eq!(file_count, 5, "should limit to 5 results: {t}");
    assert!(
        t.contains("Results are truncated"),
        "should be truncated: {t}"
    );
}

// -----------------------------------------------------------------------
// Concurrency & cancellation
// -----------------------------------------------------------------------

/// Two Glob calls should execute in parallel without interference.
#[tokio::test]
async fn test_glob_parallel_execution() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("main.rs"), "").unwrap();
    std::fs::write(dir.path().join("doc.md"), "").unwrap();

    let ctx = ToolUseContext::test_default();
    let path = dir.path().to_str().unwrap().to_string();

    let rs_fut = GlobTool.execute(json!({"pattern": "*.rs", "path": &path}), &ctx);
    let md_fut = GlobTool.execute(json!({"pattern": "*.md", "path": &path}), &ctx);
    let (rs_res, md_res) = tokio::join!(rs_fut, md_fut);

    let rs_text = text(rs_res.as_ref().unwrap());
    let md_text = text(md_res.as_ref().unwrap());

    assert!(rs_text.contains("main.rs"), "rs: {rs_text}");
    assert!(!rs_text.contains("doc.md"), "rs spilled: {rs_text}");
    assert!(md_text.contains("doc.md"), "md: {md_text}");
    assert!(!md_text.contains("main.rs"), "md spilled: {md_text}");
}

/// A pre-cancelled token should short-circuit glob walking.
#[tokio::test]
async fn test_glob_respects_cancellation() {
    let dir = tempfile::tempdir().unwrap();
    for i in 0..20 {
        std::fs::write(dir.path().join(format!("f{i}.txt")), "").unwrap();
    }

    let mut ctx = ToolUseContext::test_default();
    ctx.cancel = tokio_util::sync::CancellationToken::new();
    ctx.cancel.cancel();

    let result = GlobTool
        .execute(
            json!({
                "pattern": "*.txt",
                "path": dir.path().to_str().unwrap()
            }),
            &ctx,
        )
        .await
        .expect("cancelled glob should still return Ok");

    let t = text(&result);
    assert_eq!(
        t, "No files found",
        "pre-cancelled Glob should return empty result: {t}"
    );
}

/// `cwd_override` redirects the default search path.
#[tokio::test]
async fn test_glob_respects_cwd_override() {
    let outer = tempfile::tempdir().unwrap();
    let inner = tempfile::tempdir().unwrap();

    std::fs::write(outer.path().join("decoy.rs"), "").unwrap();
    std::fs::write(inner.path().join("real.rs"), "").unwrap();

    let mut ctx = ToolUseContext::test_default();
    ctx.cwd_override = Some(inner.path().to_path_buf());

    let result = GlobTool
        .execute(json!({"pattern": "*.rs"}), &ctx)
        .await
        .unwrap();

    let t = text(&result);
    assert!(t.contains("real.rs"), "should find override file: {t}");
    assert!(!t.contains("decoy.rs"), "must not leak to outer: {t}");
}
