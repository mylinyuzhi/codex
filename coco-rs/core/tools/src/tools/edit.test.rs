use crate::tools::edit::EditTool;

// ── R7-T25: edit description content check ──
#[test]
fn test_edit_description_includes_uniqueness_warning() {
    use coco_tool_runtime::DescriptionOptions;
    use coco_tool_runtime::Tool;
    let desc = EditTool.description(&serde_json::Value::Null, &DescriptionOptions::default());
    assert!(
        desc.contains("must use your `Read` tool"),
        "Edit description should warn about read-before-edit requirement"
    );
    assert!(
        desc.contains("FAIL if `old_string` is not unique"),
        "Edit description should warn about uniqueness requirement"
    );
    assert!(
        desc.contains("`replace_all`"),
        "Edit description should mention replace_all"
    );
}
use coco_tool_runtime::Tool;
use coco_tool_runtime::ToolUseContext;
use serde_json::json;

#[tokio::test]
async fn test_edit_single_replacement() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("test.rs");
    std::fs::write(&file, "fn hello() {\n    println!(\"hi\");\n}\n").unwrap();

    let ctx = ToolUseContext::test_default();
    let result = EditTool
        .execute(
            json!({
                "file_path": file.to_str().unwrap(),
                "old_string": "println!(\"hi\")",
                "new_string": "println!(\"hello world\")"
            }),
            &ctx,
        )
        .await
        .unwrap();

    let text = result.data.as_str().unwrap();
    assert!(text.contains("updated successfully"));
    let content = std::fs::read_to_string(&file).unwrap();
    assert!(content.contains("hello world"));
    assert!(!content.contains("\"hi\""));
}

#[tokio::test]
async fn test_edit_replace_all() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("multi.txt");
    std::fs::write(&file, "foo bar foo baz foo").unwrap();

    let ctx = ToolUseContext::test_default();
    let result = EditTool
        .execute(
            json!({
                "file_path": file.to_str().unwrap(),
                "old_string": "foo",
                "new_string": "qux",
                "replace_all": true
            }),
            &ctx,
        )
        .await
        .unwrap();

    let text = result.data.as_str().unwrap();
    assert!(text.contains("3 replacement(s)"));
    let content = std::fs::read_to_string(&file).unwrap();
    assert_eq!(content, "qux bar qux baz qux");
}

#[tokio::test]
async fn test_edit_not_unique_error() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("dup.txt");
    std::fs::write(&file, "aaa bbb aaa").unwrap();

    let ctx = ToolUseContext::test_default();
    let result = EditTool
        .execute(
            json!({
                "file_path": file.to_str().unwrap(),
                "old_string": "aaa",
                "new_string": "ccc"
            }),
            &ctx,
        )
        .await;

    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.to_string().contains("2 times"));
}

#[tokio::test]
async fn test_edit_not_found_error() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("miss.txt");
    std::fs::write(&file, "hello world").unwrap();

    let ctx = ToolUseContext::test_default();
    let result = EditTool
        .execute(
            json!({
                "file_path": file.to_str().unwrap(),
                "old_string": "zzz",
                "new_string": "xxx"
            }),
            &ctx,
        )
        .await;

    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.to_string().contains("not found"));
}

#[tokio::test]
async fn test_edit_same_string_error() {
    let tool = EditTool;
    let ctx = coco_tool_runtime::ToolUseContext::test_default();
    let result = tool.validate_input(
        &json!({
            "file_path": "/tmp/x.txt",
            "old_string": "same",
            "new_string": "same"
        }),
        &ctx,
    );

    assert!(matches!(
        result,
        coco_tool_runtime::ValidationResult::Invalid { .. }
    ));
}

#[tokio::test]
async fn test_edit_file_not_found() {
    let ctx = ToolUseContext::test_default();
    let result = EditTool
        .execute(
            json!({
                "file_path": "/nonexistent/file.txt",
                "old_string": "a",
                "new_string": "b"
            }),
            &ctx,
        )
        .await;

    assert!(result.is_err());
}

// ---------------------------------------------------------------------------
// B2.4: quote normalization + content fallback race
// ---------------------------------------------------------------------------

