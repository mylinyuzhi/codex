use super::*;

#[tokio::test]
async fn test_clear_default() {
    let result = handler("".to_string()).await.unwrap();
    // TS-aligned: `/clear` is the full reset, status text reflects
    // that plan state + caches are also cleared.
    assert!(result.contains("Conversation cleared"));
    assert!(result.contains("Plan state"));
}

#[tokio::test]
async fn test_clear_all_aliases_default() {
    // `/clear all` aliases `/clear` — same status text.
    let result = handler("all".to_string()).await.unwrap();
    assert!(result.contains("Conversation cleared"));
    assert!(result.contains("Plan state"));
}

#[tokio::test]
async fn test_clear_history() {
    let result = handler("history".to_string()).await.unwrap();
    assert!(result.contains("Message history cleared"));
}

#[tokio::test]
async fn test_clear_unknown() {
    let result = handler("foobar".to_string()).await.unwrap();
    assert!(result.contains("Unknown"));
}
