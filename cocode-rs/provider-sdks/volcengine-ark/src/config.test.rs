use super::*;

#[test]
fn test_config_new() {
    let config = ClientConfig::new("test-key");
    assert_eq!(config.api_key, "test-key");
    assert_eq!(config.base_url, ClientConfig::DEFAULT_BASE_URL);
    assert_eq!(config.timeout, ClientConfig::DEFAULT_TIMEOUT);
    assert_eq!(config.max_retries, ClientConfig::DEFAULT_MAX_RETRIES);
}

#[test]
fn test_config_builder() {
    let config = ClientConfig::new("test-key")
        .base_url("https://custom.api.com")
        .timeout(Duration::from_secs(30))
        .max_retries(5);

    assert_eq!(config.api_key, "test-key");
    assert_eq!(config.base_url, "https://custom.api.com");
    assert_eq!(config.timeout, Duration::from_secs(30));
    assert_eq!(config.max_retries, 5);
}
