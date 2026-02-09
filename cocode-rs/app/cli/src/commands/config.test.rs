use super::*;

#[tokio::test]
async fn test_show_config() {
    let config = ConfigManager::empty();
    let result = show_config(&config);
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_list_providers_empty() {
    let config = ConfigManager::empty();
    let result = list_providers(&config);
    assert!(result.is_ok());
}
