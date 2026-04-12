use super::*;

#[tokio::test]
async fn test_context_handler_output_structure() {
    let output = handler(String::new()).await.unwrap();
    assert!(output.contains("Context Window Usage"));
    assert!(output.contains("System prompt"));
    assert!(output.contains("Tool definitions"));
    assert!(output.contains("Memory files"));
    assert!(output.contains("Messages"));
    assert!(output.contains("Free"));
    assert!(output.contains("Total used"));
}

#[tokio::test]
async fn test_context_handler_contains_percentages() {
    let output = handler(String::new()).await.unwrap();
    // Should contain percentage signs in the table
    assert!(output.contains('%'));
    // Should contain the message count line
    assert!(output.contains("Messages in history"));
}

#[tokio::test]
async fn test_estimate_usage_nonexistent_dir() {
    let (msg, mem, count) =
        estimate_usage(std::path::Path::new("/tmp/nonexistent_ctx_dir_abc123")).await;
    assert_eq!(msg, 0);
    assert_eq!(mem, 0);
    assert_eq!(count, 0);
}

#[tokio::test]
async fn test_estimate_usage_with_session_file() {
    let tmp = tempfile::tempdir().unwrap();
    let session = serde_json::json!({
        "messages": [
            {"role": "user", "content": "Hello world this is a test message"},
            {"role": "assistant", "content": "Hi there! I can help you with that. Let me think about it."},
        ]
    });
    tokio::fs::write(
        tmp.path().join("session.json"),
        serde_json::to_string(&session).unwrap(),
    )
    .await
    .unwrap();

    let (msg_tokens, _mem_tokens, count) = estimate_usage(tmp.path()).await;
    assert_eq!(count, 2);
    // Each message: content_len/4 + 10 overhead
    // "Hello world this is a test message" = 34 chars => 34/4+10 = 18
    // "Hi there! I can help you with that. Let me think about it." = 58 chars => 58/4+10 = 24
    assert!(msg_tokens > 0);
}

#[test]
fn test_format_tokens() {
    assert_eq!(format_tokens(0), "0");
    assert_eq!(format_tokens(500), "500");
    assert_eq!(format_tokens(2_500), "2,500");
    assert_eq!(format_tokens(200_000), "200,000");
}
