use crate::tools::write::WriteTool;
use coco_tool::Tool;
use coco_tool::ToolUseContext;
use serde_json::json;

// ── R7-T25: write description content check ──
#[test]
fn test_write_description_includes_read_before_write_warning() {
    use coco_tool::DescriptionOptions;
    let desc = WriteTool.description(&serde_json::Value::Null, &DescriptionOptions::default());
    assert!(
        desc.contains("MUST use the `Read` tool first"),
        "Write description should warn about read-before-write requirement, got:\n{desc}"
    );
    assert!(
        desc.contains("Prefer the Edit tool"),
        "Write description should suggest Edit for modifications"
    );
    assert!(
        desc.contains("NEVER create documentation files"),
        "Write description should discourage docs files"
    );
}

#[tokio::test]
async fn test_write_new_file() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("new.txt");

    let ctx = ToolUseContext::test_default();
    let result = WriteTool
        .execute(
            json!({"file_path": file.to_str().unwrap(), "content": "hello\nworld\n"}),
            &ctx,
        )
        .await
        .unwrap();

    let text = result.data.as_str().unwrap();
    assert!(text.contains("created"));
    assert!(text.contains("2 lines"));
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "hello\nworld\n");
}

#[tokio::test]
async fn test_write_overwrite_existing() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("existing.txt");
    std::fs::write(&file, "old content").unwrap();

    let ctx = ToolUseContext::test_default();
    let result = WriteTool
        .execute(
            json!({"file_path": file.to_str().unwrap(), "content": "new content"}),
            &ctx,
        )
        .await
        .unwrap();

    let text = result.data.as_str().unwrap();
    assert!(text.contains("updated"));
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "new content");
}

#[tokio::test]
async fn test_write_creates_parent_dirs() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("a").join("b").join("c.txt");

    let ctx = ToolUseContext::test_default();
    let result = WriteTool
        .execute(
            json!({"file_path": file.to_str().unwrap(), "content": "deep"}),
            &ctx,
        )
        .await;

    assert!(result.is_ok());
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "deep");
}

#[tokio::test]
async fn test_write_missing_content() {
    let ctx = ToolUseContext::test_default();
    let result = WriteTool
        .execute(json!({"file_path": "/tmp/test.txt"}), &ctx)
        .await;

    assert!(result.is_err());
}

// ---------------------------------------------------------------------------
// B2.2: encoding preservation on overwrite
// ---------------------------------------------------------------------------

/// Overwriting a UTF-16LE file must preserve UTF-16LE encoding, not
/// silently convert it to UTF-8 which would corrupt the file.
/// TS: `FileWriteTool.ts:268-277, 297, 305` — `meta.encoding` is read
/// from disk and passed back to `writeTextContent`.
#[tokio::test]
async fn test_write_preserves_utf16le_encoding() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("utf16.txt");

    // Seed with UTF-16LE BOM + "old"
    let mut seed = vec![0xFF, 0xFE];
    for ch in "old\n".chars() {
        seed.extend_from_slice(&(ch as u16).to_le_bytes());
    }
    std::fs::write(&file, &seed).unwrap();

    let ctx = ToolUseContext::test_default();
    WriteTool
        .execute(
            json!({"file_path": file.to_str().unwrap(), "content": "new"}),
            &ctx,
        )
        .await
        .unwrap();

    let bytes = std::fs::read(&file).unwrap();
    // The file should still be UTF-16LE encoded. First two bytes are BOM.
    assert_eq!(&bytes[0..2], &[0xFF, 0xFE], "BOM must be preserved");
    // Remaining bytes should be "new" in UTF-16LE (3 chars × 2 bytes = 6 bytes).
    assert_eq!(bytes.len(), 8);
    assert_eq!(&bytes[2..4], &(b'n' as u16).to_le_bytes());
    assert_eq!(&bytes[4..6], &(b'e' as u16).to_le_bytes());
    assert_eq!(&bytes[6..8], &(b'w' as u16).to_le_bytes());
}

// ---------------------------------------------------------------------------
// B2.3: read-before-write + mtime+content race detection
// ---------------------------------------------------------------------------

