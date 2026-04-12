use super::*;

#[tokio::test]
async fn test_clear_default() {
    let result = handler("".to_string()).await.unwrap();
    assert!(result.contains("Conversation cleared"));
}

#[tokio::test]
async fn test_clear_all() {
    let result = handler("all".to_string()).await.unwrap();
    assert!(result.contains("plan state cleared"));
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