/// When the file has curly quotes ("smart" quotes) but the model emitted
/// straight quotes, the match must succeed via quote normalization.
/// TS: `FileEditTool/utils.ts:73-93` `findActualString`.
#[tokio::test]
async fn test_edit_matches_curly_quotes() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("curly.md");
    // File uses left/right curly double quotes: \u{201C} hello \u{201D}
    let curly_content = "say \u{201C}hello\u{201D} to the world";
    std::fs::write(&file, curly_content).unwrap();

    let ctx = ToolUseContext::test_default();
    let result = EditTool
        .execute(
            json!({
                "file_path": file.to_str().unwrap(),
                "old_string": "\"hello\"",  // Model uses straight quotes
                "new_string": "\"hi\""
            }),
            &ctx,
        )
        .await
        .unwrap();

    let text = result.data.as_str().unwrap();
    assert!(text.contains("updated"));
    let content = std::fs::read_to_string(&file).unwrap();
    // preserve_quote_style should have re-applied curly quotes to new_string.
    assert!(
        content.contains('\u{201C}') && content.contains('\u{201D}'),
        "curly quotes should be preserved: {content}"
    );
    assert!(content.contains("hi"));
}

/// Single curly quote (apostrophe) variant.
#[tokio::test]
async fn test_edit_matches_curly_single_quotes() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("apostrophe.txt");
    // Using right-single curly (typographic apostrophe).
    let content = "it\u{2019}s a test";
    std::fs::write(&file, content).unwrap();

    let ctx = ToolUseContext::test_default();
    let result = EditTool
        .execute(
            json!({
                "file_path": file.to_str().unwrap(),
                "old_string": "it's",
                "new_string": "that's"
            }),
            &ctx,
        )
        .await
        .unwrap();

    let text = result.data.as_str().unwrap();
    assert!(text.contains("updated"));
    let content = std::fs::read_to_string(&file).unwrap();
    assert!(
        content.contains("that\u{2019}s"),
        "curly apostrophe preserved: {content}"
    );
}

// ---------------------------------------------------------------------------
// B2.4: content-fallback race detection
// ---------------------------------------------------------------------------

use coco_context::FileReadEntry;
use coco_context::FileReadState;
use std::sync::Arc;
use tokio::sync::RwLock;

// ---------------------------------------------------------------------------
// T2: normalize_file_edit_input integration (trailing whitespace + desanitize)
// ---------------------------------------------------------------------------

/// Model emits `new_string` with trailing spaces on each line. TS
/// `normalizeFileEditInput` strips them before applying the edit
/// (except for .md files). Regression guard: the strip must happen
/// automatically so the model doesn't have to be careful.
#[tokio::test]
async fn test_edit_strips_trailing_whitespace_from_new_string() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("code.rs");
    std::fs::write(&file, "fn hello() {\n    println!(\"hi\");\n}\n").unwrap();

    let ctx = ToolUseContext::test_default();
    // Note the trailing spaces on `println!("hello");   ` and on the closing brace.
    let result = EditTool
        .execute(
            json!({
                "file_path": file.to_str().unwrap(),
                "old_string": "    println!(\"hi\");\n}",
                "new_string": "    println!(\"hello\");   \n}  "
            }),
            &ctx,
        )
        .await
        .unwrap();
    assert!(result.data.as_str().unwrap().contains("updated"));

    // Trailing spaces should NOT end up on disk.
    let content = std::fs::read_to_string(&file).unwrap();
    assert!(content.contains("println!(\"hello\")"));
    assert!(
        !content.contains("hello\");   "),
        "trailing whitespace after `hello\");` must be stripped; got: {content:?}"
    );
    assert!(
        !content.contains("}  "),
        "trailing whitespace after `}}` must be stripped; got: {content:?}"
    );
}

/// Markdown files MUST preserve trailing whitespace — two trailing
/// spaces in markdown is a hard line break. TS: `/\.(md|mdx)$/i`
/// skips the whitespace strip for these extensions.
#[tokio::test]
async fn test_edit_preserves_trailing_whitespace_in_markdown() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("notes.md");
    std::fs::write(&file, "line one\nline two\n").unwrap();

    let ctx = ToolUseContext::test_default();
    // Replace with content that has trailing 2-space hard line break.
    let result = EditTool
        .execute(
            json!({
                "file_path": file.to_str().unwrap(),
                "old_string": "line one",
                "new_string": "line one  "
            }),
            &ctx,
        )
        .await
        .unwrap();
    assert!(result.data.as_str().unwrap().contains("updated"));

    let content = std::fs::read_to_string(&file).unwrap();
    assert!(
        content.contains("line one  "),
        "markdown files must preserve trailing hard-break spaces; got: {content:?}"
    );
}

