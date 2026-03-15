use super::*;

#[tokio::test]
async fn test_resume_nonexistent_session() {
    let config = ConfigManager::empty();
    let result = run("nonexistent-session-id", &config).await;
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(err.contains("Session not found"));
}