use coco_context::FileReadEntry;
use coco_context::FileReadState;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Helper — build a context with a real FileReadState so race detection
/// checks actually fire.
fn ctx_with_file_state() -> ToolUseContext {
    let mut ctx = ToolUseContext::test_default();
    ctx.file_read_state = Some(Arc::new(RwLock::new(FileReadState::new())));
    ctx
}

/// Overwriting an existing file without a prior Read must fail. TS:
/// `FileWriteTool.ts:198-206` — enforces read-before-write to prevent
/// accidental data loss.
#[tokio::test]
async fn test_write_rejects_overwrite_without_read() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("existing.txt");
    std::fs::write(&file, "original").unwrap();

    let ctx = ctx_with_file_state();
    let result = WriteTool
        .execute(
            json!({"file_path": file.to_str().unwrap(), "content": "replaced"}),
            &ctx,
        )
        .await;

    assert!(result.is_err(), "should reject unseen-file overwrite");
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("must be read") || err.contains("Read tool"),
        "should mention read requirement: {err}"
    );
    // Original content must survive.
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "original");
}

/// New files bypass the read-before-write check (nothing to read yet).
#[tokio::test]
async fn test_write_new_file_bypasses_read_check() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("brand_new.txt");

    let ctx = ctx_with_file_state();
    let result = WriteTool
        .execute(
            json!({"file_path": file.to_str().unwrap(), "content": "fresh"}),
            &ctx,
        )
        .await
        .unwrap();

    let text = result.data.as_str().unwrap();
    assert!(text.contains("created"));
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "fresh");
}

/// Priming the file state with a correct mtime + content must allow the
/// overwrite to proceed.
#[tokio::test]
async fn test_write_allows_overwrite_after_read() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("file.txt");
    std::fs::write(&file, "first version").unwrap();
    let abs = std::fs::canonicalize(&file).unwrap();
    let mtime = coco_context::file_mtime_ms(&abs).await.unwrap();

    let ctx = ctx_with_file_state();
    {
        let mut frs = ctx.file_read_state.as_ref().unwrap().write().await;
        frs.set(
            abs,
            FileReadEntry {
                content: "first version".into(),
                mtime_ms: mtime,
                offset: None,
                limit: None,
            },
        );
    }

    let result = WriteTool
        .execute(
            json!({"file_path": file.to_str().unwrap(), "content": "second version"}),
            &ctx,
        )
        .await;
    assert!(
        result.is_ok(),
        "prior read should allow overwrite: {result:?}"
    );
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "second version");
}

/// Stale content in file_read_state (file was edited on disk since we
/// read it) must be detected via the content-hash fallback, even if the
/// mtime happens to match. TS: `FileWriteTool.ts:286-293`.
#[tokio::test]
async fn test_write_detects_content_drift() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("file.txt");
    std::fs::write(&file, "current content").unwrap();
    let abs = std::fs::canonicalize(&file).unwrap();
    let mtime = coco_context::file_mtime_ms(&abs).await.unwrap();

    let ctx = ctx_with_file_state();
    // Prime with STALE content but CURRENT mtime — simulates a file
    // edited within the same second (mtime resolution limit).
    {
        let mut frs = ctx.file_read_state.as_ref().unwrap().write().await;
        frs.set(
            abs,
            FileReadEntry {
                content: "stale cached content".into(),
                mtime_ms: mtime,
                offset: None,
                limit: None,
            },
        );
    }

    let result = WriteTool
        .execute(
            json!({"file_path": file.to_str().unwrap(), "content": "new content"}),
            &ctx,
        )
        .await;
    assert!(result.is_err(), "content drift should be detected");
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("modified") || err.contains("content"),
        "error should mention drift: {err}"
    );
}

/// New files default to UTF-8 (no BOM) — the common case.
#[tokio::test]
async fn test_write_new_file_is_utf8() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("fresh.txt");

    let ctx = ToolUseContext::test_default();
    WriteTool
        .execute(
            json!({"file_path": file.to_str().unwrap(), "content": "plain ascii"}),
            &ctx,
        )
        .await
        .unwrap();

    let bytes = std::fs::read(&file).unwrap();
    assert_eq!(bytes, b"plain ascii");
    // No BOM prepended for new files.
    assert!(!bytes.starts_with(&[0xEF, 0xBB, 0xBF]));
}

