use super::*;
use tempfile::TempDir;

fn make_config(max_size: i32, preview_size: i32, enabled: bool) -> ToolConfig {
    ToolConfig {
        max_result_size: max_size,
        result_preview_size: preview_size,
        enable_result_persistence: enabled,
        ..Default::default()
    }
}

/// Helper to extract text from ToolResultContent.
fn extract_text(content: &ToolResultContent) -> &str {
    match content {
        ToolResultContent::Text(s) => s,
        _ => panic!("Expected text content"),
    }
}

#[tokio::test]
async fn test_small_result_unchanged() {
    let temp_dir = TempDir::new().unwrap();
    let config = make_config(100, 20, true);
    let output = ToolOutput::text("small result");

    let result = persist_if_needed(output, "call-1", temp_dir.path(), &config).await;

    assert_eq!(extract_text(&result.content), "small result");
    assert!(!result.is_error);
}

#[tokio::test]
async fn test_large_result_persisted() {
    let temp_dir = TempDir::new().unwrap();
    let config = make_config(50, 20, true);
    let large_content = "x".repeat(100);
    let output = ToolOutput::text(&large_content);

    let result = persist_if_needed(output, "call-2", temp_dir.path(), &config).await;

    // Check that result contains persistence markers
    let text = extract_text(&result.content);
    assert!(text.contains(PERSISTED_OUTPUT_START));
    assert!(text.contains(PERSISTED_OUTPUT_END));
    assert!(text.contains("100 characters"));
    assert!(text.contains("call-2.txt"));

    // Verify file was created
    let file_path = temp_dir.path().join("tool-results/call-2.txt");
    assert!(file_path.exists());
    let saved = std::fs::read_to_string(&file_path).unwrap();
    assert_eq!(saved, large_content);
}

#[tokio::test]
async fn test_persistence_disabled() {
    let temp_dir = TempDir::new().unwrap();
    let config = make_config(50, 20, false);
    let large_content = "x".repeat(100);
    let output = ToolOutput::text(&large_content);

    let result = persist_if_needed(output, "call-3", temp_dir.path(), &config).await;

    // Should return original unchanged
    assert_eq!(extract_text(&result.content), large_content);
    // No file should be created
    let file_path = temp_dir.path().join("tool-results/call-3.txt");
    assert!(!file_path.exists());
}

#[tokio::test]
async fn test_preview_truncation() {
    let temp_dir = TempDir::new().unwrap();
    let config = make_config(50, 10, true);
    let large_content = "abcdefghijklmnopqrstuvwxyz".repeat(5); // 130 chars
    let output = ToolOutput::text(&large_content);

    let result = persist_if_needed(output, "call-4", temp_dir.path(), &config).await;

    let text = extract_text(&result.content);
    // Preview should contain first ~10 chars + "..."
    assert!(text.contains("abcdefghij..."));
}

#[tokio::test]
async fn test_structured_content() {
    let temp_dir = TempDir::new().unwrap();
    let config = make_config(20, 10, true);
    let json = serde_json::json!({"key": "value".repeat(10)});
    let output = ToolOutput::structured(json);

    let result = persist_if_needed(output, "call-5", temp_dir.path(), &config).await;

    // Should be persisted since JSON string representation exceeds threshold
    let text = extract_text(&result.content);
    assert!(text.contains(PERSISTED_OUTPUT_START));
}

#[tokio::test]
async fn test_error_output_preserved() {
    let temp_dir = TempDir::new().unwrap();
    let config = make_config(50, 20, true);
    let large_content = "error: ".to_string() + &"x".repeat(100);
    let output = ToolOutput::error(&large_content);

    let result = persist_if_needed(output, "call-6", temp_dir.path(), &config).await;

    // is_error flag should be preserved
    assert!(result.is_error);
}

#[tokio::test]
async fn test_utf8_safe_truncation() {
    let temp_dir = TempDir::new().unwrap();
    // Use a threshold that's larger than 10 emojis but smaller than 100
    let config = make_config(100, 10, true);
    // Multi-byte UTF-8 characters - 50 emojis = 200 bytes
    let large_content = "🔥".repeat(50);
    let output = ToolOutput::text(&large_content);

    let result = persist_if_needed(output, "call-7", temp_dir.path(), &config).await;

    let text = extract_text(&result.content);
    // Should not panic on UTF-8 boundary and should be persisted (200 bytes > 100 threshold)
    assert!(text.contains(PERSISTED_OUTPUT_START));
}

/// Verify thresholds match Claude Code v2.1.7.
#[test]
fn test_default_thresholds() {
    use cocode_protocol::DEFAULT_MAX_RESULT_SIZE;
    use cocode_protocol::DEFAULT_RESULT_PREVIEW_SIZE;
    assert_eq!(DEFAULT_MAX_RESULT_SIZE, 400_000);
    assert_eq!(DEFAULT_RESULT_PREVIEW_SIZE, 2_000);
}
