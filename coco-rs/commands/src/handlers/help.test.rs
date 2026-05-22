use super::*;

#[tokio::test]
async fn test_help_no_args() {
    let result = handler("".to_string()).await.unwrap();
    assert!(result.contains("Available Commands"));
    assert!(result.contains("/help"));
    assert!(result.contains("/config"));
    assert!(result.contains("/diff"));
}

#[tokio::test]
async fn test_help_specific_command() {
    let result = handler("model".to_string()).await.unwrap();
    assert!(result.contains("model"));
}

#[tokio::test]
async fn test_help_unknown_command() {
    let result = handler("nonexistent".to_string()).await.unwrap();
    assert!(result.contains("No command found"));
}