// ── R7-T14: team-memory secret guard tests ──
//
// TS `FileWriteTool.ts:156-160` calls `checkTeamMemSecrets(filePath,
// content)` before writing — if the path is in `.claude/memory/team/`
// AND the content has secret-shaped tokens, the write is rejected.
// coco-rs implements the same guard via `crate::check_team_mem_secret`.
// These tests cover the path predicate and the secret detector.

#[tokio::test]
async fn test_write_rejects_secret_in_team_memory_path() {
    let dir = tempfile::tempdir().unwrap();
    // Reconstruct a `.claude/memory/team/` ancestry under the temp dir.
    let team_dir = dir.path().join(".claude").join("memory").join("team");
    std::fs::create_dir_all(&team_dir).unwrap();
    let file = team_dir.join("personal.md");

    let ctx = ToolUseContext::test_default();
    let result = WriteTool
        .execute(
            json!({
                "file_path": file.to_str().unwrap(),
                // A bearer token-shaped secret triggers the redaction
                // detector, which causes the guard to reject the write.
                "content": "API_KEY=sk-ant-AAAAAAAAAAAAAAAAAAAAAA\nplain text\n"
            }),
            &ctx,
        )
        .await;

    let err = result.unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("Refusing to write") && msg.contains("secret"),
        "expected secret-guard error, got: {msg}"
    );
    // The file should NOT have been created.
    assert!(!file.exists(), "secret-guarded write must not hit disk");
}

#[tokio::test]
async fn test_write_allows_secret_outside_team_memory_path() {
    let dir = tempfile::tempdir().unwrap();
    // Same content but in a non-team-memory location → guard should
    // be inert. The user can still write whatever they want; the guard
    // only protects the synced path.
    let file = dir.path().join("notes.md");

    let ctx = ToolUseContext::test_default();
    let result = WriteTool
        .execute(
            json!({
                "file_path": file.to_str().unwrap(),
                "content": "API_KEY=sk-ant-AAAAAAAAAAAAAAAAAAAAAA\n"
            }),
            &ctx,
        )
        .await;

    assert!(
        result.is_ok(),
        "non-team-memory write should not be guarded: {result:?}"
    );
    assert!(file.exists());
}

#[tokio::test]
async fn test_write_allows_clean_content_in_team_memory_path() {
    let dir = tempfile::tempdir().unwrap();
    let team_dir = dir.path().join(".claude").join("memory").join("team");
    std::fs::create_dir_all(&team_dir).unwrap();
    let file = team_dir.join("safe.md");

    let ctx = ToolUseContext::test_default();
    let result = WriteTool
        .execute(
            json!({
                "file_path": file.to_str().unwrap(),
                // No secrets — guard permits the write.
                "content": "Just plain documentation text without any keys."
            }),
            &ctx,
        )
        .await;

    assert!(
        result.is_ok(),
        "clean content in team-memory should pass: {result:?}"
    );
    assert!(file.exists());
}

/// Custom memory dir via resolved config should still detect
/// team-memory writes in unusual locations. This exercises the Stage 1
/// (resolved) branch of `is_team_memory_path`.
#[tokio::test]
async fn test_write_secret_guard_respects_custom_memory_dir_config() {
    let dir = tempfile::tempdir().unwrap();
    let custom_memory_dir = dir.path().join("custom-memory");
    let team_dir = custom_memory_dir.join("team");
    std::fs::create_dir_all(&team_dir).unwrap();
    let file = team_dir.join("token.md");

    let mut ctx = ToolUseContext::test_default();
    ctx.memory_config.directory = Some(custom_memory_dir);
    let result = WriteTool
        .execute(
            json!({
                "file_path": file.to_str().unwrap(),
                "content": "API_KEY=sk-ant-AAAAAAAAAAAAAAAAAAAAAA\n"
            }),
            &ctx,
        )
        .await;

    let err = result.unwrap_err();
    assert!(
        err.to_string().contains("secret"),
        "expected secret-guard rejection in custom memory dir, got: {err}"
    );
    assert!(!file.exists());
}