/// Case-insensitive extension check: `.MD` and `.mdx` also preserve.
#[tokio::test]
async fn test_edit_markdown_extension_case_insensitive() {
    let dir = tempfile::tempdir().unwrap();
    for ext in &["MD", "mdx", "MdX"] {
        let file = dir.path().join(format!("doc.{ext}"));
        std::fs::write(&file, "hello\n").unwrap();

        let ctx = ToolUseContext::test_default();
        let result = EditTool
            .execute(
                json!({
                    "file_path": file.to_str().unwrap(),
                    "old_string": "hello",
                    "new_string": "world  "
                }),
                &ctx,
            )
            .await
            .unwrap();
        assert!(result.data.as_str().unwrap().contains("updated"));

        let content = std::fs::read_to_string(&file).unwrap();
        assert!(
            content.contains("world  "),
            "extension .{ext} must match case-insensitive markdown rule; got: {content:?}"
        );
    }
}

/// Desanitization: model emits `<n>` / `</n>` instead of `<name>` /
/// `</name>`. TS has both in the desanitization map (both open and
/// close tag forms), so `normalizeFileEditInput` rewrites the
/// old_string to match the real file content.
#[tokio::test]
async fn test_edit_desanitizes_sanitized_tags_in_old_string() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("log.txt");
    // File contains the REAL un-sanitized tag.
    std::fs::write(&file, "before\n<name>OK</name>\nafter\n").unwrap();

    let ctx = ToolUseContext::test_default();
    // Model emits the SANITIZED form `<n>`/`</n>` — the raw string
    // won't match the file, but desanitization rewrites it to
    // `<name>`/`</name>` before matching, which DOES match. Then the
    // same rewrite is applied to new_string so the final edit
    // replaces the real tag with a real tag.
    let result = EditTool
        .execute(
            json!({
                "file_path": file.to_str().unwrap(),
                "old_string": "<n>OK</n>",
                "new_string": "<n>UPDATED</n>"
            }),
            &ctx,
        )
        .await
        .unwrap();
    assert!(result.data.as_str().unwrap().contains("updated"));

    let content = std::fs::read_to_string(&file).unwrap();
    // The replacement should use the REAL tags, not the sanitized form.
    assert!(
        content.contains("<name>UPDATED</name>"),
        "desanitized new_string should write real tags; got: {content:?}"
    );
    assert!(
        !content.contains("<n>"),
        "sanitized form should NOT leak to disk; got: {content:?}"
    );
}

/// When file_read_state is primed with STALE content but the mtime
/// matches current disk (e.g. two edits within 1-second mtime precision
/// window), the content-fallback check must catch the drift and reject.
/// TS: `FileEditTool.ts:459-466`.
#[tokio::test]
async fn test_edit_detects_content_drift_in_race() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("race.txt");
    std::fs::write(&file, "current on disk").unwrap();
    let abs = std::fs::canonicalize(&file).unwrap();
    let mtime = coco_context::file_mtime_ms(&abs).await.unwrap();

    let mut ctx = ToolUseContext::test_default();
    ctx.file_read_state = Some(Arc::new(RwLock::new(FileReadState::new())));
    {
        let mut frs = ctx.file_read_state.as_ref().unwrap().write().await;
        frs.set(
            abs,
            FileReadEntry {
                content: "stale cached content".into(), // != current
                mtime_ms: mtime,                        // but same mtime
                offset: None,
                limit: None,
            },
        );
    }

    let result = EditTool
        .execute(
            json!({
                "file_path": file.to_str().unwrap(),
                "old_string": "current",
                "new_string": "changed"
            }),
            &ctx,
        )
        .await;

    assert!(result.is_err(), "content drift must be detected");
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("modified") || err.contains("content"),
        "error should mention drift: {err}"
    );
    // Content on disk must be unchanged.
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "current on disk");
}
