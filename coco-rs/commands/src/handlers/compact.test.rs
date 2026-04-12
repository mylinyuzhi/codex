use super::*;

#[test]
fn test_format_tokens_zero() {
    assert_eq!(format_tokens(0), "0");
}

#[test]
fn test_format_tokens_small() {
    assert_eq!(format_tokens(42), "42");
    assert_eq!(format_tokens(999), "999");
}

#[test]
fn test_format_tokens_thousands() {
    assert_eq!(format_tokens(1_000), "1,000");
    assert_eq!(format_tokens(12_345), "12,345");
    assert_eq!(format_tokens(200_000), "200,000");
    assert_eq!(format_tokens(1_234_567), "1,234,567");
}

#[tokio::test]
async fn test_compact_handler_no_args() {
    let output = handler(String::new()).await.unwrap();
    assert!(output.contains("Compacting conversation"));
    assert!(output.contains("Before compaction"));
    assert!(output.contains("After compaction"));
    assert!(output.contains("Est. tokens"));
}

#[tokio::test]
async fn test_compact_handler_with_instructions() {
    let output = handler("focus on API changes".to_string()).await.unwrap();
    assert!(output.contains("Summarization focus: focus on API changes"));
    assert!(output.contains("Before compaction"));
}

#[tokio::test]
async fn test_compact_handler_empty_session() {
    // With no sessions dir, should report 0 tokens gracefully
    let output = handler(String::new()).await.unwrap();
    assert!(output.contains("Messages:     0"));
}

#[tokio::test]
async fn test_estimate_session_tokens_nonexistent_dir() {
    let (tokens, count) =
        estimate_session_tokens(std::path::Path::new("/tmp/nonexistent_dir_abc123")).await;
    assert_eq!(tokens, 0);
    assert_eq!(count, 0);
}

#[tokio::test]
async fn test_estimate_session_tokens_with_temp_session() {
    let tmp = tempfile::tempdir().unwrap();
    let session_path = tmp.path().join("test-session.json");

    let session_json = serde_json::json!({
        "messages": [
            {"role": "user", "content": "Hello, how are you?"},
            {"role": "assistant", "content": "I'm doing well, thanks for asking!"},
            {"role": "user", "content": "Can you help me with Rust?"},
        ]
    });

    tokio::fs::write(
        &session_path,
        serde_json::to_string_pretty(&session_json).unwrap(),
    )
    .await
    .unwrap();

    let (tokens, count) = estimate_session_tokens(tmp.path()).await;
    assert_eq!(count, 3);
    assert!(
        tokens > 0,
        "should estimate non-zero tokens from session content"
    );
}
