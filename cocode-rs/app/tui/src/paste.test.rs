use super::*;

#[test]
fn test_count_lines_empty() {
    assert_eq!(count_lines(""), 0);
}

#[test]
fn test_count_lines_single() {
    assert_eq!(count_lines("hello"), 1);
}

#[test]
fn test_count_lines_multiple() {
    assert_eq!(count_lines("hello\nworld"), 2);
    assert_eq!(count_lines("a\nb\nc"), 3);
}

#[test]
fn test_count_lines_crlf() {
    assert_eq!(count_lines("hello\r\nworld"), 2);
}

#[test]
fn test_count_lines_cr() {
    assert_eq!(count_lines("hello\rworld"), 2);
}

#[test]
fn test_generate_pill_text_single_line() {
    let pill = generate_pill(1, &PasteKind::Text, 1);
    assert_eq!(pill, "[Pasted text #1]");
}

#[test]
fn test_generate_pill_text_multi_line() {
    let pill = generate_pill(2, &PasteKind::Text, 421);
    assert_eq!(pill, "[Pasted text #2 +420 lines]");
}

#[test]
fn test_generate_pill_image() {
    let pill = generate_pill(
        3,
        &PasteKind::Image {
            media_type: "image/png".to_string(),
        },
        0,
    );
    assert_eq!(pill, "[Image #3]");
}

#[test]
fn test_is_paste_pill() {
    assert!(is_paste_pill("[Pasted text #1]"));
    assert!(is_paste_pill("[Pasted text #1 +420 lines]"));
    assert!(is_paste_pill("[Image #1]"));
    assert!(is_paste_pill("[...Truncated text #1]"));
    assert!(!is_paste_pill("hello world"));
    assert!(!is_paste_pill("[Some other bracket]"));
}

fn temp_cache_dir() -> PathBuf {
    let dir = tempfile::tempdir().expect("Failed to create temp dir");
    let path = dir.path().to_path_buf();
    // Keep the tempdir alive by forgetting it (prevents cleanup)
    std::mem::forget(dir);
    path
}

#[test]
fn test_process_small_text() {
    let mut manager = PasteManager::with_cache_dir(temp_cache_dir());
    let small_text = "hello world";
    let result = manager.process_text(small_text.to_string());

    // Small text should be returned as-is
    assert_eq!(result, small_text);
    assert!(manager.entries.is_empty());
}

#[test]
fn test_process_large_text() {
    let mut manager = PasteManager::with_cache_dir(temp_cache_dir());
    let large_text = "x".repeat(2000);
    let result = manager.process_text(large_text);

    // Should return a pill
    assert!(result.starts_with("[Pasted text #"));
    assert_eq!(manager.entries.len(), 1);
}

#[test]
fn test_resolve_pills_no_pills() {
    let manager = PasteManager::with_cache_dir(temp_cache_dir());
    let text = "hello world";
    let resolved = manager.resolve_pills(text);
    assert_eq!(resolved, text);
}

#[test]
fn test_resolve_to_blocks_no_pills() {
    let manager = PasteManager::with_cache_dir(temp_cache_dir());
    let text = "hello world";
    let blocks = manager.resolve_to_blocks(text);

    assert_eq!(blocks.len(), 1);
    assert_eq!(blocks[0].as_text(), Some("hello world"));
}

#[test]
fn test_process_and_resolve_text() {
    let mut manager = PasteManager::with_cache_dir(temp_cache_dir());
    let content = "line1\nline2\nline3\n".repeat(100); // Make it large enough
    let pill = manager.process_text(content.clone());

    // Verify it's a pill
    assert!(pill.starts_with("[Pasted text #"));

    // Resolve and verify
    let resolved = manager.resolve_pills(&pill);
    assert_eq!(resolved, content);
}

#[test]
fn test_mixed_text_and_pill() {
    let mut manager = PasteManager::with_cache_dir(temp_cache_dir());
    let content = "x".repeat(2000);
    let pill = manager.process_text(content.clone());

    let input = format!("Please analyze this: {pill} and tell me what it means.");
    let resolved = manager.resolve_pills(&input);

    assert!(resolved.starts_with("Please analyze this: "));
    assert!(resolved.contains(&content));
    assert!(resolved.ends_with(" and tell me what it means."));
}

#[test]
fn test_has_pills() {
    let manager = PasteManager::with_cache_dir(temp_cache_dir());

    assert!(!manager.has_pills("hello world"));
    assert!(manager.has_pills("[Pasted text #1]"));
    assert!(manager.has_pills("[Pasted text #1 +420 lines]"));
    assert!(manager.has_pills("[Image #1]"));
    assert!(manager.has_pills("Before [Pasted text #1] after"));
}

#[test]
fn test_content_hash() {
    let hash1 = content_hash(b"hello");
    let hash2 = content_hash(b"hello");
    let hash3 = content_hash(b"world");

    assert_eq!(hash1, hash2);
    assert_ne!(hash1, hash3);
    assert_eq!(hash1.len(), 16); // 8 bytes = 16 hex chars
}
